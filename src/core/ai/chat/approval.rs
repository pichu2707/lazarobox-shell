//! The approval of one chat: what the user is asked to allow and the pure
//! transitions around it.
//!
//! A chat has at most one pending approval. Only an explicit press answers
//! it; a key repeat or release, a timeout or anything else never does. This
//! module holds no IO and does not know which key means what: the caller
//! maps keys to a [`Reply`].

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use crate::core::ai::{
    chat::engine::CallId,
    config::ApprovalMode,
    sanitize::clean,
    tools::{
        Call, Locality,
        command::Refusal,
        exec::{self, Fallback, SafeCommand},
    },
};

/// Ids come from one counter for the whole process, so an `Approvals` that is
/// rebuilt (a chat reopened, a state replaced) never hands out an id an older
/// one gave, and a late answer for the old approval cannot hit the new one.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    // Saturating, not wrapping: reaching u64::MAX takes longer than the
    // process can live, and if it ever happened the id would stop moving
    // rather than roll over to an old one. `fetch_update` always succeeds
    // here (the closure never returns None), so the fallback is unreachable.
    NEXT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            Some(n.saturating_add(1))
        })
        .unwrap_or(u64::MAX)
}

/// The reason given to a request made while another is pending.
pub const ANOTHER_PENDING: &str = "another approval is pending";

/// The outcome line of a run action that was declined or withdrawn.
pub const RUN_CANCELLED: &str = "cancelled";

/// The outcome line of a command typed into pane `number`.
pub fn run_sent_line(number: usize) -> String {
    format!("sent to pane {number}; press Enter there to run it")
}

/// Whether `line` is the outcome line of a run action. Such a line is shown
/// but never stored or sent, and it does not hide the block it came from.
pub fn is_run_outcome(line: &str) -> bool {
    line == RUN_CANCELLED
        || (line.starts_with("sent to pane ") && line.ends_with("; press Enter there to run it"))
}

/// What the user is asked to allow.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ApprovalAsk {
    /// Type `command` into a pane. The command is already validated.
    Run { command: String },
    /// Read `path` (sanitized for display) and send its text to the engine.
    Read { path: String },
    /// Call a tool of a user-declared MCP server.
    External {
        server: String,
        tool: String,
        args: String,
    },
}

/// Which calls wait for the user's answer before they run (D17).
/// `run_in_pane` always does. `read_file` does on a cloud engine, because the
/// text it returns goes to the provider, and runs free on a local one. The
/// directory tools never ask. The mode of the chat (ask or auto-type, D18) is
/// not an input: the app applies it to the ask this returns.
///
/// The path of a read ask is shown as it is: the caller passes the target
/// resolved by `tools::paths::display_target`, which exists only when display
/// sanitizing would not change it. The host opens the file from the call.
pub fn ask_for(call: &Call, locality: Locality) -> Option<ApprovalAsk> {
    match call {
        Call::RunInPane { command } => Some(ApprovalAsk::Run {
            command: command.clone(),
        }),
        Call::ReadFile { path, .. } if locality == Locality::Cloud => {
            Some(ApprovalAsk::Read { path: path.clone() })
        }
        // A user tool always asks, on any engine and in any approval mode:
        // lazarobox cannot know what it does, and a read-only hint is ignored.
        Call::External {
            server, tool, args, ..
        } => Some(ApprovalAsk::External {
            server: shown_name(server),
            tool: shown_name(tool),
            args: shown_args(args),
        }),
        // `glob` returns names only and, like the directory tools, never asks.
        Call::ReadFile { .. } | Call::FindDir { .. } | Call::ListDir { .. } | Call::Glob { .. } => {
            None
        }
    }
}

/// The most characters of a user tool's argument values an ask shows, shared
/// fairly between the keys.
pub const EXTERNAL_ARGS_SHOWN: usize = 1024;

/// The fewest characters of one value an ask shows, however many keys.
const MIN_VALUE_SHOWN: usize = 16;

/// The longest server, tool or key name an ask shows, in characters.
const SHOWN_NAME: usize = 64;

/// One sanitized line: every run of whitespace is one space. Cut at `cap`
/// characters, with an ellipsis. Returns the text and how many characters
/// were left out.
///
/// What is shown goes through `clean` (control and escape sequences removed,
/// secrets redacted) and whitespace flattening, so it can differ from the
/// exact bytes that are sent on `y`.
fn flat(text: &str, cap: usize) -> (String, usize) {
    let line = clean(text).split_whitespace().collect::<Vec<_>>().join(" ");
    let total = line.chars().count();
    if total <= cap {
        return (line, 0);
    }
    let mut cut: String = line.chars().take(cap).collect();
    cut.push('\u{2026}');
    (cut, total - cap)
}

/// A name from a server, as one sanitized line, cut with an ellipsis.
fn shown_name(name: &str) -> String {
    flat(name, SHOWN_NAME).0
}

/// The arguments as `key: value` pairs. Every top-level key is listed, and
/// each value has its own cap, so a key cannot be pushed out of view by long
/// values before it (the map sorts keys, and a model chooses their names). A
/// cut says how much is hidden: the characters of the values, out of all of
/// them.
fn shown_args(args: &Value) -> String {
    let pairs: Vec<(String, String)> = match args {
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect(),
        other => vec![(String::new(), other.to_string())],
    };
    let cap = (EXTERNAL_ARGS_SHOWN / pairs.len().max(1)).max(MIN_VALUE_SHOWN);
    let (mut hidden, mut total) = (0, 0);
    let mut parts = Vec::with_capacity(pairs.len());
    for (key, value) in &pairs {
        let (shown, cut) = flat(value, cap);
        hidden += cut;
        total += shown.chars().count().saturating_sub(usize::from(cut > 0)) + cut;
        parts.push(format!("{}: {shown}", shown_name(key)));
    }
    let mut out = parts.join(", ");
    if hidden > 0 {
        out.push_str(&format!(" (truncated: {hidden} of {total} chars hidden)"));
    }
    out
}

/// What the app does with a model's ask (D18, D21).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Route<'a> {
    /// Open the approval overlay, as in `ask` mode. In `run` mode, `Some`
    /// says why the command is not run directly.
    Prompt(Option<Fallback>),
    /// Type the command into the target pane without a prompt, no Enter.
    AutoType,
    /// Check the command, then type it and press Enter (`run` mode only).
    AutoRun(SafeCommand<'a>),
}

/// The most commands one turn may type or run without a prompt.
pub const MAX_AUTO_PER_TURN: u8 = 5;

/// The consecutive grammar or policy fallbacks after which `run` pauses for
/// the rest of the turn.
pub const MAX_RUN_FALLBACKS: u8 = 3;

/// Why a `run` prompt names no offending token.
pub const NO_CWD: &str = "the pane's directory cannot be read on this system";
/// Why a path check refuses a command: the one refusal that counts toward the pause.
pub const NOT_ALLOWED: &str = "a path in the command is not allowed to run directly";
pub const PAUSED: &str = "auto-run paused for this turn";
pub const NON_UTF8: &str = "the pane's locale is not UTF-8";
/// Why a command that the grammar accepts still takes the prompt: the pane
/// or the turn is not free. None of them is the model's doing.
pub const RUNNING: &str = "a command is still running in the pane";
pub const HALF_TYPED: &str = "the pane has a half-typed line";
pub const FOCUSED: &str = "the pane has the focus";
pub const HIDDEN: &str = "the chat's tab is not shown";
pub const TOO_MANY: &str = "too many commands this turn";

/// Whether a fallback is the model's doing: the reasons of the pane and of
/// the pause itself never count toward the pause.
pub fn counts_toward_pause(fallback: &Fallback) -> bool {
    !matches!(
        fallback.why,
        NO_CWD | PAUSED | NON_UTF8 | RUNNING | HALF_TYPED | FOCUSED | HIDDEN | TOO_MANY
    )
}

/// The longest token the reason line names.
const MAX_TOKEN_CHARS: usize = 24;

/// Whether a character is drawn as `?`: everything but printable ASCII and
/// the Latin letters the grammar accepts. This covers controls, format and
/// invisible characters, combining marks and other scripts.
fn hidden(c: char) -> bool {
    !(c.is_ascii_graphic()
        || c == ' '
        || ('\u{c0}'..='\u{24f}').contains(&c) && c != '\u{d7}' && c != '\u{f7}')
}

/// The one line the prompt shows for a fallback: the reason and, when it
/// has a span inside `command`, the token it names in backticks. Control
/// characters are drawn as `?` and a long token is cut.
pub fn reason_line(fallback: &Fallback, command: &str) -> String {
    let token = command
        .get(fallback.at.clone())
        .filter(|t| !t.trim().is_empty());
    let Some(token) = token else {
        return fallback.why.to_owned();
    };
    let mut shown: String = (token.chars().take(MAX_TOKEN_CHARS))
        .map(|c| if hidden(c) { '?' } else { c })
        .collect();
    if token.chars().nth(MAX_TOKEN_CHARS).is_some() {
        shown.push('…');
    }
    format!("{}: `{shown}`", fallback.why)
}

/// Whether the locale of a shell names UTF-8: `LC_ALL`, else `LC_CTYPE`, else
/// `LANG`, the first one that is set and not empty. Unset is not UTF-8. This
/// is lazarobox's own environment, which the shells inherit: an rc file that
/// changes the locale or sets readline's `convert-meta` is not seen (residual).
pub fn names_utf8(lc_all: Option<&str>, lc_ctype: Option<&str>, lang: Option<&str>) -> bool {
    [lc_all, lc_ctype, lang]
        .into_iter()
        .flatten()
        .find(|value| !value.is_empty())
        .is_some_and(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("utf-8") || value.contains("utf8")
        })
}

/// What the app knows when a call arrives, beyond the call itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AutoCtx {
    /// Commands already typed or run without a prompt in this turn.
    pub typed_this_turn: u8,
    /// An earlier auto-typed command in the target pane is still unsent.
    pub line_pending: bool,
    /// The target pane has the focus: the user may be typing in it.
    pub target_focused: bool,
    /// The chat's tab is the shown one: the user can see the typing.
    pub tab_shown: bool,
    /// A run is in flight in the target pane.
    pub executing: bool,
    /// The shell's real directory can be read (never on macOS).
    pub cwd_known: bool,
    /// Too many fallbacks this turn: `run` prompts until the next one.
    pub paused: bool,
    /// The user has typed into the target pane since its last Enter or
    /// Ctrl+C: the line may hold text the run would join.
    pub dirty: bool,
    /// The pane's locale names UTF-8, so bytes >= 0x80 mean what they say.
    pub utf8: bool,
}

/// `AutoType` only for a model `run_in_pane` in `auto` mode, and `AutoRun`
/// only in `run` mode for a command the grammar accepts, with fewer than
/// [`MAX_AUTO_PER_TURN`] typed or run this turn, no pending line or run in the
/// target, the target not focused or dirty, its directory known, the tab
/// shown, the run not paused and the locale UTF-8 for a non-ASCII command.
/// Every other case, a read, an external call or a command the user started,
/// prompts. `auto` and `ask` never produce `AutoRun`.
pub fn route<'a>(
    ask: &'a ApprovalAsk,
    origin: Origin,
    mode: ApprovalMode,
    ctx: AutoCtx,
) -> Route<'a> {
    match (ask, origin, mode) {
        (ApprovalAsk::Run { .. }, Origin::Tool, ApprovalMode::Auto) if busy(ctx).is_none() => {
            Route::AutoType
        }
        (ApprovalAsk::Run { command }, Origin::Tool, ApprovalMode::Run) => route_run(command, ctx),
        _ => Route::Prompt(None),
    }
}

/// Why the target or the turn is not free for a command to go without a
/// prompt: a limit, a run in flight, the user's line or the user's eyes.
fn busy(ctx: AutoCtx) -> Option<&'static str> {
    Some(if ctx.typed_this_turn >= MAX_AUTO_PER_TURN {
        TOO_MANY
    } else if ctx.executing {
        RUNNING
    } else if ctx.line_pending || ctx.dirty {
        HALF_TYPED
    } else if ctx.target_focused {
        FOCUSED
    } else if !ctx.tab_shown {
        HIDDEN
    } else {
        return None;
    })
}

/// The `run` half of [`route`]: the reasons of the pane and the pause first,
/// then the grammar, then the limits.
fn route_run(command: &str, ctx: AutoCtx) -> Route<'_> {
    let because = |why| Route::Prompt(Some(Fallback { at: 0..0, why }));
    if !ctx.cwd_known {
        return because(NO_CWD);
    }
    if ctx.paused {
        return because(PAUSED);
    }
    match exec::parse(command) {
        // A non-UTF-8 locale or readline's `convert-meta` could read the
        // bytes as something else than what was checked.
        Ok(safe) if !ctx.utf8 && !safe.as_str().is_ascii() => because(NON_UTF8),
        Ok(safe) => busy(ctx).map_or(Route::AutoRun(safe), because),
        Err(fallback) => Route::Prompt(Some(fallback)),
    }
}

/// Where the request came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// The user asked to run the last shell block of an answer.
    Block,
    /// A tool call of the engine.
    Tool,
}

/// A pending approval.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Approval {
    id: u64,
    ask: ApprovalAsk,
    origin: Origin,
    /// The tool call this approval answers: its turn and call id. `None` for
    /// an action the user started.
    call: Option<(u64, CallId)>,
    /// Why a command was not run directly, as the prompt will say it.
    why: Option<String>,
}

impl Approval {
    /// Unique and increasing within one chat: a late answer or withdrawal
    /// for an earlier approval names the wrong id and is ignored.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn ask(&self) -> &ApprovalAsk {
        &self.ask
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// The turn and call of the tool call that waits for this approval.
    pub fn call(&self) -> Option<(u64, CallId)> {
        self.call
    }

    /// Why a `run` command was not run directly, if it was meant to be.
    pub fn why(&self) -> Option<&str> {
        self.why.as_deref()
    }
}

/// What the user answered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reply {
    Approve,
    Decline,
}

/// The phase of the key event that carried a reply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    Press,
    Repeat,
    Release,
}

/// What the pane draws for a pending approval: all text is already
/// sanitized and is exactly what will be done.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApprovalPrompt {
    /// Where it goes: `run in pane 2 (~/dev)`.
    pub target: String,
    /// The exact action, as it will be done: the command to type,
    /// `read <path>`, or `<server> <tool> <arguments>`.
    pub command: String,
    /// The keys line.
    pub keys: &'static str,
    /// Whether the keys answer yet. A prompt the model opened ignores them for
    /// a moment, so a key typed before it appeared cannot answer it; the keys
    /// line shows `…` meanwhile.
    pub armed: bool,
    /// Why a `run` command was not run directly, as one line.
    pub why: Option<String>,
}

/// The keys of a run prompt.
pub const RUN_KEYS: &str = "y type it (no Enter) · n cancel · p next pane";
/// The keys of a read prompt.
pub const READ_KEYS: &str = "y read it · n deny";
/// The keys of a user tool prompt: no `p`, it has no target pane.
pub const EXTERNAL_KEYS: &str = "y call it · n deny";

/// The pending approval of a chat, if any.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Approvals {
    pending: Option<Approval>,
}

impl Approvals {
    /// The approval waiting for an answer.
    pub fn pending(&self) -> Option<&Approval> {
        self.pending.as_ref()
    }

    /// Ask the user and return the id of the new approval. Refused with
    /// [`ANOTHER_PENDING`] when one is pending; the first stays pending.
    pub fn request(&mut self, ask: ApprovalAsk, origin: Origin) -> Result<u64, Refusal> {
        self.open(ask, origin, None, None)
    }

    /// Like [`request`](Self::request) for the tool call `call` of turn
    /// `turn`: the origin is [`Origin::Tool`], and the answer is reported
    /// under that turn and call. `why` says why a `run` command was not run
    /// directly.
    pub fn request_call(
        &mut self,
        ask: ApprovalAsk,
        turn: u64,
        call: CallId,
        why: Option<String>,
    ) -> Result<u64, Refusal> {
        self.open(ask, Origin::Tool, Some((turn, call)), why)
    }

    fn open(
        &mut self,
        ask: ApprovalAsk,
        origin: Origin,
        call: Option<(u64, CallId)>,
        why: Option<String>,
    ) -> Result<u64, Refusal> {
        if self.pending.is_some() {
            return Err(Refusal::new(ANOTHER_PENDING));
        }
        let id = next_id();
        self.pending = Some(Approval {
            id,
            ask,
            origin,
            call,
            why,
        });
        Ok(id)
    }

    /// Answer approval `id`. Only a [`Press::Press`] answers; a repeat or a
    /// release leaves it pending, and so does an id that is not the pending
    /// one. Returns the answered approval.
    pub fn answer(&mut self, id: u64, reply: Reply, press: Press) -> Option<(Approval, Reply)> {
        if press != Press::Press {
            return None;
        }
        if self.pending.as_ref()?.id != id {
            return None;
        }
        self.pending.take().map(|approval| (approval, reply))
    }

    /// End approval `id` without an answer (cancel, close, a target that is
    /// gone). An id that is not the pending one changes nothing. The caller
    /// records the result as denied.
    pub fn withdraw(&mut self, id: u64) -> Option<Approval> {
        if self.pending.as_ref()?.id != id {
            return None;
        }
        self.pending.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREE: AutoCtx = AutoCtx {
        typed_this_turn: 0,
        line_pending: false,
        target_focused: false,
        tab_shown: true,
        executing: false,
        cwd_known: true,
        paused: false,
        dirty: false,
        utf8: true,
    };

    fn run_ask() -> ApprovalAsk {
        ask_of("cd x")
    }

    fn ask_of(command: &str) -> ApprovalAsk {
        ApprovalAsk::Run {
            command: command.into(),
        }
    }

    /// The command a route would run, if it runs one.
    fn runs<'a>(route: &Route<'a>) -> Option<&'a str> {
        match route {
            Route::AutoRun(safe) => Some(safe.as_str()),
            _ => None,
        }
    }

    fn why(route: &Route<'_>) -> Option<&'static str> {
        match route {
            Route::Prompt(Some(fallback)) => Some(fallback.why),
            _ => None,
        }
    }

    // Spec (chat-pane, approval mode): D18 routing.
    #[test]
    fn a_model_run_in_auto_mode_is_typed_when_nothing_blocks_it() {
        let ask = run_ask();
        let route = route(&ask, Origin::Tool, ApprovalMode::Auto, FREE);
        assert_eq!(route, Route::AutoType);
    }

    #[test]
    fn ask_mode_and_other_asks_always_prompt() {
        let read = ApprovalAsk::Read { path: "~/a".into() };
        let external = ApprovalAsk::External {
            server: "s".into(),
            tool: "t".into(),
            args: "{}".into(),
        };
        assert_eq!(
            route(&run_ask(), Origin::Tool, ApprovalMode::Ask, FREE),
            Route::Prompt(None)
        );
        for ask in [&read, &external] {
            assert_eq!(
                route(ask, Origin::Tool, ApprovalMode::Auto, FREE),
                Route::Prompt(None)
            );
        }
        assert_eq!(
            route(&run_ask(), Origin::Block, ApprovalMode::Auto, FREE),
            Route::Prompt(None)
        );
    }

    #[test]
    fn each_flood_limit_falls_back_to_the_prompt() {
        let blocked = [
            AutoCtx {
                line_pending: true,
                ..FREE
            },
            AutoCtx {
                target_focused: true,
                ..FREE
            },
            AutoCtx {
                tab_shown: false,
                ..FREE
            },
            AutoCtx {
                typed_this_turn: MAX_AUTO_PER_TURN,
                ..FREE
            },
        ];
        for ctx in blocked {
            assert_eq!(
                route(&run_ask(), Origin::Tool, ApprovalMode::Auto, ctx),
                Route::Prompt(None),
                "{ctx:?}"
            );
        }
    }

    #[test]
    fn the_fifth_call_of_a_turn_is_typed_and_the_sixth_prompts() {
        let fifth = AutoCtx {
            typed_this_turn: MAX_AUTO_PER_TURN - 1,
            ..FREE
        };
        assert_eq!(MAX_AUTO_PER_TURN, 5);
        assert_eq!(
            route(&run_ask(), Origin::Tool, ApprovalMode::Auto, fifth),
            Route::AutoType
        );
    }

    // Spec (pane-execution, run routing): the matrix of S3.1.
    #[test]
    fn run_mode_runs_a_parsed_command_when_nothing_blocks_it() {
        let ask = ask_of("cd ~/dev");
        let route = route(&ask, Origin::Tool, ApprovalMode::Run, FREE);
        assert_eq!(runs(&route), Some("cd ~/dev"));
    }

    #[test]
    fn auto_and_ask_never_produce_a_run() {
        for ask in ["cd x", "ls", "pwd", "mkdir -p a"] {
            for mode in [ApprovalMode::Auto, ApprovalMode::Ask] {
                let ask = ask_of(ask);
                let route = route(&ask, Origin::Tool, mode, FREE);
                assert!(runs(&route).is_none(), "{mode:?} {ask:?}");
            }
        }
    }

    #[test]
    fn every_context_bit_turns_a_run_into_the_prompt() {
        let blocked = [
            AutoCtx {
                line_pending: true,
                ..FREE
            },
            AutoCtx {
                target_focused: true,
                ..FREE
            },
            AutoCtx {
                tab_shown: false,
                ..FREE
            },
            AutoCtx {
                typed_this_turn: MAX_AUTO_PER_TURN,
                ..FREE
            },
            AutoCtx {
                executing: true,
                ..FREE
            },
            AutoCtx {
                cwd_known: false,
                ..FREE
            },
            AutoCtx {
                paused: true,
                ..FREE
            },
            AutoCtx {
                dirty: true,
                ..FREE
            },
        ];
        let ask = ask_of("pwd");
        for ctx in blocked {
            let route = route(&ask, Origin::Tool, ApprovalMode::Run, ctx);
            assert!(matches!(route, Route::Prompt(_)), "{ctx:?}");
        }
    }

    #[test]
    fn the_unreadable_cwd_and_the_pause_name_their_reason() {
        let ask = ask_of("pwd");
        let blind = AutoCtx {
            cwd_known: false,
            ..FREE
        };
        let paused = AutoCtx {
            paused: true,
            ..FREE
        };
        let route_in = |ctx| route(&ask, Origin::Tool, ApprovalMode::Run, ctx);
        assert_eq!(
            why(&route_in(blind)),
            Some("the pane's directory cannot be read on this system")
        );
        assert_eq!(
            why(&route_in(paused)),
            Some("auto-run paused for this turn")
        );
    }

    #[test]
    fn a_command_off_the_grammar_prompts_with_the_offending_token() {
        let ask = ask_of("ls *");
        let got = route(&ask, Origin::Tool, ApprovalMode::Run, FREE);
        let Route::Prompt(Some(fallback)) = got else {
            panic!("{got:?}")
        };
        assert_eq!((fallback.at, fallback.why), (3..4, "shell metacharacter"));
        let ask = ask_of("git status");
        let got = route(&ask, Origin::Tool, ApprovalMode::Run, FREE);
        assert_eq!(why(&got), Some("not a safe command"));
    }

    // Spec (pane-execution, S3.6b): bytes >= 0x80 need a UTF-8 locale.
    #[test]
    fn a_non_ascii_command_runs_only_under_a_utf8_locale() {
        let accented = ask_of("cd m\u{fa}sica");
        let ascii = ask_of("cd music");
        let c = AutoCtx {
            utf8: false,
            ..FREE
        };
        let got = route(&accented, Origin::Tool, ApprovalMode::Run, c);
        assert_eq!(why(&got), Some("the pane's locale is not UTF-8"));
        let got = route(&accented, Origin::Tool, ApprovalMode::Run, FREE);
        assert_eq!(runs(&got), Some("cd m\u{fa}sica"));
        let got = route(&ascii, Origin::Tool, ApprovalMode::Run, c);
        assert_eq!(runs(&got), Some("cd music"));
    }

    #[test]
    fn the_locale_names_utf8_by_the_first_variable_that_is_set() {
        let table = [
            ((None, None, None), false),
            ((None, None, Some("C")), false),
            ((None, None, Some("en_US.UTF-8")), true),
            ((None, None, Some("es_ES.utf8")), true),
            ((None, Some("C.UTF-8"), Some("C")), true),
            ((Some("C"), Some("C.UTF-8"), Some("en_US.UTF-8")), false),
            ((Some(""), None, Some("en_US.UTF-8")), true),
            ((Some("en_US.UTF-8"), None, Some("C")), true),
            ((None, None, Some("en_US.ISO-8859-1")), false),
        ];
        for ((all, ctype, lang), want) in table {
            assert_eq!(
                names_utf8(all, ctype, lang),
                want,
                "{all:?} {ctype:?} {lang:?}"
            );
        }
    }

    #[test]
    fn only_a_model_run_can_run() {
        let read = ApprovalAsk::Read { path: "~/a".into() };
        for (ask, origin) in [
            (run_ask(), Origin::Block),
            (read.clone(), Origin::Tool),
            (read, Origin::Block),
        ] {
            let route = route(&ask, origin, ApprovalMode::Run, FREE);
            assert_eq!(route, Route::Prompt(None), "{ask:?} {origin:?}");
        }
    }

    // The pane being busy is a reason too, so the prompt can say it.
    #[test]
    fn a_busy_pane_names_its_reason_and_never_counts_toward_the_pause() {
        let ask = ask_of("pwd");
        let table = [
            (
                AutoCtx {
                    executing: true,
                    ..FREE
                },
                RUNNING,
            ),
            (
                AutoCtx {
                    line_pending: true,
                    ..FREE
                },
                HALF_TYPED,
            ),
            (
                AutoCtx {
                    dirty: true,
                    ..FREE
                },
                HALF_TYPED,
            ),
            (
                AutoCtx {
                    target_focused: true,
                    ..FREE
                },
                FOCUSED,
            ),
            (
                AutoCtx {
                    tab_shown: false,
                    ..FREE
                },
                HIDDEN,
            ),
            (
                AutoCtx {
                    typed_this_turn: MAX_AUTO_PER_TURN,
                    ..FREE
                },
                TOO_MANY,
            ),
        ];
        for (ctx, reason) in table {
            let got = route(&ask, Origin::Tool, ApprovalMode::Run, ctx);
            assert_eq!(why(&got), Some(reason), "{ctx:?}");
            assert!(matches!(&got, Route::Prompt(Some(f)) if !counts_toward_pause(f)));
        }
        // A command off the grammar still names the grammar, not the pane.
        let off = ask_of("git status");
        let busy = AutoCtx {
            executing: true,
            ..FREE
        };
        let got = route(&off, Origin::Tool, ApprovalMode::Run, busy);
        assert_eq!(why(&got), Some("not a safe command"));
    }

    // The line the prompt shows: the reason and the offending token.
    #[test]
    fn the_reason_line_names_the_token_when_there_is_one() {
        let line = |text: &str| {
            let fallback = exec::parse(text).unwrap_err();
            reason_line(&fallback, text)
        };
        assert_eq!(line("cd a && ls"), "shell metacharacter: `&&`");
        assert_eq!(line("git status"), "not a safe command: `git`");
        assert_eq!(line("ls -R src"), "flag not allowed: `-R`");
        // Nothing to name: no token, or a token that is only a space.
        assert_eq!(line("ls "), "trailing space");
        let no_token = Fallback {
            at: 0..0,
            why: NO_CWD,
        };
        assert_eq!(reason_line(&no_token, "pwd"), NO_CWD);
        // A span outside the text is ignored, never sliced.
        let stale = Fallback {
            at: 2..90,
            why: "x",
        };
        assert_eq!(reason_line(&stale, "pwd"), "x");
        // A long token is cut and a control character is not drawn raw.
        let long = format!("cd {}&", "a".repeat(200));
        assert!(line(&long).chars().count() < 60, "{}", line(&long));
        let ctl = Fallback { at: 0..3, why: "w" };
        assert_eq!(reason_line(&ctl, "a\u{1b}b"), "w: `a?b`");
        // Latin letters stay readable.
        let latin = Fallback {
            at: 0..10,
            why: "w",
        };
        assert_eq!(reason_line(&latin, "ñandú_é"), "w: `ñandú_é`");
        // So are the characters that reorder or hide text.
        let marks = [
            '\u{202e}', '\u{200b}', '\u{2066}', '\u{00a0}', '\u{3164}', '\u{0301}',
        ];
        for c in marks.into_iter().chain(['я', '日', '😀']) {
            let text = format!("a{c}b");
            let hidden = Fallback {
                at: 0..text.len(),
                why: "w",
            };
            assert_eq!(reason_line(&hidden, &text), "w: `a?b`", "{c:?}");
        }
    }

    // Reasons of the pause: the ones that are not the model's doing do not count.
    #[test]
    fn only_a_grammar_or_policy_fallback_counts_toward_the_pause() {
        let counted = |command: &str| {
            let ask = ask_of(command);
            let got = route(&ask, Origin::Tool, ApprovalMode::Run, FREE);
            matches!(&got, Route::Prompt(Some(f)) if counts_toward_pause(f))
        };
        assert!(counted("ls *"));
        assert!(counted("git status"));
        for ctx in [
            AutoCtx {
                paused: true,
                ..FREE
            },
            AutoCtx {
                cwd_known: false,
                ..FREE
            },
            AutoCtx {
                utf8: false,
                ..FREE
            },
        ] {
            let ask = ask_of("cd m\u{fa}sica");
            let got = route(&ask, Origin::Tool, ApprovalMode::Run, ctx);
            assert!(matches!(&got, Route::Prompt(Some(f)) if !counts_toward_pause(f)));
        }
    }

    // Spec (agent-tools, approval by locality): D17 table.
    #[test]
    fn which_calls_ask_depends_on_the_tool_and_the_locality() {
        let find = Call::FindDir { query: "x".into() };
        let list = Call::ListDir { path: "~".into() };
        let read = Call::ReadFile {
            path: "~/a.txt".into(),
            lines: None,
        };
        let run = Call::RunInPane {
            command: "cd x".into(),
        };
        for locality in [Locality::Local, Locality::Cloud] {
            assert_eq!(ask_for(&find, locality), None);
            assert_eq!(ask_for(&list, locality), None);
            // Spec (agent-tools): glob runs without approval on every engine.
            let glob = Call::Glob {
                pattern: "**/*.rs".into(),
                dir: None,
            };
            assert_eq!(ask_for(&glob, locality), None);
            assert_eq!(
                ask_for(&run, locality),
                Some(ApprovalAsk::Run {
                    command: "cd x".into()
                })
            );
        }
        assert_eq!(ask_for(&read, Locality::Local), None);
        assert_eq!(
            ask_for(&read, Locality::Cloud),
            Some(ApprovalAsk::Read {
                path: "~/a.txt".into()
            })
        );
    }

    fn run(command: &str) -> ApprovalAsk {
        ApprovalAsk::Run {
            command: command.into(),
        }
    }

    #[test]
    fn a_request_becomes_the_pending_approval() {
        let mut a = Approvals::default();
        assert!(a.pending().is_none());
        a.request(run("ls"), Origin::Block).unwrap();
        let p = a.pending().unwrap();
        assert_eq!(p.ask(), &run("ls"));
        assert_eq!(p.origin(), Origin::Block);
    }

    #[test]
    fn a_tool_request_names_its_turn_and_call() {
        let mut a = Approvals::default();
        a.request_call(run("ls"), 4, CallId(9), None).unwrap();
        let p = a.pending().unwrap();
        assert_eq!((p.origin(), p.call()), (Origin::Tool, Some((4, CallId(9)))));
        assert_eq!(p.why(), None);
        let err = a.request_call(run("pwd"), 4, CallId(10), None).unwrap_err();
        assert_eq!(err.reason(), ANOTHER_PENDING);
        assert_eq!(a.pending().unwrap().call(), Some((4, CallId(9))));
    }

    #[test]
    fn a_tool_request_keeps_why_it_was_not_run_directly() {
        let mut a = Approvals::default();
        let why = Some("shell metacharacter".to_owned());
        a.request_call(run("ls *"), 4, CallId(9), why).unwrap();
        assert_eq!(a.pending().unwrap().why(), Some("shell metacharacter"));
    }

    #[test]
    fn a_user_request_names_no_call() {
        let mut a = Approvals::default();
        a.request(run("ls"), Origin::Block).unwrap();
        assert_eq!(a.pending().unwrap().call(), None);
    }

    #[test]
    fn a_second_request_is_refused_and_the_first_stays() {
        let mut a = Approvals::default();
        a.request(run("one"), Origin::Block).unwrap();
        let err = a
            .request(ApprovalAsk::Read { path: "/x".into() }, Origin::Tool)
            .unwrap_err();
        assert_eq!(err.reason(), "another approval is pending");
        assert_eq!(a.pending().unwrap().ask(), &run("one"));
    }

    #[test]
    fn all_three_asks_can_be_pending() {
        for ask in [
            run("ls"),
            ApprovalAsk::Read { path: "/x".into() },
            ApprovalAsk::External {
                server: "s".into(),
                tool: "t".into(),
                args: "{}".into(),
            },
        ] {
            let mut a = Approvals::default();
            a.request(ask.clone(), Origin::Tool).unwrap();
            assert_eq!(a.pending().unwrap().ask(), &ask);
        }
    }

    #[test]
    fn a_press_answers_and_clears() {
        for reply in [Reply::Approve, Reply::Decline] {
            let mut a = Approvals::default();
            let id = a.request(run("ls"), Origin::Block).unwrap();
            let (done, got) = a.answer(id, reply, Press::Press).unwrap();
            assert_eq!(done.ask(), &run("ls"));
            assert_eq!(got, reply);
            assert!(a.pending().is_none());
        }
    }

    #[test]
    fn repeat_and_release_never_answer() {
        let mut a = Approvals::default();
        let id = a.request(run("ls"), Origin::Block).unwrap();
        for press in [Press::Repeat, Press::Release] {
            assert_eq!(a.answer(id, Reply::Approve, press), None);
            assert!(a.pending().is_some());
        }
    }

    #[test]
    fn nothing_pending_nothing_answered() {
        let mut a = Approvals::default();
        assert_eq!(a.answer(0, Reply::Approve, Press::Press), None);
        assert_eq!(a.withdraw(0), None);
    }

    #[test]
    fn withdraw_ends_it_and_allows_a_new_request() {
        let mut a = Approvals::default();
        let id = a.request(run("ls"), Origin::Block).unwrap();
        assert_eq!(a.withdraw(id).unwrap().ask(), &run("ls"));
        assert!(a.pending().is_none());
        a.request(run("pwd"), Origin::Tool).unwrap();
        assert_eq!(a.pending().unwrap().ask(), &run("pwd"));
    }

    #[test]
    fn ids_increase_with_every_request() {
        let mut a = Approvals::default();
        let first = a.request(run("a"), Origin::Block).unwrap();
        a.withdraw(first).unwrap();
        let second = a.request(run("b"), Origin::Block).unwrap();
        assert!(second > first);
        assert_eq!(a.pending().unwrap().id(), second);
    }

    #[test]
    fn a_stale_answer_leaves_the_new_approval_pending() {
        let mut a = Approvals::default();
        let old = a.request(run("A"), Origin::Block).unwrap();
        a.withdraw(old).unwrap();
        a.request(run("B"), Origin::Tool).unwrap();
        for press in [Press::Press, Press::Repeat, Press::Release] {
            assert_eq!(a.answer(old, Reply::Approve, press), None);
        }
        assert_eq!(a.pending().unwrap().ask(), &run("B"));
    }

    #[test]
    fn a_late_withdraw_does_not_cancel_the_new_approval() {
        let mut a = Approvals::default();
        let old = a.request(run("A"), Origin::Block).unwrap();
        a.withdraw(old).unwrap();
        a.request(run("B"), Origin::Tool).unwrap();
        assert_eq!(a.withdraw(old), None);
        assert_eq!(a.pending().unwrap().ask(), &run("B"));
    }

    #[test]
    fn two_instances_never_share_an_id() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..5 {
            let mut a = Approvals::default();
            let id = a.request(run("x"), Origin::Block).unwrap();
            assert!(ids.insert(id), "id {id} was reused");
            a.withdraw(id).unwrap();
            let id = a.request(run("y"), Origin::Tool).unwrap();
            assert!(ids.insert(id), "id {id} was reused");
        }
        // A rebuilt Approvals cannot be answered with an old id.
        let mut old = Approvals::default();
        let stale = old.request(run("A"), Origin::Block).unwrap();
        let mut fresh = Approvals::default();
        fresh.request(run("B"), Origin::Block).unwrap();
        assert_eq!(fresh.answer(stale, Reply::Approve, Press::Press), None);
        assert!(fresh.pending().is_some());
    }

    fn external_call(args: serde_json::Value) -> Call {
        Call::External {
            name: "fs__write".into(),
            server: "fs".into(),
            tool: "write".into(),
            args,
        }
    }

    // 4c: a user tool always asks, on either locality, whatever it claims.
    #[test]
    fn an_external_call_always_asks_with_server_tool_and_arguments() {
        for locality in [Locality::Local, Locality::Cloud] {
            assert_eq!(
                ask_for(&external_call(serde_json::json!({"path": "~/x"})), locality),
                Some(ApprovalAsk::External {
                    server: "fs".into(),
                    tool: "write".into(),
                    args: r#"path: "~/x""#.into(),
                })
            );
        }
    }

    #[test]
    fn the_shown_arguments_are_sanitized_and_capped_with_a_visible_cut() {
        let hostile = serde_json::json!({"k": "a\u{1b}[31mb\u{202e}c"});
        let Some(ApprovalAsk::External { args, .. }) =
            ask_for(&external_call(hostile), Locality::Cloud)
        else {
            panic!("an external call asks");
        };
        assert!(
            !args.chars().any(|c| c.is_control() || c == '\u{202e}'),
            "{args:?}"
        );
        let long = serde_json::json!({"k": "x".repeat(EXTERNAL_ARGS_SHOWN * 2)});
        let Some(ApprovalAsk::External { args, .. }) =
            ask_for(&external_call(long), Locality::Cloud)
        else {
            panic!("an external call asks");
        };
        // 2 quotes plus the text: the hidden count is exact.
        let total = EXTERNAL_ARGS_SHOWN * 2 + 2;
        let hidden = total - EXTERNAL_ARGS_SHOWN;
        assert!(
            args.ends_with(&format!(" (truncated: {hidden} of {total} chars hidden)")),
            "{args}"
        );
        assert!(
            args.chars().count() < EXTERNAL_ARGS_SHOWN + 80,
            "{}",
            args.len()
        );
    }

    fn shown(args: serde_json::Value) -> String {
        match ask_for(&external_call(args), Locality::Cloud) {
            Some(ApprovalAsk::External { args, .. }) => args,
            other => panic!("an external call asks: {other:?}"),
        }
    }

    // W1: the map sorts keys, so a key sorted last must not fall past the cut.
    #[test]
    fn a_dangerous_key_sorted_last_stays_visible_behind_long_padding() {
        let args = serde_json::json!({
            "a_pad": "x".repeat(1500),
            "b_pad": "y".repeat(1500),
            "zz_cmd": "rm -rf ~",
        });
        let text = shown(args);
        assert!(text.contains(r#"zz_cmd: "rm -rf ~""#), "{text}");
        assert!(
            text.contains("a_pad: ") && text.contains("b_pad: "),
            "{text}"
        );
        assert!(text.contains("chars hidden)"), "{text}");
    }

    #[test]
    fn every_key_is_listed_even_when_all_values_are_cut() {
        let mut map = serde_json::Map::new();
        for n in 0..8 {
            map.insert(format!("key{n}"), serde_json::json!("v".repeat(400)));
        }
        let text = shown(serde_json::Value::Object(map));
        for n in 0..8 {
            assert!(text.contains(&format!("key{n}: ")), "{text}");
        }
        assert!(text.chars().count() < 1024 + 8 * 24 + 80, "{}", text.len());
    }

    #[test]
    fn nothing_is_cut_when_it_all_fits() {
        let text = shown(serde_json::json!({"a": 1, "b": "two"}));
        assert_eq!(text, r#"a: 1, b: "two""#);
    }

    // W2: a name that is cut says so, and words stay apart.
    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_words_keep_a_space() {
        let call = Call::External {
            name: "s__t".into(),
            server: format!("{}\n\t  {}", "a".repeat(10), "b".repeat(100)),
            tool: "two   words".into(),
            args: serde_json::json!({}),
        };
        let Some(ApprovalAsk::External { server, tool, .. }) = ask_for(&call, Locality::Local)
        else {
            panic!("an external call asks");
        };
        assert_eq!(tool, "two words");
        assert!(
            server.starts_with(&format!("{} {}", "a".repeat(10), "b")),
            "{server}"
        );
        assert!(server.ends_with('\u{2026}'), "{server}");
        assert_eq!(server.chars().count(), 64 + 1);
    }

    #[test]
    fn hostile_server_and_tool_names_are_sanitized_in_the_ask() {
        let call = Call::External {
            name: "s__t".into(),
            server: "s\u{1b}[2J".into(),
            tool: "t\u{202e}x".into(),
            args: serde_json::json!({}),
        };
        let Some(ApprovalAsk::External { server, tool, .. }) = ask_for(&call, Locality::Local)
        else {
            panic!("an external call asks");
        };
        assert!(
            !format!("{server}{tool}")
                .chars()
                .any(|c| c.is_control() || c == '\u{202e}')
        );
    }
}
