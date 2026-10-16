//! The approval of one chat: what the user is asked to allow and the pure
//! transitions around it.
//!
//! A chat has at most one pending approval. Only an explicit press answers
//! it; a key repeat or release, a timeout or anything else never does. This
//! module holds no IO and does not know which key means what: the caller
//! maps keys to a [`Reply`].

use std::sync::atomic::{AtomicU64, Ordering};

use crate::core::ai::tools::command::Refusal;

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
    /// The exact command, as it will be written.
    pub command: String,
}

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
        if self.pending.is_some() {
            return Err(Refusal::new(ANOTHER_PENDING));
        }
        let id = next_id();
        self.pending = Some(Approval { id, ask, origin });
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
