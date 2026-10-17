//! The approver of a chat: a call that needs the user waits here until the app
//! reports the user's answer.
//!
//! Every wait is keyed by (pane, turn, call). The app answers with
//! `Effect::ToolDecision`, which the runtime turns into [`Waits::decide`]. A
//! wait ends in exactly one of three ways: the answer arrives, the future is
//! dropped (the call was cancelled or its session closed), or the chat or turn
//! is ended with [`Waits::end_pane`] / [`Waits::end_turn`], which deny. There is
//! no timer here: a timer never approves, and a turn that runs out of time ends
//! its waits through `end_turn`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use futures::{FutureExt, future::BoxFuture};
use tokio::sync::oneshot;

use super::{Approver, Pending, pending::Waiting};
use crate::core::{
    ai::chat::{
        approval::ApprovalAsk,
        engine::{CallId, Decision},
    },
    layout::PaneId,
};

/// What a call is told when its chat or turn ended before the user answered.
pub const ENDED: &str = "the chat ended before the user answered";

type Key = (PaneId, u64, CallId);

/// The calls waiting for the user's answer, for every chat of the app.
#[derive(Default)]
pub struct Waits {
    waiting: Mutex<HashMap<Key, oneshot::Sender<Decision>>>,
}

impl Waits {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn map(&self) -> MutexGuard<'_, HashMap<Key, oneshot::Sender<Decision>>> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The approver of turn `turn` of `pane`. Every call it holds counts in
    /// `pending` while it waits.
    pub fn approver(
        self: &Arc<Self>,
        pane: PaneId,
        turn: u64,
        pending: Pending,
    ) -> Arc<dyn Approver> {
        Arc::new(ForTurn {
            waits: self.clone(),
            pane,
            turn,
            pending,
        })
    }

    /// Delivers the user's answer to the call that waits for it. `false` when
    /// nothing waits under that key: the call ended, was answered already, or
    /// belongs to another turn.
    pub fn decide(&self, pane: PaneId, turn: u64, call: CallId, decision: Decision) -> bool {
        match self.map().remove(&(pane, turn, call)) {
            Some(wait) => wait.send(decision).is_ok(),
            None => false,
        }
    }

    /// Claims the answer of the call that waits under the key: nobody else
    /// can answer it now. `None` when nothing waits (the turn ended, the call
    /// was cancelled or answered). An effect that must happen only if the call
    /// is still owed an answer takes the wait first and sends on the result.
    pub fn take(&self, pane: PaneId, turn: u64, call: CallId) -> Option<oneshot::Sender<Decision>> {
        self.map().remove(&(pane, turn, call))
    }

    /// Denies every call of `pane`: its chat closed.
    pub fn end_pane(&self, pane: PaneId) {
        self.map().retain(|(p, _, _), _| *p != pane);
    }

    /// Denies every call of one turn: the turn is over.
    pub fn end_turn(&self, pane: PaneId, turn: u64) {
        self.map().retain(|(p, t, _), _| (*p, *t) != (pane, turn));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map().len()
    }
}

struct ForTurn {
    waits: Arc<Waits>,
    pane: PaneId,
    turn: u64,
    pending: Pending,
}

/// A registered wait; leaving the map is its drop.
struct Entry {
    waits: Arc<Waits>,
    key: Key,
    _waiting: Waiting,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.waits.map().remove(&self.key);
    }
}

impl Approver for ForTurn {
    fn ask(&self, id: CallId, _: &ApprovalAsk) -> BoxFuture<'static, Decision> {
        let (answer, rx) = oneshot::channel();
        let key = (self.pane, self.turn, id);
        self.waits.map().insert(key, answer);
        let entry = Entry {
            waits: self.waits.clone(),
            key,
            _waiting: self.pending.enter(),
        };
        async move {
            let _entry = entry;
            // A sender dropped without an answer is an ended chat or turn.
            rx.await.unwrap_or(Decision::Denied(ENDED))
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const ASK: ApprovalAsk = ApprovalAsk::Read {
        path: String::new(),
    };

    fn pane(n: u32) -> PaneId {
        PaneId::for_test(n)
    }

    fn read(path: &str) -> ApprovalAsk {
        ApprovalAsk::Read { path: path.into() }
    }

    #[tokio::test]
    async fn an_answer_reaches_the_call_that_waits() {
        let waits = Waits::new();
        let approver = waits.approver(pane(1), 3, Pending::new());
        let wait = approver.ask(CallId(9), &read("/a"));
        assert!(waits.decide(pane(1), 3, CallId(9), Decision::Sent));
        assert_eq!(wait.await, Decision::Sent);
        assert_eq!(waits.len(), 0);
    }

    // The trait contract: the wait exists when `ask` returns, before the
    // future is polled, because an answer may come the instant the call is
    // announced.
    #[tokio::test]
    async fn the_wait_is_registered_before_the_future_is_polled() {
        let waits = Waits::new();
        let approver = waits.approver(pane(1), 1, Pending::new());
        let wait = approver.ask(CallId(1), &read("/a"));
        assert_eq!(waits.len(), 1);
        assert!(waits.decide(pane(1), 1, CallId(1), Decision::Denied("no")));
        assert_eq!(wait.await, Decision::Denied("no"));
    }

    #[tokio::test]
    async fn an_answer_for_another_key_is_stale() {
        let waits = Waits::new();
        let approver = waits.approver(pane(1), 2, Pending::new());
        let mut wait = approver.ask(CallId(5), &read("/a"));
        for (p, t, c) in [(2, 2, 5), (1, 3, 5), (1, 2, 6)] {
            assert!(!waits.decide(pane(p), t, CallId(c), Decision::Sent));
        }
        assert!((&mut wait).now_or_never().is_none(), "still waiting");
        assert!(waits.decide(pane(1), 2, CallId(5), Decision::Sent));
    }

    // The claim comes before an effect: whoever takes the wait owns the one
    // answer, and nobody else can answer it.
    #[tokio::test]
    async fn taking_a_wait_leaves_the_one_answer_to_the_taker() {
        let waits = Waits::new();
        let wait = waits
            .approver(pane(1), 1, Pending::new())
            .ask(CallId(1), &ASK);
        let sender = waits.take(pane(1), 1, CallId(1)).expect("it waits");
        assert!(waits.take(pane(1), 1, CallId(1)).is_none(), "taken once");
        assert!(!waits.decide(pane(1), 1, CallId(1), Decision::Sent));
        assert!(sender.send(Decision::Sent).is_ok());
        assert_eq!(wait.await, Decision::Sent);
        assert_eq!(waits.len(), 0);
    }

    #[tokio::test]
    async fn nothing_can_be_taken_from_a_wait_that_ended() {
        let waits = Waits::new();
        let approver = waits.approver(pane(1), 1, Pending::new());
        let (ended, dropped) = (approver.ask(CallId(1), &ASK), approver.ask(CallId(2), &ASK));
        waits.end_turn(pane(1), 1);
        assert!(waits.take(pane(1), 1, CallId(1)).is_none());
        drop(dropped);
        assert!(waits.take(pane(1), 1, CallId(2)).is_none());
        assert_eq!(ended.await, Decision::Denied(ENDED));
    }

    #[tokio::test]
    async fn a_call_is_answered_once() {
        let waits = Waits::new();
        let approver = waits.approver(pane(1), 1, Pending::new());
        let wait = approver.ask(CallId(1), &read("/a"));
        assert!(waits.decide(pane(1), 1, CallId(1), Decision::Sent));
        assert!(!waits.decide(pane(1), 1, CallId(1), Decision::Denied("late")));
        assert_eq!(wait.await, Decision::Sent);
    }

    #[tokio::test]
    async fn a_dropped_call_leaves_no_wait_behind() {
        let waits = Waits::new();
        let pending = Pending::new();
        let approver = waits.approver(pane(1), 1, pending.clone());
        let wait = approver.ask(CallId(1), &read("/a"));
        assert_eq!((waits.len(), pending.count()), (1, 1));
        drop(wait);
        assert_eq!((waits.len(), pending.count()), (0, 0));
        assert!(!waits.decide(pane(1), 1, CallId(1), Decision::Sent));
    }

    #[tokio::test]
    async fn a_waiting_call_counts_as_pending_until_it_ends() {
        let waits = Waits::new();
        let pending = Pending::new();
        let approver = waits.approver(pane(1), 1, pending.clone());
        let one = approver.ask(CallId(1), &read("/a"));
        let two = approver.ask(CallId(2), &read("/b"));
        assert_eq!(pending.count(), 2);
        waits.decide(pane(1), 1, CallId(1), Decision::Sent);
        one.await;
        assert_eq!(pending.count(), 1);
        drop(two);
        assert_eq!(pending.count(), 0);
    }

    // Two chats: ending one denies only its calls.
    #[tokio::test]
    async fn ending_a_chat_denies_its_calls_and_no_other() {
        let waits = Waits::new();
        let a = waits.approver(pane(1), 1, Pending::new());
        let b = waits.approver(pane(2), 1, Pending::new());
        let (a1, a2) = (a.ask(CallId(1), &ASK), a.ask(CallId(2), &ASK));
        let mut kept = b.ask(CallId(3), &ASK);
        waits.end_pane(pane(1));
        assert_eq!(a1.await, Decision::Denied(ENDED));
        assert_eq!(a2.await, Decision::Denied(ENDED));
        assert!((&mut kept).now_or_never().is_none());
        assert!(waits.decide(pane(2), 1, CallId(3), Decision::Sent));
        assert_eq!(kept.await, Decision::Sent);
    }

    #[tokio::test]
    async fn ending_a_turn_denies_that_turn_only() {
        let waits = Waits::new();
        let old = waits
            .approver(pane(1), 1, Pending::new())
            .ask(CallId(1), &ASK);
        let new = waits
            .approver(pane(1), 2, Pending::new())
            .ask(CallId(2), &ASK);
        waits.end_turn(pane(1), 1);
        assert_eq!(old.await, Decision::Denied(ENDED));
        assert!(waits.decide(pane(1), 2, CallId(2), Decision::Sent));
        assert_eq!(new.await, Decision::Sent);
    }

    // A timer never approves: however long the user takes, the call stays
    // waiting; only an answer, a drop or an end resolves it.
    #[tokio::test(start_paused = true)]
    async fn time_alone_never_answers() {
        let waits = Waits::new();
        let mut wait = waits
            .approver(pane(1), 1, Pending::new())
            .ask(CallId(1), &ASK);
        tokio::time::sleep(Duration::from_secs(3600)).await;
        assert!((&mut wait).now_or_never().is_none());
        assert_eq!(waits.len(), 1);
    }
}
