//! Child-process helpers shared by the runtime's probes and, later, the chat
//! CLI engines.

/// Makes reads of `fd` return at once (`WouldBlock`) instead of waiting.
pub fn nonblocking(fd: &impl std::os::fd::AsRawFd) {
    // SAFETY: `fcntl` on a descriptor the caller owns reads and sets flags.
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

/// Makes `command`'s child get `fd` as descriptor `target`, open. The
/// parent's `fd` stays close-on-exec, so no other child gets it.
pub fn hand_over(command: &mut tokio::process::Command, fd: std::os::fd::RawFd, target: i32) {
    // SAFETY: between fork and exec only `dup2` and `fcntl` run (both
    // async-signal-safe), on a descriptor the parent keeps open.
    unsafe {
        command.pre_exec(move || {
            // `dup2` onto itself keeps the close-on-exec flag: clear it.
            if fd == target {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            } else if libc::dup2(fd, target) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

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

    /// The leader's exit status once it has exited on its own, `None` while
    /// it runs. Before reaping it, the group gets SIGKILL for what the
    /// leader left in it: until the reap, the leader's pid (the group id)
    /// cannot be reused. A leader already reaped, or one the check cannot
    /// see, is only reaped and disarmed: its pid may be another process's
    /// by now. Disarms once reaped.
    pub fn reap_if_exited(
        &mut self,
        child: &mut tokio::process::Child,
    ) -> Option<std::io::Result<std::process::ExitStatus>> {
        let state = child
            .id()
            .and_then(|pid| libc::pid_t::try_from(pid).ok())
            .and_then(exited_unreaped);
        match state {
            Some(false) => return None,
            Some(true) => self.kill(),
            None => {}
        }
        let status = child.try_wait();
        self.disarm();
        match status {
            Ok(Some(status)) => Some(Ok(status)),
            Ok(None) => Some(Err(std::io::Error::other("exited but not reaped"))),
            Err(error) => Some(Err(error)),
        }
    }
}

/// Whether the child `pid` has exited, without reaping it. `None` when it
/// cannot be told (no such child: reaped elsewhere).
fn exited_unreaped(pid: libc::pid_t) -> Option<bool> {
    // SAFETY: an all-zero siginfo_t is a valid value.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a writable siginfo_t; WNOWAIT leaves the child
    // waitable, so nothing is reaped here.
    let read = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &raw mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    // SAFETY: waitid filled `info` (or left it zeroed, with WNOHANG).
    (read == 0).then(|| unsafe { info.si_pid() } != 0)
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

/// How long a test waits for a signalled process to be gone. Polling returns
/// as soon as it is, so a generous bound costs nothing on a pass; it only
/// matters on a runner so starved that a SIGKILLed process is not scheduled to
/// exit for seconds. Tests of the runtime use this one bound, never their own.
#[cfg(test)]
pub(crate) const GONE_WITHIN: std::time::Duration = std::time::Duration::from_secs(20);

/// No such process, or only a zombie nobody has collected yet.
#[cfg(test)]
pub(crate) fn stopped(pid: libc::pid_t) -> bool {
    matches!(process_state(pid), None | Some('Z'))
}

/// A gap between two polls this far above the poll interval means the runner
/// itself stalled (a starved VM), not that the process is slow to exit.
#[cfg(test)]
const STALL_GAP: std::time::Duration = std::time::Duration::from_secs(1);

/// After the deadline the wait keeps polling this long before it judges, so
/// one instant of bad luck at the deadline cannot fail a process that exits a
/// moment later.
#[cfg(test)]
const GRACE: std::time::Duration = std::time::Duration::from_millis(250);

/// What the wait loop does next.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
enum GoneStep {
    Gone,
    Poll,
    Fail,
}

/// The decisions of a gone-wait, free of clocks and sleeping so they can be
/// tested: it tracks the largest gap between polls, grants a grace after the
/// deadline and, when a stall was seen, extends the deadline once.
#[cfg(test)]
struct GoneWatch {
    start: std::time::Instant,
    last: std::time::Instant,
    end: std::time::Instant,
    grace_end: Option<std::time::Instant>,
    largest_gap: std::time::Duration,
    extended: bool,
}

#[cfg(test)]
impl GoneWatch {
    fn new(now: std::time::Instant, within: std::time::Duration) -> Self {
        Self {
            start: now,
            last: now,
            end: now + within,
            grace_end: None,
            largest_gap: std::time::Duration::ZERO,
            extended: false,
        }
    }

    /// Records a poll at `now` that found the process gone or not.
    fn step(&mut self, now: std::time::Instant, gone: bool) -> GoneStep {
        self.largest_gap = self
            .largest_gap
            .max(now.saturating_duration_since(self.last));
        self.last = now;
        if gone {
            return GoneStep::Gone;
        }
        if now < self.end {
            return GoneStep::Poll;
        }
        match self.grace_end {
            None => {
                self.grace_end = Some(now + GRACE);
                GoneStep::Poll
            }
            Some(grace_end) if now < grace_end => GoneStep::Poll,
            Some(_) if self.stalled() && !self.extended => {
                // A stall does not count against the process: wait once more.
                self.extended = true;
                self.end = now + GONE_WITHIN;
                self.grace_end = None;
                GoneStep::Poll
            }
            Some(_) => GoneStep::Fail,
        }
    }

    /// How long the wait has run, as of the last poll.
    fn elapsed(&self) -> std::time::Duration {
        self.last.saturating_duration_since(self.start)
    }

    fn largest_gap(&self) -> std::time::Duration {
        self.largest_gap
    }

    fn stalled(&self) -> bool {
        self.largest_gap > STALL_GAP
    }

    fn extended(&self) -> bool {
        self.extended
    }
}

/// The contents of a file, or why it could not be read: never a panic.
#[cfg(all(test, target_os = "linux"))]
fn read_or_missing(path: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) => format!("{path}: {}", text.trim().replace('\n', " | ")),
        Err(e) => format!("{path}: unreadable ({e})"),
    }
}

/// What a process that should be gone is doing: pid, state letter and, on
/// Linux, the State/PPid/Name lines of `/proc`.
#[cfg(test)]
pub(crate) fn describe(pid: libc::pid_t) -> String {
    describe_with(pid, &GoneWatch::new(std::time::Instant::now(), GONE_WITHIN))
}

/// [`describe`] plus what the wait saw of the runner: the largest gap between
/// polls, whether a stall extended the wait and, on Linux, its load.
#[cfg(test)]
fn describe_with(pid: libc::pid_t, watch: &GoneWatch) -> String {
    #[cfg(target_os = "linux")]
    let more = {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        let lines: Vec<&str> = status
            .lines()
            .filter(|l| {
                ["State:", "PPid:", "Name:"]
                    .iter()
                    .any(|k| l.starts_with(k))
            })
            .collect();
        format!("{lines:?}")
    };
    // No /proc elsewhere: `ps` answers, or says why it could not.
    #[cfg(not(target_os = "linux"))]
    let more = match std::process::Command::new("ps")
        .args(["-o", "stat=,ppid=,comm=", "-p", &pid.to_string()])
        .output()
    {
        Ok(out) => format!("ps: {}", String::from_utf8_lossy(&out.stdout).trim()),
        Err(e) => format!("no /proc here and `ps` could not run: {e}"),
    };
    let text = format!(
        "pid {pid} still runs after {:?}: state {:?} {more}\n\
         largest poll gap {:?} (a stall is above {STALL_GAP:?}); stall extension: {}",
        watch.elapsed(),
        process_state(pid),
        watch.largest_gap(),
        if watch.extended() { "yes" } else { "no" },
    );
    #[cfg(target_os = "linux")]
    let text = {
        let load: Vec<String> = ["/proc/loadavg", "/proc/pressure/cpu"]
            .iter()
            .map(|path| read_or_missing(path))
            .collect();
        format!("{text}\n{}", load.join("\n"))
    };
    text
}

/// Waits (on the async clock) for `pid` to be gone, a zombie counting as gone
/// unless `zombie` is false. Says what it found on stderr when it gives up, so
/// the output of the failed test shows it.
#[cfg(test)]
pub(crate) async fn wait_gone_with(pid: libc::pid_t, zombie: bool) -> bool {
    let done = |pid| {
        if zombie {
            stopped(pid)
        } else {
            process_state(pid).is_none()
        }
    };
    let mut watch = GoneWatch::new(std::time::Instant::now(), GONE_WITHIN);
    loop {
        match watch.step(std::time::Instant::now(), done(pid)) {
            GoneStep::Gone => return true,
            GoneStep::Poll => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
            GoneStep::Fail => {
                eprintln!("{}", describe_with(pid, &watch));
                return false;
            }
        }
    }
}

/// [`wait_gone_with`] counting a zombie as gone.
#[cfg(test)]
pub(crate) async fn wait_gone(pid: libc::pid_t) -> bool {
    wait_gone_with(pid, true).await
}

/// [`wait_gone`] for a test that cannot use the async clock.
#[cfg(test)]
pub(crate) fn wait_gone_blocking(pid: libc::pid_t) -> bool {
    wait_gone_blocking_with(pid, true)
}

/// [`wait_gone_with`] for a test that cannot use the async clock: a zombie
/// counts as gone unless `zombie` is false (the process must be reaped).
#[cfg(test)]
pub(crate) fn wait_gone_blocking_with(pid: libc::pid_t, zombie: bool) -> bool {
    let mut watch = GoneWatch::new(std::time::Instant::now(), GONE_WITHIN);
    loop {
        let gone = if zombie {
            stopped(pid)
        } else {
            process_state(pid).is_none()
        };
        match watch.step(std::time::Instant::now(), gone) {
            GoneStep::Gone => return true,
            GoneStep::Poll => std::thread::sleep(std::time::Duration::from_millis(20)),
            GoneStep::Fail => {
                eprintln!("{}", describe_with(pid, &watch));
                return false;
            }
        }
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

    #[tokio::test]
    async fn reap_if_exited_leaves_a_running_leader_alone_and_reaps_an_ended_one() {
        let mut child = tokio::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .expect("sleep starts");
        let pid = child.id().expect("a pid") as libc::pid_t;
        let mut group = GroupKill::new(pid).expect("a fresh group is a valid target");
        assert!(group.reap_if_exited(&mut child).is_none());
        assert!(group.is_armed(), "a running leader keeps the guard armed");
        // SAFETY: a plain signal to the sleep this test started.
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let status = loop {
            if let Some(status) = group.reap_if_exited(&mut child) {
                break status.expect("reaped");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        assert_eq!(status.signal(), Some(libc::SIGTERM));
        assert!(!group.is_armed(), "reaping disarms the guard");
        assert_eq!(child.id(), None, "reaped");
    }

    // A leader already reaped elsewhere: its pid may belong to another
    // process now, so the guard is only disarmed, never fired.
    #[tokio::test]
    async fn reap_if_exited_never_signals_a_leader_reaped_elsewhere() {
        let mut child = tokio::process::Command::new("true")
            .process_group(0)
            .spawn()
            .expect("true starts");
        let pid = child.id().expect("a pid") as libc::pid_t;
        let mut group = GroupKill::new(pid).expect("a fresh group is a valid target");
        child.wait().await.expect("reaped");
        // A fake target: a signal here would show up as a dead sentinel.
        let mut sentinel = tokio::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .expect("sleep starts");
        let target = sentinel.id().expect("a pid") as libc::pid_t;
        group.0 = Some(target);
        assert!(group.reap_if_exited(&mut child).is_some());
        assert!(!group.is_armed(), "disarmed");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            sentinel.try_wait().expect("try_wait").is_none(),
            "the group was signalled"
        );
        crate::runtime::watch::unregister(pid);
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
        assert!(
            super::wait_gone(grandchild).await,
            "the group's grandchild is killed"
        );
    }

    // The shared diagnostic names the pid and the state a "stopped" process
    // was left in, and a killed one (zombie or reaped) is gone.
    #[tokio::test]
    async fn a_process_that_still_runs_is_described_and_a_killed_one_is_gone() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        assert!(!stopped(pid));
        let why = describe(pid);
        assert!(
            why.contains(&format!("pid {pid} still runs")) && why.contains("state Some("),
            "{why}"
        );
        // Only Linux has the /proc lines; elsewhere `ps` answers.
        #[cfg(target_os = "linux")]
        assert!(why.contains("PPid:"), "{why}");
        #[cfg(not(target_os = "linux"))]
        assert!(why.contains("ps: "), "{why}");
        child.kill().unwrap();
        assert!(wait_gone(pid).await);
        child.wait().unwrap();
        assert!(wait_gone_blocking(pid));
    }

    // The strict blocking wait lets a zombie stand and ends only when the
    // process is reaped; the lenient one accepts the zombie.
    #[test]
    fn the_strict_blocking_wait_needs_the_process_reaped() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        child.kill().unwrap();
        assert!(wait_gone_blocking(pid), "a zombie counts as gone");
        child.wait().unwrap();
        assert!(wait_gone_blocking_with(pid, false), "reaped is gone");
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

    use std::time::{Duration, Instant};

    /// Drives a watch with a fake clock: each poll happens `gap` after the last.
    fn drive(watch: &mut GoneWatch, now: &mut Instant, gap: Duration, gone: bool) -> GoneStep {
        *now += gap;
        watch.step(*now, gone)
    }

    #[test]
    fn a_gone_process_ends_the_wait_at_once() {
        let t0 = Instant::now();
        let mut w = GoneWatch::new(t0, GONE_WITHIN);
        assert_eq!(w.step(t0, true), GoneStep::Gone);
    }

    #[test]
    fn without_a_stall_the_wait_fails_after_the_deadline_and_the_grace() {
        let t0 = Instant::now();
        let mut w = GoneWatch::new(t0, GONE_WITHIN);
        let mut now = t0;
        let poll = Duration::from_millis(20);
        assert_eq!(drive(&mut w, &mut now, poll, false), GoneStep::Poll);
        // Steady polls up to the deadline, none of them a stall.
        for _ in 0..40 {
            drive(&mut w, &mut now, Duration::from_millis(500), false);
        }
        // Past the deadline: grace first, not a verdict from one instant.
        assert_eq!(drive(&mut w, &mut now, poll, false), GoneStep::Poll);
        assert_eq!(drive(&mut w, &mut now, poll, false), GoneStep::Poll);
        // The grace is over.
        assert_eq!(drive(&mut w, &mut now, GRACE, false), GoneStep::Fail);
        assert!(!w.extended());
    }

    #[test]
    fn a_process_that_goes_during_the_grace_passes() {
        let t0 = Instant::now();
        let mut w = GoneWatch::new(t0, GONE_WITHIN);
        let mut now = t0;
        assert_eq!(drive(&mut w, &mut now, GONE_WITHIN, false), GoneStep::Poll);
        let poll = Duration::from_millis(20);
        assert_eq!(drive(&mut w, &mut now, poll, true), GoneStep::Gone);
    }

    #[test]
    fn a_stall_extends_the_wait_once() {
        let t0 = Instant::now();
        let mut w = GoneWatch::new(t0, GONE_WITHIN);
        let mut now = t0;
        // The runner freezes for longer than STALL_GAP between two polls.
        assert_eq!(
            drive(&mut w, &mut now, STALL_GAP * 3, false),
            GoneStep::Poll
        );
        assert_eq!(w.largest_gap(), STALL_GAP * 3);
        assert!(w.stalled());
        // Deadline and grace run out: extended once, so keep waiting.
        assert_eq!(drive(&mut w, &mut now, GONE_WITHIN, false), GoneStep::Poll);
        assert_eq!(drive(&mut w, &mut now, GRACE, false), GoneStep::Poll);
        assert!(w.extended());
        // A process that exits in the extra time still passes.
        let mut other = GoneWatch::new(t0, GONE_WITHIN);
        let mut n2 = t0;
        drive(&mut other, &mut n2, STALL_GAP * 3, false);
        drive(&mut other, &mut n2, GONE_WITHIN, false);
        drive(&mut other, &mut n2, GRACE, false);
        assert_eq!(
            drive(&mut other, &mut n2, Duration::from_millis(20), true),
            GoneStep::Gone
        );
        // The second expiry is final: a stall does not extend twice.
        let poll = Duration::from_millis(20);
        assert_eq!(drive(&mut w, &mut now, GONE_WITHIN, false), GoneStep::Poll);
        assert_eq!(drive(&mut w, &mut now, poll, false), GoneStep::Poll);
        assert_eq!(drive(&mut w, &mut now, GRACE, false), GoneStep::Fail);
    }

    #[test]
    fn small_gaps_are_not_a_stall() {
        let t0 = Instant::now();
        let mut w = GoneWatch::new(t0, GONE_WITHIN);
        let mut now = t0;
        for _ in 0..10 {
            drive(&mut w, &mut now, Duration::from_millis(30), false);
        }
        assert!(!w.stalled());
        assert_eq!(w.largest_gap(), Duration::from_millis(30));
    }

    #[test]
    fn the_description_reports_the_gap_and_the_extension() {
        let t0 = Instant::now();
        let mut w = GoneWatch::new(t0, GONE_WITHIN);
        w.step(t0 + Duration::from_millis(2500), false);
        let text = describe_with(std::process::id() as libc::pid_t, &w);
        assert!(text.contains("largest poll gap 2.5s"), "{text}");
        assert!(text.contains("stall extension: no"), "{text}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_description_carries_the_load_on_linux() {
        let w = GoneWatch::new(Instant::now(), GONE_WITHIN);
        let text = describe_with(std::process::id() as libc::pid_t, &w);
        assert!(text.contains("/proc/loadavg:"), "{text}");
        assert!(text.contains("/proc/pressure/cpu:"), "{text}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_missing_proc_file_is_named_not_a_panic() {
        let text = read_or_missing("/proc/no-such-file-here");
        assert!(text.contains("unreadable"), "{text}");
    }
}
