//! The approval of one chat: what the user is asked to allow and the pure
//! transitions around it.
//!
//! A chat has at most one pending approval. Only an explicit press answers
//! it; a key repeat or release, a timeout or anything else never does. This
//! module holds no IO and does not know which key means what: the caller
//! maps keys to a [`Reply`].

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::core::ai::{
    chat::{
        engine::{CallId, Decision},
        review::{self, BatchReview, CommitReview},
        review_view::ReviewView,
    },
    config::{ApprovalMode, Task},
    orchestra::WorkerKey,
    sanitize::clean,
    tools::{
        Call, Locality, allow,
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

/// How long a prompt the model opened ignores `y`, `n` and Esc, so a key the
/// user was already typing cannot answer it. It is longer than the usual OS
/// auto-repeat delay (X11 660 ms, KDE 600 ms): the first repeat of a key held
/// from before the prompt, which comes past the hold window, still finds the
/// prompt unarmed. The review of a batch waits as long; see
/// [`ApprovalAsk::arm_delay`].
pub const ASK_ARM_DELAY: Duration = Duration::from_millis(800);

/// The reason given to a request made while another is pending.
pub const ANOTHER_PENDING: &str = "another approval is pending";

/// How many worker asks may wait behind the one shown; one more is refused
/// with [`ANOTHER_PENDING`], so a flood never grows without limit.
pub const MAX_QUEUED: usize = 16;

/// The reason given to a worker ask that finds the queue full.
pub const QUEUE_FULL: &str = "approval queue full";

/// The outcome line of a run action that was declined or withdrawn.
pub const RUN_CANCELLED: &str = "cancelled";

/// The outcome line of a command typed into pane `number`, with why it was
/// not run when `y` in `run` mode could not run it.
pub fn run_sent_line(number: usize, not_run: Option<&str>) -> String {
    let line = format!("sent to pane {number}; press Enter there to run it");
    match not_run {
        Some(why) => format!("{line} (not run: {why})"),
        None => line,
    }
}

/// Whether `line` is the outcome line of a run action. Such a line is shown
/// but never stored or sent, and it does not hide the block it came from.
pub fn is_run_outcome(line: &str) -> bool {
    line == RUN_CANCELLED
        || (line.starts_with("sent to pane ")
            && (line.ends_with("; press Enter there to run it")
                || line.contains("; press Enter there to run it (not run: ")))
}

/// What the user is asked to allow.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ApprovalAsk {
    /// Type `command` into a pane. The command is already validated.
    Run { command: String },
    /// Read `path` (sanitized for display) and send its text to the engine.
    /// `reach` is where the host found the read leads, and `cloud` whether the
    /// engine is a cloud one; only the approval mode reads them (see
    /// [`answers_itself`]). [`ask_for`] puts [`Reach::Other`]; the host fills
    /// in what it found. The ask an edit opens is always `Other`.
    Read {
        path: String,
        reach: Reach,
        cloud: bool,
    },
    /// A worker's read outside its run's scope: `path`, `reach` and `cloud`
    /// as in `Read`, so the approval mode answers it exactly as the same chat
    /// read (see [`answers_itself`]), and the worker's `label` (see
    /// [`worker_label`]) for its prompt.
    WorkerRead {
        path: String,
        label: String,
        /// The worker that asks: the session the answer is addressed to.
        worker: WorkerKey,
        reach: Reach,
        cloud: bool,
    },
    /// Search the text of the files under `dir` (the root when `.`) for
    /// `pattern`, and send the matching lines to the engine. Both are one
    /// sanitized line. `reach` is [`Reach::Root`] when the host found the
    /// chat's root concrete enough to search on its own, [`Reach::Broad`]
    /// when it is HOME or above; [`ask_for`] puts [`Reach::Other`].
    Search {
        pattern: String,
        dir: String,
        reach: Reach,
    },
    /// Call a tool of a user-declared MCP server.
    External {
        server: String,
        tool: String,
        args: String,
    },
    /// Run `program` with `args` in the sandbox, in `cwd` (relative to the
    /// root, `.` for the root). All three are exactly what will run: the host
    /// refuses a call whose text would not show as it is. Never routed by the
    /// approval mode: every mode asks. `script` is the body of the
    /// `package.json` script a `npm`/`yarn`/`pnpm` command will run (the
    /// manager reads it when it runs), as sanitized lines the host fills in;
    /// [`ask_for`] leaves it `None`.
    Exec {
        program: String,
        args: Vec<String>,
        cwd: String,
        script: Option<String>,
        /// The seconds the run may take, as the host will enforce them.
        /// [`ask_for`] puts the default; the host fills in the clamped value.
        timeout_secs: u64,
        /// The engine is a cloud one: its provider sees the output.
        cloud: bool,
        /// What a run of this chat that was cancelled left changed in the
        /// watched git places, as one sanitized line the host fills in.
        earlier: Option<String>,
    },
    /// Show the diff of the repository to the engine: file text leaves the
    /// machine, so a cloud engine asks (`git_status` and `git_log` return
    /// names and subjects only and never ask). `path` is `.` for the whole
    /// tree, as one clean line. Never routed by the approval mode.
    GitDiff { staged: bool, path: String },
    /// Stage `paths` in the index (one clean line, as the user will read it).
    /// `files` is how many files a dry run says would be staged; the host
    /// fills it in, [`ask_for`] leaves it `None`. Never routed by the
    /// approval mode: every mode asks.
    GitAdd { paths: String, files: Option<usize> },
    /// Record the index as a commit. The user reads the message, the signing
    /// notice and the whole staged diff in the review view; it is never routed
    /// by the approval mode, so no mode approves it.
    Commit(Arc<CommitReview>),
    /// Write a proposed batch of files. The user reads it in the review view;
    /// it is never routed by the approval mode, so no mode approves it.
    Batch(Arc<BatchReview>),
    /// Change the chat folder (the user's `/cd`, never a tool call). `from`
    /// is the current folder as one sanitized line, `to` the resolved folder
    /// that becomes the chat's on `y`, and `note` a fact the user must see
    /// (a broad folder). Only `y` answers; every other key cancels. After
    /// `y` the ask stays up with `checking` set while the folder is resolved
    /// once more: only Esc answers then.
    Folder {
        from: String,
        to: PathBuf,
        note: Option<String>,
        checking: bool,
    },
}

/// Where a read leads, as the host found it on the real path (every link
/// followed) before it asked. A fact for the approval mode, never a verdict:
/// the read itself judges the path again when it runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reach {
    /// Inside the chat's root, and the path guard allows it.
    Root,
    /// Inside the chat's root, but the root is HOME or a folder above it
    /// (`/`): too broad to keep a read anywhere, so `run` never lets it go.
    Broad,
    /// The path guard refuses it (a sensitive, missing or hidden path): the
    /// read returns the refusal and opens nothing.
    Refused,
    /// Anything else: outside the root, or not judged (the ask of an edit, a
    /// search nobody judged).
    Other,
    /// A local read outside the root whose path cannot be shown as it would
    /// be opened ([`UNSHOWABLE`]): no prompt may approve it, so the ask's
    /// `path` is empty. `ask` and `auto` let it run as they always have.
    Unshowable,
}

/// Why a read whose path cannot be shown exactly as it would be opened is
/// refused: the user must never approve a path that differs from the one read.
pub const UNSHOWABLE: &str = "path cannot be shown safely";

impl ApprovalAsk {
    /// The ask of a read of `path` on a cloud engine, judged by nobody yet:
    /// every mode prompts for it.
    pub fn read(path: impl Into<String>) -> Self {
        Self::Read {
            path: path.into(),
            reach: Reach::Other,
            cloud: true,
        }
    }

    /// Whether a worker raised the ask (a read outside its scope, a batch it
    /// proposed): only those queue behind a pending approval.
    pub fn is_worker(&self) -> bool {
        match self {
            Self::WorkerRead { .. } => true,
            Self::Batch(review) => review.label().is_some(),
            _ => false,
        }
    }

    /// The worker whose session the ask belongs to, `None` for a chat's own.
    /// The answer, and the withdrawal when the worker ends, are addressed to
    /// that session and no other.
    pub fn worker(&self) -> Option<WorkerKey> {
        match self {
            Self::WorkerRead { worker, .. } => Some(*worker),
            Self::Batch(review) => review.worker(),
            _ => None,
        }
    }

    /// How long after it opens the keys start to answer this ask: the review
    /// of a batch waits longer than any other ask, because the user has to
    /// read it first.
    pub fn arm_delay(&self) -> Duration {
        match self {
            Self::Batch(_) | Self::Commit(_) => review::ARM_DELAY,
            _ => ASK_ARM_DELAY,
        }
    }
}

/// Which calls wait for the user's answer before they run (D17).
/// `run_in_pane` always does. `read_file`, `grep` and `edit_file` do on a cloud
/// engine, because the text they return goes to the provider, and run free on a
/// local one. The directory tools never ask. The mode of the chat (ask or auto-type, D18) is
/// not an input: the app applies it to the ask this returns.
///
/// The path of a read ask is shown as it is: the caller passes the target
/// resolved by `tools::paths::display_target`, which exists only when display
/// sanitizing would not change it. The host opens the file from the call.
/// A worker's read is not decided here: outside the run's scope the host asks
/// [`ApprovalAsk::WorkerRead`] where a chat read would ask.
/// The host also asks for a local read that leads outside the chat's root,
/// so that `run` can prompt for it (see [`answers_itself`]).
pub fn ask_for(call: &Call, locality: Locality) -> Option<ApprovalAsk> {
    match call {
        Call::RunInPane { command } => Some(ApprovalAsk::Run {
            command: command.clone(),
        }),
        Call::ReadFile { path, .. } if locality == Locality::Cloud => {
            Some(ApprovalAsk::read(path.clone()))
        }
        // An edit answers whether a text occurs in the file, so on a cloud
        // engine it asks like a read of that file, one no mode answers.
        Call::EditFile { path, .. } if locality == Locality::Cloud => {
            Some(ApprovalAsk::read(path.clone()))
        }
        // A command always asks, on any engine and in any approval mode.
        Call::RunCommand { .. } => {
            let (program, args, cwd) = call.command()?;
            Some(ApprovalAsk::Exec {
                program: program.to_owned(),
                args: args.into_iter().map(str::to_owned).collect(),
                cwd: cwd.unwrap_or(".").to_owned(),
                script: None,
                timeout_secs: u64::try_from(allow::TIMEOUT_DEFAULT).unwrap_or(120),
                cloud: locality == Locality::Cloud,
                earlier: None,
            })
        }
        // The diff holds file text, so it goes to a cloud provider only after
        // the user's `y`; on a local engine it runs free.
        Call::GitDiff { staged, path } if locality == Locality::Cloud => {
            let path = flat(path.as_deref().unwrap_or("."), SEARCH_SHOWN).0;
            Some(ApprovalAsk::GitDiff {
                staged: *staged,
                path: if path.is_empty() { ".".into() } else { path },
            })
        }
        // An add changes the index: it always asks, on any engine and mode.
        Call::GitAdd { paths } => Some(ApprovalAsk::GitAdd {
            paths: flat(&joined_paths(paths), GIT_ADD_SHOWN).0,
            files: None,
        }),
        // A user tool always asks, on any engine and in any approval mode:
        // lazarobox cannot know what it does, and a read-only hint is ignored.
        Call::External {
            server, tool, args, ..
        } => Some(ApprovalAsk::External {
            server: shown_name(server),
            tool: shown_name(tool),
            args: shown_args(args),
        }),
        Call::Grep { pattern, dir } if locality == Locality::Cloud => {
            let dir = flat(dir.as_deref().unwrap_or("."), SEARCH_SHOWN).0;
            Some(ApprovalAsk::Search {
                pattern: flat(pattern, SEARCH_SHOWN).0,
                // A blank folder is the root, as no folder is.
                dir: if dir.is_empty() { ".".into() } else { dir },
                reach: Reach::Other,
            })
        }
        // `glob` returns names only and, like the directory tools, never asks.
        // Staging touches no disk and asks nothing: the host builds the batch
        // ask itself when `propose_batch` seals the batch. An orchestration
        // call touches nothing of the user's: it never asks.
        Call::ReadFile { .. }
        | Call::FindDir { .. }
        | Call::ListDir { .. }
        | Call::Glob { .. }
        | Call::Grep { .. }
        | Call::CreateDir { .. }
        | Call::WriteFile { .. }
        | Call::EditFile { .. }
        | Call::ProposeBatch
        | Call::GitStatus
        | Call::GitDiff { .. }
        | Call::GitLog { .. }
        // The host reads the index and builds the commit review itself.
        | Call::GitCommit { .. }
        | Call::Orchestra(_) => None,
    }
}

/// The label of a worker's asks: `<kind> <engine>><model>`, the engine and
/// the model each one sanitized line, cut like a server name.
pub fn worker_label(kind: Task, engine: &str, model: &str) -> String {
    format!("{} {}>{}", kind.id(), shown_name(engine), shown_name(model))
}

/// The most characters of the paths of an add that its prompt shows. The host
/// refuses an add whose paths are longer, so the prompt never hides one.
pub const GIT_ADD_SHOWN: usize = 400;

/// The paths of an add as the prompt writes them: a path with whitespace is
/// quoted, so two paths never read as one.
fn joined_paths(paths: &[String]) -> String {
    let word = |p: &String| {
        if p.contains(char::is_whitespace) {
            format!("'{p}'")
        } else {
            p.clone()
        }
    };
    paths.iter().map(word).collect::<Vec<_>>().join(" ")
}

/// The paths of an add as the prompt will show them, or `None` when the line
/// would be cut or changed: the host refuses such an add before it asks,
/// because `y` must approve exactly the paths that are staged.
pub fn add_line(paths: &[String]) -> Option<String> {
    if paths.iter().any(|p| p.contains('\'')) {
        return None;
    }
    let joined = joined_paths(paths);
    let (line, cut) = flat(&joined, GIT_ADD_SHOWN);
    (cut == 0 && line == joined && !line.is_empty()).then_some(line)
}

/// The most characters of a search ask's pattern or folder that are shown.
const SEARCH_SHOWN: usize = 512;

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

/// What a command prompt must say before the user answers: the timeout, that
/// the network is on and a test can read `.env`, where the output goes, which
/// project files the program runs (tier B) and, for an install, that the
/// scripts of the dependencies run too. One line per fact.
pub fn exec_notice(
    program: &str,
    args: &[String],
    timeout_secs: u64,
    cloud: bool,
    earlier: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "timeout {timeout_secs} s · network is on · a test can read .env and send it out"
    )];
    lines.push(
        if cloud {
            "the output goes to the model and to its cloud provider: .env text or tokens in logs"
        } else {
            "the output goes to the model"
        }
        .to_owned(),
    );
    if let Some(files) = allow::runs_files(program) {
        lines.push(format!("runs project code: {files}"));
    }
    if allow::installs(program, args) {
        if allow::skips_scripts(args) {
            lines.push("install scripts are skipped".to_owned());
        } else {
            lines.push(
                "runs the install scripts of the project and, depending on the package \
                 manager, its dependencies"
                    .to_owned(),
            );
            lines.push("`--ignore-scripts` skips them".to_owned());
        }
    }
    if let Some(earlier) = earlier {
        lines.push(earlier.to_owned());
    }
    lines.join("\n")
}

/// The most characters of one script body an ask shows.
pub const SCRIPT_SHOWN: usize = 200;

/// `name: body` of a `package.json` script, as one sanitized line with the
/// body cut (visibly) at [`SCRIPT_SHOWN`] characters. The body is the
/// project's text, so it is cleaned like any text from outside.
pub fn script_line(name: &str, body: &str) -> String {
    format!("{}: {}", shown_name(name), flat(body, SCRIPT_SHOWN).0)
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
/// Why a command that leaves the chat's folder asks: a path it acts on, or
/// the pane's directory, is elsewhere. Asking is no fault of the model.
pub const OUTSIDE_ROOT: &str = "the command reaches outside the chat's folder";
/// Why every command of a cloud chat asks when its folder is HOME or `/`.
pub const BROAD_ROOT: &str = "the chat's folder is your home or /";
pub const PAUSED: &str = "auto-run paused for this turn";
pub const NON_UTF8: &str = "the pane's locale is not UTF-8";
/// Why a command that the grammar accepts still takes the prompt: the pane
/// or the turn is not free. None of them is the model's doing.
pub const RUNNING: &str = "a command is still running in the pane";
pub const HALF_TYPED: &str = "the pane has a half-typed line";
/// Why `y` in `run` mode types a command instead of running it: one Enter
/// cannot end a multi-line paste, and its output has no single echo row.
pub const MULTI_LINE: &str = "a multi-line command is typed, not run";
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

/// How a chat in `mode` answers a model's read or search ask itself, with
/// no prompt; `None` opens the prompt. A read or a search let go gets
/// [`Decision::Unprompted`], which the host never takes for the user's `y`.
///
/// In `run`: a search of a concrete root (its walk never leaves the root),
/// and a cloud read that stays in such a root or that the path guard refuses
/// (the read then returns the refusal). A read or a search under a root that
/// is HOME or `/` ([`Reach::Broad`]) prompts, and so does a read that leads
/// outside the root, on every engine; one whose path cannot be shown is
/// denied, as no prompt may approve it. In `ask` and `auto`: only a local
/// read outside the root, which runs free there as it always has; every
/// other ask prompts. A worker's read outside its run's scope is answered
/// exactly as the same chat read. A command, a user tool, a batch and the ask
/// of an edit are never answered here.
pub fn answers_itself(ask: &ApprovalAsk, mode: ApprovalMode) -> Option<Decision> {
    let (ApprovalAsk::Read { reach, cloud, .. } | ApprovalAsk::WorkerRead { reach, cloud, .. }) =
        ask
    else {
        let search = matches!(
            ask,
            ApprovalAsk::Search {
                reach: Reach::Root,
                ..
            }
        );
        return (search && mode == ApprovalMode::Run).then_some(Decision::Unprompted);
    };
    match (mode, reach, cloud) {
        (ApprovalMode::Run, Reach::Root | Reach::Refused, true) => Some(Decision::Unprompted),
        (ApprovalMode::Run, Reach::Unshowable, _) => Some(Decision::Denied(UNSHOWABLE)),
        (ApprovalMode::Run, _, _) => None,
        // The local engine asks only for a read outside the root.
        (ApprovalMode::Ask | ApprovalMode::Auto, Reach::Other | Reach::Unshowable, false) => {
            Some(Decision::Unprompted)
        }
        (ApprovalMode::Ask | ApprovalMode::Auto, _, _) => None,
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
    /// Facts the user must see before answering, one per line. Unlike `why`
    /// this is not advice: the prompt is answerable only while all of it is
    /// on screen.
    pub notice: Option<String>,
}

/// The keys of a run prompt.
pub const RUN_KEYS: &str = "y type it (no Enter) · n cancel · p next pane";
/// The keys of a model's run prompt in `run` mode, where `y` runs it.
pub const RUN_NOW_KEYS: &str = "y run it · n cancel · p next pane";
/// The keys of a read prompt.
pub const READ_KEYS: &str = "y read it · n deny";
/// The keys of a search prompt.
pub const SEARCH_KEYS: &str = "y search · n deny";
/// The keys of the folder prompt: nothing but `y` changes it.
pub const FOLDER_KEYS: &str = "y change folder · any other key cancels";
/// The keys of the folder prompt while the folder is checked after `y`.
pub const FOLDER_CHECKING_KEYS: &str = "Esc cancels";
/// The keys of a user tool prompt: no `p`, it has no target pane.
pub const EXTERNAL_KEYS: &str = "y call it · n deny";

/// The pending approval of a chat, if any, and the worker asks queued behind
/// it (first in, first shown).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Approvals {
    pending: Option<Approval>,
    /// Worker asks that arrived while another was shown, oldest first.
    queue: VecDeque<Approval>,
    /// A queued ask just became the pending one and the app has not armed it.
    promoted: bool,
    /// The review view of a pending [`ApprovalAsk::Batch`]. It goes with the
    /// approval, however that ends.
    review: Option<ReviewView>,
}

impl Approvals {
    /// The approval waiting for an answer.
    pub fn pending(&self) -> Option<&Approval> {
        self.pending.as_ref()
    }

    /// The worker asks waiting behind the pending one, oldest first.
    pub fn queued(&self) -> impl Iterator<Item = &Approval> {
        self.queue.iter()
    }

    /// How many worker asks wait behind the pending one.
    pub fn waiting(&self) -> usize {
        self.queue.len()
    }

    /// Whether the pending approval was a queued one, promoted by an answer
    /// or a withdrawal since the last call. The app then arms it afresh.
    pub fn take_promoted(&mut self) -> bool {
        std::mem::take(&mut self.promoted)
    }

    /// Ask the user and return the id of the new approval. Refused with
    /// [`ANOTHER_PENDING`] when one is pending; the first stays pending. A
    /// worker's ask (see [`ApprovalAsk::is_worker`]) queues instead, up to
    /// [`MAX_QUEUED`].
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
        // Only a tool call (it has a call) queues: a review the user opened
        // is never a worker's ask, whatever its label.
        let worker = call.is_some() && ask.is_worker();
        if self.pending.is_some() {
            if !worker {
                return Err(Refusal::new(ANOTHER_PENDING));
            }
            if self.queue.len() >= MAX_QUEUED {
                return Err(Refusal::new(QUEUE_FULL));
            }
        }
        let id = next_id();
        let approval = Approval {
            id,
            ask,
            origin,
            call,
            why,
        };
        if self.pending.is_some() {
            self.queue.push_back(approval);
        } else {
            self.review = None;
            self.pending = Some(approval);
        }
        Ok(id)
    }

    /// The pending approval ended: the oldest queued ask takes its place.
    fn promote(&mut self) {
        self.review = None;
        self.pending = self.queue.pop_front();
        self.promoted = self.pending.is_some();
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
        let approval = self.pending.take();
        self.promote();
        approval.map(|approval| (approval, reply))
    }

    /// End approval `id` without an answer (cancel, close, a target that is
    /// gone). An id that is not the pending one changes nothing. The caller
    /// records the result as denied.
    pub fn withdraw(&mut self, id: u64) -> Option<Approval> {
        if self.pending.as_ref()?.id != id {
            let at = self.queue.iter().position(|queued| queued.id == id)?;
            return self.queue.remove(at);
        }
        let approval = self.pending.take();
        self.promote();
        approval
    }

    /// The review view of the pending batch, if it is one.
    pub fn review(&self) -> Option<&ReviewView> {
        self.review.as_ref()
    }

    pub fn review_mut(&mut self) -> Option<&mut ReviewView> {
        self.review.as_mut()
    }

    /// The pane is `size` now: the rows are laid out again, none counts as
    /// seen until a frame draws it.
    pub fn fit_review(&mut self, size: (usize, usize)) {
        if let Some(view) = self.review.as_mut() {
            view.fit(size);
        }
    }

    /// Attaches the view to the pending approval.
    pub fn set_review(&mut self, view: ReviewView) {
        if self.pending.is_some() {
            self.review = Some(view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A typed command's line, with or without why `y` did not run it, is an
    // outcome: shown, never stored or sent.
    #[test]
    fn a_sent_line_with_the_reason_is_still_a_run_outcome() {
        let plain = run_sent_line(1, None);
        let why = run_sent_line(1, Some(NO_CWD));
        assert_eq!(plain, "sent to pane 1; press Enter there to run it");
        assert_eq!(
            why,
            "sent to pane 1; press Enter there to run it (not run: the pane's directory cannot be read on this system)"
        );
        assert!(is_run_outcome(&plain) && is_run_outcome(&why));
        assert!(!is_run_outcome("sent to pane 1 (not run: x)"));
    }

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

    // Spec (agent-orchestrator, Outside read asks with a label): the label
    // is the kind and `engine>model`, the engine and the model each one
    // sanitized line cut like a server name.
    #[test]
    fn a_worker_label_is_the_kind_and_engine_model_on_one_clipped_line() {
        assert_eq!(
            worker_label(Task::Review, "claude", "opus"),
            "review claude>opus"
        );
        let model = format!("qwen\n\x1b[31m{}", "m".repeat(100));
        let label = worker_label(Task::Code, "ollama", &model);
        assert!(label.starts_with("code ollama>qwen "), "{label:?}");
        assert!(
            !label.contains('\n') && !label.contains('\x1b'),
            "{label:?}"
        );
        assert!(label.ends_with('\u{2026}'), "{label:?}");
        assert_eq!(label.chars().count(), "code ollama>".len() + SHOWN_NAME + 1);
    }

    // Spec (agent-orchestrator, Outside read follows the mode as a chat read):
    // a worker's read ask is never typed or run: where the mode does not
    // answer it, it prompts, as the same chat read does.
    #[test]
    fn a_worker_read_routes_as_the_same_chat_read_in_every_mode() {
        for (reach, cloud) in [
            (Reach::Other, true),
            (Reach::Root, true),
            (Reach::Other, false),
        ] {
            let worker = ApprovalAsk::WorkerRead {
                worker: crate::core::ai::orchestra::WorkerKey::for_test(1, 1),
                path: "~/a".into(),
                label: "review claude>opus".into(),
                reach,
                cloud,
            };
            let chat = ApprovalAsk::Read {
                path: "~/a".into(),
                reach,
                cloud,
            };
            for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
                let got = route(&worker, Origin::Tool, mode, FREE);
                assert_eq!(got, route(&chat, Origin::Tool, mode, FREE), "{mode:?}");
                assert_eq!(got, Route::Prompt(None), "{mode:?}");
            }
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
        let read = ApprovalAsk::read("~/a");
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
        for ask in ["cd x", "ls", "pwd", "mkdir -p a", "rg foo", "git status"] {
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
        let ask = ask_of("cargo build");
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
        let read = ApprovalAsk::read("~/a");
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
        let off = ask_of("cargo build");
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
        assert_eq!(line("cargo build"), "not a safe command: `cargo`");
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
        assert!(counted("cargo build"));
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
            Some(ApprovalAsk::read("~/a.txt"))
        );
    }

    // Spec (code-write, edit_file staging): on a cloud engine an edit asks
    // like a read of the same file, because its answers say what the file
    // holds; on a local one it never asks. It routes like a read in every mode.
    #[test]
    fn a_cloud_edit_asks_like_a_read_and_routes_like_one() {
        let edit = Call::EditFile {
            path: "~/a.txt".into(),
            old: "SECRET-OLD".into(),
            new: "SECRET-NEW".into(),
        };
        let read = Call::ReadFile {
            path: "~/a.txt".into(),
            lines: None,
        };
        assert_eq!(ask_for(&edit, Locality::Local), None);
        let ask = ask_for(&edit, Locality::Cloud).expect("a cloud edit asks");
        assert_eq!(ask, ask_for(&read, Locality::Cloud).unwrap());
        assert!(!format!("{ask:?}").contains("SECRET"));
        for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
            for origin in [Origin::Tool, Origin::Block] {
                let read_ask = ask_for(&read, Locality::Cloud).unwrap();
                assert_eq!(
                    route(&ask, origin, mode, FREE),
                    route(&read_ask, origin, mode, FREE),
                    "{mode:?}"
                );
            }
        }
    }

    fn search_ask(pattern: &str, dir: &str) -> ApprovalAsk {
        ApprovalAsk::Search {
            pattern: pattern.into(),
            dir: dir.into(),
            reach: Reach::Other,
        }
    }

    // The whole table of what a mode answers itself: every mode, every read
    // reach on both kinds of engine, every search reach, and the asks no mode
    // answers. `Some(None)` is a prompt.
    #[test]
    fn answers_itself_follows_its_table_for_every_mode_ask_and_reach() {
        use ApprovalMode::{Ask, Auto, Run};
        use Reach::{Broad, Other, Refused, Root, Unshowable};
        let free = Some(Decision::Unprompted);
        let read = |reach, cloud| ApprovalAsk::Read {
            path: "~/a".into(),
            reach,
            cloud,
        };
        let search = |reach| ApprovalAsk::Search {
            pattern: "x".into(),
            dir: ".".into(),
            reach,
        };
        let unshowable = Some(Decision::Denied(UNSHOWABLE));
        #[rustfmt::skip]
        let reads = [
            // (reach, cloud, run, ask and auto)
            (Root, true, free, None),
            (Refused, true, free, None),
            (Broad, true, None, None),
            (Other, true, None, None),
            (Unshowable, true, unshowable, None),
            (Root, false, None, None),
            (Refused, false, None, None),
            (Broad, false, None, None),
            (Other, false, None, free),
            (Unshowable, false, unshowable, free),
        ];
        // A worker's read outside its run's scope follows the same table
        // (agent-orchestrator, Outside read follows the mode as a chat read).
        let worker = |reach, cloud| ApprovalAsk::WorkerRead {
            worker: crate::core::ai::orchestra::WorkerKey::for_test(1, 1),
            path: "~/a".into(),
            label: "review claude>opus".into(),
            reach,
            cloud,
        };
        for (reach, cloud, run, others) in reads {
            for ask in [read(reach, cloud), worker(reach, cloud)] {
                assert_eq!(answers_itself(&ask, Run), run, "{ask:?}");
                for mode in [Ask, Auto] {
                    assert_eq!(answers_itself(&ask, mode), others, "{mode:?} {ask:?}");
                }
            }
        }
        for (reach, run) in [(Root, free), (Broad, None), (Other, None), (Refused, None)] {
            assert_eq!(answers_itself(&search(reach), Run), run, "{reach:?}");
            for mode in [Ask, Auto] {
                assert_eq!(answers_itself(&search(reach), mode), None, "{mode:?}");
            }
        }
        let never = [
            run("ls"),
            ApprovalAsk::External {
                server: "s".into(),
                tool: "t".into(),
                args: "{}".into(),
            },
            ApprovalAsk::read("~/a"),
        ];
        for ask in never {
            for mode in [Ask, Auto, Run] {
                assert_eq!(answers_itself(&ask, mode), None, "{mode:?} {ask:?}");
            }
        }
    }

    // Spec (agent-tools, approval model): grep asks on Cloud, runs free on Local.
    #[test]
    fn grep_asks_on_cloud_with_pattern_and_path_and_runs_free_on_local() {
        let whole = Call::Grep {
            pattern: "fn main".into(),
            dir: None,
        };
        let narrowed = Call::Grep {
            pattern: "TODO".into(),
            dir: Some("src/core".into()),
        };
        assert_eq!(ask_for(&whole, Locality::Local), None);
        assert_eq!(ask_for(&narrowed, Locality::Local), None);
        assert_eq!(
            ask_for(&whole, Locality::Cloud),
            Some(search_ask("fn main", "."))
        );
        assert_eq!(
            ask_for(&narrowed, Locality::Cloud),
            Some(search_ask("TODO", "src/core"))
        );
    }

    // The model chooses both strings: the label shows them as one clean line.
    #[test]
    fn a_search_ask_shows_clean_one_line_text() {
        let call = Call::Grep {
            pattern: "a\u{1b}[31mb\nc".into(),
            dir: Some("x\ny".into()),
        };
        let Some(ApprovalAsk::Search { pattern, dir, .. }) = ask_for(&call, Locality::Cloud) else {
            panic!("a cloud grep must ask");
        };
        assert_eq!((pattern.as_str(), dir.as_str()), ("ab c", "x y"));
    }

    // An empty folder is the root, as `None` is: the label never shows nothing.
    #[test]
    fn an_empty_search_folder_is_shown_as_the_root() {
        for dir in [None, Some(""), Some("  ")] {
            let call = Call::Grep {
                pattern: "x".into(),
                dir: dir.map(Into::into),
            };
            assert_eq!(ask_for(&call, Locality::Cloud), Some(search_ask("x", ".")));
        }
    }

    // Spec (agent-tools): no auto approval; a search ask is `Prompt` in every mode.
    #[test]
    fn a_search_ask_prompts_in_every_mode() {
        let ask = search_ask("x", ".");
        for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
            for origin in [Origin::Tool, Origin::Block] {
                assert_eq!(route(&ask, origin, mode, FREE), Route::Prompt(None));
            }
        }
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
            .request(ApprovalAsk::read("/x"), Origin::Tool)
            .unwrap_err();
        assert_eq!(err.reason(), "another approval is pending");
        assert_eq!(a.pending().unwrap().ask(), &run("one"));
    }

    fn worker_read(path: &str, label: &str) -> ApprovalAsk {
        ApprovalAsk::WorkerRead {
            worker: crate::core::ai::orchestra::WorkerKey::for_test(1, 1),
            path: path.into(),
            label: label.into(),
            reach: Reach::Other,
            cloud: false,
        }
    }

    // Spec (agent-tools, Approval queue for workers; FIFO order): asks A then
    // B from different workers are shown A first, then B, one at a time.
    #[test]
    fn worker_asks_queue_in_arrival_order_and_show_one_at_a_time() {
        let mut a = Approvals::default();
        let first = a
            .request_call(worker_read("a", "review claude>opus"), 1, CallId(1), None)
            .unwrap();
        let second = a
            .request_call(worker_read("b", "general codex>x"), 1, CallId(2), None)
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(a.pending().unwrap().id(), first);
        assert_eq!(a.waiting(), 1);
        let (done, _) = a.answer(first, Reply::Approve, Press::Press).unwrap();
        assert_eq!(done.call(), Some((1, CallId(1))));
        assert_eq!(a.pending().unwrap().id(), second);
        assert_eq!(a.waiting(), 0);
        assert!(a.take_promoted());
        assert!(!a.take_promoted(), "reported once");
        a.answer(second, Reply::Decline, Press::Press).unwrap();
        assert!(a.pending().is_none() && !a.take_promoted());
    }

    // Spec (chat-pane, Approval label and queue): each answer applies to its
    // own prompt only; an id that is not the shown one changes nothing.
    #[test]
    fn an_answer_applies_to_its_own_prompt_only() {
        let mut a = Approvals::default();
        a.request_call(worker_read("a", "w"), 1, CallId(1), None)
            .unwrap();
        let second = a
            .request_call(worker_read("b", "w"), 1, CallId(2), None)
            .unwrap();
        assert!(a.answer(second, Reply::Approve, Press::Press).is_none());
        assert_eq!(a.waiting(), 1);
    }

    // Spec (agent-tools): cancelling a session denies its pending and queued
    // approvals. A queued ask can be withdrawn without touching the shown one.
    #[test]
    fn a_queued_ask_can_be_withdrawn_and_the_shown_one_stays() {
        let mut a = Approvals::default();
        let first = a
            .request_call(worker_read("a", "w"), 1, CallId(1), None)
            .unwrap();
        let second = a
            .request_call(worker_read("b", "w"), 1, CallId(2), None)
            .unwrap();
        let withdrawn = a.withdraw(second).unwrap();
        assert_eq!(withdrawn.call(), Some((1, CallId(2))));
        assert_eq!(a.pending().unwrap().id(), first);
        assert_eq!(a.waiting(), 0);
        assert!(!a.take_promoted());
    }

    // A chat's own asks keep the one-pending rule; only a worker's queue.
    #[test]
    fn a_chat_ask_is_still_refused_beside_a_pending_worker_ask() {
        let mut a = Approvals::default();
        a.request_call(worker_read("a", "w"), 1, CallId(1), None)
            .unwrap();
        let err = a
            .request_call(ApprovalAsk::read("/x"), 1, CallId(2), None)
            .unwrap_err();
        assert_eq!(err.reason(), ANOTHER_PENDING);
        assert_eq!(a.waiting(), 0);
    }

    // The queue is bounded: a flood is refused, never grown without limit.
    #[test]
    fn the_worker_queue_is_bounded() {
        let mut a = Approvals::default();
        for n in 0..=MAX_QUEUED as u64 {
            a.request_call(worker_read("a", "w"), 1, CallId(n), None)
                .unwrap();
        }
        let err = a
            .request_call(worker_read("z", "w"), 1, CallId(999), None)
            .unwrap_err();
        assert_eq!(err.reason(), QUEUE_FULL);
        assert_eq!(a.waiting(), MAX_QUEUED);
    }

    #[test]
    fn all_three_asks_can_be_pending() {
        for ask in [
            run("ls"),
            ApprovalAsk::read("/x"),
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

    // Spec (command-runner, Approval on every command): a run always asks, on
    // either locality, and shows the program, the arguments and the folder.
    #[test]
    fn a_run_command_call_always_asks_with_program_arguments_and_folder() {
        let call = |cwd: Option<&str>| {
            let mut args = serde_json::json!({"program": "cargo", "args": ["test", "-p", "x y"]});
            if let Some(cwd) = cwd {
                args["cwd"] = cwd.into();
            }
            Call::RunCommand { args }
        };
        for locality in [Locality::Local, Locality::Cloud] {
            for (cwd, shown) in [(None, "."), (Some("crates/x"), "crates/x")] {
                assert_eq!(
                    ask_for(&call(cwd), locality),
                    Some(ApprovalAsk::Exec {
                        program: "cargo".into(),
                        args: vec!["test".into(), "-p".into(), "x y".into()],
                        cwd: shown.into(),
                        script: None,
                        timeout_secs: 120,
                        cloud: locality == Locality::Cloud,
                        earlier: None,
                    })
                );
            }
        }
    }

    // Spec (git-tools, Read verbs): `git_status` and `git_log` run free;
    // `git_diff` shows file text, so it asks on a cloud engine only.
    #[test]
    fn git_diff_asks_on_a_cloud_engine_and_the_other_read_verbs_never_do() {
        let diff = Call::GitDiff {
            staged: true,
            path: Some("src/a.rs".into()),
        };
        assert_eq!(ask_for(&diff, Locality::Local), None);
        assert_eq!(
            ask_for(&diff, Locality::Cloud),
            Some(ApprovalAsk::GitDiff {
                staged: true,
                path: "src/a.rs".into()
            })
        );
        let whole = Call::GitDiff {
            staged: false,
            path: None,
        };
        assert_eq!(
            ask_for(&whole, Locality::Cloud),
            Some(ApprovalAsk::GitDiff {
                staged: false,
                path: ".".into()
            })
        );
        let log = Call::GitLog {
            max_count: None,
            path: None,
        };
        for locality in [Locality::Local, Locality::Cloud] {
            assert_eq!(ask_for(&Call::GitStatus, locality), None);
            assert_eq!(ask_for(&log, locality), None);
        }
    }

    // Spec (git-tools, add): every engine asks for an add, with the paths as
    // one line; a path with a space is quoted so two never read as one.
    #[test]
    fn an_add_asks_on_every_engine_with_the_paths_on_one_line() {
        let add = Call::GitAdd {
            paths: vec!["src/a.rs".into(), "my notes.md".into()],
        };
        for locality in [Locality::Local, Locality::Cloud] {
            assert_eq!(
                ask_for(&add, locality),
                Some(ApprovalAsk::GitAdd {
                    paths: "src/a.rs 'my notes.md'".into(),
                    files: None
                })
            );
        }
    }

    #[test]
    fn the_paths_of_an_add_that_would_not_show_whole_have_no_line() {
        let paths = |p: &[&str]| p.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            add_line(&paths(&["a.rs", "b.rs"])).as_deref(),
            Some("a.rs b.rs")
        );
        assert_eq!(add_line(&paths(&[&"a".repeat(GIT_ADD_SHOWN + 1)])), None);
        assert_eq!(
            add_line(&paths(&["a\nb"])),
            None,
            "a newline is not one line"
        );
        assert_eq!(
            add_line(&paths(&["a  b"])),
            None,
            "spaces are not collapsed"
        );
        assert_eq!(add_line(&[]), None);
        assert_eq!(
            add_line(&paths(&["'a", "b'"])),
            None,
            "a quote makes the quoting ambiguous"
        );
    }

    // The commit asks through the review the host builds; `ask_for` alone
    // gives no ask, which the host never takes for a free run (it always
    // fills one in or refuses).
    #[test]
    fn a_commit_has_no_ask_of_its_own_in_ask_for() {
        let commit = Call::GitCommit {
            message: "m".into(),
        };
        assert_eq!(ask_for(&commit, Locality::Cloud), None);
    }

    #[test]
    fn a_git_diff_path_is_shown_as_one_clean_line() {
        let call = Call::GitDiff {
            staged: false,
            path: Some("a\n\u{202e}b".into()),
        };
        let Some(ApprovalAsk::GitDiff { path, .. }) = ask_for(&call, Locality::Cloud) else {
            panic!("no ask");
        };
        assert!(
            path.chars().all(|c| !c.is_control() && c != '\u{202e}'),
            "{path:?}"
        );
    }

    #[test]
    fn a_git_diff_ask_prompts_in_every_mode_and_origin() {
        let ask = ApprovalAsk::GitDiff {
            staged: false,
            path: ".".into(),
        };
        for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
            for origin in [Origin::Tool, Origin::Block] {
                assert_eq!(route(&ask, origin, mode, FREE), Route::Prompt(None));
            }
            assert_eq!(answers_itself(&ask, mode), None);
        }
    }

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    // Spec (command-runner, Approval on every command): the prompt says that
    // network is on, that a test can read `.env`, and that the output goes to
    // the model.
    #[test]
    fn the_notice_names_the_timeout_the_network_the_env_file_and_the_model() {
        let notice = exec_notice("cargo", &words(&["test"]), 45, false, None);
        assert!(notice.contains("timeout 45 s"), "{notice}");
        assert!(notice.contains("network is on"), "{notice}");
        assert!(notice.contains(".env"), "{notice}");
        assert!(notice.contains("output goes to the model"), "{notice}");
        assert!(!notice.contains("provider"), "{notice}");
    }

    #[test]
    fn on_a_cloud_engine_the_notice_says_the_provider_sees_the_output() {
        let notice = exec_notice("cargo", &words(&["test"]), 120, true, None);
        assert!(notice.contains("provider"), "{notice}");
        assert!(notice.contains("tokens in logs"), "{notice}");
    }

    // Tier B (design D12): the files a command runs are named.
    #[test]
    fn the_notice_warns_about_the_build_files_each_program_runs() {
        for (program, file) in [
            ("cargo", "build.rs"),
            ("npm", "package.json"),
            ("yarn", "package.json"),
            ("pnpm", "package.json"),
            ("pytest", "conftest.py"),
            ("go", "go.mod"),
        ] {
            let notice = exec_notice(program, &words(&["test"]), 120, false, None);
            assert!(notice.contains(file), "{program}: {notice}");
        }
    }

    #[test]
    fn an_install_says_it_runs_the_scripts_and_maybe_those_of_the_dependencies() {
        for verb in ["install", "ci"] {
            let notice = exec_notice("npm", &words(&[verb]), 120, false, None);
            assert!(
                notice.contains(
                    "runs the install scripts of the project and, depending on the package \
                     manager, its dependencies"
                ),
                "{notice}"
            );
            assert!(notice.contains("`--ignore-scripts` skips them"), "{notice}");
        }
        let test = exec_notice("npm", &words(&["test"]), 120, false, None);
        assert!(!test.contains("install scripts"), "{test}");
    }

    #[test]
    fn an_install_with_ignore_scripts_says_they_are_skipped() {
        let notice = exec_notice(
            "pnpm",
            &words(&["install", "--ignore-scripts"]),
            120,
            false,
            None,
        );
        assert!(notice.contains("install scripts are skipped"), "{notice}");
        assert!(!notice.contains("dependencies"), "{notice}");
        let again = exec_notice(
            "pnpm",
            &words(&["install", "--ignore-scripts", "--no-ignore-scripts"]),
            120,
            false,
            None,
        );
        assert!(!again.contains("are skipped"), "{again}");
    }

    #[test]
    fn the_notice_names_the_config_file_each_manager_reads() {
        for (program, file, other) in [
            ("npm", ".npmrc", ".yarnrc.yml"),
            ("pnpm", ".npmrc", ".yarnrc.yml"),
            ("yarn", ".yarnrc.yml", ".npmrc"),
        ] {
            let notice = exec_notice(program, &words(&["test"]), 120, false, None);
            assert!(notice.contains(file), "{program}: {notice}");
            assert!(!notice.contains(other), "{program}: {notice}");
        }
    }

    #[test]
    fn an_exec_ask_prompts_in_every_mode_and_origin() {
        let ask = ApprovalAsk::Exec {
            program: "cargo".into(),
            args: words(&["test"]),
            cwd: ".".into(),
            script: None,
            timeout_secs: 120,
            cloud: false,
            earlier: None,
        };
        for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
            for origin in [Origin::Tool, Origin::Block] {
                assert_eq!(route(&ask, origin, mode, FREE), Route::Prompt(None));
            }
        }
    }

    #[test]
    fn an_earlier_cancelled_run_that_changed_git_files_is_told() {
        let notice = exec_notice(
            "go",
            &words(&["test"]),
            120,
            false,
            Some("changed .git/config"),
        );
        assert!(notice.contains("changed .git/config"), "{notice}");
    }

    #[test]
    fn a_script_line_is_one_clean_line_cut_visibly() {
        assert_eq!(script_line("test", "jest\n  --ci"), "test: jest --ci");
        let hostile = script_line("t\u{1b}[31m", "a\u{1b}[2Jb\u{202e}c");
        assert!(hostile.chars().all(|c| !c.is_control()), "{hostile:?}");
        assert!(!hostile.contains('\u{202e}'), "{hostile:?}");
        let long = script_line("build", &"x".repeat(500));
        assert!(long.ends_with('\u{2026}'), "{long}");
        assert_eq!(long.chars().count(), "build: ".len() + SCRIPT_SHOWN + 1);
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
