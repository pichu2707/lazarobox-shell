//! The CLI runtime against fake CLIs: shell scripts in a temp dir that stand
//! in for `claude`. Real processes and the real clock, every wait bounded. No
//! real CLI, credential file or network is touched.

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use tempfile::TempDir;
use tokio::{sync::mpsc, time::timeout};

use super::*;
use crate::{
    core::{
        ai::{
            chat::{
                adapters::{Job, Wire},
                engine::{ChatError, ChatMessage, ChatRequest, Finish, Role, StreamEvent},
            },
            jail::{Backend, Cli, JailPolicy, JailSupport},
        },
        layout::PaneId,
    },
    runtime::{
        chat::{APP_CHANNEL, Chats, runner_with},
        loopback::Timeouts,
    },
};

/// A claude that answers `hi` and records how it was run in `$REC`.
const GOOD: &str = r#"
printf '%s\0' "$@" > "$REC/args"
cat > "$REC/stdin"
pwd > "$REC/pwd"
env > "$REC/env"
test -d "$TMPDIR" && echo yes > "$REC/tmpdir_existed"
printf '%s\n' '{"type":"system","subtype":"init","tools":[]}'
printf '%s\n' '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}},"parent_tool_use_id":null}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"hi","stop_reason":"end_turn"}'
"#;

/// A scratch machine: a `bin` dir with the fake CLI, a private data dir, a
/// home and a record dir. Under the crate dir, not `/tmp`, which the jail
/// layout refuses as too broad.
struct Lab {
    _root: TempDir,
    bin: PathBuf,
    data: PathBuf,
    home: PathBuf,
    rec: PathBuf,
}

fn lab() -> Lab {
    let root = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let base = fs::canonicalize(root.path()).unwrap();
    let lab = Lab {
        bin: base.join("bin"),
        data: base.join("data/lazarobox"),
        home: base.join("home"),
        rec: base.join("rec"),
        _root: root,
    };
    for dir in [&lab.bin, &lab.home, &lab.rec] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::create_dir_all(&lab.data).unwrap();
    lab
}

fn script(path: &Path, rec: &Path, body: &str) {
    let text = format!("#!/bin/sh\nREC='{}'\n{body}\n", rec.display());
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl Lab {
    fn claude(&self, body: &str) -> &Self {
        script(&self.bin.join("claude"), &self.rec, body);
        self
    }

    fn helper(&self, body: &str) -> PathBuf {
        let path = self.rec.join("helper");
        script(&path, &self.rec, body);
        path
    }

    fn env(&self) -> CliEnv {
        CliEnv {
            path: self.bin.clone().into_os_string(),
            home: Some(self.home.clone()),
            data: Some(self.data.clone()),
            jail: JailSupport::Unavailable("test"),
            helper: PathBuf::from("/nonexistent/helper"),
            timeouts: Timeouts {
                connect: Duration::from_secs(1),
                first_byte: Duration::from_secs(10),
                idle: Duration::from_secs(10),
                total: Duration::from_secs(30),
                slow: Duration::from_secs(20),
            },
            grace: Duration::from_secs(2),
            inject: Vec::new(),
        }
    }

    fn recorded(&self, name: &str) -> String {
        fs::read_to_string(self.rec.join(name)).unwrap_or_default()
    }

    fn args(&self) -> Vec<String> {
        self.recorded("args")
            .split('\0')
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

fn jail_on() -> JailSupport {
    JailSupport::Available {
        backend: Backend::Landlock,
        abi: 7,
    }
}

fn request() -> ChatRequest {
    ChatRequest {
        engine: "claude".into(),
        model: Some("sonnet".into()),
        system: "be brief".into(),
        memory: None,
        messages: vec![ChatMessage {
            role: Role::User,
            content: "hi".into(),
        }],
    }
}

fn job(policy: JailPolicy) -> Job {
    Job {
        wire: Wire::ClaudeStreamJson,
        binary: "claude",
        cli: Cli::Claude,
        args: vec!["-p".into()],
        stdin: b"hi".to_vec(),
        jail: policy,
    }
}

/// One turn through `process_turn`, up to and including the final event.
async fn run_job(env: &CliEnv, job: Job) -> Vec<StreamEvent> {
    let (tx, mut rx) = mpsc::channel(64);
    let turn = async {
        process_turn(env, job, &tx).await;
        drop(tx);
    };
    let collect = async {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    };
    drop_after(turn, collect).await
}

/// Runs `turn`, drops its sender and gathers what it sent.
async fn drop_after<F, G>(turn: F, collect: G) -> Vec<StreamEvent>
where
    F: std::future::Future<Output = ()>,
    G: std::future::Future<Output = Vec<StreamEvent>>,
{
    let both = async {
        let (_, events) = tokio::join!(turn, collect);
        events
    };
    timeout(Duration::from_secs(30), both)
        .await
        .expect("the turn ends within 30 s")
}

/// One turn through the real runner, up to the final event.
async fn run(env: CliEnv, req: ChatRequest) -> Vec<StreamEvent> {
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(runner_with(None, env), tx);
    chats.send(PaneId::for_test(1), 1, req);
    let mut events = Vec::new();
    loop {
        let (_, _, event) = timeout(Duration::from_secs(30), rx.recv())
            .await
            .expect("an event within 30 s")
            .expect("the channel is open");
        let last = matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_));
        events.push(event);
        if last {
            return events;
        }
    }
}

fn failure(events: &[StreamEvent]) -> String {
    match events.last() {
        Some(StreamEvent::Error(ChatError::Failed(text))) => text.clone(),
        other => panic!("a failure expected, got {other:?} in {events:?}"),
    }
}

fn gone(pid: libc::pid_t) -> bool {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit(')')
            .next()
            .unwrap_or("")
            .trim()
            .starts_with('Z'),
    }
}

async fn wait_gone(pid: libc::pid_t, bound: Duration) -> bool {
    let end = Instant::now() + bound;
    while !gone(pid) && Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    gone(pid)
}

fn pid_of(lab: &Lab, name: &str) -> libc::pid_t {
    lab.recorded(name).trim().parse().expect("a recorded pid")
}

#[tokio::test]
async fn a_turn_streams_the_answer_and_ends_with_done() {
    let lab = lab();
    lab.claude(GOOD);
    let events = run(lab.env(), request()).await;
    assert_eq!(
        events,
        [
            StreamEvent::Delta("hi".into()),
            StreamEvent::Done(Finish::Stop)
        ]
    );
}

#[tokio::test]
async fn stdin_gets_the_transcript_then_eof_and_argv_never_holds_it() {
    let lab = lab();
    lab.claude(GOOD);
    let mut req = request();
    req.messages[0].content = "the secret question".into();
    run(lab.env(), req).await;
    // `cat` only returns at EOF, so a recorded file proves the pipe closed.
    assert_eq!(lab.recorded("stdin"), "the secret question");
    let args = lab.args();
    assert_eq!(args[0], "-p");
    assert!(args.contains(&"--model=sonnet".to_owned()));
    assert!(args.contains(&"be brief".to_owned()));
    assert!(args.iter().all(|a| !a.contains("secret question")));
}

#[tokio::test]
async fn there_is_no_shell_so_metacharacters_stay_data() {
    let lab = lab();
    lab.claude(GOOD);
    let pwned = lab.rec.join("pwned");
    let nasty = format!(
        "; touch {} $(touch {}2) | cat `id`",
        pwned.display(),
        pwned.display()
    );
    let mut req = request();
    req.system = nasty.clone();
    req.messages[0].content = nasty.clone();
    let events = run(lab.env(), req).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert!(lab.args().contains(&nasty), "the prompt is one argument");
    assert_eq!(lab.recorded("stdin"), nasty);
    assert!(!pwned.exists() && !lab.rec.join("pwned2").exists());
}

#[tokio::test]
async fn the_cli_runs_in_a_private_cwd_with_a_private_tmpdir() {
    let lab = lab();
    lab.claude(GOOD);
    run(lab.env(), request()).await;
    let cwd = lab.data.join("cli-cwd");
    assert_eq!(lab.recorded("pwd").trim(), cwd.to_str().unwrap());
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&cwd), 0o700);
    assert_eq!(mode(&lab.data.join("cli-tmp")), 0o700);
    assert_eq!(
        fs::read_dir(&cwd).unwrap().count(),
        0,
        "the cwd stays empty"
    );
    let env = lab.recorded("env");
    let tmp = env
        .lines()
        .find_map(|l| l.strip_prefix("TMPDIR="))
        .expect("TMPDIR is set");
    assert!(
        tmp.starts_with(lab.data.join("cli-tmp").to_str().unwrap()),
        "{tmp}"
    );
    assert_eq!(lab.recorded("tmpdir_existed").trim(), "yes");
    assert!(env.lines().any(|l| l == "NO_COLOR=1"));
    assert!(
        !Path::new(tmp).exists(),
        "the tmp dir is wiped after the turn"
    );
}

#[tokio::test]
async fn a_loose_cwd_is_tightened_and_a_symlinked_one_is_refused() {
    let lab = lab();
    lab.claude(GOOD);
    let cwd = lab.data.join("cli-cwd");
    fs::create_dir(&cwd).unwrap();
    fs::set_permissions(&cwd, fs::Permissions::from_mode(0o755)).unwrap();
    run(lab.env(), request()).await;
    assert_eq!(
        fs::metadata(&cwd).unwrap().permissions().mode() & 0o777,
        0o700
    );

    fs::remove_dir(&cwd).unwrap();
    let elsewhere = lab.rec.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, &cwd).unwrap();
    fs::remove_file(lab.rec.join("args")).ok();
    let events = run(lab.env(), request()).await;
    assert_eq!(
        failure(&events),
        "cannot prepare the private working directory"
    );
    assert!(!lab.rec.join("args").exists(), "the CLI never started");
}

#[test]
fn the_command_drops_the_agent_variables_and_sets_the_private_ones() {
    let command = command(
        Path::new("/bin/true"),
        vec![OsString::from("-p")],
        (Path::new("/data/cli-cwd"), Path::new("/data/cli-tmp/1-1")),
        &[],
    );
    let envs: Vec<_> = command
        .as_std()
        .get_envs()
        .map(|(k, v)| (k.to_owned(), v.map(OsString::from)))
        .collect();
    let has = |key: &str, value: Option<&str>| {
        envs.contains(&(OsString::from(key), value.map(OsString::from)))
    };
    for name in SCRUBBED {
        assert!(has(name, None), "{name} removed");
    }
    assert!(has("NO_COLOR", Some("1")));
    assert!(has("TMPDIR", Some("/data/cli-tmp/1-1")));
    assert_eq!(
        command.as_std().get_current_dir(),
        Some(Path::new("/data/cli-cwd"))
    );
}

#[tokio::test]
async fn the_cli_sees_neither_agents_nor_outer_claude_state_but_keeps_its_settings() {
    let lab = lab();
    lab.claude(GOOD);
    let mut env = lab.env();
    let gone = [
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "SSH_ASKPASS",
        "GIT_ASKPASS",
        "GPG_AGENT_INFO",
        "CLAUDECODE",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_SSE_PORT",
    ];
    let kept = [
        "ANTHROPIC_BASE_URL",
        "HTTPS_PROXY",
        "SSL_CERT_FILE",
        "XDG_CONFIG_HOME",
        "DBUS_SESSION_BUS_ADDRESS",
    ];
    for name in gone.iter().chain(&kept) {
        env.inject.push(((*name).into(), "v".into()));
    }
    run(env, request()).await;
    let dump = lab.recorded("env");
    let has = |name: &str| dump.lines().any(|l| l.starts_with(&format!("{name}=")));
    for name in gone {
        assert!(!has(name), "{name} must be removed");
    }
    for name in kept {
        assert!(has(name), "{name} must be kept");
    }
    assert!(has("HOME") && has("PATH"));
}

#[tokio::test]
async fn a_failing_cli_shows_the_last_stderr_line_sanitized() {
    let lab = lab();
    lab.claude(
        "cat > /dev/null\necho first >&2\nprintf 'second \\033[31mred\\033[0m\\n' >&2\nexit 1",
    );
    let events = run(lab.env(), request()).await;
    assert_eq!(failure(&events), "second red");
}

#[tokio::test]
async fn a_huge_stderr_does_not_block_and_only_its_tail_counts() {
    let lab = lab();
    lab.claude("cat > /dev/null\nhead -c 300000 /dev/zero | tr '\\0' x >&2\necho >&2\necho the end >&2\nexit 1");
    let events = run(lab.env(), request()).await;
    assert_eq!(failure(&events), "the end");
}

#[tokio::test]
async fn a_non_zero_exit_with_no_output_is_an_error_naming_the_status() {
    let lab = lab();
    lab.claude("cat > /dev/null\nexit 7");
    let events = run(lab.env(), request()).await;
    assert_eq!(failure(&events), "claude exited with status 7");
}

#[tokio::test]
async fn a_clean_exit_with_no_output_is_an_error() {
    let lab = lab();
    lab.claude("cat > /dev/null\nexit 0");
    let events = run(lab.env(), request()).await;
    assert_eq!(events, [StreamEvent::Error(ChatError::Interrupted)]);
}

#[tokio::test]
async fn an_exit_125_without_the_marker_is_an_ordinary_failure() {
    let lab = lab();
    lab.claude("cat > /dev/null\nexit 125");
    let events = run(lab.env(), request()).await;
    assert_eq!(failure(&events), "claude exited with status 125");
}

#[tokio::test]
async fn errors_never_carry_the_conversation() {
    let lab = lab();
    lab.claude("cat > /dev/null\nexit 1");
    let mut req = request();
    req.messages[0].content = "TOP-SECRET-QUESTION".into();
    req.system = "TOP-SECRET-PROMPT".into();
    let text = failure(&run(lab.env(), req).await);
    assert!(!text.contains("SECRET"), "{text}");
}

#[tokio::test]
async fn a_missing_binary_fails_without_running_anything() {
    let lab = lab();
    let events = run(lab.env(), request()).await;
    assert_eq!(failure(&events), "claude was not found on PATH");
}

#[tokio::test]
async fn a_refused_request_runs_nothing() {
    let lab = lab();
    lab.claude(GOOD);
    let mut req = request();
    req.model = Some("--dangerously-skip-permissions".into());
    let events = run(lab.env(), req).await;
    assert_eq!(failure(&events), "invalid model name");
    assert!(!lab.rec.join("args").exists());
}

#[tokio::test]
async fn a_required_cli_without_a_jail_never_starts() {
    let lab = lab();
    lab.claude(GOOD);
    let events = run_job(&lab.env(), job(JailPolicy::Required)).await;
    assert_eq!(events, [StreamEvent::Error(ChatError::Jail)]);
    assert!(!lab.rec.join("args").exists(), "nothing was spawned");
}

#[tokio::test]
async fn a_layered_cli_without_a_jail_runs_bare() {
    let lab = lab();
    lab.claude(GOOD);
    let events = run_job(&lab.env(), job(JailPolicy::Layered)).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert_eq!(lab.args(), ["-p"]);
}

#[tokio::test]
async fn a_jailed_turn_goes_through_the_helper_with_the_spec_rules() {
    let lab = lab();
    lab.claude(GOOD);
    fs::create_dir(lab.home.join(".claude")).unwrap();
    fs::write(lab.home.join(".claude.json"), "").unwrap();
    // A pass-through helper: records its argv, drops the rules, execs the CLI.
    let helper = lab.helper(
        "printf '%s\\0' \"$@\" > \"$REC/helper_args\"\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"",
    );
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = helper;
    let events = run_job(&env, job(JailPolicy::Required)).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));

    let raw = lab.recorded("helper_args");
    let args: Vec<&str> = raw.split('\0').filter(|a| !a.is_empty()).collect();
    assert_eq!(args[0], "__jail-exec");
    let pair = |flag: &str, path: &Path| {
        args.windows(2)
            .any(|w| w[0] == flag && w[1] == path.to_str().unwrap())
    };
    assert!(pair("--rw", &lab.home.join(".claude")), "{args:?}");
    assert!(
        pair("--rw-file", &lab.home.join(".claude.json")),
        "{args:?}"
    );
    assert!(pair("--ro", &lab.data.join("cli-cwd")), "{args:?}");
    assert!(pair("--rx", &lab.bin), "the CLI's own folder: {args:?}");
    let sh = fs::canonicalize("/bin/sh").unwrap();
    assert!(pair("--rx", &sh), "the interpreter, one file: {args:?}");
    assert!(
        args.windows(2)
            .any(|w| w[0] == "--rw" && w[1].starts_with(lab.data.join("cli-tmp").to_str().unwrap())),
        "the private tmp: {args:?}"
    );
    assert!(
        !args.iter().any(|a| *a == lab.home.to_str().unwrap()),
        "never $HOME"
    );
    let dashes = args.iter().position(|a| *a == "--").expect("--");
    assert_eq!(args[dashes + 1], lab.bin.join("claude").to_str().unwrap());
    assert_eq!(args[dashes + 2..], ["-p"]);
}

#[tokio::test]
async fn the_helper_refusing_with_its_marker_is_a_jail_error() {
    let lab = lab();
    lab.claude(GOOD);
    let helper = lab.helper("echo 'lazarobox-jail: jail not enforced: test' >&2\nexit 125");
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = helper;
    let events = run(env, request()).await;
    assert_eq!(events, [StreamEvent::Error(ChatError::Jail)]);
    assert!(!lab.rec.join("args").exists(), "the CLI never ran");
}

#[tokio::test]
async fn a_layout_the_jail_refuses_runs_claude_bare_but_stops_a_required_cli() {
    let lab = lab();
    lab.claude(GOOD);
    // `~/.claude` pointing at $HOME would grant the whole home folder.
    symlink(&lab.home, lab.home.join(".claude")).unwrap();
    let helper = lab.helper("echo called > \"$REC/helper_called\"\nexit 99");
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = helper;
    let events = run_job(&env, job(JailPolicy::Layered)).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert!(!lab.rec.join("helper_called").exists(), "bare: no helper");
    let events = run_job(&env, job(JailPolicy::Required)).await;
    assert_eq!(events, [StreamEvent::Error(ChatError::Jail)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credential_files_are_never_opened() {
    let lab = lab();
    lab.claude(GOOD);
    // A FIFO blocks any open for reading, so an open would hang the turn.
    let fifo = |path: PathBuf| {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        // SAFETY: a plain mkfifo with a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    };
    fifo(lab.home.join(".claude.json"));
    fifo(lab.home.join(".claude/.credentials.json"));
    fifo(lab.home.join(".ssh/id_ed25519"));
    fifo(lab.home.join(".codex/auth.json"));
    let helper = lab.helper("while [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"");
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = helper;
    let events = run(env, request()).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
}

#[tokio::test]
async fn no_first_byte_in_time_fails_and_kills_the_group() {
    let lab = lab();
    lab.claude(
        "cat > /dev/null\nsleep 300 &\necho $! > \"$REC/child\"\necho $$ > \"$REC/leader\"\nwait",
    );
    let mut env = lab.env();
    env.timeouts.first_byte = Duration::from_millis(400);
    let events = run(env, request()).await;
    assert_eq!(failure(&events), "no answer from the engine");
    assert!(wait_gone(pid_of(&lab, "leader"), Duration::from_secs(2)).await);
    assert!(wait_gone(pid_of(&lab, "child"), Duration::from_secs(2)).await);
}

#[tokio::test]
async fn a_stalled_stream_fails_after_the_idle_limit() {
    let lab = lab();
    lab.claude(
        "cat > /dev/null\nprintf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\"}'\nsleep 300",
    );
    let mut env = lab.env();
    env.timeouts.idle = Duration::from_millis(400);
    let events = run(env, request()).await;
    assert_eq!(failure(&events), "the engine stopped answering");
}

#[tokio::test]
async fn the_total_limit_ends_a_turn_that_keeps_talking() {
    let lab = lab();
    lab.claude("cat > /dev/null\nwhile true; do printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"status\"}'; sleep 0.05; done");
    let mut env = lab.env();
    env.timeouts.total = Duration::from_millis(600);
    let events = run(env, request()).await;
    assert_eq!(failure(&events), "the turn took too long");
}

#[tokio::test]
async fn a_slow_start_sends_one_slow_event_first() {
    let lab = lab();
    lab.claude(&format!("cat > /dev/null\nsleep 0.6\n{GOOD}"));
    let mut env = lab.env();
    env.timeouts.slow = Duration::from_millis(150);
    let events = run(env, request()).await;
    assert_eq!(events[0], StreamEvent::Slow);
    assert_eq!(
        events.iter().filter(|e| **e == StreamEvent::Slow).count(),
        1
    );
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
}

#[tokio::test]
async fn cancelling_a_turn_kills_the_whole_group_within_a_second() {
    let lab = lab();
    lab.claude("cat > /dev/null\nsleep 300 &\necho $! > \"$REC/child\"\necho $$ > \"$REC/leader\"\nprintf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\"}'\nwait");
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(runner_with(None, lab.env()), tx);
    chats.send(PaneId::for_test(1), 1, request());
    // The CLI is up once it has recorded both pids.
    let up = Instant::now() + Duration::from_secs(10);
    while (lab.recorded("child").is_empty() || lab.recorded("leader").is_empty())
        && Instant::now() < up
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (leader, child) = (pid_of(&lab, "leader"), pid_of(&lab, "child"));
    assert!(!gone(leader) && !gone(child));
    chats.cancel(PaneId::for_test(1));
    assert!(wait_gone(leader, Duration::from_secs(1)).await, "leader");
    assert!(wait_gone(child, Duration::from_secs(1)).await, "grandchild");
    assert!(rx.try_recv().is_err(), "a cancelled turn says nothing more");
}

#[tokio::test]
async fn a_cli_that_lingers_after_its_result_is_stopped_and_the_answer_stands() {
    let lab = lab();
    lab.claude(&format!(
        "{GOOD}\nsleep 300 &\necho $! > \"$REC/child\"\nsleep 300"
    ));
    let mut env = lab.env();
    env.grace = Duration::from_millis(300);
    let events = run(env, request()).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert!(wait_gone(pid_of(&lab, "child"), Duration::from_secs(2)).await);
}

#[tokio::test]
async fn background_children_of_a_cli_that_exits_normally_are_killed() {
    let lab = lab();
    lab.claude(&format!(
        "{GOOD}\nsleep 300 > /dev/null 2>&1 &\necho $! > \"$REC/child\""
    ));
    let events = run(lab.env(), request()).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert!(wait_gone(pid_of(&lab, "child"), Duration::from_secs(2)).await);
}

#[test]
fn the_sweep_removes_turn_dirs_of_dead_processes_only() {
    let lab = lab();
    let scratch = lab.rec.join("cli-tmp");
    for name in ["100-1", "200-2", "300-3", "weird", "7x-1"] {
        fs::create_dir_all(scratch.join(name)).unwrap();
        fs::write(scratch.join(name).join("leftover"), "x").unwrap();
    }
    fs::write(scratch.join("400-4"), "a file, not a turn dir").unwrap();
    sweep_stale(&scratch, &|pid| pid == 200);
    let left = |name: &str| scratch.join(name).exists();
    assert!(!left("100-1") && !left("300-3"), "dead pids are removed");
    assert!(left("200-2"), "a live lazarobox keeps its dir");
    assert!(
        left("weird") && left("7x-1"),
        "unknown names are left alone"
    );
    assert!(left("400-4"), "only folders are removed");
}

#[test]
fn liveness_means_a_live_process_of_ours_not_just_any_pid() {
    let me = std::process::id();
    assert!(is_lazarobox(me), "this very process");
    assert!(!is_lazarobox(1), "init is alive but is not us");
    assert!(!is_lazarobox(u32::MAX - 1), "no such process");
}

#[tokio::test]
async fn a_turn_sweeps_the_leftovers_of_a_crashed_run_but_not_a_concurrent_turn() {
    let lab = lab();
    lab.claude(GOOD);
    let scratch = lab.data.join("cli-tmp");
    fs::create_dir_all(scratch.join("2000000000-1")).unwrap();
    let mine = scratch.join(format!("{}-4000000000", std::process::id()));
    fs::create_dir_all(&mine).unwrap();
    run(lab.env(), request()).await;
    assert!(!scratch.join("2000000000-1").exists(), "dead run swept");
    assert!(mine.exists(), "a turn of this process is never swept");
}
