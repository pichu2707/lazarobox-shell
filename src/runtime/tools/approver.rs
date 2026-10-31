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
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use futures::{FutureExt, future::BoxFuture};
use tokio::sync::oneshot;

use super::{Approver, Pending, pending::Waiting};
use crate::core::{
    ai::chat::{
        approval::ApprovalAsk,
        engine::{Answer, CallId, Decision},
    },
    ai::orchestra::{SessionKey, WorkerKey},
    ai::tools::report::RunReport,
    layout::PaneId,
};

/// What a call is told when its chat or turn ended before the user answered.
pub const ENDED: &str = "the chat ended before the user answered";

/// What a call is told when its turn or chat ended while a command it had
/// started was still running: the command did run.
pub const RAN_ENDED: &str = "the command was typed and run, but the turn ended before it finished";

type Key = (SessionKey, u64, CallId);

/// The calls waiting for the user's answer, for every chat of the app.
#[derive(Default)]
pub struct Waits {
    waiting: Mutex<HashMap<Key, oneshot::Sender<Answer>>>,
    /// Turns and chats that ended: `park` refuses them. A new `ask` under the
    /// same key revives it. Lock order: `waiting`, then `ended`.
    ended: Mutex<Ended>,
}

#[derive(Default)]
struct Ended {
    turns: HashSet<(PaneId, u64)>,
    panes: HashSet<PaneId>,
    /// The calls whose command was written and whose wait is parked, so an
    /// end tells them the truth (`RAN_ENDED`) and not `ENDED`.
    parked: HashSet<Key>,
    /// Worker sessions that ended: a late ask of theirs is denied.
    sessions: HashSet<SessionKey>,
    /// The attempt of each worker session that may ask (set by `for_worker`):
    /// the approver of an earlier attempt is denied. Attempts count from 1.
    current: HashMap<SessionKey, u64>,
    attempts: u64,
}

impl Ended {
    /// Whether `attempt` of worker session `session` may no longer ask: the
    /// worker ended, or a retry replaced that attempt.
    fn over(&self, session: SessionKey, attempt: u64) -> bool {
        self.sessions.contains(&session)
            || self
                .current
                .get(&session)
                .is_some_and(|now| *now != attempt)
    }
}

impl Waits {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn map(&self) -> MutexGuard<'_, HashMap<Key, oneshot::Sender<Answer>>> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The approver of turn `turn` of `pane`. Every call it holds counts in
    /// `pending` while it waits.
    pub fn approver(
        self: &Arc<Self>,
        session: impl Into<SessionKey>,
        turn: u64,
        pending: Pending,
    ) -> Arc<dyn Approver> {
        Arc::new(ForTurn {
            waits: self.clone(),
            session: session.into(),
            attempt: 0,
            turn,
            pending,
        })
    }

    /// Delivers the user's answer to the call that waits for it. `false` when
    /// nothing waits under that key: the call ended, was answered already, or
    /// belongs to another turn.
    pub fn decide(
        &self,
        session: impl Into<SessionKey>,
        turn: u64,
        call: CallId,
        decision: Decision,
    ) -> bool {
        match self.map().remove(&(session.into(), turn, call)) {
            Some(wait) => wait.send(decision.into()).is_ok(),
            None => false,
        }
    }

    /// Puts a claimed wait back after the command was written, so the turn and
    /// chat cleanup, and the drop of the call, also cover the running call.
    /// `false` when the call is gone, or its turn or chat already ended; the
    /// command was written, so the call is then told `RAN_ENDED`. A call whose
    /// future was dropped has a closed sender, which covers an entry that left
    /// the map already. The checks run under the lock, so an `end_*` or a
    /// dropped call cannot slip in between the check and the insert.
    pub fn park(
        &self,
        session: impl Into<SessionKey>,
        turn: u64,
        call: CallId,
        wait: oneshot::Sender<Answer>,
    ) -> bool {
        let session = session.into();
        let pane = session.pane();
        let mut waiting = self.map();
        let dead = {
            let ended = self.ended();
            ended.panes.contains(&pane) || ended.turns.contains(&(pane, turn))
        };
        if wait.is_closed() {
            return false;
        }
        if dead {
            let _ = wait.send(Decision::Denied(RAN_ENDED).into());
            return false;
        }
        self.ended().parked.insert((session, turn, call));
        waiting.insert((session, turn, call), wait);
        true
    }

    /// Ends a parked call with the report of its run. `false` when nothing is
    /// parked under the key (the turn ended, the call was dropped).
    pub fn finish(
        &self,
        session: impl Into<SessionKey>,
        turn: u64,
        call: CallId,
        report: Box<RunReport>,
    ) -> bool {
        let key = (session.into(), turn, call);
        let wait = self.map().remove(&key);
        self.ended().parked.remove(&key);
        match wait {
            Some(wait) => wait.send(Answer::Ran(report)).is_ok(),
            None => false,
        }
    }

    /// Claims the answer of the call that waits under the key: nobody else
    /// can answer it now. `None` when nothing waits (the turn ended, the call
    /// was cancelled or answered). An effect that must happen only if the call
    /// is still owed an answer takes the wait first and sends on the result.
    pub fn take(
        &self,
        session: impl Into<SessionKey>,
        turn: u64,
        call: CallId,
    ) -> Option<oneshot::Sender<Answer>> {
        self.map().remove(&(session.into(), turn, call))
    }

    fn ended(&self) -> MutexGuard<'_, Ended> {
        self.ended.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ends every wait `ended` names: a parked run is told it did run, any
    /// other is dropped (`ENDED`).
    fn end_where(&self, ended: impl Fn(&Key) -> bool) {
        let mut waiting = self.map();
        let keys: Vec<Key> = waiting.keys().filter(|key| ended(key)).copied().collect();
        let mut state = self.ended();
        for key in keys {
            if let Some(wait) = waiting.remove(&key)
                && state.parked.remove(&key)
            {
                let _ = wait.send(Decision::Denied(RAN_ENDED).into());
            }
        }
        state.parked.retain(|key| !ended(key));
    }

    /// Denies every call of `pane`: its chat closed.
    pub fn end_pane(&self, pane: PaneId) {
        self.end_where(|key| key.0.pane() == pane);
        self.ended().panes.insert(pane);
    }

    /// Denies every call of one turn: the turn is over.
    pub fn end_turn(&self, pane: PaneId, turn: u64) {
        self.end_where(|key| (key.0.pane(), key.1) == (pane, turn));
        self.ended().turns.insert((pane, turn));
    }

    /// Denies every ask of one worker, pending or queued in the app, and
    /// refuses the asks it makes later: the worker was cancelled or ended.
    /// The asks of the other workers, and of the pane, are untouched.
    pub fn end_worker(&self, pane: PaneId, worker: WorkerKey) {
        let session = SessionKey::Worker(pane, worker);
        self.end_where(|key| key.0 == session);
        self.ended().sessions.insert(session);
    }

    /// How many waits are registered (a test waits for its call to ask).
    #[cfg(test)]
    pub(crate) fn registered(&self) -> usize {
        self.len()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map().len()
    }
}

struct ForTurn {
    waits: Arc<Waits>,
    session: SessionKey,
    /// The attempt of a worker session this approver was made for; `0` for
    /// one made without (a pane, or a test).
    attempt: u64,
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
    fn for_worker(&self, key: WorkerKey) -> Option<Arc<dyn Approver>> {
        let session = SessionKey::Worker(self.session.pane(), key);
        // A new attempt of a worker that ended (a retry) asks again; the
        // approvers of earlier attempts never do.
        let attempt = {
            let mut ended = self.waits.ended();
            ended.sessions.remove(&session);
            ended.attempts += 1;
            let attempt = ended.attempts;
            ended.current.insert(session, attempt);
            attempt
        };
        Some(Arc::new(ForTurn {
            waits: self.waits.clone(),
            session,
            attempt,
            turn: self.turn,
            pending: self.pending.clone(),
        }))
    }

    fn end_worker(&self, key: WorkerKey) {
        self.waits.end_worker(self.session.pane(), key);
    }

    fn is_over(&self) -> bool {
        self.waits.ended().over(self.session, self.attempt)
    }

    fn ask(&self, id: CallId, _: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        let (answer, rx) = oneshot::channel();
        let key = (self.session, self.turn, id);
        let pane = self.session.pane();
        {
            let mut waiting = self.waits.map();
            let mut ended = self.waits.ended();
            // A worker that ended asks no more: the call is denied unseen.
            if ended.over(self.session, self.attempt) {
                return async { Decision::Denied(ENDED).into() }.boxed();
            }
            ended.panes.remove(&pane);
            ended.turns.remove(&(pane, self.turn));
            ended.parked.remove(&key);
            waiting.insert(key, answer);
        }
        let entry = Entry {
            waits: self.waits.clone(),
            key,
            _waiting: self.pending.enter(),
        };
        async move {
            let _entry = entry;
            // A sender dropped without an answer is an ended chat or turn.
            rx.await.unwrap_or(Decision::Denied(ENDED).into())
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::core::ai::tools::settle::End;

    const ASK: ApprovalAsk = ApprovalAsk::Read {
        path: String::new(),
        reach: crate::core::ai::chat::approval::Reach::Other,
        cloud: true,
    };

    fn pane(n: u32) -> PaneId {
        PaneId::for_test(n)
    }

    fn read(path: &str) -> ApprovalAsk {
        ApprovalAsk::read(path)
    }

    #[tokio::test]
    async fn an_answer_reaches_the_call_that_waits() {
        let waits = Waits::new();
        let approver = waits.approver(pane(1), 3, Pending::new());
        let wait = approver.ask(CallId(9), &read("/a"));
        assert!(waits.decide(pane(1), 3, CallId(9), Decision::Sent));
        assert_eq!(wait.await, Decision::Sent.into());
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
        assert_eq!(wait.await, Decision::Denied("no").into());
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
        assert!(sender.send(Decision::Sent.into()).is_ok());
        assert_eq!(wait.await, Decision::Sent.into());
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
        assert_eq!(ended.await, Decision::Denied(ENDED).into());
    }

    #[tokio::test]
    async fn a_call_is_answered_once() {
        let waits = Waits::new();
        let approver = waits.approver(pane(1), 1, Pending::new());
        let wait = approver.ask(CallId(1), &read("/a"));
        assert!(waits.decide(pane(1), 1, CallId(1), Decision::Sent));
        assert!(!waits.decide(pane(1), 1, CallId(1), Decision::Denied("late")));
        assert_eq!(wait.await, Decision::Sent.into());
    }

    fn worker(n: u32) -> WorkerKey {
        WorkerKey::for_test(1, n)
    }

    fn of_worker(waits: &Arc<Waits>, n: u32) -> Arc<dyn Approver> {
        waits.approver(SessionKey::Worker(pane(1), worker(n)), 7, Pending::new())
    }

    // Spec (agent-orchestrator, Cancel denies pending asks; agent-tools,
    // Sessions keyed by session key): ending worker 2 denies its asks and
    // leaves worker 1's, and the pane's own, waiting.
    #[tokio::test]
    async fn ending_one_worker_denies_only_its_own_asks() {
        let waits = Waits::new();
        let (one, two) = (of_worker(&waits, 1), of_worker(&waits, 2));
        let own = waits.approver(pane(1), 7, Pending::new());
        let mut kept = one.ask(CallId(1), &read("/a"));
        let (first, second) = (
            two.ask(CallId(2), &read("/b")),
            two.ask(CallId(3), &read("/c")),
        );
        let mut pane_ask = own.ask(CallId(4), &read("/d"));
        waits.end_worker(pane(1), worker(2));
        assert_eq!(first.now_or_never(), Some(Decision::Denied(ENDED).into()));
        assert_eq!(second.now_or_never(), Some(Decision::Denied(ENDED).into()));
        assert!((&mut kept).now_or_never().is_none(), "worker 1 still waits");
        assert!(
            (&mut pane_ask).now_or_never().is_none(),
            "the pane still waits"
        );
        assert!(waits.decide(
            SessionKey::Worker(pane(1), worker(1)),
            7,
            CallId(1),
            Decision::Sent
        ));
        assert!(waits.decide(pane(1), 7, CallId(4), Decision::Sent));
    }

    // Spec (agent-tools, Cross-session call refused): an answer for worker 1
    // never lands on the same call id of worker 2 or of the pane.
    #[tokio::test]
    async fn an_answer_names_its_session() {
        let waits = Waits::new();
        let mut wait = of_worker(&waits, 2).ask(CallId(5), &read("/a"));
        assert!(!waits.decide(
            SessionKey::Worker(pane(1), worker(1)),
            7,
            CallId(5),
            Decision::Sent
        ));
        assert!(!waits.decide(pane(1), 7, CallId(5), Decision::Sent));
        assert!((&mut wait).now_or_never().is_none(), "still waiting");
        assert!(waits.decide(
            SessionKey::Worker(pane(1), worker(2)),
            7,
            CallId(5),
            Decision::Sent
        ));
    }

    #[tokio::test]
    async fn an_ended_worker_is_denied_its_later_asks() {
        let waits = Waits::new();
        waits.end_worker(pane(1), worker(2));
        let late = of_worker(&waits, 2).ask(CallId(1), &read("/a"));
        assert_eq!(late.now_or_never(), Some(Decision::Denied(ENDED).into()));
        assert_eq!(waits.len(), 0);
    }

    // A failed worker retried keeps its key: its next attempt opens a fresh
    // session, whose asks wait again.
    #[tokio::test]
    async fn a_retried_worker_asks_again_after_its_first_attempt_ended() {
        let waits = Waits::new();
        let turn = waits.approver(pane(1), 7, Pending::new());
        let first = turn.for_worker(worker(2)).expect("a session approver");
        waits.end_worker(pane(1), worker(2));
        let late = first.ask(CallId(1), &read("/a"));
        assert_eq!(late.now_or_never(), Some(Decision::Denied(ENDED).into()));
        let second = turn.for_worker(worker(2)).expect("a session approver");
        let mut wait = second.ask(CallId(2), &read("/a"));
        assert!(
            (&mut wait).now_or_never().is_none(),
            "it waits for the user"
        );
        let session = SessionKey::Worker(pane(1), worker(2));
        assert!(waits.decide(session, 7, CallId(2), Decision::Sent));
    }

    // Only the current attempt of a worker may ask: the approver of an earlier
    // attempt, still held by a stale call, is denied unseen after a retry.
    #[tokio::test]
    async fn the_approver_of_an_earlier_attempt_cannot_ask_after_a_retry() {
        let waits = Waits::new();
        let turn = waits.approver(pane(1), 7, Pending::new());
        let first = turn.for_worker(worker(2)).expect("a session approver");
        waits.end_worker(pane(1), worker(2));
        let second = turn.for_worker(worker(2)).expect("a session approver");
        let stale = first.ask(CallId(1), &read("/a"));
        assert_eq!(stale.now_or_never(), Some(Decision::Denied(ENDED).into()));
        assert_eq!(waits.len(), 0, "nothing registered for the stale attempt");
        let mut current = second.ask(CallId(2), &read("/a"));
        assert!(
            (&mut current).now_or_never().is_none(),
            "the new attempt waits"
        );
    }

    // The pane closing or the turn ending takes its workers' asks too.
    #[tokio::test]
    async fn a_closed_pane_denies_its_workers_asks_as_well() {
        let waits = Waits::new();
        let wait = of_worker(&waits, 1).ask(CallId(1), &read("/a"));
        waits.end_pane(pane(1));
        assert_eq!(wait.now_or_never(), Some(Decision::Denied(ENDED).into()));
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
        assert_eq!(a1.await, Decision::Denied(ENDED).into());
        assert_eq!(a2.await, Decision::Denied(ENDED).into());
        assert!((&mut kept).now_or_never().is_none());
        assert!(waits.decide(pane(2), 1, CallId(3), Decision::Sent));
        assert_eq!(kept.await, Decision::Sent.into());
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
        assert_eq!(old.await, Decision::Denied(ENDED).into());
        assert!(waits.decide(pane(1), 2, CallId(2), Decision::Sent));
        assert_eq!(new.await, Decision::Sent.into());
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

    fn report() -> Box<RunReport> {
        Box::new(RunReport {
            number: 2,
            end: End::Settled,
            cwd: None,
            cd_target: None,
            output: Some(String::new()),
        })
    }

    // The runtime takes the wait, writes, then parks the sender again so the
    // cleanup of the wait also covers the running call.
    #[tokio::test]
    async fn a_parked_call_is_finished_with_the_report() {
        let waits = Waits::new();
        let wait = waits
            .approver(pane(1), 1, Pending::new())
            .ask(CallId(1), &ASK);
        let sender = waits.take(pane(1), 1, CallId(1)).expect("it waits");
        waits.park(pane(1), 1, CallId(1), sender);
        assert_eq!(waits.len(), 1);
        assert!(
            !waits.decide(pane(1), 1, CallId(2), Decision::Sent),
            "other key"
        );
        assert!(waits.finish(pane(1), 1, CallId(1), report()));
        assert_eq!(wait.await, Answer::Ran(report()));
        assert!(!waits.finish(pane(1), 1, CallId(1), report()), "once");
        assert_eq!(waits.len(), 0);
    }

    #[tokio::test]
    async fn ending_a_turn_or_chat_also_covers_a_parked_call() {
        let waits = Waits::new();
        let approver = waits.approver(pane(1), 1, Pending::new());
        let (a, b) = (approver.ask(CallId(1), &ASK), approver.ask(CallId(2), &ASK));
        for call in [CallId(1), CallId(2)] {
            let sender = waits.take(pane(1), 1, call).expect("it waits");
            waits.park(pane(1), 1, call, sender);
        }
        waits.end_turn(pane(1), 1);
        assert_eq!(a.await, Decision::Denied(RAN_ENDED).into());
        assert_eq!(waits.len(), 0);
        let again = waits
            .approver(pane(1), 2, Pending::new())
            .ask(CallId(3), &ASK);
        let sender = waits.take(pane(1), 2, CallId(3)).expect("it waits");
        waits.park(pane(1), 2, CallId(3), sender);
        waits.end_pane(pane(1));
        assert_eq!(again.await, Decision::Denied(RAN_ENDED).into());
        drop(b);
    }

    #[tokio::test]
    async fn parking_a_call_whose_future_was_dropped_leaves_nothing() {
        let waits = Waits::new();
        let wait = waits
            .approver(pane(1), 1, Pending::new())
            .ask(CallId(1), &ASK);
        let sender = waits.take(pane(1), 1, CallId(1)).expect("it waits");
        drop(wait);
        waits.park(pane(1), 1, CallId(1), sender);
        assert_eq!(waits.len(), 0);
    }

    #[test]
    fn a_decision_converts_and_stays_copy() {
        fn copy<T: Copy>(_: T) {}
        copy(Decision::Sent);
        assert_eq!(
            Answer::from(Decision::Busy(3)),
            Answer::Decided(Decision::Busy(3))
        );
    }

    #[tokio::test]
    async fn a_parked_call_of_an_ended_turn_or_chat_is_told_it_ran() {
        let waits = Waits::new();
        let a = waits.approver(pane(1), 1, Pending::new());
        let (w1, w2, w3) = (
            a.ask(CallId(1), &ASK),
            a.ask(CallId(2), &ASK),
            waits
                .approver(pane(2), 1, Pending::new())
                .ask(CallId(3), &ASK),
        );
        let s1 = waits.take(pane(1), 1, CallId(1)).unwrap();
        let s2 = waits.take(pane(1), 1, CallId(2)).unwrap();
        let s3 = waits.take(pane(2), 1, CallId(3)).unwrap();
        waits.end_turn(pane(1), 1);
        waits.end_pane(pane(2));
        assert!(!waits.park(pane(1), 1, CallId(1), s1), "turn ended");
        assert!(!waits.park(pane(2), 1, CallId(3), s3), "chat ended");
        assert_eq!(waits.len(), 0);
        // The command was written before the park: the call is told so.
        assert_eq!(w1.await, Decision::Denied(RAN_ENDED).into());
        assert_eq!(w3.await, Decision::Denied(RAN_ENDED).into());
        // A later turn of the same chat is alive.
        let w4 = waits
            .approver(pane(1), 2, Pending::new())
            .ask(CallId(4), &ASK);
        let s4 = waits.take(pane(1), 2, CallId(4)).unwrap();
        assert!(waits.park(pane(1), 2, CallId(4), s4));
        drop((w2, w4, s2));
    }

    #[tokio::test]
    async fn a_closed_sender_is_not_parked_and_a_new_ask_revives_a_key() {
        let waits = Waits::new();
        let approver = waits.approver(pane(1), 1, Pending::new());
        let wait = approver.ask(CallId(1), &ASK);
        let sender = waits.take(pane(1), 1, CallId(1)).unwrap();
        drop(wait);
        assert!(!waits.park(pane(1), 1, CallId(1), sender));
        waits.end_turn(pane(1), 1);
        let again = approver.ask(CallId(2), &ASK);
        let sender = waits.take(pane(1), 1, CallId(2)).unwrap();
        assert!(
            waits.park(pane(1), 1, CallId(2), sender),
            "a new wait is alive"
        );
        drop(again);
    }

    // A consumer of the taken sender still answers with a plain Decision.
    #[tokio::test]
    async fn a_decision_sent_through_the_taken_sender_reaches_the_call() {
        let waits = Waits::new();
        let wait = waits
            .approver(pane(1), 1, Pending::new())
            .ask(CallId(1), &ASK);
        let sender = waits.take(pane(1), 1, CallId(1)).unwrap();
        assert!(sender.send(Decision::Busy(3).into()).is_ok());
        assert_eq!(wait.await, Answer::Decided(Decision::Busy(3)));
    }
}
