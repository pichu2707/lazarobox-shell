//! Runs the local clipboard tool of a copy (see [`crate::core::clipboard`]).
//!
//! One worker thread owns every run, so two copies never race on the same
//! tool and the loop never waits. Invariants:
//! - the newest queued copy wins: older ones are dropped before a run;
//! - the tool's stdout and stderr are null (`xclip`, `xsel` and `wl-copy`
//!   fork a server that would hold a captured pipe open for good);
//! - a run is bounded: past the timeout the child is killed and reaped.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::core::clipboard::{Session, Tool, candidates};
use crate::runtime::desktop::which;

/// How long a tool may run before it is killed and the copy reported as
/// timed out.
pub const CLIP_TIMEOUT: Duration = Duration::from_secs(2);

/// How often a running tool is checked for its exit.
const POLL: Duration = Duration::from_millis(10);

/// What a finished run reports: the copy's number and how it went.
pub type Report = Box<dyn Fn(u64, Result<(), String>) + Send + 'static>;

struct Job {
    seq: u64,
    text: String,
}

/// The worker that puts text on the clipboard with one local tool.
pub struct Clipboard {
    tool: Tool,
    jobs: mpsc::Sender<Job>,
}

impl Clipboard {
    /// The first of the session's tools found on `path`, with its program.
    pub fn find(path: &OsStr, session: Session) -> Option<(Tool, PathBuf)> {
        Self::find_in(session, &|cmd| which(cmd, Some(path)))
    }

    fn find_in(
        session: Session,
        lookup: &dyn Fn(&str) -> Option<PathBuf>,
    ) -> Option<(Tool, PathBuf)> {
        candidates(session)
            .into_iter()
            .find_map(|tool| lookup(tool.name()).map(|program| (tool, program)))
    }

    /// Starts the worker for `tool` run as `program`. `report` is called on
    /// the worker thread once per run.
    pub fn start(tool: Tool, program: PathBuf, timeout: Duration, report: Report) -> Self {
        let (jobs, queue) = mpsc::channel();
        thread::spawn(move || {
            while let Some(job) = newest(&queue) {
                report(job.seq, run(tool, &program, &job.text, timeout));
            }
        });
        Self { tool, jobs }
    }

    /// The tool this worker runs.
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Queues copy `seq` of `text`; the answer comes through the report.
    /// An error means the worker is gone and nothing will answer.
    pub fn copy(&self, seq: u64, text: String) -> Result<(), String> {
        self.jobs
            .send(Job { seq, text })
            .map_err(|_| "the clipboard worker stopped".to_owned())
    }
}

/// The newest queued job, dropping the older ones; waits for the first.
/// `None` once every sender is gone.
fn newest(queue: &mpsc::Receiver<Job>) -> Option<Job> {
    let mut job = queue.recv().ok()?;
    while let Ok(next) = queue.try_recv() {
        job = next;
    }
    Some(job)
}

/// Runs `tool` as `program` with `text` on stdin and waits at most
/// `timeout`. `Err` carries a short reason for the notice.
fn run(tool: Tool, program: &Path, text: &str, timeout: Duration) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(tool.args())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let payload = tool.payload(text);
        // On its own thread: a tool that never reads must not stall the
        // wait below. A tool that exits early breaks the pipe; its exit
        // status says how it went, so the write error is not read.
        thread::spawn(move || {
            let _ = stdin.write_all(&payload);
        });
    }
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(match status.code() {
                    Some(code) => format!("exit code {code}"),
                    None => "killed by a signal".to_owned(),
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".to_owned());
            }
            Ok(None) => thread::sleep(POLL),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::time::Instant;

    use super::*;

    type Answer = (u64, Result<(), String>);

    /// A fake tool: the script `body` under `name` in `dir`.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let program = dir.join(name);
        fs::write(&program, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        program
    }

    fn worker(program: PathBuf, timeout: Duration) -> (Clipboard, mpsc::Receiver<Answer>) {
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let report: Report = Box::new(move |seq, result| {
            let _ = tx.lock().unwrap().send((seq, result));
        });
        (Clipboard::start(Tool::WlCopy, program, timeout, report), rx)
    }

    fn answer(rx: &mpsc::Receiver<Answer>) -> Answer {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the worker never answered")
    }

    fn wait_for(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(text) = fs::read_to_string(path)
                && !text.is_empty()
            {
                return text;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{} never appeared", path.display());
    }

    #[test]
    fn the_first_tool_found_for_the_session_is_the_one() {
        let session = Session {
            wayland: true,
            ..Session::default()
        };
        let found = |names: &'static [&'static str]| {
            move |cmd: &str| {
                names
                    .contains(&cmd)
                    .then(|| PathBuf::from(format!("/usr/bin/{cmd}")))
            }
        };
        assert_eq!(
            Clipboard::find_in(session, &found(&["xclip", "wl-copy"])),
            Some((Tool::WlCopy, PathBuf::from("/usr/bin/wl-copy")))
        );
        assert_eq!(
            Clipboard::find_in(session, &found(&["xsel"])),
            Some((Tool::Xsel, PathBuf::from("/usr/bin/xsel")))
        );
        assert_eq!(Clipboard::find_in(session, &found(&[])), None);
        let ssh = Session {
            ssh: true,
            ..session
        };
        assert_eq!(Clipboard::find_in(ssh, &found(&["wl-copy"])), None);
    }

    #[test]
    fn a_tool_that_exits_zero_gets_the_text_on_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let program = script(
            dir.path(),
            "wl-copy",
            &format!("cat > '{0}.tmp'\nmv '{0}.tmp' '{0}'", out.display()),
        );
        let (clip, rx) = worker(program, CLIP_TIMEOUT);
        clip.copy(7, "héllo\nworld".to_owned()).unwrap();
        assert_eq!(answer(&rx), (7, Ok(())));
        assert_eq!(wait_for(&out), "héllo\nworld");
    }

    #[test]
    fn a_tool_that_exits_with_an_error_gives_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "wl-copy", "cat > /dev/null\nexit 3");
        let (clip, rx) = worker(program, CLIP_TIMEOUT);
        clip.copy(1, "x".to_owned()).unwrap();
        assert_eq!(answer(&rx), (1, Err("exit code 3".to_owned())));
    }

    #[test]
    fn a_tool_that_cannot_start_gives_the_reason() {
        let (clip, rx) = worker(PathBuf::from("/nonexistent/wl-copy"), CLIP_TIMEOUT);
        clip.copy(1, "x".to_owned()).unwrap();
        let (seq, result) = answer(&rx);
        assert_eq!(seq, 1);
        assert!(result.unwrap_err().contains("No such file"));
    }

    #[test]
    fn a_tool_that_hangs_is_killed_and_reported_as_timed_out() {
        let dir = tempfile::tempdir().unwrap();
        let pid = dir.path().join("pid");
        let program = script(
            dir.path(),
            "wl-copy",
            &format!("echo $$ > '{}'\nexec sleep 30", pid.display()),
        );
        let (clip, rx) = worker(program, Duration::from_secs(1));
        clip.copy(2, "x".to_owned()).unwrap();
        assert_eq!(answer(&rx), (2, Err("timed out".to_owned())));
        let pid = wait_for(&pid);
        // `kill -0` succeeds only while the process exists.
        let alive = Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive, "the child is still there");
    }

    /// Reads `/proc`, so Linux only.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_tool_gets_null_stdout_and_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("fds");
        let program = script(
            dir.path(),
            "wl-copy",
            &format!(
                "cat > /dev/null\none=$(readlink /proc/$$/fd/1)\ntwo=$(readlink /proc/$$/fd/2)\nprintf '%s\\n%s\\n' \"$one\" \"$two\" > '{0}.tmp'\nmv '{0}.tmp' '{0}'",
                out.display()
            ),
        );
        let (clip, rx) = worker(program, CLIP_TIMEOUT);
        clip.copy(1, "x".to_owned()).unwrap();
        assert_eq!(answer(&rx).1, Ok(()));
        assert_eq!(wait_for(&out), "/dev/null\n/dev/null\n");
    }

    #[test]
    fn the_newest_queued_copy_wins() {
        let dir = tempfile::tempdir().unwrap();
        let started = dir.path().join("started");
        let log = dir.path().join("log");
        let release = dir.path().join("release");
        let program = script(
            dir.path(),
            "wl-copy",
            &format!(
                "echo go > '{}'\ncat >> '{}'\necho >> '{1}'\nwhile [ ! -f '{}' ]; do sleep 0.02; done",
                started.display(),
                log.display(),
                release.display()
            ),
        );
        let (clip, rx) = worker(program, CLIP_TIMEOUT);
        clip.copy(1, "first".to_owned()).unwrap();
        wait_for(&started);
        clip.copy(2, "second".to_owned()).unwrap();
        clip.copy(3, "third".to_owned()).unwrap();
        fs::write(&release, "go").unwrap();
        assert_eq!(answer(&rx), (1, Ok(())));
        assert_eq!(answer(&rx), (3, Ok(())));
        assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());
        assert_eq!(fs::read_to_string(&log).unwrap(), "first\nthird\n");
    }
}
