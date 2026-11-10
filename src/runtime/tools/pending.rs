//! How many tool calls of a turn wait for the user right now.
//!
//! The turn's timers read it: while it is above zero the first-byte and idle
//! limits do not run (D13), because the engine is silent only because the user
//! is deciding. The total limit ignores it. A [`Pending`] is made per turn and
//! reaches the timers as a task-local, so no adapter signature carries it.

use std::sync::Arc;

use tokio::sync::watch;

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

    /// Whether `other` is this very count (a clone of it).
    pub fn same(&self, other: &Pending) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
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
    fn drop(&mut self) {
        (self.0).0.send_modify(|n| *n = n.saturating_sub(1));
    }
}

tokio::task_local! {
    /// The pending count of the turn this task runs.
    pub static TURN: Pending;
}

/// The pending count of the turn the current task runs, if it runs one.
pub fn current() -> Option<Pending> {
    TURN.try_with(Pending::clone).ok()
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
