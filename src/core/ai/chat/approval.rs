//! The approval of one chat: what the user is asked to allow and the pure
//! transitions around it.
//!
//! A chat has at most one pending approval. Only an explicit press answers
//! it; a key repeat or release, a timeout or anything else never does. This
//! module holds no IO and does not know which key means what: the caller
//! maps keys to a [`Reply`].

use crate::core::ai::tools::command::Refusal;

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
    ask: ApprovalAsk,
    origin: Origin,
}

impl Approval {
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

    /// Ask the user. Refused with [`ANOTHER_PENDING`] when one is pending;
    /// the first stays pending.
    pub fn request(&mut self, ask: ApprovalAsk, origin: Origin) -> Result<(), Refusal> {
        if self.pending.is_some() {
            return Err(Refusal::new(ANOTHER_PENDING));
        }
        self.pending = Some(Approval { ask, origin });
        Ok(())
    }

    /// Answer the pending approval. Only a [`Press::Press`] answers; a repeat
    /// or a release leaves it pending. Returns the answered approval.
    pub fn answer(&mut self, reply: Reply, press: Press) -> Option<(Approval, Reply)> {
        if press != Press::Press {
            return None;
        }
        self.pending.take().map(|approval| (approval, reply))
    }

    /// End the pending approval without an answer (cancel, close, a target
    /// that is gone). The caller records it as denied.
    pub fn withdraw(&mut self) -> Option<Approval> {
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
            a.request(run("ls"), Origin::Block).unwrap();
            let (done, got) = a.answer(reply, Press::Press).unwrap();
            assert_eq!(done.ask(), &run("ls"));
            assert_eq!(got, reply);
            assert!(a.pending().is_none());
        }
    }

    #[test]
    fn repeat_and_release_never_answer() {
        let mut a = Approvals::default();
        a.request(run("ls"), Origin::Block).unwrap();
        for press in [Press::Repeat, Press::Release] {
            assert_eq!(a.answer(Reply::Approve, press), None);
            assert!(a.pending().is_some());
        }
    }

    #[test]
    fn nothing_pending_nothing_answered() {
        let mut a = Approvals::default();
        assert_eq!(a.answer(Reply::Approve, Press::Press), None);
        assert_eq!(a.withdraw(), None);
    }

    #[test]
    fn withdraw_ends_it_and_allows_a_new_request() {
        let mut a = Approvals::default();
        a.request(run("ls"), Origin::Block).unwrap();
        assert_eq!(a.withdraw().unwrap().ask(), &run("ls"));
        assert!(a.pending().is_none());
        a.request(run("pwd"), Origin::Tool).unwrap();
        assert_eq!(a.pending().unwrap().ask(), &run("pwd"));
    }
}
