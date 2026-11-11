//! How many tool calls of a turn wait for the user right now.
//!
//! The turn's timers read it: while it is above zero the first-byte and idle
//! limits do not run (D13), because the engine is silent only because the user
//! is deciding. A chat turn's total limit ignores it; a worker turn's total
//! and its deadline pause on it too ([`pausing_timeout`], D5). A [`Pending`]
//! is made per turn (per worker, for a worker turn) and reaches the timers as
//! a task-local, so no adapter signature carries it.

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::watch,
    time::{Instant, sleep},
};

/// The count of calls of one turn that wait for the user. Clones share it.
#[derive(Clone)]
pub struct Pending(Arc<watch::Sender<usize>>);

impl Pending {
    pub fn new() -> Self {
        Self(Arc::new(watch::channel(0).0))
    }

    /// One more call waits for the user, until the guard is dropped.
    pub fn enter(&self) -> Waiting {
        self.0.send_modify(|n| *n += 1);
        Waiting(self.clone())
    }

    /// How many calls wait now.
    pub fn count(&self) -> usize {
        *self.0.borrow()
    }

    /// A receiver that wakes whenever the count changes.
    pub fn watch(&self) -> watch::Receiver<usize> {
        self.0.subscribe()
    }
}

impl Default for Pending {
    fn default() -> Self {
        Self::new()
    }
}

/// A call that waits for the user; dropping it (answered, cancelled, closed)
/// takes it out of the count.
pub struct Waiting(Pending);

impl Drop for Waiting {
    /// Never panics, so a drop during an unwind cannot abort: the count only
    /// goes down by the one its own `enter` added, and `saturating_sub` keeps
    /// the arithmetic from wrapping whatever the order of the drops.
    fn drop(&mut self) {
        (self.0).0.send_modify(|n| *n = n.saturating_sub(1));
    }
}

tokio::task_local! {
    /// The pending count of the turn this task runs.
    pub static TURN: Pending;
    /// Set only around a worker turn ([`worker_turn`]).
    static WORKER: ();
}

/// The pending count of the turn the current task runs, if it runs one.
pub fn current() -> Option<Pending> {
    TURN.try_with(Pending::clone).ok()
}

/// The limit of [`pausing_timeout`] or [`total_limit`] passed.
#[derive(Debug, PartialEq, Eq)]
pub struct Elapsed;

/// `fut` under `limit`, a budget that only runs while no call of `pending`
/// waits for the user (D5): the clock stops when the count rises above zero
/// and goes on with what was left when it falls back to zero. `fut` itself
/// runs all along; only the limit stops.
pub async fn pausing_timeout<F: Future>(
    limit: Duration,
    pending: &Pending,
    fut: F,
) -> Result<F::Output, Elapsed> {
    let mut rx = pending.watch();
    let mut left = limit;
    tokio::pin!(fut);
    loop {
        let paused = *rx.borrow_and_update() > 0;
        let since = Instant::now();
        // `pending` is borrowed for the whole call, so its sender outlives
        // `rx` and `changed` never fails.
        tokio::select! {
            out = &mut fut => return Ok(out),
            () = sleep(left), if !paused => return Err(Elapsed),
            _ = rx.changed() => {
                if !paused {
                    left = left.saturating_sub(since.elapsed());
                }
            }
        }
    }
}

/// `fut` under a turn's total limit. In a worker turn ([`worker_turn`]) the
/// limit pauses while the turn's own count ([`TURN`]) is above zero, as the
/// worker's deadline does; a chat turn's total never pauses.
pub async fn total_limit<F: Future>(limit: Duration, fut: F) -> Result<F::Output, Elapsed> {
    match current().filter(|_| WORKER.try_with(|()| ()).is_ok()) {
        Some(pending) => pausing_timeout(limit, &pending, fut).await,
        None => tokio::time::timeout(limit, fut).await.map_err(|_| Elapsed),
    }
}

/// `fut` as a worker turn: its total limit pauses while it waits for the
/// user. Only the run manager's `Turn::run` wraps a turn in it.
pub fn worker_turn<F: Future>(fut: F) -> impl Future<Output = F::Output> {
    WORKER.scope((), fut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_follows_the_waiting_calls() {
        let pending = Pending::new();
        assert_eq!(pending.count(), 0);
        let a = pending.enter();
        let b = pending.enter();
        assert_eq!(pending.count(), 2);
        drop(a);
        assert_eq!(pending.count(), 1);
        drop(b);
        assert_eq!(pending.count(), 0);
    }

    #[test]
    fn a_waiting_dropped_by_an_unwind_leaves_the_count() {
        let pending = Pending::new();
        let held = pending.clone();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _wait = held.enter();
            panic!("a turn that panics while its call waits");
        }));
        assert!(unwound.is_err());
        assert_eq!(pending.count(), 0);
    }

    #[test]
    fn clones_share_the_count() {
        let pending = Pending::new();
        let other = pending.clone();
        let _wait = other.enter();
        assert_eq!(pending.count(), 1);
    }

    #[tokio::test]
    async fn a_watcher_wakes_on_every_change() {
        let pending = Pending::new();
        let mut rx = pending.watch();
        let wait = pending.enter();
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow_and_update(), 1);
        drop(wait);
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow_and_update(), 0);
    }

    // The pausing clock (D5) ---------------------------------------------

    const S: Duration = Duration::from_secs(1);

    /// `fut`, failed instead of hung when it never ends: with no timer left
    /// a paused clock would wait forever, and this hour passes at once.
    async fn bounded<F: Future>(fut: F) -> F::Output {
        tokio::time::timeout(3600 * S, fut)
            .await
            .expect("the clock never ended it")
    }

    /// Works `work`, waits `wait` for the user, and so on, then works
    /// forever: the clock alone ends it. Returns when and how it ended.
    async fn plan(limit: Duration, steps: &[(Duration, Duration)]) -> (Duration, bool) {
        let pending = Pending::new();
        let start = Instant::now();
        let fut = async {
            for (work, wait) in steps {
                sleep(*work).await;
                let open = pending.enter();
                sleep(*wait).await;
                drop(open);
            }
            std::future::pending::<()>().await;
        };
        let ended = bounded(pausing_timeout(limit, &pending, fut)).await;
        (start.elapsed(), ended.is_err())
    }

    #[tokio::test(start_paused = true)]
    async fn the_time_spent_waiting_does_not_count() {
        let pending = Pending::new();
        let start = Instant::now();
        let fut = async {
            sleep(S).await;
            let open = pending.enter();
            sleep(10 * S).await;
            drop(open);
            sleep(S).await;
            7
        };
        assert_eq!(bounded(pausing_timeout(3 * S, &pending, fut)).await, Ok(7));
        assert_eq!(start.elapsed(), 12 * S);
    }

    #[tokio::test(start_paused = true)]
    async fn work_with_no_wait_runs_the_limit_out() {
        let pending = Pending::new();
        let start = Instant::now();
        let fut = sleep(4 * S);
        assert_eq!(pausing_timeout(3 * S, &pending, fut).await, Err(Elapsed));
        assert_eq!(start.elapsed(), 3 * S);
    }

    // 1 + 1 + 1 s of work around three waits of 5 s: the limit of 3 s passes
    // once the work sums to it, after all three waits.
    #[tokio::test(start_paused = true)]
    async fn several_waits_sum() {
        let steps = [(S, 5 * S), (S, 5 * S), (S / 2, 5 * S)];
        assert_eq!(plan(3 * S, &steps).await, (3 * S + 15 * S, true));
    }

    // The limit would pass at 3 s, while the user still decides: it waits for
    // the answer (at 101 s), then the 2 s that were left run.
    #[tokio::test(start_paused = true)]
    async fn a_wait_still_open_at_the_limit_holds_it() {
        assert_eq!(plan(3 * S, &[(S, 100 * S)]).await, (103 * S, true));
    }

    // An ask already waiting when the clock starts (a queued one) holds it
    // from the start.
    #[tokio::test(start_paused = true)]
    async fn a_wait_open_at_the_start_holds_the_clock() {
        let pending = Pending::new();
        let open = pending.enter();
        let start = Instant::now();
        let fut = async {
            sleep(50 * S).await;
            drop(open);
            std::future::pending::<()>().await
        };
        let ended = bounded(pausing_timeout(3 * S, &pending, fut)).await;
        assert_eq!(ended, Err(Elapsed));
        assert_eq!(start.elapsed(), 53 * S);
    }

    #[tokio::test]
    async fn only_a_turn_task_sees_its_count() {
        assert!(current().is_none());
        let pending = Pending::new();
        let _wait = pending.enter();
        let seen = TURN
            .scope(pending, async { current().map(|p| p.count()) })
            .await;
        assert_eq!(seen, Some(1));
    }
}
