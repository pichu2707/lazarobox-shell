//! The approval of one chat: what the user is asked to allow and the pure
//! transitions around it.
//!
//! A chat has at most one pending approval. Only an explicit press answers
//! it; a key repeat or release, a timeout or anything else never does. This
//! module holds no IO and does not know which key means what: the caller
//! maps keys to a [`Reply`].

use std::sync::atomic::{AtomicU64, Ordering};

use crate::core::ai::{
    chat::engine::CallId,
    config::ApprovalMode,
    tools::{Call, Locality, command::Refusal},
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
        Call::ReadFile { path } if locality == Locality::Cloud => {
            Some(ApprovalAsk::Read { path: path.clone() })
        }
        Call::ReadFile { .. } | Call::FindDir { .. } | Call::ListDir { .. } => None,
    }
}

/// What the app does with a model's ask (D18).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// Open the approval overlay, as in `ask` mode.
    Prompt,
    /// Type the command into the target pane without a prompt, no Enter.
    AutoType,
}

/// The most commands one turn may type without a prompt.
pub const MAX_AUTO_PER_TURN: u8 = 5;

/// What the app knows when a call arrives, beyond the call itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AutoCtx {
    /// Commands already typed without a prompt in this turn.
    pub typed_this_turn: u8,
    /// An earlier auto-typed command in the target pane is still unsent.
    pub line_pending: bool,
    /// The target pane has the focus: the user may be typing in it.
    pub target_focused: bool,
    /// The chat's tab is the shown one: the user can see the typing.
    pub tab_shown: bool,
}

/// `AutoType` only for a model `run_in_pane` in `auto` mode, with fewer than
/// [`MAX_AUTO_PER_TURN`] typed this turn, no pending line in the target, the
/// target not focused and the tab shown. Every other case, a read, an
/// external call or a command the user started, prompts.
pub fn route(ask: &ApprovalAsk, origin: Origin, mode: ApprovalMode, ctx: AutoCtx) -> Route {
    let free = ctx.typed_this_turn < MAX_AUTO_PER_TURN
        && !ctx.line_pending
        && !ctx.target_focused
        && ctx.tab_shown;
    match (ask, origin, mode) {
        (ApprovalAsk::Run { .. }, Origin::Tool, ApprovalMode::Auto) if free => Route::AutoType,
        _ => Route::Prompt,
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
    /// The exact action, as it will be done: the command to type, or
    /// `read <path>`.
    pub command: String,
    /// The keys line.
    pub keys: &'static str,
    /// Whether the keys answer yet. A prompt the model opened ignores them for
    /// a moment, so a key typed before it appeared cannot answer it; the keys
    /// line shows `…` meanwhile.
    pub armed: bool,
}

/// The keys of a run prompt.
pub const RUN_KEYS: &str = "y type it (no Enter) · n cancel · p next pane";
/// The keys of a read prompt.
pub const READ_KEYS: &str = "y read it · n deny";

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
        self.open(ask, origin, None)
    }

    /// Like [`request`](Self::request) for the tool call `call` of turn
    /// `turn`: the origin is [`Origin::Tool`], and the answer is reported
    /// under that turn and call.
    pub fn request_call(
        &mut self,
        ask: ApprovalAsk,
        turn: u64,
        call: CallId,
    ) -> Result<u64, Refusal> {
        self.open(ask, Origin::Tool, Some((turn, call)))
    }

    fn open(
        &mut self,
        ask: ApprovalAsk,
        origin: Origin,
        call: Option<(u64, CallId)>,
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
    };

    fn run_ask() -> ApprovalAsk {
        ApprovalAsk::Run {
            command: "cd x".into(),
        }
    }

    // Spec (chat-pane, approval mode): D18 routing.
    #[test]
    fn a_model_run_in_auto_mode_is_typed_when_nothing_blocks_it() {
        let route = route(&run_ask(), Origin::Tool, ApprovalMode::Auto, FREE);
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
            Route::Prompt
        );
        for ask in [&read, &external] {
            assert_eq!(
                route(ask, Origin::Tool, ApprovalMode::Auto, FREE),
                Route::Prompt
            );
        }
        assert_eq!(
            route(&run_ask(), Origin::Block, ApprovalMode::Auto, FREE),
            Route::Prompt
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
                Route::Prompt,
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

    // Spec (agent-tools, approval by locality): D17 table.
    #[test]
    fn which_calls_ask_depends_on_the_tool_and_the_locality() {
        let find = Call::FindDir { query: "x".into() };
        let list = Call::ListDir { path: "~".into() };
        let read = Call::ReadFile {
            path: "~/a.txt".into(),
        };
        let run = Call::RunInPane {
            command: "cd x".into(),
        };
        for locality in [Locality::Local, Locality::Cloud] {
            assert_eq!(ask_for(&find, locality), None);
            assert_eq!(ask_for(&list, locality), None);
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
        a.request_call(run("ls"), 4, CallId(9)).unwrap();
        let p = a.pending().unwrap();
        assert_eq!((p.origin(), p.call()), (Origin::Tool, Some((4, CallId(9)))));
        let err = a.request_call(run("pwd"), 4, CallId(10)).unwrap_err();
        assert_eq!(err.reason(), ANOTHER_PENDING);
        assert_eq!(a.pending().unwrap().call(), Some((4, CallId(9))));
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
}
