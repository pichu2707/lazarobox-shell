//! What the menu's Feedback form attaches about this host (see
//! [`crate::core::menu::feedback::host`]). Read from the environment and
//! `/proc`; nothing leaves the machine from here.

use crate::core::menu::feedback::{self, Facts};

/// The facts of this process's host; the engines are added by the app.
pub fn host_facts() -> Facts {
    let osrelease = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok();
    feedback::host(
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        |name| std::env::var(name).ok(),
        osrelease.as_deref(),
    )
}
