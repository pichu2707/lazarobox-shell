//! Starting `engram serve` for the user: finding the binary, spawning it
//! detached and keeping its log. Nothing here ever signals the server.

use std::{
    ffi::OsString,
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::{process::Command, sync::oneshot};

use crate::{
    core::ai::{
        memory::offer::{Binary, last_log_line},
        parse::parse_version,
    },
    runtime::ai_detect,
};

/// The log file name under the state directory.
const LOG_NAME: &str = "engram-serve.log";
/// A log larger than this is emptied before the next start.
const MAX_LOG: u64 = 1024 * 1024;
/// How much of the log's end is read for the failure line.
const TAIL: u64 = 4096;

/// A server that was spawned. Dropping it leaves the process running; the
/// only thing that ever ends it is the user (or the server itself).
#[derive(Debug)]
pub struct Running {
    pub pid: u32,
    /// Resolves when the child has exited and been reaped.
    pub exit: oneshot::Receiver<()>,
    /// Where this start's output begins in the log.
    pub log_from: u64,
}

/// The state directory for the log: `$XDG_STATE_HOME/lazarobox` when that is
/// set to an absolute path, else `$HOME/.local/state/lazarobox`.
pub fn state_dir(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let absolute = |value: OsString| Some(PathBuf::from(value)).filter(|p| p.is_absolute());
    let base = xdg
        .and_then(absolute)
        .or_else(|| home.and_then(absolute).map(|h| h.join(".local/state")))?;
    Some(base.join("lazarobox"))
}

/// What the worker needs to start a server. A trait so the worker's logic is
/// tested without processes; `Server` is the real one.
pub trait Launcher: Send + Sync + 'static {
    /// The first `engram` on the PATH and its version output.
    fn find(&self) -> impl Future<Output = Binary> + Send;
    /// Spawns `<binary> serve <port>` detached.
    fn spawn(&self, binary: &Path, port: u16) -> impl Future<Output = io::Result<Running>> + Send;
    /// The last log line written since `from`.
    fn log_line(&self, from: u64) -> impl Future<Output = Option<String>> + Send;
}

/// A launcher that does not look and cannot start: a worker without the
/// `engram serve` offer.
pub struct NoServer;

impl Launcher for NoServer {
    async fn find(&self) -> Binary {
        Binary::NotChecked
    }

    async fn spawn(&self, _binary: &Path, _port: u16) -> io::Result<Running> {
        Err(io::Error::other("starting a server is not available"))
    }

    async fn log_line(&self, _from: u64) -> Option<String> {
        None
    }
}

impl Launcher for Server {
    async fn find(&self) -> Binary {
        Server::find(self).await
    }

    async fn spawn(&self, binary: &Path, port: u16) -> io::Result<Running> {
        Server::spawn(self, binary, port).await
    }

    async fn log_line(&self, from: u64) -> Option<String> {
        Server::log_line(self, from).await
    }
}

/// Finds, starts and reads the log of the user's `engram serve`.
pub struct Server {
    path: OsString,
    state: Option<PathBuf>,
}

impl Server {
    /// A server for the process environment.
    pub fn from_env() -> Self {
        Self {
            path: std::env::var_os("PATH").unwrap_or_default(),
            state: state_dir(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME")),
        }
    }

    pub fn new(path: OsString, state: PathBuf) -> Self {
        Self {
            path,
            state: Some(state),
        }
    }

    /// The first `engram` on the PATH and what its version command says.
    pub async fn find(&self) -> Binary {
        let Some(path) = ai_detect::find_on_path("engram", &self.path) else {
            return Binary::Missing;
        };
        // A path that is not UTF-8 cannot be shown to the user as it is.
        let Some(shown) = path.to_str().map(str::to_string) else {
            return Binary::Missing;
        };
        let version = ai_detect::version(&path)
            .await
            .and_then(|out| parse_version(&out));
        Binary::Found {
            path: shown,
            version,
        }
    }

    /// Spawns `<binary> serve <port>` detached, its output appended to the
    /// log. The caller probes health; this only starts the process.
    pub async fn spawn(&self, binary: &Path, port: u16) -> io::Result<Running> {
        let state = self
            .state
            .as_deref()
            .ok_or_else(|| io::Error::other("no state directory"))?;
        let (log, log_from) = open_log(state)?;
        let mut child = None;
        for attempt in 0..5 {
            // A binary that is still being written fails with ETXTBSY for a
            // moment (an upgrade): that one error is retried briefly.
            match command(binary, port, &log)?.spawn() {
                Ok(spawned) => {
                    child = Some(spawned);
                    break;
                }
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(e) => return Err(e),
            }
        }
        let mut child = child.ok_or_else(|| io::Error::other("could not spawn"))?;
        let pid = child.id().ok_or_else(|| io::Error::other("no pid"))?;
        let (done, exit) = oneshot::channel();
        // The reaper: an early exit is waited for, so no zombie is left.
        tokio::spawn(async move {
            let _ = child.wait().await;
            let _ = done.send(());
        });
        Ok(Running {
            pid,
            exit,
            log_from,
        })
    }

    /// The last line this start wrote to the log, sanitized and capped.
    pub async fn log_line(&self, from: u64) -> Option<String> {
        let file = self.state.as_ref()?.join(LOG_NAME);
        tokio::task::spawn_blocking(move || {
            let mut file = File::open(file).ok()?;
            let len = file.metadata().ok()?.len();
            file.seek(SeekFrom::Start(from.max(len.saturating_sub(TAIL))))
                .ok()?;
            let mut tail = Vec::new();
            file.take(TAIL).read_to_end(&mut tail).ok()?;
            last_log_line(&tail)
        })
        .await
        .ok()?
    }
}

/// Opens the log for appending: directory 0700, file 0600, never through a
/// symlink, emptied when over `MAX_LOG`. Returns it and where this start's
/// output begins.
fn open_log(dir: &Path) -> io::Result<(File, u64)> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let log = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(LOG_NAME))?;
    let meta = log.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::other("the log is not a regular file"));
    }
    if meta.permissions().mode() & 0o777 != 0o600 {
        log.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    if meta.len() > MAX_LOG {
        log.set_len(0)?;
    }
    let from = log.metadata()?.len();
    Ok((log, from))
}

/// The spawn command: argv exactly `serve <port>`, no shell, no stdin, both
/// output streams on the log, `ENGRAM_PORT` removed (the argument wins),
/// everything else (including `ENGRAM_DATA_DIR`) inherited as is, its own
/// session, and left running if the handle is dropped.
fn command(binary: &Path, port: u16, log: &File) -> io::Result<Command> {
    let mut cmd = Command::new(binary);
    cmd.arg("serve")
        .arg(port.to_string())
        .env_remove("ENGRAM_PORT")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log.try_clone()?))
        .kill_on_drop(false);
    // SAFETY: the closure only calls `setsid`, which is async-signal-safe: a
    // terminal hang-up must not reach the server.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
        path::{Path, PathBuf},
        time::Duration,
    };

    use super::*;
    use crate::core::ai::memory::offer::Binary;

    /// Kills the pid it holds when dropped, so a failing test leaks nothing.
    struct Reap(Option<u32>);

    impl Reap {
        fn new(pid: u32) -> Self {
            Self(Some(pid))
        }
    }

    impl Drop for Reap {
        fn drop(&mut self) {
            if let Some(pid) = self.0.take() {
                // SAFETY: a plain signal to a pid this test spawned.
                unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            }
        }
    }

    fn alive(pid: u32) -> bool {
        // SAFETY: signal 0 only checks that the pid exists.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    /// A fake `engram` that reports a version, or records how it was run and
    /// then stays up (`exec sleep`, so the pid stays the script's).
    fn fake(dir: &Path, version: &str, record: &Path, tail: &str) -> PathBuf {
        let path = dir.join("engram");
        let body = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"--version\" ]; then echo '{version}'; exit 0; fi\n\
             {{ echo \"argc=$#\"; for a in \"$@\"; do echo \"arg=$a\"; done; \
             echo \"stdin=$(readlink /proc/$$/fd/0)\"; env | sort; }} > '{rec}.tmp'\n\
             mv '{rec}.tmp' '{rec}'\n\
             {tail}\n",
            rec = record.display()
        );
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    struct Rig {
        _dir: tempfile::TempDir,
        bin_dir: PathBuf,
        state: PathBuf,
        record: PathBuf,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir(&bin_dir).unwrap();
        let state = dir.path().join("state");
        let record = dir.path().join("record");
        Rig {
            _dir: dir,
            bin_dir,
            state,
            record,
        }
    }

    impl Rig {
        fn server(&self) -> Server {
            Server::new(self.bin_dir.clone().into(), self.state.clone())
        }

        async fn recorded(&self) -> String {
            for _ in 0..200 {
                if let Ok(text) = fs::read_to_string(&self.record) {
                    return text;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            panic!("the fake never recorded its run");
        }
    }

    fn lines_of(record: &str, key: &str) -> Vec<String> {
        record
            .lines()
            .filter_map(|l| l.strip_prefix(&format!("{key}=")).map(str::to_string))
            .collect()
    }

    #[test]
    fn the_state_dir_is_xdg_state_home_then_home() {
        let dir = |xdg: Option<&str>, home: Option<&str>| {
            state_dir(xdg.map(OsString::from), home.map(OsString::from))
        };
        assert_eq!(
            dir(Some("/x/state"), Some("/h")),
            Some(PathBuf::from("/x/state/lazarobox"))
        );
        assert_eq!(
            dir(None, Some("/h")),
            Some(PathBuf::from("/h/.local/state/lazarobox"))
        );
        // An empty or relative XDG value is ignored, as the XDG spec says.
        assert_eq!(
            dir(Some(""), Some("/h")),
            Some(PathBuf::from("/h/.local/state/lazarobox"))
        );
        assert_eq!(
            dir(Some("rel"), Some("/h")),
            Some(PathBuf::from("/h/.local/state/lazarobox"))
        );
        assert_eq!(dir(None, None), None);
        assert_eq!(dir(None, Some("relative-home")), None);
    }

    #[tokio::test]
    async fn find_gives_the_absolute_path_and_the_version_output() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exit 0");
        let found = rig.server().find().await;
        assert_eq!(
            found,
            Binary::Found {
                path: script.to_string_lossy().into_owned(),
                version: Some("engram 3.4.0".into()),
            }
        );
    }

    #[tokio::test]
    async fn find_reports_a_missing_binary_and_an_unreadable_version() {
        let rig = rig();
        assert_eq!(rig.server().find().await, Binary::Missing);
        let script = rig.bin_dir.join("engram");
        fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            rig.server().find().await,
            Binary::Found {
                path: script.to_string_lossy().into_owned(),
                version: None,
            }
        );
    }

    #[tokio::test]
    async fn find_treats_a_path_that_is_not_utf8_as_missing() {
        use std::os::unix::ffi::OsStrExt;
        let rig = rig();
        let odd = rig.bin_dir.join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
        fs::create_dir(&odd).unwrap();
        fake(&odd, "engram 3.4.0", &rig.record, "exit 0");
        let server = Server::new(odd.into_os_string(), rig.state.clone());
        assert_eq!(server.find().await, Binary::Missing);
    }

    #[tokio::test]
    async fn spawn_runs_serve_port_with_the_documented_environment() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let running = rig.server().spawn(&script, 8123).await.unwrap();
        let _reap = Reap::new(running.pid);
        let record = rig.recorded().await;
        assert_eq!(lines_of(&record, "argc"), ["2"]);
        assert_eq!(lines_of(&record, "arg"), ["serve", "8123"]);
        assert_eq!(lines_of(&record, "stdin"), ["/dev/null"]);
        assert!(
            lines_of(&record, "ENGRAM_PORT").is_empty(),
            "ENGRAM_PORT must not reach the child"
        );
        // ENGRAM_DATA_DIR is neither added nor changed.
        let inherited = std::env::var("ENGRAM_DATA_DIR").ok();
        assert_eq!(
            lines_of(&record, "ENGRAM_DATA_DIR"),
            inherited.into_iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_command_removes_engram_port_and_never_sets_the_data_dir() {
        let rig = rig();
        let log = fs::File::create(rig.record.with_extension("log")).unwrap();
        let cmd = command(Path::new("/bin/true"), 8123, &log).unwrap();
        let std_cmd = cmd.as_std();
        let envs: Vec<_> = std_cmd.get_envs().collect();
        assert!(
            envs.iter().any(|(k, v)| *k == "ENGRAM_PORT" && v.is_none()),
            "{envs:?}"
        );
        assert!(
            envs.iter().all(|(k, _)| *k != "ENGRAM_DATA_DIR"),
            "{envs:?}"
        );
        assert_eq!(std_cmd.get_program(), "/bin/true");
        let args: Vec<_> = std_cmd.get_args().collect();
        assert_eq!(args, ["serve", "8123"]);
    }

    #[tokio::test]
    async fn the_child_is_its_own_session_leader() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let running = rig.server().spawn(&script, 8123).await.unwrap();
        let _reap = Reap::new(running.pid);
        rig.recorded().await;
        let stat = fs::read_to_string(format!("/proc/{}/stat", running.pid)).unwrap();
        // pid (comm) state ppid pgrp session ...; comm may hold spaces.
        let after = stat.rsplit_once(')').unwrap().1;
        let fields: Vec<&str> = after.split_whitespace().collect();
        let (pgrp, session) = (fields[2], fields[3]);
        assert_eq!(session, running.pid.to_string(), "sid == pid");
        assert_eq!(pgrp, running.pid.to_string());
    }

    #[tokio::test]
    async fn the_log_is_private_under_a_permissive_umask_and_gets_both_streams() {
        let rig = rig();
        let script = fake(
            &rig.bin_dir,
            "engram 3.4.0",
            &rig.record,
            "echo to-stdout; echo to-stderr >&2; exec sleep 30",
        );
        // SAFETY: umask only changes this process's file mode mask.
        let old = unsafe { libc::umask(0) };
        let spawned = rig.server().spawn(&script, 8123).await;
        // SAFETY: restores the mask read above.
        unsafe { libc::umask(old) };
        let running = spawned.unwrap();
        let _reap = Reap::new(running.pid);
        rig.recorded().await;
        let mode = |p: &Path| fs::metadata(p).unwrap().mode() & 0o777;
        assert_eq!(mode(&rig.state), 0o700);
        let log = rig.state.join("engram-serve.log");
        assert_eq!(mode(&log), 0o600);
        for _ in 0..100 {
            let text = fs::read_to_string(&log).unwrap();
            if text.contains("to-stdout") && text.contains("to-stderr") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the child's output never reached the log");
    }

    #[tokio::test]
    async fn an_existing_loose_log_is_tightened_to_0600() {
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        let log = rig.state.join("engram-serve.log");
        fs::write(&log, "old\n").unwrap();
        fs::set_permissions(&log, fs::Permissions::from_mode(0o666)).unwrap();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let running = rig.server().spawn(&script, 8123).await.unwrap();
        let _reap = Reap::new(running.pid);
        assert_eq!(fs::metadata(&log).unwrap().mode() & 0o777, 0o600);
        assert!(fs::read_to_string(&log).unwrap().starts_with("old\n"));
    }

    #[tokio::test]
    async fn a_symlinked_log_is_refused_and_nothing_is_spawned() {
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        let target = rig.state.join("victim");
        fs::write(&target, "precious").unwrap();
        symlink(&target, rig.state.join("engram-serve.log")).unwrap();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let result = rig.server().spawn(&script, 8123).await;
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "precious");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!rig.record.exists(), "the binary must not have run");
    }

    #[tokio::test]
    async fn a_log_over_1_mib_is_truncated_and_one_at_exactly_1_mib_is_kept() {
        for (len, kept) in [(MAX_LOG + 1, false), (MAX_LOG, true)] {
            let rig = rig();
            fs::create_dir_all(&rig.state).unwrap();
            let log = rig.state.join("engram-serve.log");
            fs::write(&log, vec![b'x'; len as usize]).unwrap();
            let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
            let running = rig.server().spawn(&script, 8123).await.unwrap();
            let _reap = Reap::new(running.pid);
            let size = fs::metadata(&log).unwrap().len();
            if kept {
                assert!(size >= MAX_LOG, "{size}");
                assert_eq!(running.log_from, MAX_LOG);
            } else {
                assert!(size < 1024, "truncated, got {size}");
                assert_eq!(running.log_from, 0);
            }
        }
    }

    #[tokio::test]
    async fn the_child_survives_dropping_every_handle() {
        let rig = rig();
        let script = fake(&rig.bin_dir, "engram 3.4.0", &rig.record, "exec sleep 30");
        let server = rig.server();
        let running = server.spawn(&script, 8123).await.unwrap();
        let pid = running.pid;
        let _reap = Reap::new(pid);
        rig.recorded().await;
        drop(running);
        drop(server);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(alive(pid), "dropping the handle must not kill the server");
    }

    #[tokio::test]
    async fn a_child_that_exits_is_reaped_and_reported() {
        let rig = rig();
        let script = fake(
            &rig.bin_dir,
            "engram 3.4.0",
            &rig.record,
            "echo 'bind: address already in use' >&2; exit 1",
        );
        let running = rig.server().spawn(&script, 8123).await.unwrap();
        let (pid, from) = (running.pid, running.log_from);
        tokio::time::timeout(Duration::from_secs(5), running.exit)
            .await
            .expect("the exit is reported")
            .expect("the reaper task ran");
        // Reaped: no zombie is left under the test process.
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists()
                || fs::read_to_string(format!("/proc/{pid}/stat"))
                    .map(|s| !s.contains(") Z "))
                    .unwrap_or(true)
        );
        assert_eq!(
            rig.server().log_line(from).await.as_deref(),
            Some("bind: address already in use")
        );
    }

    #[tokio::test]
    async fn the_log_line_ignores_what_was_there_before_this_start() {
        let rig = rig();
        fs::create_dir_all(&rig.state).unwrap();
        let log = rig.state.join("engram-serve.log");
        fs::write(&log, "stale failure from an older run\n").unwrap();
        let from = fs::metadata(&log).unwrap().len();
        assert_eq!(rig.server().log_line(from).await, None);
        fs::write(&log, "stale failure from an older run\nnew failure\n").unwrap();
        assert_eq!(
            rig.server().log_line(from).await.as_deref(),
            Some("new failure")
        );
    }

    #[tokio::test]
    async fn a_path_with_spaces_and_shell_characters_needs_no_shell() {
        let rig = rig();
        let odd = rig.bin_dir.join("a b;$(touch pwned)");
        fs::create_dir(&odd).unwrap();
        let script = fake(&odd, "engram 3.4.0", &rig.record, "exec sleep 30");
        let running = rig.server().spawn(&script, 8123).await.unwrap();
        let _reap = Reap::new(running.pid);
        let record = rig.recorded().await;
        assert_eq!(lines_of(&record, "arg"), ["serve", "8123"]);
        assert!(!Path::new("pwned").exists());
    }
}
