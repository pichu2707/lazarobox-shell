//! Who may run a command now (design D17): one run per chat and a global
//! `Semaphore(2)`. Both are taken after the user's `y` and before the spawn.
//! A busy chat is refused at once; the global wait is capped, then refused.
//! The permit gives both back when it is dropped, so a cancelled call frees
//! its place.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::core::ai::tools::stage::StageKey;

/// Runs at once in the whole app.
const SLOTS: usize = 2;
/// How long a run waits for a global slot.
const WAIT: Duration = Duration::from_secs(60);

pub(super) const CHAT_BUSY: &str = "a command is already running in this chat";
pub(super) const APP_BUSY: &str = "too many commands are running; try again later";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Refused {
    /// This chat already has a run.
    Chat,
    /// The app is at its cap and no slot freed within the wait.
    Full,
}

impl Refused {
    pub(super) fn text(self) -> &'static str {
        match self {
            Refused::Chat => CHAT_BUSY,
            Refused::Full => APP_BUSY,
        }
    }
}

type Claimed = Arc<Mutex<HashSet<StageKey>>>;

pub(super) struct RunGate {
    claimed: Claimed,
    slots: Arc<Semaphore>,
    wait: Duration,
}

impl Default for RunGate {
    fn default() -> Self {
        Self::new(SLOTS, WAIT)
    }
}

/// A chat's claim: removed when dropped, also while it still waits.
struct Claim {
    claimed: Claimed,
    key: StageKey,
}

impl Drop for Claim {
    fn drop(&mut self) {
        lock(&self.claimed).remove(&self.key);
    }
}

/// The right to run one command. Dropping it frees the chat and the slot.
pub(super) struct Permit {
    _claim: Claim,
    _slot: OwnedSemaphorePermit,
}

fn lock(set: &Mutex<HashSet<StageKey>>) -> std::sync::MutexGuard<'_, HashSet<StageKey>> {
    set.lock().unwrap_or_else(PoisonError::into_inner)
}

impl RunGate {
    pub(super) fn new(slots: usize, wait: Duration) -> Self {
        Self {
            claimed: Claimed::default(),
            slots: Arc::new(Semaphore::new(slots)),
            wait,
        }
    }

    /// Whether `key` has a run (or a call waiting for a slot).
    pub(super) fn is_busy(&self, key: StageKey) -> bool {
        lock(&self.claimed).contains(&key)
    }

    /// Takes the chat, then a global slot (waiting at most the cap).
    pub(super) async fn acquire(&self, key: StageKey) -> Result<Permit, Refused> {
        if !lock(&self.claimed).insert(key) {
            return Err(Refused::Chat);
        }
        let claim = Claim {
            claimed: self.claimed.clone(),
            key,
        };
        match tokio::time::timeout(self.wait, self.slots.clone().acquire_owned()).await {
            Ok(Ok(slot)) => Ok(Permit {
                _claim: claim,
                _slot: slot,
            }),
            _ => Err(Refused::Full),
        }
    }
}
