//! Child-process helpers shared by the runtime's probes and, later, the chat
//! CLI engines.

/// Kills a process group on `kill()` or, while still armed, on drop.
///
/// Put it after the `Child` it guards so it drops first: a dropped future then
/// still kills the group while the leader has not been reaped yet.
///
/// # Reaping: `disarm()` is MANDATORY
///
/// A reaped pid can be recycled, and its pgid with it. The moment the leader
/// is reaped the guard MUST be disarmed, otherwise a later `kill()` or the
/// drop would `killpg` a group that now belongs to an unrelated process. Do
/// not reap with a bare `child.wait()`: use [`GroupKill::reap`], which awaits
/// the leader and disarms in one step. (A caller that reaps by other means
/// must call [`GroupKill::disarm`] right after, with nothing in between.)
/// `kill()` itself stays correct only BEFORE the reap.
///
/// On a normal leader exit, `reap()` disarms WITHOUT killing the group, so
/// background grandchildren survive. Adapters must use
/// [`GroupKill::kill_and_reap`] (or call `kill()` before `reap()`) there.
///
/// # Construction is fallible
///
/// `killpg(0)` signals our own group (the UI included), `killpg(1)` and
/// negative values reach other groups. [`GroupKill::new`] therefore returns
/// `None` for `pgid <= 1` and for our own process group, so a guard that
/// exists can only ever target a group other than ours.
pub struct GroupKill(Option<libc::pid_t>);

impl GroupKill {
    /// Arms the guard for the group whose leader is `pgid`. `None` when the
    /// target is unsafe: `pgid <= 1` or our own process group. The caller
    /// then owns the cleanup of the child (for example by dropping it with
    /// `kill_on_drop`) and must not assume a group kill is in place.
    pub fn new(pgid: libc::pid_t) -> Option<Self> {
        // SAFETY: getpgrp has no preconditions and cannot fail.
        let own = unsafe { libc::getpgrp() };
        let guard = Self::checked(pgid, own)?;
        // The watcher stops it if lazarobox ends before it is reaped.
        crate::runtime::watch::register(pgid);
        Some(guard)
    }

    /// `new` with our own group given, so the refusal rules are testable.
    fn checked(pgid: libc::pid_t, own: libc::pid_t) -> Option<Self> {
        // Not `bool::then_some`: it builds the guard eagerly and would drop
        // (and so killpg) a refused one, e.g. `killpg(0)` on our own group.
        if pgid > 1 && pgid != own {
            Some(Self(Some(pgid)))
        } else {
            None
        }
    }

    /// Sends SIGKILL to the whole group. Only valid while the leader has not
    /// been reaped.
    pub fn kill(&self) {
        if let Some(pgid) = self.0 {
            // SAFETY: killpg only sends a signal; `pgid` passed the checks in
            // `new` and the leader has not been reaped (the guard is armed).
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
    }

    /// Disarms the guard: neither `kill` nor drop signals the group any more.
    /// MANDATORY right after the leader is reaped.
    pub fn disarm(&mut self) {
        if let Some(pgid) = self.0.take() {
            crate::runtime::watch::unregister(pgid);
        }
    }

    /// Whether `kill` and drop would still signal the group.
    #[cfg(test)]
    fn is_armed(&self) -> bool {
        self.0.is_some()
    }

    /// Awaits the leader's exit and disarms the guard in the same step, so
    /// the pid cannot be recycled while the guard is still armed.
    ///
    /// Cancel-safe: if the future is dropped before the wait completes, the
    /// leader is not reaped and the guard stays armed (and kills on drop).
    ///
    /// This does NOT kill the group: background grandchildren survive a
    /// leader that exits on its own. Adapters whose engine exits normally
    /// should use [`GroupKill::kill_and_reap`] instead.
    pub async fn reap(
        &mut self,
        child: &mut tokio::process::Child,
    ) -> std::io::Result<std::process::ExitStatus> {
        let status = child.wait().await;
        // `Child::wait` has reaped (or failed past reaping): either way the
        // pid must not be signalled any more.
        self.disarm();
        status
    }

    /// Kills the whole group, then reaps the leader and disarms. The leader
    /// has not been reaped yet, so its pgid is still ours to signal: this is
    /// the call for a normal leader exit, where `reap()` alone would leave
    /// background grandchildren running.
    pub async fn kill_and_reap(
        &mut self,
        child: &mut tokio::process::Child,
    ) -> std::io::Result<std::process::ExitStatus> {
        self.kill();
        self.reap(child).await
    }
}

impl Drop for GroupKill {
    fn drop(&mut self) {
        self.kill();
    }
}

/// When process `pid` started, in units only comparable with each other: what
/// tells it apart from a later process that reuses its pid. A zombie still
/// has one on Linux; `None` once it is gone, or where it cannot be read.
pub fn start_time(pid: libc::pid_t) -> Option<u64> {
    stat(pid).map(|(start, _)| start)
}

/// Whether `pid` is still the process that started at `start` and has not
/// ended: false for a zombie, a reused pid or a process that is gone.
pub fn running(pid: libc::pid_t, start: u64) -> bool {
    stat(pid) == Some((start, false))
}

/// The start time of `pid` and whether it is a zombie.
fn stat(pid: libc::pid_t) -> Option<(u64, bool)> {
    #[cfg(target_os = "linux")]
    {
        // pid (comm) state ppid ... starttime is field 22; comm may hold
        // spaces and parentheses, so count from the last ')'.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
        let state = fields.next()?;
        let start = fields.nth(18)?.parse().ok()?;
        Some((start, matches!(state, "Z" | "X")))
    }
    #[cfg(target_os = "macos")]
    {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: `info` is a writable buffer of exactly `size` bytes.
        let read = unsafe {
            libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
        };
        (read == size).then(|| {
            let start = info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec;
            (start, info.pbi_status == libc::SZOMB)
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// The state letter of a process (`Z` for a zombie), `None` once it is gone.
/// Linux reads `/proc`; elsewhere `ps` answers the same question.
#[cfg(test)]
pub(crate) fn process_state(pid: libc::pid_t) -> Option<char> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit(')').next()?.trim().chars().next()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output();
        ps_state(out)
    }
}

/// Reads the outcome of `ps -o stat= -p PID`, failing closed: only "ps ran
/// and found no such pid" (exit status 1, empty output, as on macOS/BSD)
/// counts as gone. A `ps` that cannot run, or ends any other way, panics so
/// that `wait_gone` and `is_gone` can never pass vacuously.
#[cfg(test)]
fn ps_state(out: std::io::Result<std::process::Output>) -> Option<char> {
    let out = out.unwrap_or_else(|e| panic!("`ps` could not run: {e}"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let state = stdout.trim().chars().next();
    match (out.status.code(), state) {
        (Some(1), None) => None,
        (Some(0), Some(c)) => Some(c),
        _ => panic!(
            "`ps` ended unexpectedly: {:?}, stdout {stdout:?}, stderr {:?}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    fn ps_output(code: i32, stdout: &str) -> std::io::Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        })
    }

    #[test]
    fn ps_state_reads_the_state_letter() {
        assert_eq!(ps_state(ps_output(0, "S+\n")), Some('S'));
        assert_eq!(ps_state(ps_output(0, "Z\n")), Some('Z'));
    }

    #[test]
    fn ps_state_treats_status_1_with_no_output_as_gone() {
        assert_eq!(ps_state(ps_output(1, "")), None);
    }

    #[test]
    #[should_panic(expected = "could not run")]
    fn ps_state_panics_when_ps_cannot_spawn() {
        let _ = ps_state(Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
    }

    #[test]
    #[should_panic(expected = "ended unexpectedly")]
    fn ps_state_panics_on_an_unexpected_exit_status() {
        let _ = ps_state(ps_output(2, ""));
    }

    #[test]
    #[should_panic(expected = "ended unexpectedly")]
    fn ps_state_panics_on_status_1_with_output() {
        let _ = ps_state(ps_output(1, "garbage"));
    }

    fn own_group() -> libc::pid_t {
        // SAFETY: getpgrp has no preconditions and cannot fail.
        unsafe { libc::getpgrp() }
    }

    #[test]
    fn refuses_pid_zero_which_would_signal_our_own_group() {
        assert!(GroupKill::new(0).is_none());
    }

    #[test]
    fn refuses_pid_one() {
        assert!(GroupKill::new(1).is_none());
    }

    #[test]
    fn refuses_negative_values() {
        assert!(GroupKill::new(-1).is_none());
        assert!(GroupKill::new(-12345).is_none());
        assert!(GroupKill::new(libc::pid_t::MIN).is_none());
    }

    #[test]
    fn refuses_our_own_process_group() {
        assert!(GroupKill::new(own_group()).is_none());
    }

    #[test]
    fn accepts_a_foreign_group_id() {
        // Pure check, no signal: a pid that is neither special nor our group.
        let other = own_group().wrapping_add(1).max(2);
        assert!(GroupKill::checked(other, own_group()).is_some_and(|mut g| {
            // Disarm so dropping it does not signal a group we do not own.
            g.disarm();
            true
        }));
    }

    #[tokio::test]
    async fn reap_waits_for_the_leader_and_disarms() {
        let mut child = tokio::process::Command::new("true")
            .process_group(0)
            .spawn()
            .expect("true starts");
        let pid = child.id().expect("a pid") as libc::pid_t;
        let mut group = GroupKill::new(pid).expect("a fresh group is a valid target");
        let status = group.reap(&mut child).await.expect("wait succeeds");
        assert!(status.success());
        assert!(!group.is_armed(), "reaping disarms the guard");
    }

    #[cfg(target_os = "linux")]
    /// Kills the recorded pid when dropped, so a failed assertion cannot leak
    /// a `sleep 300`.
    struct KillOnDrop(libc::pid_t);

    #[cfg(target_os = "linux")]
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            // SAFETY: a plain signal to a pid this test spawned.
            unsafe { libc::kill(self.0, libc::SIGKILL) };
        }
    }

    #[cfg(target_os = "linux")]
    /// No such process, or only a zombie nobody has collected yet.
    fn is_gone(pid: libc::pid_t) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => true,
            Ok(stat) => stat
                .rsplit(')')
                .next()
                .unwrap_or("")
                .trim()
                .starts_with('Z'),
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn kill_and_reap_takes_down_grandchildren_of_a_leader_that_exited() {
        use tokio::io::AsyncReadExt;

        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "sleep 300 >/dev/null 2>&1 & echo $!; exit 0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .process_group(0)
            .spawn()
            .expect("sh starts");
        let pid = child.id().expect("a pid") as libc::pid_t;
        let mut group = GroupKill::new(pid).expect("a fresh group is a valid target");
        let mut out = String::new();
        // EOF: the leader exited (the grandchild's stdout is redirected).
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            child.stdout.take().unwrap().read_to_string(&mut out),
        )
        .await
        .expect("the leader exits within 5 s")
        .expect("stdout reads");
        let grandchild: libc::pid_t = out.trim().parse().expect("the grandchild pid");
        let _cleanup = KillOnDrop(grandchild);

        let status = group
            .kill_and_reap(&mut child)
            .await
            .expect("wait succeeds");
        assert!(status.success());
        assert!(!group.is_armed(), "reaping disarms the guard");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !is_gone(grandchild) && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(is_gone(grandchild), "the group's grandchild is killed");
    }

    #[test]
    fn start_time_tells_a_process_apart_and_ends_with_it() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        let first = start_time(pid).expect("a running child has one");
        assert_eq!(start_time(pid), Some(first), "stable while it runs");
        let own = start_time(std::process::id() as libc::pid_t).expect("ours too");
        assert_ne!(own, 0);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(start_time(pid), None, "gone once reaped");
    }

    #[test]
    fn running_is_false_for_a_zombie_and_for_another_start() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        let start = start_time(pid).unwrap();
        assert!(running(pid, start));
        assert!(
            !running(pid, start + 1),
            "a reused pid is not the same process"
        );
        // SAFETY: kill only sends a signal to our own child.
        unsafe { libc::kill(pid, libc::SIGKILL) };
        while process_state(pid) != Some('Z') {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!running(pid, start), "a zombie has ended");
        child.wait().unwrap();
        assert!(!running(pid, start));
    }
}
