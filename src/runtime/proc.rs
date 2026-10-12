//! Child-process helpers shared by the runtime's probes and, later, the chat
//! CLI engines.

/// Kills a process group on `kill()` or, while still armed, on drop.
///
/// Put it after the `Child` it guards so it drops first: a dropped future then
/// still kills the group while the leader has not been reaped yet. A reaped
/// pid can be recycled, and its pgid with it.
pub struct GroupKill(Option<libc::pid_t>);

impl GroupKill {
    /// Arms the guard for the group whose leader is `pgid`.
    pub fn new(pgid: libc::pid_t) -> Self {
        Self(Some(pgid))
    }

    /// Sends SIGKILL to the whole group.
    pub fn kill(&self) {
        if let Some(pgid) = self.0 {
            // SAFETY: killpg only sends a signal; the group was just spawned
            // by this process and its leader has not been reaped.
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
    }

    /// Disarms the guard: neither `kill` nor drop signals the group any more.
    pub fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for GroupKill {
    fn drop(&mut self) {
        self.kill();
    }
}
