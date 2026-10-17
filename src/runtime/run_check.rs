//! The check half of a run (design D21): the path policy for the exact bytes
//! of a safe command, against the shell's real directory. The disk work runs
//! on the blocking pool under a bound; a failure or timeout is a refusal.

use std::{path::PathBuf, time::Duration};

use tokio::{sync::mpsc, time::timeout};

use crate::{
    app::RunEvent,
    core::{
        ai::{
            chat::{approval::NOT_ALLOWED, engine::CallId},
            tools::{
                exec::{Verb, parse},
                paths::{PathVerdict, resolve_exec},
            },
        },
        layout::PaneId,
    },
    runtime::tools::{ToolEnv, fs::load_roots},
};

/// Longest the disk work of one check may take.
pub const TIMEOUT: Duration = Duration::from_secs(2);
pub const TIMED_OUT: &str = "the path check did not finish in time";
const NO_ROOTS: &str = "the secret locations cannot be read";

/// What the check reads from the process: the same environment the read tools
/// judge paths by.
pub fn process_env() -> ToolEnv {
    ToolEnv {
        home: std::env::var_os("HOME"),
        xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
        xdg_data_home: std::env::var_os("XDG_DATA_HOME"),
        origin: PathBuf::from("/"),
    }
}

/// Starts the checks of runs and sends their answers to the event loop.
pub struct Checker {
    env: ToolEnv,
    tx: mpsc::Sender<(PaneId, RunEvent)>,
}

impl Checker {
    pub fn new(env: ToolEnv, tx: mpsc::Sender<(PaneId, RunEvent)>) -> Self {
        Self { env, tx }
    }

    /// Checks `command` for `pane` against `cwd` on the blocking pool and
    /// sends exactly one answer. Returns at once; needs a tokio runtime.
    pub fn start(&self, pane: PaneId, call: CallId, cwd: PathBuf, command: String) {
        let (env, tx) = (self.env.clone(), self.tx.clone());
        tokio::spawn(async move {
            let work = move || check(&env, call, cwd, &command);
            let event = within(TIMEOUT, call, work).await;
            let _ = tx.send((pane, event)).await;
        });
    }
}

/// Parses `command` again (the same bytes the app routed) and judges every
/// path it touches against `cwd`, the shell's real directory. The answer
/// carries `cwd` and, for a `cd`, the real directory it goes to.
pub fn check(env: &ToolEnv, call: CallId, cwd: PathBuf, command: &str) -> RunEvent {
    let refused = |reason| RunEvent::Refused { call, reason };
    let safe = match parse(command) {
        Ok(safe) => safe,
        Err(fallback) => return refused(fallback.why),
    };
    let Ok(roots) = load_roots(env) else {
        return refused(NO_ROOTS);
    };
    let mut cd_target = None;
    for (input, want) in safe.targets() {
        match resolve_exec(&roots, &cwd, input, want) {
            PathVerdict::Allowed(real) if safe.verb() == Verb::Cd => cd_target = Some(real),
            PathVerdict::Allowed(_) => {}
            PathVerdict::Refused => return refused(NOT_ALLOWED),
        }
    }
    RunEvent::Checked {
        call,
        cwd,
        cd_target,
    }
}

/// Runs `work` on the blocking pool for at most `bound`. A check that is too
/// slow, or whose thread dies, is refused; the thread itself cannot be
/// stopped, but nothing waits for it.
pub async fn within(
    bound: Duration,
    call: CallId,
    work: impl FnOnce() -> RunEvent + Send + 'static,
) -> RunEvent {
    match timeout(bound, tokio::task::spawn_blocking(work)).await {
        Ok(Ok(event)) => event,
        _ => RunEvent::Refused {
            call,
            reason: TIMED_OUT,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    struct Fixture {
        _dir: tempfile::TempDir,
        home: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for sub in ["home/dev/app", "home/.ssh", "srv/data"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        Fixture {
            home: root.join("home"),
            _dir: dir,
        }
    }

    fn env(home: Option<&Path>) -> ToolEnv {
        ToolEnv {
            home: home.map(|h| h.as_os_str().to_owned()),
            xdg_config_home: None,
            xdg_data_home: None,
            origin: PathBuf::from("/"),
        }
    }

    fn run(f: &Fixture, cwd: &str, command: &str) -> RunEvent {
        check(&env(Some(&f.home)), CallId(4), f.home.join(cwd), command)
    }

    fn reason(event: &RunEvent) -> Option<&'static str> {
        match event {
            RunEvent::Refused { reason, .. } => Some(reason),
            RunEvent::Checked { .. } | RunEvent::NotRun { .. } => None,
        }
    }

    // S3.8: the answer carries the checked cwd and the resolved cd target.
    #[test]
    fn a_safe_command_is_checked_against_the_real_cwd_and_answers_with_it() {
        let f = fixture();
        assert_eq!(
            run(&f, "dev", "ls app"),
            RunEvent::Checked {
                call: CallId(4),
                cwd: f.home.join("dev"),
                cd_target: None,
            }
        );
        assert_eq!(
            run(&f, "dev", "cd app"),
            RunEvent::Checked {
                call: CallId(4),
                cwd: f.home.join("dev"),
                cd_target: Some(f.home.join("dev/app")),
            }
        );
        assert!(matches!(
            run(&f, "dev", "mkdir -p app/new/deeper"),
            RunEvent::Checked { .. }
        ));
    }

    #[test]
    fn a_path_outside_home_runs_unless_it_is_secret() {
        let f = fixture();
        let data = f.home.parent().unwrap().join("srv/data");
        let cd = format!("cd {}", data.display());
        assert!(matches!(run(&f, "dev", &cd), RunEvent::Checked { .. }));
        assert!(reason(&run(&f, "dev", "cd ../.ssh")).is_some());
        assert!(reason(&run(&f, "dev", "cd ~/.ssh")).is_some());
        assert!(reason(&run(&f, ".ssh", "ls")).is_some(), "the cwd itself");
    }

    #[test]
    fn mkdir_outside_home_and_a_missing_directory_are_refused() {
        let f = fixture();
        assert!(reason(&run(&f, "dev", "mkdir ../../srv/x")).is_some());
        assert!(reason(&run(&f, "dev", "cd nowhere")).is_some());
    }

    #[test]
    fn the_command_is_parsed_again_and_the_grammar_reason_comes_back() {
        let f = fixture();
        assert_eq!(reason(&run(&f, "dev", "ls *")), Some("shell metacharacter"));
        assert_eq!(
            reason(&run(&f, "dev", "git status")),
            Some("not a safe command")
        );
    }

    #[test]
    fn roots_that_cannot_be_loaded_refuse_everything() {
        let f = fixture();
        let none = check(&env(None), CallId(1), f.home.clone(), "pwd");
        assert!(reason(&none).is_some());
        let relative = ToolEnv {
            home: Some("relative".into()),
            ..env(None)
        };
        assert!(reason(&check(&relative, CallId(1), f.home.clone(), "pwd")).is_some());
    }

    // The disk work is bounded: a slow answer becomes a refusal.
    #[tokio::test]
    async fn a_check_that_takes_too_long_is_refused() {
        let slow = within(Duration::from_millis(20), CallId(8), || {
            std::thread::sleep(Duration::from_millis(400));
            RunEvent::Checked {
                call: CallId(8),
                cwd: PathBuf::from("/"),
                cd_target: None,
            }
        })
        .await;
        assert_eq!(
            slow,
            RunEvent::Refused {
                call: CallId(8),
                reason: TIMED_OUT
            }
        );
        let quick = within(Duration::from_secs(5), CallId(9), || RunEvent::Refused {
            call: CallId(9),
            reason: "x",
        })
        .await;
        assert_eq!(reason(&quick), Some("x"));
        assert_eq!(TIMEOUT, Duration::from_secs(2));
    }
}
