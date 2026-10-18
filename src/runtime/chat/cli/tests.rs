//! The CLI runtime against fake CLIs: shell scripts in a temp dir that stand
//! in for `claude`. Real processes and the real clock, every wait bounded. No
//! real CLI, credential file or network is touched.

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicUsize},
    time::{Duration, Instant},
};

use tempfile::TempDir;
use tokio::{sync::mpsc, time::timeout};

use super::*;
use crate::{
    core::{
        ai::{
            chat::{
                adapters::{Job, TmpFile, Wire},
                engine::{ChatError, ChatMessage, ChatRequest, Finish, Role, StreamEvent, Tools},
            },
            jail::{Backend, Cli, JailOff, JailPolicy, JailSupport, Os, SeatbeltFault},
            tools::wiring::{McpWire, Secret, Unwire},
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
        // An interpreter in a group-writable folder is refused, and a
        // developer's umask can be 002.
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
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
            // The child gets this `PATH`: the system tools the fakes use too.
            path: OsString::from(format!("{}:/usr/bin:/bin", self.bin.display())),
            home: Some(self.home.clone()),
            data: Some(self.data.clone()),
            jail: JailSupport::Unavailable(JailOff::NotProbed),
            helper: PathBuf::from("/nonexistent/helper"),
            timeouts: Timeouts {
                connect: Duration::from_secs(1),
                first_byte: Duration::from_secs(10),
                idle: Duration::from_secs(10),
                total: Duration::from_secs(30),
                slow: Duration::from_secs(20),
                quiet: None,
            },
            grace: Duration::from_secs(2),
            codex_home: None,
            shim: None,
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
        tools: Tools::Off,
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
        tmp_files: Vec::new(),
        mcp: None,
        tools: None,
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

/// `run_all` without the notices, which most tests are not about.
async fn run(env: CliEnv, req: ChatRequest) -> Vec<StreamEvent> {
    let mut events = run_all(env, req).await;
    events.retain(|e| !matches!(e, StreamEvent::Notice(_)));
    events
}

/// One turn through the real runner, up to the final event.
async fn run_all(env: CliEnv, req: ChatRequest) -> Vec<StreamEvent> {
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(runner_with(None, env, None), tx);
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

/// The reason of the one jail error `events` hold.
fn jail_reason(events: &[StreamEvent]) -> String {
    match events {
        [StreamEvent::Error(ChatError::Jail(reason))] => reason.clone(),
        other => panic!("one jail error expected, got {other:?}"),
    }
}

fn failure(events: &[StreamEvent]) -> String {
    match events.last() {
        Some(StreamEvent::Error(ChatError::Failed(text))) => text.clone(),
        other => panic!("a failure expected, got {other:?} in {events:?}"),
    }
}

fn gone(pid: libc::pid_t) -> bool {
    matches!(crate::runtime::proc::process_state(pid), None | Some('Z'))
}

/// How long a process may take to die after its group was signalled. Polling
/// returns as soon as it is gone, so a generous bound costs nothing on a pass
/// and keeps a loaded CI runner from failing the tests.
const GONE: Duration = Duration::from_secs(20);

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
async fn the_child_runs_with_the_path_the_cli_was_found_on() {
    let lab = lab();
    lab.claude(&format!("echo \"$PATH\" > \"$REC/path\"\n{GOOD}"));
    let mut env = lab.env();
    let wanted = format!("{}:/usr/bin:/bin:/opt/login/bin", lab.bin.display());
    env.path = OsString::from(&wanted);
    let events = run(env, request()).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert_eq!(lab.recorded("path").trim(), wanted);
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

#[test]
fn the_inherited_gemini_trust_variable_is_dropped_but_a_wired_turns_own_wins() {
    let envs_of = |inject: &[(OsString, OsString)]| {
        let command = command(
            Path::new("/bin/true"),
            Vec::new(),
            (Path::new("/data/cli-cwd"), Path::new("/data/cli-tmp/1-1")),
            inject,
        );
        command
            .as_std()
            .get_envs()
            .find(|(k, _)| *k == "GEMINI_CLI_TRUST_WORKSPACE")
            .map(|(_, v)| v.map(OsString::from))
    };
    assert_eq!(envs_of(&[]), Some(None), "removed, so not inherited");
    let wired = [("GEMINI_CLI_TRUST_WORKSPACE".into(), "true".into())];
    assert_eq!(envs_of(&wired), Some(Some("true".into())));
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
async fn the_turns_token_is_not_shown_from_the_stderr_tail() {
    // A short token, which the generic token filter would let through.
    let lab = lab();
    lab.claude("cat > /dev/null\necho 'cannot reach hunter2 now' >&2\nexit 1");
    let mut job = claude_job(&lab, true);
    job.mcp.as_mut().unwrap().token = Secret::new("hunter2");
    let events = run_job(&lab.env(), job).await;
    assert_eq!(failure(&events), "cannot reach (..) now", "{events:?}");
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
    assert_eq!(jail_reason(&events), "the jail was not probed");
    assert!(!lab.rec.join("args").exists(), "nothing was spawned");
}

#[tokio::test]
async fn a_layered_cli_without_a_jail_runs_bare() {
    let lab = lab();
    lab.claude(GOOD);
    let mut events = run_job(&lab.env(), job(JailPolicy::Layered)).await;
    events.retain(|e| !matches!(e, StreamEvent::Notice(_)));
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
    // A script is started through its interpreter, one exact file the jail
    // allows, and not through the kernel's shebang handling.
    let dashes = args.iter().position(|a| *a == "--").expect("--");
    assert_eq!(args[dashes + 1], sh.to_str().unwrap());
    assert_eq!(args[dashes + 2], lab.bin.join("claude").to_str().unwrap());
    assert_eq!(args[dashes + 3..], ["-p"]);
}

/// The args after `--` of what the pass-through helper was given.
fn helper_program(lab: &Lab) -> Vec<String> {
    let raw = lab.recorded("helper_args");
    let args: Vec<&str> = raw.split('\0').filter(|a| !a.is_empty()).collect();
    let dashes = args.iter().position(|a| *a == "--").expect("--");
    args[dashes + 1..].iter().map(|a| (*a).to_owned()).collect()
}

/// The pass-through helper: records its argv, drops the rules, execs the CLI.
const PASS: &str = "printf '%s\\0' \"$@\" > \"$REC/helper_args\"\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"";

const CODEX_ANSWER: &str = r#"
cat > "$REC/stdin"
printf '%s\n' '{"type":"thread.started","thread_id":"t"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"ok"}}'
printf '%s\n' '{"type":"turn.completed","usage":{}}'
"#;

fn codex_job(args: &[&str]) -> Job {
    Job {
        wire: Wire::CodexJsonl,
        binary: "codex",
        cli: Cli::Codex,
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        stdin: b"hi".to_vec(),
        jail: JailPolicy::Required,
        tmp_files: Vec::new(),
        mcp: None,
        tools: None,
    }
}

/// A `#!/usr/bin/env fakenode` codex (as an npm install has) and the
/// interpreter in another folder of `PATH`.
fn node_style_codex(lab: &Lab) -> PathBuf {
    let interp_dir = lab.bin.parent().unwrap().join("nodebin");
    fs::create_dir_all(&interp_dir).unwrap();
    fs::set_permissions(&interp_dir, fs::Permissions::from_mode(0o755)).unwrap();
    let interp = interp_dir.join("fakenode");
    script(&interp, &lab.rec, "exec /bin/sh \"$@\"");
    let cli = lab.bin.join("codex");
    script(&cli, &lab.rec, CODEX_ANSWER);
    let text =
        fs::read_to_string(&cli)
            .unwrap()
            .replacen("#!/bin/sh", "#!/usr/bin/env fakenode", 1);
    fs::write(&cli, text).unwrap();
    interp_dir
}

#[tokio::test]
async fn an_env_shebang_script_is_started_through_its_interpreter_not_through_env() {
    let lab = lab();
    let interp_dir = node_style_codex(&lab);
    let mut env = lab.env();
    env.path = std::env::join_paths([&lab.bin, &interp_dir]).unwrap();
    env.jail = jail_on();
    env.helper = lab.helper(PASS);
    let events = run_job(&env, codex_job(&["exec", "-"])).await;
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Done(Finish::Stop)),
        "{events:?}"
    );
    let program = helper_program(&lab);
    let interp = fs::canonicalize(interp_dir.join("fakenode")).unwrap();
    assert_eq!(
        program,
        [
            interp.to_str().unwrap(),
            lab.bin.join("codex").to_str().unwrap(),
            "exec",
            "-"
        ]
    );
    let raw = lab.recorded("helper_args");
    let args: Vec<&str> = raw.split('\0').collect();
    let granted = |flag: &str, path: &Path| {
        args.windows(2)
            .any(|w| w[0] == flag && w[1] == path.to_str().unwrap())
    };
    assert!(
        granted("--rx", &interp),
        "the interpreter, one file: {args:?}"
    );
    assert!(
        !args.iter().any(|a| a.ends_with("/usr/bin/env")),
        "env is never granted: {args:?}"
    );
}

#[tokio::test]
async fn codex_home_is_the_state_folder_when_set_and_dot_codex_otherwise() {
    let lab = lab();
    let _ = node_style_codex(&lab);
    let interp_dir = lab.bin.parent().unwrap().join("nodebin");
    let mut env = lab.env();
    env.path = std::env::join_paths([&lab.bin, &interp_dir]).unwrap();
    env.jail = jail_on();
    env.helper = lab.helper(PASS);
    let rw = |lab: &Lab| -> Vec<String> {
        let raw = lab.recorded("helper_args");
        let args: Vec<&str> = raw.split('\0').collect();
        args.windows(2)
            .filter(|w| w[0] == "--rw")
            .map(|w| w[1].to_owned())
            .collect()
    };

    fs::create_dir(lab.home.join(".codex")).unwrap();
    run_job(&env, codex_job(&["exec", "-"])).await;
    assert!(rw(&lab).contains(&lab.home.join(".codex").to_str().unwrap().to_owned()));

    let custom = lab.home.join("custom-codex");
    fs::create_dir(&custom).unwrap();
    env.codex_home = Some(custom.clone());
    run_job(&env, codex_job(&["exec", "-"])).await;
    let rw = rw(&lab);
    assert!(rw.contains(&custom.to_str().unwrap().to_owned()), "{rw:?}");
    assert!(
        !rw.contains(&lab.home.join(".codex").to_str().unwrap().to_owned()),
        "{rw:?}"
    );
}

#[tokio::test]
async fn a_codex_home_that_is_the_home_folder_is_refused_so_codex_does_not_start() {
    let lab = lab();
    let _ = node_style_codex(&lab);
    let interp_dir = lab.bin.parent().unwrap().join("nodebin");
    let mut env = lab.env();
    env.path = std::env::join_paths([&lab.bin, &interp_dir]).unwrap();
    env.jail = jail_on();
    env.helper = lab.helper("echo called > \"$REC/helper_called\"\nexit 99");
    env.codex_home = Some(lab.home.clone());
    let events = run_job(&env, codex_job(&["exec", "-"])).await;
    assert!(
        jail_reason(&events).contains("too broad for the jail"),
        "{events:?}"
    );
    assert!(!lab.rec.join("helper_called").exists());
}

#[tokio::test]
async fn the_helper_refusing_with_its_marker_is_a_jail_error() {
    let lab = lab();
    lab.claude(GOOD);
    let helper = lab.helper(
        "echo 'lazarobox-jail: jail not enforced: ABI 2 is below the required 3' >&2\nexit 125",
    );
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = helper;
    let events = run(env, request()).await;
    assert_eq!(jail_reason(&events), "ABI 2 is below the required 3");
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
    assert!(
        jail_reason(&events).contains("too broad for the jail"),
        "{events:?}"
    );
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
    assert!(wait_gone(pid_of(&lab, "leader"), GONE).await);
    assert!(wait_gone(pid_of(&lab, "child"), GONE).await);
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
async fn a_cli_quiet_after_its_init_line_sends_one_slow_event_and_still_answers() {
    let lab = lab();
    // GOOD, with a pause between the init line and the first token.
    let pause = "sleep 0.8\nprintf '%s\\n' '{\"type\":\"stream_event\"";
    lab.claude(&GOOD.replacen("printf '%s\\n' '{\"type\":\"stream_event\"", pause, 1));
    let mut env = lab.env();
    env.timeouts.quiet = Some(Duration::from_millis(200));
    env.timeouts.idle = Duration::from_secs(5);
    let events = run(env, request()).await;
    assert_eq!(
        events.iter().filter(|e| **e == StreamEvent::Slow).count(),
        1
    );
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
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
async fn cancelling_a_turn_kills_the_whole_group() {
    let lab = lab();
    lab.claude("cat > /dev/null\nsleep 300 &\necho $! > \"$REC/child\"\necho $$ > \"$REC/leader\"\nprintf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\"}'\nwait");
    let (tx, mut rx) = mpsc::channel(APP_CHANNEL);
    let mut chats = Chats::new(runner_with(None, lab.env(), None), tx);
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
    assert!(wait_gone(leader, GONE).await, "leader");
    assert!(wait_gone(child, GONE).await, "grandchild");
    while let Ok((_, _, event)) = rx.try_recv() {
        assert!(matches!(event, StreamEvent::Notice(_)), "{event:?}");
    }
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
    assert!(wait_gone(pid_of(&lab, "child"), GONE).await);
}

#[tokio::test]
async fn background_children_of_a_cli_that_exits_normally_are_killed() {
    let lab = lab();
    lab.claude(&format!(
        "{GOOD}\nsleep 300 > /dev/null 2>&1 &\necho $! > \"$REC/child\""
    ));
    let events = run(lab.env(), request()).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert!(wait_gone(pid_of(&lab, "child"), GONE).await);
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

/// Outside Linux there is no `/proc`: `/bin/ps -o comm= -p PID` answers
/// instead. Only `ps` exiting 1 with nothing on stdout says the process is
/// gone. Every other failure (it cannot run, an odd status, no text, bytes that
/// are not UTF-8) says nothing, and nothing said must never read as gone.
#[test]
fn the_comm_of_a_process_is_read_from_ps_outside_linux() {
    use std::os::unix::process::ExitStatusExt;
    let out = |code: i32, stdout: &[u8]| {
        Ok(std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        })
    };
    assert_eq!(
        comm_from_ps(out(0, b"/opt/bin/lazarobox-shell\n")),
        Comm::Is("/opt/bin/lazarobox-shell".to_owned())
    );
    assert_eq!(comm_from_ps(out(1, b"")), Comm::Gone, "no such process");
    assert_eq!(comm_from_ps(out(1, b"\n")), Comm::Gone);
    assert_eq!(
        comm_from_ps(out(1, b"x\n")),
        Comm::Unknown,
        "status 1 with text"
    );
    assert_eq!(comm_from_ps(out(0, b"\n")), Comm::Unknown, "no text");
    assert_eq!(comm_from_ps(out(2, b"")), Comm::Unknown, "an odd status");
    assert_eq!(
        comm_from_ps(out(0, b"\xff\xfe\n")),
        Comm::Unknown,
        "not UTF-8"
    );
    assert_eq!(
        comm_from_ps(Err(io::Error::from(io::ErrorKind::WouldBlock))),
        Comm::Unknown,
        "ps cannot run (fork EAGAIN)"
    );
}

/// A process we cannot check is kept: the sweep deletes only what is surely
/// the leftover of a dead run.
#[test]
fn an_entry_whose_owner_cannot_be_checked_is_kept() {
    let own = Some("lazarobox-shell");
    assert!(
        alive_by_comm(4242, own, &|_| Comm::Unknown),
        "unknown is alive"
    );
    assert!(!alive_by_comm(4242, own, &|_| Comm::Gone), "gone is dead");
    assert!(
        alive_by_comm(4242, own, &|_| Comm::Is("lazarobox-shell".to_owned())),
        "ours"
    );
}

/// macOS prints the full path of the executable and Linux the short name of
/// at most 15 bytes. Programs are told apart by file name, cut like Linux
/// cuts it, so two installs of one program (or a dev build beside an installed
/// copy) are the same program and a different binary is not.
#[test]
fn programs_are_compared_by_file_name_cut_to_15_bytes() {
    let is = |comm: &str| Comm::Is(comm.to_owned());
    let own = Some("/Users/a/.cargo/bin/lazarobox-shell");
    assert!(alive_by_comm(7, own, &|_| is(
        "/Users/a/src/target/debug/lazarobox-shell"
    )));
    assert!(
        alive_by_comm(7, own, &|_| is("lazarobox-shell\n")),
        "the Linux spelling"
    );
    assert!(
        !alive_by_comm(7, own, &|_| is("/usr/bin/zsh")),
        "another binary"
    );
    assert!(
        !alive_by_comm(7, own, &|_| is("/Users/a/bin/lazarobox")),
        "a prefix is not it"
    );
    let long = Some("/x/lazarobox-shell-extra");
    assert!(
        alive_by_comm(7, long, &|_| is("lazarobox-shell")),
        "cut like Linux"
    );
    assert!(
        !alive_by_comm(7, Some("/"), &|_| is("/")),
        "no name to compare"
    );
}

/// A pid that cannot exist is never asked of `ps`.
#[test]
fn a_number_that_cannot_be_a_pid_has_no_comm() {
    assert!(!is_pid(u32::MAX - 1));
    assert!(!is_pid(0));
    assert!(is_pid(1));
    assert!(is_pid(i32::MAX as u32));
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

fn notices(events: &[StreamEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Notice(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn running_bare_because_there_is_no_jail_is_announced_first() {
    let lab = lab();
    lab.claude(GOOD);
    let events = run_all(lab.env(), request()).await;
    assert_eq!(
        events[0],
        StreamEvent::Notice("claude runs without the jail here: the jail was not probed".into())
    );
    assert_eq!(notices(&events).len(), 1);
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
}

#[tokio::test]
async fn running_bare_because_the_layout_was_refused_is_announced_too() {
    let lab = lab();
    lab.claude(GOOD);
    symlink(&lab.home, lab.home.join(".claude")).unwrap();
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = lab.helper("exit 99");
    let events = run_all(env, request()).await;
    assert_eq!(
        notices(&events),
        [
            "claude runs without the jail here: ~ is too broad for the jail; keep the CLI and its \
         state in folders of their own"
        ]
    );
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
}

#[tokio::test]
async fn a_jailed_turn_and_a_refused_one_announce_nothing() {
    let lab = lab();
    lab.claude(GOOD);
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = lab.helper("while [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"");
    let events = run_all(env, request()).await;
    assert!(notices(&events).is_empty(), "{events:?}");
    let events = run_job(&lab.env(), job(JailPolicy::Required)).await;
    assert_eq!(jail_reason(&events), "the jail was not probed");
}

#[test]
fn without_our_own_comm_nothing_is_swept_and_our_own_pid_is_always_alive() {
    let lab = lab();
    let scratch = lab.rec.join("cli-tmp");
    let me = std::process::id();
    let dead = scratch.join("100-1");
    let mine = scratch.join(format!("{me}-5"));
    for dir in [&dead, &mine] {
        fs::create_dir_all(dir).unwrap();
    }
    // /proc/self/comm unreadable: the sweep cannot tell anyone apart.
    sweep_with(&scratch, None, &|_| Comm::Gone);
    assert!(dead.exists() && mine.exists(), "no sweep at all");
    // Readable for everyone but unreadable for our own pid: still alive.
    sweep_with(&scratch, Some("lazarobox"), &|pid| {
        if pid == me {
            Comm::Unknown
        } else {
            Comm::Is("other".to_owned())
        }
    });
    assert!(!dead.exists(), "a foreign pid is swept");
    assert!(mine.exists(), "our own pid is never swept");
}

/// A minimal little-endian ELF64 file whose `PT_INTERP` string is `interp`,
/// stored at file offset `at`.
fn elf_with_interp(path: &Path, interp: &[u8], at: usize) {
    let mut img = vec![0u8; at + interp.len()];
    img[..4].copy_from_slice(b"\x7fELF");
    img[4] = 2;
    img[5] = 1;
    img[0x20..0x28].copy_from_slice(&64u64.to_le_bytes());
    img[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());
    img[0x38..0x3a].copy_from_slice(&1u16.to_le_bytes());
    img[64..68].copy_from_slice(&3u32.to_le_bytes());
    img[72..80].copy_from_slice(&(at as u64).to_le_bytes());
    img[96..104].copy_from_slice(&(interp.len() as u64).to_le_bytes());
    img[at..].copy_from_slice(interp);
    fs::write(path, img).unwrap();
}

#[test]
fn the_loader_is_found_even_when_its_string_is_far_from_the_start_of_the_file() {
    // Homebrew's node keeps `.interp` at 0x8d98, past the first 4 KiB.
    let lab = lab();
    let near = lab.bin.join("near");
    let far = lab.bin.join("far");
    elf_with_interp(&near, b"/lib64/near-ld.so\0", 300);
    elf_with_interp(&far, b"/home/linuxbrew/.linuxbrew/lib/ld.so\0", 0x8d98);
    assert_eq!(read_loader(&near), Some(PathBuf::from("/lib64/near-ld.so")));
    assert_eq!(
        read_loader(&far),
        Some(PathBuf::from("/home/linuxbrew/.linuxbrew/lib/ld.so"))
    );
}

#[test]
fn no_loader_for_a_script_a_missing_file_or_a_string_past_the_end() {
    let lab = lab();
    let text = lab.bin.join("text");
    fs::write(&text, "#!/bin/sh\necho hi\n").unwrap();
    assert_eq!(read_loader(&text), None);
    assert_eq!(read_loader(&lab.bin.join("missing")), None);
    let cut = lab.bin.join("cut");
    elf_with_interp(&cut, b"/lib64/ld.so\0", 0x9000);
    let file = fs::OpenOptions::new().write(true).open(&cut).unwrap();
    file.set_len(0x9000 + 3).unwrap();
    assert_eq!(read_loader(&cut), None, "a cut string is not trusted");
}

// Linux only: the loader is the `PT_INTERP` of an ELF file, and a macOS binary
// is Mach-O, which has none (`read_loader` gives `None`; the jail that needs the
// loader is not offered there, so the chat runs unjailed with a notice).
#[cfg(target_os = "linux")]
#[test]
fn the_loader_of_a_real_dynamic_binary_is_read() {
    // The test binary itself is a dynamic executable.
    let me = std::env::current_exe().unwrap();
    let loader = read_loader(&me).expect("a dynamic executable has a loader");
    assert!(loader.is_absolute(), "{loader:?}");
}

#[tokio::test]
async fn the_interpreter_installed_next_to_the_cli_wins_over_the_first_one_on_path() {
    // An npm install puts `node` and the CLI in the same `bin` folder, but the
    // first `node` on PATH can be another one (Homebrew's, whose libraries the
    // jail does not grant). The sibling is the one that came with the CLI.
    let lab = lab();
    let elsewhere = lab.bin.parent().unwrap().join("first-on-path");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o755)).unwrap();
    script(&elsewhere.join("fakenode"), &lab.rec, "exit 97");
    script(&lab.bin.join("fakenode"), &lab.rec, "exec /bin/sh \"$@\"");
    let cli = lab.bin.join("codex");
    script(&cli, &lab.rec, CODEX_ANSWER);
    let text =
        fs::read_to_string(&cli)
            .unwrap()
            .replacen("#!/bin/sh", "#!/usr/bin/env fakenode", 1);
    fs::write(&cli, text).unwrap();

    let mut env = lab.env();
    env.path = std::env::join_paths([&elsewhere, &lab.bin]).unwrap();
    env.jail = jail_on();
    env.helper = lab.helper(PASS);
    let events = run_job(&env, codex_job(&["exec", "-"])).await;
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Done(Finish::Stop)),
        "{events:?}"
    );
    let program = helper_program(&lab);
    assert_eq!(program[0], lab.bin.join("fakenode").to_str().unwrap());
}

/// A fake `codex` that records its pid and a background child's, then replays
/// `line`, then waits: only a kill ends it.
fn lingering_codex(lab: &Lab, line: &str) {
    fs::write(lab.rec.join("lines"), format!("{line}\n")).unwrap();
    let body = "cat > \"$REC/stdin\"\necho $$ > \"$REC/leader\"\nsleep 300 &\necho $! > \"$REC/child\"\ncat \"$REC/lines\"\nwait";
    script(&lab.bin.join("codex"), &lab.rec, body);
}

/// A fake `codex` that replays `lines` on stdout and then runs `tail`.
fn fake_codex(lab: &Lab, lines: &[&str], tail: &str) {
    fs::write(lab.rec.join("lines"), lines.join("\n") + "\n").unwrap();
    let body = format!("cat > \"$REC/stdin\"\ncat \"$REC/lines\"\n{tail}");
    script(&lab.bin.join("codex"), &lab.rec, &body);
}

#[tokio::test]
async fn a_failed_codex_turn_shows_the_api_message_even_though_the_process_exits_1() {
    let lab = lab();
    fake_codex(
        &lab,
        &[
            r#"{"type":"thread.started","thread_id":"t"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"turn.failed","error":{"message":"{\"type\":\"error\",\"status\":400,\"error\":{\"message\":\"The 'gpt-5.5' model is not supported when using Codex with a ChatGPT account.\"}}"}}"#,
        ],
        "exit 1",
    );
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = lab.helper(PASS);
    let events = run_job(&env, codex_job(&["exec", "-"])).await;
    let [StreamEvent::Error(ChatError::Failed(message))] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert!(
        message.starts_with(
            "The 'gpt-5.5' model is not supported when using Codex with a ChatGPT account."
        ),
        "{message}"
    );
}

#[tokio::test]
async fn a_tool_call_ends_the_codex_turn_and_stops_the_process() {
    let lab = lab();
    lingering_codex(
        &lab,
        r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"id","status":"in_progress"}}"#,
    );
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = lab.helper(PASS);
    let events = run_job(&env, codex_job(&["exec", "-"])).await;
    assert_eq!(
        events,
        [StreamEvent::Error(ChatError::Failed(
            "codex tried to use a tool".into()
        ))]
    );
    assert!(wait_gone(pid_of(&lab, "leader"), GONE).await);
}

#[tokio::test]
async fn codex_without_a_jail_spawns_nothing() {
    let lab = lab();
    fake_codex(&lab, &[], "touch \"$REC/ran\"");
    let events = run_job(&lab.env(), codex_job(&["exec", "-"])).await;
    assert_eq!(jail_reason(&events), "the jail was not probed");
    assert!(!lab.rec.join("ran").exists());
    assert!(!lab.rec.join("stdin").exists());
}

#[tokio::test]
async fn a_decoder_error_kills_the_group_at_once_without_the_exit_grace() {
    // A tool refusal and malformed data are the decoder's own verdict: the
    // process has nothing more to say, so it is not given the grace to exit.
    for line in [
        r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution"}}"#,
        "{this is not json",
    ] {
        let lab = lab();
        lingering_codex(&lab, line);
        let mut env = lab.env();
        env.jail = jail_on();
        env.helper = lab.helper(PASS);
        env.grace = Duration::from_secs(20);
        let started = Instant::now();
        let events = run_job(&env, codex_job(&["exec", "-"])).await;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?} for {line}",
            started.elapsed()
        );
        assert!(
            matches!(
                events.as_slice(),
                [StreamEvent::Error(ChatError::Failed(_))]
            ),
            "{events:?}"
        );
        assert!(wait_gone(pid_of(&lab, "leader"), GONE).await);
        assert!(wait_gone(pid_of(&lab, "child"), GONE).await);
    }
}

#[tokio::test]
async fn a_stream_that_just_ends_still_waits_for_the_exit_so_the_jail_marker_is_seen() {
    let lab = lab();
    fake_codex(
        &lab,
        &[],
        "echo 'lazarobox-jail: jail not enforced: whatever the CLI made it say' >&2\nexit 125",
    );
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = lab.helper(PASS);
    let events = run_job(&env, codex_job(&["exec", "-"])).await;
    assert_eq!(
        jail_reason(&events),
        "",
        "text outside the vocabulary is hidden"
    );
}

/// An executable file `name` in `dir` (mode 0755) and `dir` itself set to `mode`.
fn exe_in(dir: &Path, name: &str, mode: u32) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let file = dir.join(name);
    fs::write(&file, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(dir, fs::Permissions::from_mode(mode)).unwrap();
    file
}

#[test]
fn an_interpreter_in_a_tmp_folder_is_never_trusted() {
    for base in ["/tmp", "/var/tmp"] {
        if !Path::new(base).is_dir() {
            continue;
        }
        let dir = tempfile::tempdir_in(base).unwrap();
        let file = exe_in(&dir.path().join("bin"), "fakenode", 0o755);
        let path = dir.path().join("bin").into_os_string();
        // The refusal names the real path: on macOS `/tmp` is a link into
        // `/private`, so it differs from the spelling the test created.
        let real = fs::canonicalize(&file).unwrap();
        assert_eq!(
            interpreter_for(&dir.path().join("bin/codex"), "fakenode", &path),
            Err(JailRefusal::UntrustedInterpreter {
                path: real.clone(),
                why: Untrusted::Scratch
            }),
            "{base}"
        );
        assert_eq!(
            interpreter_trust(&file),
            Err((real, Untrusted::Scratch)),
            "{base}"
        );
    }
}

/// macOS keeps `/tmp` and `/var/tmp` as links into `/private`, and `$TMPDIR` is
/// under `/var/folders`; a canonical path never starts with the link names.
/// The rule is pure, so it is checked here with the macOS spellings on any OS.
#[test]
fn the_scratch_folders_of_macos_count_as_scratch_by_their_real_names() {
    for scratch in [
        "/tmp/x/node",
        "/var/tmp/x/node",
        "/private/tmp/x/node",
        "/private/var/tmp/x/node",
        "/private/var/folders/zz/abc123/T/x/node",
    ] {
        assert!(in_scratch(Path::new(scratch)), "{scratch}");
    }
    for fine in [
        "/usr/local/bin/node",
        "/opt/homebrew/bin/node",
        "/private/etc/node",
        "/private/var/db/node",
        "/private/tmpfoo/node",
        "/tmpfoo/node",
    ] {
        assert!(!in_scratch(Path::new(fine)), "{fine}");
    }
}

#[test]
fn an_interpreter_in_a_group_or_world_writable_folder_is_not_trusted() {
    let lab = lab();
    let base = lab.bin.parent().unwrap().to_owned();
    for mode in [0o775, 0o757, 0o777, 0o722] {
        let file = exe_in(&base.join(format!("w{mode:o}")), "fakenode", mode);
        assert!(interpreter_trust(&file).is_err(), "{mode:o}");
    }
    let ok = exe_in(&base.join("ok"), "fakenode", 0o755);
    assert_eq!(interpreter_trust(&ok), Ok(fs::canonicalize(&ok).unwrap()));
    // A file others can write is no better than a folder they can write.
    fs::set_permissions(&ok, fs::Permissions::from_mode(0o757)).unwrap();
    let real = fs::canonicalize(&ok).unwrap();
    let writable = Untrusted::Writable {
        path: real.clone(),
        yours: true,
    };
    assert_eq!(interpreter_trust(&ok), Err((real, writable)));
}

#[test]
fn the_jail_keeps_refusing_a_group_writable_folder_that_cargo_accepts() {
    let lab = lab();
    let base = lab.bin.parent().unwrap().to_owned();
    let file = exe_in(&base.join("g"), "fakecargo", 0o775);
    // The jail's rule is untouched: group write counts as writable...
    assert!(interpreter_trust(&file).is_err());
    assert!(trusted_real_path(&file).is_none());
    // ...while the update install accepts the user's own 0775 folder, but only
    // when the user's group is private to them. The files of this test get the
    // user's own group, so the answer follows the same fact the code reads.
    let expected = own_group_is_private().then(|| fs::canonicalize(&file).unwrap());
    if expected.is_none() {
        eprintln!("this user's group is not private: group write must be refused");
    }
    assert_eq!(trusted_cargo_path(&file), expected);
    // Either way, a folder anyone can write is out.
    let open = exe_in(&base.join("o"), "fakecargo", 0o777);
    assert_eq!(trusted_cargo_path(&open), None);
}

/// The facts about a file's owner for [`Rule::open`].
fn owner(user_is_me: bool, group_is_mine: bool, group_private: bool) -> Owner {
    Owner {
        user_is_me,
        group_is_mine,
        group_private,
    }
}

#[test]
fn the_cargo_rule_trusts_group_write_only_for_a_private_group_of_the_user() {
    let mine = owner(true, true, true);
    let shared = owner(true, true, false);
    let foreign_gid = owner(true, false, true);
    let root_private = owner(false, true, true);
    let root_shared = owner(false, false, false);
    let other_user = owner(false, true, true);
    // (mode, owner, trusted)
    for (mode, who, trusted) in [
        // The user's own private group: group write is fine.
        (0o755, mine, true),
        (0o775, mine, true),
        (0o2775, mine, true),
        // Never other write, whoever owns it.
        (0o757, mine, false),
        (0o777, mine, false),
        (0o772, mine, false),
        (0o002, mine, false),
        // A shared group (`users`, `staff`) is not private.
        (0o775, shared, false),
        (0o755, shared, true),
        (0o757, shared, false),
        // A group that is not the user's own.
        (0o775, foreign_gid, false),
        (0o755, foreign_gid, true),
        // Root-owned: group write is out, even with a "private" group.
        (0o775, root_private, false),
        (0o2775, root_shared, false),
        (0o755, root_private, true),
        (0o755, root_shared, true),
        (0o777, root_shared, false),
        // Another user's file: group write is out, plain 0755 is as before.
        (0o775, other_user, false),
        (0o755, other_user, true),
    ] {
        assert_eq!(Rule::OwnGroup.open(mode, who), !trusted, "{mode:o} {who:?}");
    }
    // The jail ignores who owns what: group or other write is always open.
    for who in [mine, shared, foreign_gid, root_private, root_shared] {
        assert!(Rule::Jail.open(0o775, who), "{who:?}");
        assert!(Rule::Jail.open(0o757, who), "{who:?}");
        assert!(!Rule::Jail.open(0o755, who), "{who:?}");
    }
}

#[test]
fn a_group_is_private_only_when_it_is_the_users_own_and_has_no_one_else() {
    // (user, group name, members) -> private
    for (user, group, members, private) in [
        ("ana", "ana", &[][..], true),
        ("ana", "ana", &["ana"][..], true),
        ("ana", "ana", &["ana", "bob"][..], false),
        ("ana", "ana", &["bob"][..], false),
        ("ana", "users", &[][..], false),
        ("ana", "staff", &["ana"][..], false),
        ("ana", "bob", &[][..], false),
        ("", "", &[][..], false),
    ] {
        assert_eq!(
            private_group(user, group, members),
            private,
            "{user} {group} {members:?}"
        );
    }
}

#[test]
fn a_cargo_link_in_a_trusted_folder_pointing_into_tmp_is_refused() {
    let lab = lab();
    let base = lab.bin.parent().unwrap().to_owned();
    let planted = tempfile::tempdir_in("/tmp").unwrap();
    let target = exe_in(&planted.path().join("bin"), "cargo", 0o755);
    let bin = base.join("linkbin");
    fs::create_dir_all(&bin).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    let link = bin.join("cargo");
    symlink(&target, &link).unwrap();
    assert_eq!(trusted_cargo_path(&link), None);
}

#[test]
fn the_folder_that_holds_a_cargo_link_is_checked_too() {
    let lab = lab();
    let base = lab.bin.parent().unwrap().to_owned();
    // A tight target, far from the link's folder.
    let target = exe_in(&base.join("real"), "rustup", 0o755);
    for (mode, trusted) in [(0o755, true), (0o757, false), (0o777, false)] {
        let bin = base.join(format!("holder{mode:o}"));
        fs::create_dir_all(&bin).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(mode)).unwrap();
        let link = bin.join("cargo");
        symlink(&target, &link).unwrap();
        assert_eq!(trusted_cargo_path(&link).is_some(), trusted, "{mode:o}");
    }
    // Its own ancestors count as well, not only its own folder.
    let outer = base.join("outer");
    let bin = outer.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    symlink(&target, bin.join("cargo")).unwrap();
    fs::set_permissions(&outer, fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(trusted_cargo_path(&bin.join("cargo")), None);
}

#[test]
fn a_group_writable_folder_above_the_parent_passes_for_a_private_group() {
    // `~` 0775 with `~/.cargo/bin` 0755: every folder above counts, and
    // group write is fine only while the group is private.
    let lab = lab();
    let base = lab.bin.parent().unwrap().to_owned();
    let file = exe_in(&base.join("home/.cargo/bin"), "cargo", 0o755);
    fs::set_permissions(base.join("home"), fs::Permissions::from_mode(0o775)).unwrap();
    let expected = own_group_is_private().then(|| fs::canonicalize(&file).unwrap());
    assert_eq!(trusted_cargo_path(&file), expected);
    // The same folder open to others is refused whatever the group is.
    fs::set_permissions(base.join("home"), fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(trusted_cargo_path(&file), None);
}

#[test]
fn an_untrusted_sibling_falls_back_to_a_trusted_interpreter_on_path_or_nothing() {
    let lab = lab();
    let base = lab.bin.parent().unwrap().to_owned();
    let sibling_dir = base.join("loose");
    exe_in(&sibling_dir, "fakenode", 0o777);
    let other_loose = base.join("loose2");
    exe_in(&other_loose, "fakenode", 0o775);
    let good = exe_in(&base.join("good"), "fakenode", 0o755);
    let found = sibling_dir.join("codex");
    let path = std::env::join_paths([&other_loose, &base.join("good")]).unwrap();
    // The first PATH entry is untrusted too: the first trusted one wins.
    assert_eq!(
        interpreter_for(&found, "fakenode", &path),
        Ok(fs::canonicalize(good).unwrap())
    );
    let only_loose = std::env::join_paths([&other_loose]).unwrap();
    assert_eq!(
        interpreter_for(&found, "fakenode", &only_loose),
        Err(JailRefusal::UntrustedInterpreter {
            path: fs::canonicalize(sibling_dir.join("fakenode")).unwrap(),
            why: Untrusted::Writable {
                path: fs::canonicalize(&sibling_dir).unwrap(),
                yours: true,
            },
        })
    );
    // No such interpreter anywhere: the refusal names the missing one.
    assert_eq!(
        interpreter_for(&found, "absent-node", &only_loose),
        Err(JailRefusal::NoInterpreter("absent-node".into()))
    );
}

#[tokio::test]
async fn a_cli_whose_only_interpreter_is_untrusted_is_not_started() {
    let lab = lab();
    let interp_dir = node_style_codex(&lab);
    fs::set_permissions(&interp_dir, fs::Permissions::from_mode(0o777)).unwrap();
    let mut env = lab.env();
    env.path = std::env::join_paths([&lab.bin, &interp_dir]).unwrap();
    env.jail = jail_on();
    env.helper = lab.helper("echo called > \"$REC/helper_called\"\nexit 99");
    let events = run_job(&env, codex_job(&["exec", "-"])).await;
    let reason = jail_reason(&events);
    assert!(
        reason.contains("is writable by group or others")
            && reason.contains("; run: chmod go-w -- '"),
        "{reason}"
    );
    assert!(!lab.rec.join("helper_called").exists());
}

#[tokio::test]
async fn a_script_whose_fixed_interpreter_path_is_gone_says_so() {
    let lab = lab();
    let cli = lab.bin.join("codex");
    script(&cli, &lab.rec, CODEX_ANSWER);
    let text =
        fs::read_to_string(&cli)
            .unwrap()
            .replacen("#!/bin/sh", "#!/nonexistent/old/node", 1);
    fs::write(&cli, text).unwrap();
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = lab.helper("exit 99");
    let events = run_job(&env, codex_job(&["exec", "-"])).await;
    assert_eq!(
        jail_reason(&events),
        "/nonexistent/old/node (from the script's #! line) does not exist; reinstall the CLI"
    );
}

#[tokio::test]
async fn a_home_that_is_not_set_or_does_not_exist_gets_its_own_reason() {
    let lab = lab();
    lab.claude(GOOD);
    let mut env = lab.env();
    env.jail = jail_on();
    env.home = None;
    let events = run_job(&env, job(JailPolicy::Required)).await;
    assert_eq!(
        jail_reason(&events),
        "$HOME is not set or not an absolute path; set it and restart"
    );
    env.home = Some(lab.home.join("gone"));
    let events = run_job(&env, job(JailPolicy::Required)).await;
    let reason = jail_reason(&events);
    assert!(
        reason.ends_with("gone) does not exist; create it or fix $HOME"),
        "{reason}"
    );
}

#[tokio::test]
async fn a_relative_codex_home_never_reaches_the_child_but_an_absolute_one_does() {
    // The jail ignores a relative `$CODEX_HOME` (it would resolve against the
    // private cwd), so codex must not see it either: it would then keep its
    // state somewhere the rules do not cover.
    for (value, kept) in [
        ("rel/codex", false),
        (".", false),
        ("", false),
        ("/abs/codex", true),
    ] {
        let lab = lab();
        lab.claude(GOOD);
        let mut env = lab.env();
        env.inject.push(("CODEX_HOME".into(), value.into()));
        run(env, request()).await;
        let dump = lab.recorded("env");
        let has = dump.lines().any(|l| l.starts_with("CODEX_HOME="));
        assert_eq!(has, kept, "{value:?}");
    }
}

/// A CLI that is an ELF whose loader is `interp`, run jailed with a helper that
/// must never be called.
async fn jailed_elf_with_loader(interp: &[u8]) -> (Lab, Vec<StreamEvent>) {
    let lab = lab();
    let cli = lab.bin.join("codex");
    elf_with_interp(&cli, interp, 300);
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = lab.helper("echo called > \"$REC/helper_called\"\nexit 99");
    let events = run_job(&env, codex_job(&["exec", "-"])).await;
    (lab, events)
}

#[tokio::test]
async fn a_loader_that_is_not_a_regular_file_stops_the_jailed_cli() {
    let lab_dir = lab();
    let folder = lab_dir.bin.display().to_string();
    for interp in [
        format!("{folder}\0"),
        "/nonexistent/ld.so\0".to_owned(),
        format!("{}/pipe\0", lab_dir.rec.display()),
    ] {
        if interp.contains("/pipe") {
            let c = std::ffi::CString::new(format!("{}/pipe", lab_dir.rec.display())).unwrap();
            // SAFETY: a plain mkfifo with a valid NUL-terminated path.
            assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        }
        let (lab, events) = jailed_elf_with_loader(interp.as_bytes()).await;
        let reason = jail_reason(&events);
        assert!(
            reason.contains("is not a regular file"),
            "{interp:?}: {reason}"
        );
        assert!(!lab.rec.join("helper_called").exists());
    }
}

#[tokio::test]
async fn a_loader_in_tmp_stops_the_jailed_cli() {
    let (lab, events) = jailed_elf_with_loader(b"/tmp/evil-ld.so\0").await;
    assert!(
        jail_reason(&events).contains("program loader /tmp/evil-ld.so"),
        "{events:?}"
    );
    assert!(!lab.rec.join("helper_called").exists());
}

const GEMINI_ANSWER: &str = r#"
printf '%s\0' "$@" > "$REC/args"
cat > "$REC/stdin"
for last; do :; done
cp "$last" "$REC/policy_copy"
{ stat -c %a "$last" 2>/dev/null || stat -f %Lp "$last"; } > "$REC/policy_mode"
printf '%s\n' '{"type":"init","session_id":"s","model":"m"}'
printf '%s\n' '{"type":"message","role":"assistant","content":"ok","delta":true}'
printf '%s\n' '{"type":"result","status":"success"}'
"#;

const POLICY: &str = "[[rule]]\ntoolName = \"*\"\n";

fn gemini_job(jail: JailPolicy) -> Job {
    Job {
        wire: Wire::GeminiStreamJson,
        binary: "gemini",
        cli: Cli::Gemini,
        args: vec!["-p".into(), "x".into()],
        stdin: b"hi".to_vec(),
        jail,
        tmp_files: vec![TmpFile {
            flag: "--policy",
            name: "gemini-policy.toml",
            content: POLICY,
        }],
        mcp: None,
        tools: None,
    }
}

#[tokio::test]
async fn a_job_file_is_written_private_to_the_turn_folder_and_named_last() {
    let lab = lab();
    script(&lab.bin.join("gemini"), &lab.rec, GEMINI_ANSWER);
    let events = run_job(&lab.env(), gemini_job(JailPolicy::Layered)).await;
    assert_eq!(
        events,
        [
            StreamEvent::Notice(
                "gemini runs without the jail here: the jail was not probed".into()
            ),
            StreamEvent::Delta("ok".into()),
            StreamEvent::Done(Finish::Stop)
        ]
    );
    let args = lab.args();
    assert_eq!(args[..2], ["-p", "x"]);
    assert_eq!(args[2], "--policy");
    let path = PathBuf::from(&args[3]);
    assert_eq!(args.len(), 4, "{args:?}");
    assert_eq!(path.file_name().unwrap(), "gemini-policy.toml");
    assert!(
        path.starts_with(lab.data.join("cli-tmp")),
        "inside the per-turn temp folder: {path:?}"
    );
    assert_eq!(lab.recorded("policy_copy"), POLICY);
    assert_eq!(lab.recorded("policy_mode").trim(), "600");
    assert!(!path.exists(), "the turn folder is wiped after the turn");
}

#[tokio::test]
async fn a_policy_file_whose_path_gemini_would_split_refuses_the_turn() {
    // gemini splits `--policy` on `,`: `<data>,x/..` would load `<data>`.
    let lab = lab();
    script(&lab.bin.join("gemini"), &lab.rec, GEMINI_ANSWER);
    let mut env = lab.env();
    env.data = Some(lab.data.with_file_name("data,x"));
    let events = run_job(&env, gemini_job(JailPolicy::Layered)).await;
    assert_eq!(
        events,
        [StreamEvent::Error(ChatError::Failed(
            UNUSABLE_FILE_PATH.into()
        ))]
    );
    assert!(!lab.rec.join("argv").exists(), "gemini never started");
}

#[tokio::test]
async fn a_jailed_job_file_is_inside_the_folder_the_jail_lets_the_cli_write() {
    let lab = lab();
    script(&lab.bin.join("gemini"), &lab.rec, GEMINI_ANSWER);
    let mut env = lab.env();
    env.jail = jail_on();
    env.helper = lab.helper(PASS);
    let events = run_job(&env, gemini_job(JailPolicy::Required)).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    let program = helper_program(&lab);
    let n = program.len();
    assert_eq!(program[n - 4..n - 2], ["-p", "x"]);
    assert_eq!(program[n - 2], "--policy");
    let path = PathBuf::from(&program[n - 1]);
    let raw = lab.recorded("helper_args");
    let rules: Vec<&str> = raw.split('\0').collect();
    let rw: Vec<&str> = rules
        .windows(2)
        .filter(|w| w[0] == "--rw")
        .map(|w| w[1])
        .collect();
    assert!(
        rw.iter().any(|dir| path.parent() == Some(Path::new(dir))),
        "{path:?} must sit in a rw folder: {rw:?}"
    );
    assert_eq!(lab.recorded("policy_copy"), POLICY);
}

#[tokio::test]
async fn a_job_without_a_file_gets_no_extra_argument() {
    let lab = lab();
    lab.claude(GOOD);
    run(lab.env(), request()).await;
    assert!(lab.args().iter().all(|a| a != "--policy"));
}

/// A Homebrew-like tree: an interpreter that is an ELF with a search path of
/// its own (`opt` links into a Cellar), the OpenSSL config and a CA bundle
/// reached through a link, and a script CLI that asks for the interpreter.
fn brew_like(lab: &Lab) -> (PathBuf, PathBuf) {
    let base = lab.bin.parent().unwrap().to_path_buf();
    let dir = |path: PathBuf| {
        fs::create_dir_all(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    let node_bin = dir(base.join("Cellar/node/1/bin"));
    let node_lib = dir(base.join("Cellar/node/1/lib"));
    let ssl_lib = dir(base.join("Cellar/openssl@3/3.6/lib"));
    dir(base.join("opt"));
    symlink(
        base.join("Cellar/openssl@3/3.6"),
        base.join("opt/openssl@3"),
    )
    .unwrap();
    dir(base.join("etc/openssl@3"));
    dir(base.join("etc/ca"));
    fs::write(base.join("etc/openssl@3/openssl.cnf"), "").unwrap();
    fs::write(base.join("etc/ca/cert.pem"), "").unwrap();
    symlink(
        base.join("etc/ca/cert.pem"),
        base.join("etc/openssl@3/cert.pem"),
    )
    .unwrap();
    let rpath = format!(
        "$ORIGIN/../lib:{0}/opt/openssl@3/lib:{0}/opt/missing/lib:/usr/lib",
        base.display()
    );
    let node = node_bin.join("fakenode");
    fs::write(
        &node,
        crate::core::ai::jail::rpath::tests::image_with_rpath(&rpath),
    )
    .unwrap();
    fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
    // The CLI is a script found through a link next to the interpreter.
    let cli = node_bin.join("codex");
    script(&cli, &lab.rec, CODEX_ANSWER);
    let text =
        fs::read_to_string(&cli)
            .unwrap()
            .replacen("#!/bin/sh", "#!/usr/bin/env fakenode", 1);
    fs::write(&cli, text).unwrap();
    symlink(&cli, lab.bin.join("codex")).unwrap();
    let _ = (node_lib, ssl_lib);
    (base, node_bin)
}

fn helper_rules(lab: &Lab, flag: &str) -> Vec<String> {
    let raw = lab.recorded("helper_args");
    let args: Vec<&str> = raw.split('\0').collect();
    args.windows(2)
        .filter(|w| w[0] == flag)
        .map(|w| w[1].to_owned())
        .collect()
}

#[tokio::test]
async fn the_library_folders_in_the_interpreters_elf_are_granted_read_only() {
    let lab = lab();
    let (base, node_bin) = brew_like(&lab);
    let mut env = lab.env();
    env.path = std::env::join_paths([&lab.bin, &node_bin]).unwrap();
    env.jail = jail_on();
    env.helper = lab.helper("printf '%s\\0' \"$@\" > \"$REC/helper_args\"\nexit 0");
    run_job(&env, codex_job(&["exec", "-"])).await;
    let canon = |p: PathBuf| fs::canonicalize(p).unwrap().to_str().unwrap().to_owned();

    let ro = helper_rules(&lab, "--ro");
    assert!(
        ro.contains(&canon(base.join("Cellar/node/1/lib"))),
        "{ro:?}"
    );
    assert!(
        ro.contains(&canon(base.join("opt/openssl@3/lib"))),
        "through the link: {ro:?}"
    );
    assert!(
        !ro.iter().any(|d| d.contains("missing")),
        "a folder that does not exist gets no rule: {ro:?}"
    );
    assert_eq!(ro.iter().filter(|d| *d == "/usr/lib").count(), 1, "{ro:?}");
    let rx = helper_rules(&lab, "--rx");
    assert!(
        !rx.iter()
            .any(|d| d.contains("openssl") || d.ends_with("/lib")),
        "libraries are never executable: {rx:?}"
    );
    assert!(
        !helper_rules(&lab, "--rw")
            .iter()
            .any(|d| d.contains("openssl"))
    );
}

#[tokio::test]
async fn the_homebrew_openssl_config_and_ca_bundle_are_exact_read_only_files() {
    let lab = lab();
    let (base, node_bin) = brew_like(&lab);
    let mut env = lab.env();
    env.path = std::env::join_paths([&lab.bin, &node_bin]).unwrap();
    env.jail = jail_on();
    env.helper = lab.helper("printf '%s\\0' \"$@\" > \"$REC/helper_args\"\nexit 0");
    run_job(&env, codex_job(&["exec", "-"])).await;
    let files = helper_rules(&lab, "--ro-file");
    let cnf = fs::canonicalize(base.join("etc/openssl@3/openssl.cnf")).unwrap();
    let cert = fs::canonicalize(base.join("etc/ca/cert.pem")).unwrap();
    assert!(
        files.contains(&cnf.to_str().unwrap().to_owned()),
        "{files:?}"
    );
    assert!(
        files.contains(&cert.to_str().unwrap().to_owned()),
        "the link's target, not the link: {files:?}"
    );
}

#[tokio::test]
async fn an_interpreter_without_a_search_path_gets_no_extra_rules() {
    let lab = lab();
    let _ = node_style_codex(&lab);
    let interp_dir = lab.bin.parent().unwrap().join("nodebin");
    let mut env = lab.env();
    env.path = std::env::join_paths([&lab.bin, &interp_dir]).unwrap();
    env.jail = jail_on();
    env.helper = lab.helper("printf '%s\\0' \"$@\" > \"$REC/helper_args\"\nexit 0");
    run_job(&env, codex_job(&["exec", "-"])).await;
    let ro = helper_rules(&lab, "--ro");
    assert!(
        ro.iter().all(|d| d.starts_with("/usr")
            || d.starts_with("/lib")
            || d == "/etc"
            || d.starts_with("/proc")
            || d.starts_with("/sys")
            || d.contains("cli-cwd")),
        "{ro:?}"
    );
}

#[tokio::test]
async fn several_job_files_are_each_private_and_named_in_order() {
    let lab = lab();
    script(&lab.bin.join("gemini"), &lab.rec, GEMINI_TWO);
    let mut job = gemini_job(JailPolicy::Layered);
    job.tmp_files.push(TmpFile {
        flag: "--extra",
        name: "extra.txt",
        content: "more",
    });
    let events = run_job(&lab.env(), job).await;
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    let args = lab.args();
    assert_eq!(args[..2], ["-p", "x"]);
    assert_eq!(args[2], "--policy");
    assert_eq!(args[4], "--extra");
    assert_eq!(args.len(), 6, "{args:?}");
    let (policy, extra) = (PathBuf::from(&args[3]), PathBuf::from(&args[5]));
    assert_eq!(extra.file_name().unwrap(), "extra.txt");
    assert_eq!(policy.parent(), extra.parent(), "one per-turn folder");
    assert_eq!(lab.recorded("extra_copy"), "more");
    assert_eq!(lab.recorded("extra_mode").trim(), "600");
    assert_eq!(lab.recorded("dir_mode").trim(), "700");
    assert!(!extra.exists() && !policy.exists(), "wiped after the turn");
}

const GEMINI_TWO: &str = r#"
printf '%s\0' "$@" > "$REC/args"
cat > "$REC/stdin"
for last; do :; done
cp "$last" "$REC/extra_copy"
{ stat -c %a "$last" 2>/dev/null || stat -f %Lp "$last"; } > "$REC/extra_mode"
{ stat -c %a "$(dirname "$last")" 2>/dev/null || stat -f %Lp "$(dirname "$last")"; } > "$REC/dir_mode"
printf '%s\n' '{"type":"init","session_id":"s","model":"m"}'
printf '%s\n' '{"type":"message","role":"assistant","content":"ok","delta":true}'
printf '%s\n' '{"type":"result","status":"success"}'
"#;

/// The claude of a turn with tools: it also copies the config it was given.
const MCP_CLAUDE: &str = r#"
if [ -e "$TMPDIR/mcp.json" ]; then echo yes > "$REC/mcp_during"; fi
for a; do
  case "$a" in
    --mcp-config=*)
      cp "${a#--mcp-config=}" "$REC/mcp_copy"
      { stat -c %a "${a#--mcp-config=}" 2>/dev/null || stat -f %Lp "${a#--mcp-config=}"; } > "$REC/mcp_mode"
      ;;
  esac
done
"#;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

fn claude_job(lab: &Lab, wired: bool) -> Job {
    Job {
        wire: Wire::ClaudeStreamJson,
        binary: "claude",
        cli: Cli::Claude,
        args: vec![
            "-p".into(),
            "--strict-mcp-config".into(),
            "--safe-mode".into(),
        ],
        stdin: b"hi".to_vec(),
        jail: JailPolicy::Layered,
        tmp_files: Vec::new(),
        mcp: wired.then(|| McpWire {
            socket: lab.rec.join("s.sock"),
            token: Secret::new(TOKEN),
            unwire: Unwire::none(),
        }),
        tools: None,
    }
}

fn with_shim(lab: &Lab, mut env: CliEnv, shim: &str) -> CliEnv {
    fs::write(lab.rec.join("s.sock"), "").unwrap();
    env.shim = Some(PathBuf::from(shim));
    env.jail = jail_on();
    env.helper = lab.helper(PASS);
    env
}

const OFF_NOTICE: &str = "tools are off for this turn: they could not be set up";

#[tokio::test]
async fn a_wired_claude_gets_the_config_the_flags_the_timeout_and_the_jail_grant() {
    let lab = lab();
    script(
        &lab.bin.join("claude"),
        &lab.rec,
        &format!("{MCP_CLAUDE}{GOOD}"),
    );
    let shim = lab.bin.join("shim");
    fs::write(&shim, "").unwrap();
    // The shim must pass the trust check, whatever the developer's umask.
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let events = run_job(&env, claude_job(&lab, true)).await;
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Done(Finish::Stop)),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, StreamEvent::Notice(_))),
        "{events:?}"
    );
    let args = lab.args();
    let n = args.len();
    // --safe-mode hides MCP servers: a wired turn trades it for an empty
    // --setting-sources, in the = form so nothing after it is its value.
    assert_eq!(args[..2], ["-p", "--strict-mcp-config"]);
    assert!(args.iter().all(|a| a != "--safe-mode"), "{args:?}");
    assert!(args[n - 3].starts_with("--mcp-config="), "{args:?}");
    assert!(args[n - 3].ends_with("/mcp.json"), "{args:?}");
    assert_eq!(args[n - 2], "--allowedTools=mcp__lazarobox");
    assert_eq!(args[n - 1], "--setting-sources=");
    let config = lab.recorded("mcp_copy");
    assert!(
        config.contains(TOKEN) && config.contains(shim.to_str().unwrap()),
        "{config}"
    );
    assert_eq!(lab.recorded("mcp_mode").trim(), "600");
    // The config existed while claude ran, and the token file is gone after.
    assert_eq!(lab.recorded("mcp_during").trim(), "yes");
    let config_path = PathBuf::from(args[n - 3].trim_start_matches("--mcp-config="));
    assert!(!config_path.exists(), "mcp.json is wiped after the turn");
    assert!(!config_path.parent().unwrap().exists(), "so is its folder");
    assert!(
        lab.recorded("env")
            .lines()
            .any(|l| l == "MCP_TOOL_TIMEOUT=900000")
    );
    // The token reaches the engine only through the config file.
    assert!(!lab.recorded("helper_args").contains(TOKEN));
    assert!(!args.iter().any(|a| a.contains(TOKEN)));
    assert!(!lab.recorded("env").contains(TOKEN));
    let helper = lab.recorded("helper_args");
    let helper: Vec<&str> = helper.split('\0').collect();
    let has = |flag: &str, path: &str| helper.windows(2).any(|w| w == [flag, path]);
    assert!(has("--rx", shim.to_str().unwrap()), "{helper:?}");
    assert!(
        has("--rw-file", lab.rec.join("s.sock").to_str().unwrap()),
        "{helper:?}"
    );
}

#[tokio::test]
async fn without_a_jail_a_wired_claude_runs_as_text_and_says_so() {
    let lab = lab();
    script(
        &lab.bin.join("claude"),
        &lab.rec,
        &format!("{MCP_CLAUDE}{GOOD}"),
    );
    let mut env = lab.env();
    env.shim = Some(lab.bin.join("shim"));
    let events = run_job(&env, claude_job(&lab, true)).await;
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Done(Finish::Stop)),
        "{events:?}"
    );
    let notices: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Notice(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(notices.len(), 1, "{events:?}");
    assert!(notices[0].contains(OFF_NOTICE), "{notices:?}");
    assert_eq!(lab.args(), ["-p", "--strict-mcp-config", "--safe-mode"]);
    assert_eq!(lab.recorded("mcp_copy"), "");
    assert!(!lab.recorded("env").contains("MCP_TOOL_TIMEOUT"));
}

#[tokio::test]
async fn a_jailed_claude_without_a_shim_program_stays_jailed_and_text_only() {
    let lab = lab();
    script(
        &lab.bin.join("claude"),
        &lab.rec,
        &format!("{MCP_CLAUDE}{GOOD}"),
    );
    let mut env = with_shim(&lab, lab.env(), "/unused");
    env.shim = None;
    let events = run_job(&env, claude_job(&lab, true)).await;
    assert_eq!(
        events[0],
        StreamEvent::Notice(OFF_NOTICE.into()),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert_eq!(lab.args(), ["-p", "--strict-mcp-config", "--safe-mode"]);
    assert!(
        !lab.recorded("helper_args").is_empty(),
        "still through the jail helper"
    );
}

/// A shim program that passes the trust check and that the jail still refuses
/// to grant: it sits in `~/.ssh`, which no layout may open.
fn shim_in_ssh(lab: &Lab) -> PathBuf {
    let dir = lab.home.join(".ssh");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    let shim = dir.join("lazarobox-shell");
    fs::write(&shim, "").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    shim
}

#[test]
fn the_jail_refuses_that_shim_and_only_that_shim() {
    let lab = lab();
    script(&lab.bin.join("claude"), &lab.rec, GOOD);
    let shim = shim_in_ssh(&lab);
    let socket = lab.rec.join("s.sock");
    fs::write(&socket, "").unwrap();
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let dirs = Dirs::create(env.data.as_deref().unwrap()).unwrap();
    let binary = lab.bin.join("claude");
    let grant = Some((shim.as_path(), socket.as_path()));
    let refusal = jail_plan(&env, Cli::Claude, &binary, &dirs, grant).unwrap_err();
    assert_eq!(refusal, JailRefusal::RootTooBroad(shim.clone()));
    // The same layout without the shim is fine: it is the only cause.
    assert!(jail_plan(&env, Cli::Claude, &binary, &dirs, None).is_ok());
}

#[tokio::test]
async fn a_shim_the_jail_refuses_retries_the_jail_without_it() {
    let lab = lab();
    script(
        &lab.bin.join("claude"),
        &lab.rec,
        &format!("{MCP_CLAUDE}{GOOD}"),
    );
    let shim = shim_in_ssh(&lab);
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let events = run_job(&env, claude_job(&lab, true)).await;
    assert_eq!(
        events[0],
        StreamEvent::Notice(OFF_NOTICE.into()),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    // The retry ran: through the jail helper, with no flag of the tools, no
    // grant for the shim or the socket, and no config file on disk.
    assert_eq!(lab.args(), ["-p", "--strict-mcp-config", "--safe-mode"]);
    let helper = lab.recorded("helper_args");
    assert!(!helper.is_empty(), "still through the jail helper");
    assert!(
        !helper.contains(shim.to_str().unwrap()) && !helper.contains("s.sock"),
        "{helper:?}"
    );
    assert_eq!(lab.recorded("mcp_copy"), "", "no --mcp-config was given");
    assert_eq!(
        lab.recorded("mcp_during"),
        "",
        "no config file while it ran"
    );
    assert!(!lab.recorded("env").contains("MCP_TOOL_TIMEOUT"));
    let leftovers = fs::read_dir(lab.data.join("cli-tmp")).unwrap().count();
    assert_eq!(leftovers, 0, "the turn's folder is wiped");
}

/// A shim program the trust check refuses (it sits in a folder others can
/// write) is retried without it, like an interpreter: text with the notice.
async fn assert_untrusted_shim_falls_back(lab: &Lab, shim: &Path) {
    script(
        &lab.bin.join("claude"),
        &lab.rec,
        &format!("{MCP_CLAUDE}{GOOD}"),
    );
    let env = with_shim(lab, lab.env(), shim.to_str().unwrap());
    let events = run_job(&env, claude_job(lab, true)).await;
    assert_eq!(
        events[0],
        StreamEvent::Notice(OFF_NOTICE.into()),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert_eq!(lab.args(), ["-p", "--strict-mcp-config", "--safe-mode"]);
    let helper = lab.recorded("helper_args");
    assert!(
        !helper.is_empty() && !helper.contains(shim.to_str().unwrap()),
        "{helper:?}"
    );
}

#[tokio::test]
async fn a_shim_in_a_folder_others_can_write_is_not_granted() {
    let lab = lab();
    let dir = lab.bin.join("shimdir");
    fs::create_dir(&dir).unwrap();
    let shim = dir.join("lazarobox-shell");
    fs::write(&shim, "").unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o775)).unwrap();
    assert_untrusted_shim_falls_back(&lab, &shim).await;
}

#[tokio::test]
async fn a_shim_that_others_can_write_is_not_granted() {
    let lab = lab();
    let shim = lab.bin.join("shim");
    fs::write(&shim, "").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o666)).unwrap();
    assert_untrusted_shim_falls_back(&lab, &shim).await;
}

#[tokio::test]
async fn a_shim_whose_loader_others_can_write_is_not_granted() {
    let lab = lab();
    let dir = lab.bin.join("loaderdir");
    fs::create_dir(&dir).unwrap();
    let loader = dir.join("ld.so");
    fs::write(&loader, "").unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o775)).unwrap();
    // A minimal ELF64 whose PT_INTERP names the loader.
    let name = format!("{}\0", loader.display());
    let mut img = vec![0u8; 512];
    img[..6].copy_from_slice(b"\x7fELF\x02\x01");
    img[0x20..0x28].copy_from_slice(&64u64.to_le_bytes());
    img[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());
    img[0x38..0x3a].copy_from_slice(&1u16.to_le_bytes());
    img[64..68].copy_from_slice(&3u32.to_le_bytes());
    img[72..80].copy_from_slice(&300u64.to_le_bytes());
    img[96..104].copy_from_slice(&(name.len() as u64).to_le_bytes());
    img[300..300 + name.len()].copy_from_slice(name.as_bytes());
    let shim = lab.bin.join("shim");
    fs::write(&shim, img).unwrap();
    assert_untrusted_shim_falls_back(&lab, &shim).await;
}

/// A wire that counts how often its turn said the engine is not wired.
fn counted(job: &mut Job) -> Arc<AtomicUsize> {
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    job.mcp.as_mut().unwrap().unwire = Unwire::new(move || {
        seen.fetch_add(1, Ordering::SeqCst);
    });
    count
}

#[tokio::test]
async fn a_turn_that_runs_as_text_ends_its_session_and_a_wired_one_keeps_it() {
    // Wired: the session is the engine's.
    let lab = lab();
    script(
        &lab.bin.join("claude"),
        &lab.rec,
        &format!("{MCP_CLAUDE}{GOOD}"),
    );
    let shim = lab.bin.join("shim");
    fs::write(&shim, "").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let mut job = claude_job(&lab, true);
    let wired = counted(&mut job);
    run_job(&env, job).await;
    assert_eq!(wired.load(Ordering::SeqCst), 0);

    // No jail (a bare run), no shim program, a shim the jail refuses: text.
    let mut bare_env = lab.env();
    bare_env.shim = Some(shim.clone());
    let mut no_shim = with_shim(&lab, lab.env(), "/unused");
    no_shim.shim = None;
    let refused = with_shim(&lab, lab.env(), "/tmp/lazarobox-shell");
    for (what, env) in [
        ("bare", bare_env),
        ("no shim", no_shim),
        ("refused", refused),
    ] {
        let mut job = claude_job(&lab, true);
        let count = counted(&mut job);
        let events = run_job(&env, job).await;
        assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
        assert_eq!(count.load(Ordering::SeqCst), 1, "{what}");
    }
}

#[tokio::test]
async fn a_turn_the_plan_refuses_ends_its_session_too() {
    let lab = lab();
    let mut env = lab.env();
    env.path = OsString::new();
    let mut job = claude_job(&lab, true);
    let count = counted(&mut job);
    let events = run_job(&env, job).await;
    assert!(matches!(events.last(), Some(StreamEvent::Error(_))));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

/// A claude that starts one of lazarobox's own tools, then answers.
const OWN_TOOL: &str = r#"
cat > /dev/null
printf '%s\n' '{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t","name":"mcp__lazarobox__find_dir","input":{}}},"parent_tool_use_id":null}'
printf '%s\n' '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}},"parent_tool_use_id":null}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"hi"}'
"#;

#[tokio::test]
async fn only_a_wired_turn_lets_claude_use_our_tools() {
    let lab = lab();
    script(&lab.bin.join("claude"), &lab.rec, OWN_TOOL);
    let shim = lab.bin.join("shim");
    fs::write(&shim, "").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    let wired = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let events = run_job(&wired, claude_job(&lab, true)).await;
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Done(Finish::Stop)),
        "{events:?}"
    );
    // The same stream in a turn that fell back to text: nothing was offered.
    let mut bare = lab.env();
    bare.shim = Some(shim);
    let events = run_job(&bare, claude_job(&lab, true)).await;
    assert_eq!(failure(&events), "claude tried to use a tool", "{events:?}");
}

/// The final process of a plan, with the turn's own private folder (which is
/// different every time) named `<tmp>`.
fn final_process(plan: &Plan) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
    let tmp = plan.dirs.tmp.display().to_string();
    let text = |s: &OsString| s.to_string_lossy().replace(&tmp, "<tmp>");
    (
        plan.program.clone(),
        plan.argv.iter().map(text).collect(),
        plan.vars.iter().map(|(k, v)| (text(k), text(v))).collect(),
    )
}

// Spec (agent-tools): while an engine is outside the verified set (an empty
// one here, through `build_with`), a turn with tools on starts exactly the
// process of a turn without: the same program, argv (jail helper arguments
// included), variables, notice and stdin.
#[test]
fn while_dormant_no_engine_starts_a_different_process_with_tools_on() {
    use crate::core::ai::chat::adapters::{Invocation, adapter_for};
    let lab = lab();
    for name in ["claude", "codex", "gemini"] {
        script(&lab.bin.join(name), &lab.rec, GOOD);
    }
    let shim = lab.bin.join("shim");
    fs::write(&shim, "").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    let socket = lab.rec.join("s.sock");
    fs::write(&socket, "").unwrap();
    let wire = Box::new(McpWire {
        socket,
        token: Secret::new(TOKEN),
        unwire: Unwire::none(),
    });
    for (jail, shim) in [
        (JailSupport::Unavailable(JailOff::NotProbed), None),
        (jail_on(), Some(shim)),
    ] {
        let jail_off = matches!(jail, JailSupport::Unavailable(_));
        let mut env = lab.env();
        env.jail = jail;
        env.helper = lab.helper(PASS);
        env.shim = shim;
        for engine in ["claude", "codex", "gemini"] {
            let mut off = request();
            off.engine = engine.into();
            let mut on = off.clone();
            on.tools = Tools::On {
                origin: Some(lab.home.clone()),
                external: Default::default(),
                mcp: Some(wire.clone()),
            };
            let adapter = adapter_for(engine).unwrap();
            let (Invocation::Process(off), Invocation::Process(on)) =
                (adapter.build_with(&off, &[]), adapter.build_with(&on, &[]))
            else {
                panic!("{engine} is a CLI engine");
            };
            assert_eq!(on.mcp, None, "{engine}");
            assert_eq!(on, off, "{engine}");
            // A refusal (a required jail that is off) is part of the process
            // too: it must be the same.
            let render = |planned: Result<Plan, StreamEvent>| {
                planned.map(|p| (final_process(&p), p.notice.clone(), p.wired))
            };
            let (off, on) = (render(plan(&env, &off)), render(plan(&env, &on)));
            assert_eq!(on, off, "{engine}");
            assert!(on.is_ok_and(|(_, _, wired)| !wired) || jail_off, "{engine}");
        }
    }
}

#[tokio::test]
async fn a_claude_job_without_a_wire_is_untouched_even_with_a_shim_program() {
    let lab = lab();
    script(
        &lab.bin.join("claude"),
        &lab.rec,
        &format!("{MCP_CLAUDE}{GOOD}"),
    );
    let env = with_shim(&lab, lab.env(), "/opt/lb/lazarobox-shell");
    let events = run_job(&env, claude_job(&lab, false)).await;
    assert_eq!(
        events,
        [
            StreamEvent::Delta("hi".into()),
            StreamEvent::Done(Finish::Stop)
        ]
    );
    assert_eq!(lab.args(), ["-p", "--strict-mcp-config", "--safe-mode"]);
}

#[test]
fn the_wiring_names_the_shims_subcommand_and_token_variable() {
    use crate::runtime::mcp::shim;
    assert_eq!(wiring::SUBCOMMAND, shim::SUBCOMMAND);
    assert_eq!(wiring::TOKEN_ENV, shim::TOKEN_ENV);
}

#[test]
fn the_real_machine_resolves_its_own_executable_once_as_the_shim() {
    let env = CliEnv::from_env(JailSupport::Unavailable(JailOff::NotProbed));
    let shim = env.shim.expect("current_exe works in tests");
    assert!(shim.is_absolute() && shim.is_file(), "{shim:?}");
    assert_eq!(shim, fs::canonicalize(&shim).unwrap());
    assert_ne!(shim, Path::new("/proc/self/exe"));
}

/// A codex of a turn with tools: it records its argv and env, then answers.
const MCP_CODEX: &str = r#"
printf '%s\0' "$@" > "$REC/args"
env > "$REC/env"
if [ -e "$TMPDIR/mcp.json" ]; then echo yes > "$REC/mcp_during"; fi
"#;

fn wired_codex_job(lab: &Lab) -> Job {
    Job {
        mcp: claude_job(lab, true).mcp,
        tools: None,
        ..codex_job(&["exec", "-"])
    }
}

fn trusted_shim(lab: &Lab) -> PathBuf {
    let shim = lab.bin.join("shim");
    fs::write(&shim, "").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    shim
}

#[tokio::test]
async fn a_wired_codex_gets_the_overrides_and_the_jail_grant_and_the_token_only_in_its_env() {
    let lab = lab();
    script(
        &lab.bin.join("codex"),
        &lab.rec,
        &format!("{MCP_CODEX}{CODEX_ANSWER}"),
    );
    let shim = trusted_shim(&lab);
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let events = run_job(&env, wired_codex_job(&lab)).await;
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Done(Finish::Stop)),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, StreamEvent::Notice(_))),
        "{events:?}"
    );
    let args = lab.args();
    assert_eq!(args[..2], ["exec", "-"]);
    let socket = lab.rec.join("s.sock");
    let want = wiring::codex(&shim, &wired_codex_job(&lab).mcp.unwrap())
        .unwrap()
        .args;
    assert_eq!(args[2..], want[..], "{args:?}");
    assert!(args.contains(&format!(
        "mcp_servers.lazarobox.command=\"{}\"",
        shim.display()
    )));
    assert!(args.iter().any(|a| a.contains(socket.to_str().unwrap())));
    // The token reaches codex only through its own environment.
    assert!(
        lab.recorded("env")
            .lines()
            .any(|l| l == format!("LAZAROBOX_MCP_TOKEN={TOKEN}"))
    );
    assert!(!args.iter().any(|a| a.contains(TOKEN)), "{args:?}");
    assert!(!lab.recorded("helper_args").contains(TOKEN));
    // codex has no config file: the turn's folder held no `mcp.json` while it
    // ran (the fake records one if it finds it, and it is not there).
    assert_eq!(
        lab.recorded("mcp_during"),
        "",
        "no config file while it ran"
    );
    assert!(!args.iter().any(|a| a.contains("mcp.json")), "{args:?}");
    let helper = lab.recorded("helper_args");
    let helper: Vec<&str> = helper.split('\0').collect();
    let has = |flag: &str, path: &str| helper.windows(2).any(|w| w == [flag, path]);
    assert!(has("--rx", shim.to_str().unwrap()), "{helper:?}");
    assert!(has("--rw-file", socket.to_str().unwrap()), "{helper:?}");
}

/// A codex turn that fell back to text: it ran bare of the tools, said so, and
/// ended its session once.
async fn assert_codex_text_fallback(lab: &Lab, env: &CliEnv) {
    script(
        &lab.bin.join("codex"),
        &lab.rec,
        &format!("{MCP_CODEX}{CODEX_ANSWER}"),
    );
    let mut job = wired_codex_job(lab);
    let count = counted(&mut job);
    let events = run_job(env, job).await;
    assert_eq!(
        events[0],
        StreamEvent::Notice(OFF_NOTICE.into()),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert_eq!(count.load(Ordering::SeqCst), 1, "Unwire exactly once");
    assert!(
        !lab.recorded("env").contains("LAZAROBOX_MCP_TOKEN"),
        "no token in the child env"
    );
    let args = lab.args();
    assert_eq!(args, ["exec", "-"], "no -c mcp_servers entries: {args:?}");
    assert!(!args.iter().any(|a| a.contains("mcp_servers")));
    assert_eq!(lab.recorded("mcp_during"), "");
}

#[tokio::test]
async fn a_shim_the_jail_refuses_retries_codex_without_the_tools() {
    let lab = lab();
    let shim = shim_in_ssh(&lab);
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    assert_codex_text_fallback(&lab, &env).await;
    let helper = lab.recorded("helper_args");
    assert!(
        !helper.is_empty() && !helper.contains(shim.to_str().unwrap()),
        "{helper:?}"
    );
}

#[tokio::test]
async fn an_untrusted_shim_runs_codex_as_text_with_the_notice() {
    let lab = lab();
    let shim = lab.bin.join("shim");
    fs::write(&shim, "").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o666)).unwrap();
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    assert_codex_text_fallback(&lab, &env).await;
    let helper = lab.recorded("helper_args");
    assert!(
        !helper.is_empty() && !helper.contains(shim.to_str().unwrap()),
        "{helper:?}"
    );
}

#[tokio::test]
async fn the_turns_token_is_not_shown_from_a_codex_stderr_tail() {
    let lab = lab();
    script(
        &lab.bin.join("codex"),
        &lab.rec,
        "cat > /dev/null\necho \"cannot reach $LAZAROBOX_MCP_TOKEN now\" >&2\nexit 1",
    );
    let shim = trusted_shim(&lab);
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let mut job = wired_codex_job(&lab);
    job.mcp.as_mut().unwrap().token = Secret::new("hunter2");
    let events = run_job(&env, job).await;
    assert_eq!(failure(&events), "cannot reach (..) now", "{events:?}");
}

/// A codex that starts one of lazarobox's own tools, then answers.
const CODEX_OWN_TOOL: &str = r#"
cat > /dev/null
printf '%s\n' '{"type":"item.started","item":{"id":"i","type":"mcp_tool_call","server":"lazarobox","tool":"find_dir","arguments":{},"status":"in_progress"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"j","type":"agent_message","text":"ok"}}'
printf '%s\n' '{"type":"turn.completed","usage":{}}'
"#;

#[tokio::test]
async fn only_a_wired_turn_lets_codex_use_our_tools() {
    let lab = lab();
    script(&lab.bin.join("codex"), &lab.rec, CODEX_OWN_TOOL);
    let shim = trusted_shim(&lab);
    let wired = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let events = run_job(&wired, wired_codex_job(&lab)).await;
    assert_eq!(
        events.last(),
        Some(&StreamEvent::Done(Finish::Stop)),
        "{events:?}"
    );
    // The same stream in a turn that fell back to text (no shim program).
    let mut bare = with_shim(&lab, lab.env(), "/unused");
    bare.shim = None;
    let mut job = wired_codex_job(&lab);
    let count = counted(&mut job);
    let events = run_job(&bare, job).await;
    assert_eq!(failure(&events), "codex tried to use a tool", "{events:?}");
    assert_eq!(count.load(Ordering::SeqCst), 1, "its session ended");
}

/// A gemini of a turn with tools: it records where it runs, what it finds in
/// the workspace `.gemini` folder, its argv and its env.
macro_rules! gemini_record {
    () => {
        r#"
printf '%s\0' "$@" > "$REC/args"
pwd -P > "$REC/cwd"
env > "$REC/env"
cat > /dev/null
if [ -e "$TMPDIR/work" ]; then echo yes > "$REC/work_during"; fi
if [ -e .gemini/settings.json ]; then
  cp .gemini/settings.json "$REC/settings_copy"
  { stat -c %a .gemini/settings.json 2>/dev/null || stat -f %Lp .gemini/settings.json; } > "$REC/settings_mode"
  { stat -c %a .gemini 2>/dev/null || stat -f %Lp .gemini; } > "$REC/dotgemini_mode"
  { stat -c %a . 2>/dev/null || stat -f %Lp .; } > "$REC/work_mode"
fi
for a; do
  case "$a" in
    --admin-policy=*)
      cp "${a#--admin-policy=}" "$REC/policy_copy"
      { stat -c %a "${a#--admin-policy=}" 2>/dev/null || stat -f %Lp "${a#--admin-policy=}"; } > "$REC/policy_mode"
      ;;
  esac
done
printf '%s\n' '{"type":"init","session_id":"s","model":"m"}'
"#
    };
}

const GEMINI_END: &str = r#"
printf '%s\n' '{"type":"message","role":"assistant","content":"ok","delta":true}'
printf '%s\n' '{"type":"result","status":"success"}'
"#;

/// The record, a call to one of lazarobox's tools, and an answer.
const MCP_GEMINI: &str = concat!(
    gemini_record!(),
    r#"
printf '%s\n' '{"type":"tool_use","tool_name":"mcp_lazarobox_find_dir","tool_id":"t","parameters":{}}'
printf '%s\n' '{"type":"tool_result","tool_id":"t","status":"success"}'
"#
);

/// The record and an answer, no tool.
const MCP_GEMINI_TEXT: &str = gemini_record!();

fn wired_gemini_job(lab: &Lab) -> Job {
    Job {
        wire: Wire::GeminiStreamJson,
        binary: "gemini",
        cli: Cli::Gemini,
        args: [
            "--skip-trust",
            "-p",
            "x",
            "--allowed-mcp-server-names",
            "none",
        ]
        .map(str::to_owned)
        .into(),
        stdin: b"hi".to_vec(),
        jail: JailPolicy::Required,
        tmp_files: vec![TmpFile {
            flag: "--policy",
            name: "gemini-policy.toml",
            content: POLICY,
        }],
        mcp: claude_job(lab, true).mcp,
        tools: None,
    }
}

#[tokio::test]
async fn a_wired_gemini_runs_in_a_private_work_folder_with_the_settings_and_the_admin_policy() {
    let lab = lab();
    script(
        &lab.bin.join("gemini"),
        &lab.rec,
        &format!("{MCP_GEMINI}{GEMINI_END}"),
    );
    let shim = trusted_shim(&lab);
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let events = run_job(&env, wired_gemini_job(&lab)).await;
    assert_eq!(
        events,
        [
            StreamEvent::Delta("ok".into()),
            StreamEvent::Done(Finish::Stop)
        ],
        "our tool call passes the wired decoder, no notice"
    );
    let args = lab.args();
    // The pinned argv, its own policy file, then the wiring in the `=` form
    // (both flags are variadic: nothing is left to swallow what follows).
    assert_eq!(
        args[..5],
        [
            "--skip-trust",
            "-p",
            "x",
            "--allowed-mcp-server-names",
            "none"
        ]
    );
    assert_eq!(args[5], "--policy");
    assert!(args[6].ends_with("/gemini-policy.toml"), "{args:?}");
    let n = args.len();
    assert_eq!(n, 9, "{args:?}");
    assert_eq!(args[7], "--allowed-mcp-server-names=lazarobox");
    assert!(args[8].starts_with("--admin-policy="), "{args:?}");
    assert!(args[8].ends_with("/gemini-mcp-policy.toml"), "{args:?}");
    // It ran in `<turn tmp>/work`, not in the shared empty folder.
    let tmp = Path::new(&args[6]).parent().unwrap().to_owned();
    assert_eq!(
        lab.recorded("cwd").trim(),
        tmp.join("work").to_str().unwrap()
    );
    assert!(tmp.starts_with(lab.data.join("cli-tmp")));
    // The settings hold the token and the server, 0600 in 0700 folders.
    let settings = lab.recorded("settings_copy");
    assert!(
        settings.contains(TOKEN) && settings.contains(shim.to_str().unwrap()),
        "{settings}"
    );
    assert!(settings.contains("\"trust\": true") && settings.contains("900000"));
    assert_eq!(lab.recorded("settings_mode").trim(), "600");
    assert_eq!(lab.recorded("dotgemini_mode").trim(), "700");
    assert_eq!(lab.recorded("work_mode").trim(), "700");
    let policy = lab.recorded("policy_copy");
    assert!(policy.contains("mcpName = \"lazarobox\""), "{policy}");
    assert_eq!(lab.recorded("policy_mode").trim(), "600");
    // The token is in the file only: not argv, not the helper's, not the env.
    assert!(!args.iter().any(|a| a.contains(TOKEN)), "{args:?}");
    assert!(!lab.recorded("helper_args").contains(TOKEN));
    assert!(!lab.recorded("env").contains(TOKEN));
    // `--skip-trust` trusts the folder too late: the env var does it in time.
    assert!(
        lab.recorded("env")
            .lines()
            .any(|l| l == "GEMINI_CLI_TRUST_WORKSPACE=true"),
        "{}",
        lab.recorded("env")
    );
    // The user's own servers (`~/.gemini/settings.json`) load whether or not
    // the folder is trusted: the allow-list is what keeps them out.
    assert!(
        args.iter()
            .any(|a| a == "--allowed-mcp-server-names=lazarobox"),
        "{args:?}"
    );
    let helper = lab.recorded("helper_args");
    let helper: Vec<&str> = helper.split('\0').collect();
    let has = |flag: &str, path: &str| helper.windows(2).any(|w| w == [flag, path]);
    assert!(has("--rx", shim.to_str().unwrap()), "{helper:?}");
    assert!(
        has("--rw-file", lab.rec.join("s.sock").to_str().unwrap()),
        "{helper:?}"
    );
    // Wiped after the turn, the work folder and the token with it.
    assert!(!tmp.exists(), "{tmp:?}");
}

#[tokio::test]
async fn the_turns_token_is_not_shown_from_a_gemini_stderr_tail() {
    let lab = lab();
    script(
        &lab.bin.join("gemini"),
        &lab.rec,
        "cat > /dev/null\necho \"cannot reach $(cat .gemini/settings.json | tr -d ' \\n') now\" >&2\nexit 1",
    );
    let shim = trusted_shim(&lab);
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    let mut job = wired_gemini_job(&lab);
    job.mcp.as_mut().unwrap().token = Secret::new("hunter2");
    let events = run_job(&env, job).await;
    let text = failure(&events);
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains("(..)"), "{text}");
}

/// A gemini turn that fell back to text: it ran in the shared empty folder
/// with the pinned argv and nothing of the wiring, said so, and ended its
/// session once.
async fn assert_gemini_text_fallback(lab: &Lab, env: &CliEnv) {
    script(
        &lab.bin.join("gemini"),
        &lab.rec,
        &format!("{MCP_GEMINI_TEXT}{GEMINI_END}"),
    );
    let mut job = wired_gemini_job(lab);
    let count = counted(&mut job);
    let events = run_job(env, job).await;
    assert_eq!(
        events[0],
        StreamEvent::Notice(OFF_NOTICE.into()),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&StreamEvent::Done(Finish::Stop)));
    assert_eq!(count.load(Ordering::SeqCst), 1, "Unwire exactly once");
    let args = lab.args();
    assert_eq!(args.len(), 7, "{args:?}");
    assert!(
        !args.iter().any(|a| a.starts_with("--admin-policy")
            || a.starts_with("--allowed-mcp-server-names=")
            || a.contains("gemini-mcp-policy")),
        "{args:?}"
    );
    assert_eq!(
        Path::new(lab.recorded("cwd").trim()),
        lab.data.join("cli-cwd")
    );
    assert_eq!(lab.recorded("work_during"), "");
    assert_eq!(lab.recorded("settings_copy"), "");
    assert!(!lab.recorded("env").contains(TOKEN));
    assert!(!lab.recorded("env").contains("GEMINI_CLI_TRUST_WORKSPACE"));
}

#[tokio::test]
async fn a_shim_the_jail_refuses_retries_gemini_without_the_tools() {
    let lab = lab();
    let shim = shim_in_ssh(&lab);
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    assert_gemini_text_fallback(&lab, &env).await;
    let helper = lab.recorded("helper_args");
    assert!(
        !helper.is_empty() && !helper.contains(shim.to_str().unwrap()),
        "{helper:?}"
    );
}

#[tokio::test]
async fn an_untrusted_shim_runs_gemini_as_text_with_the_notice() {
    let lab = lab();
    let shim = lab.bin.join("shim");
    fs::write(&shim, "").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o666)).unwrap();
    let env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    assert_gemini_text_fallback(&lab, &env).await;
}

#[tokio::test]
async fn only_a_wired_turn_lets_gemini_use_our_tools() {
    let lab = lab();
    script(
        &lab.bin.join("gemini"),
        &lab.rec,
        &format!("{MCP_GEMINI}{GEMINI_END}"),
    );
    // No shim program: the same stream in a turn that fell back to text.
    let mut bare = with_shim(&lab, lab.env(), "/unused");
    bare.shim = None;
    let mut job = wired_gemini_job(&lab);
    let count = counted(&mut job);
    let events = run_job(&bare, job).await;
    assert_eq!(failure(&events), "gemini tried to use a tool", "{events:?}");
    assert_eq!(count.load(Ordering::SeqCst), 1, "its session ended");
}

// The tools prompt reaches an engine only on the attempt that is wired. These
// tests build the job with the real adapters and run it through `plan`.

#[derive(Clone, Copy, PartialEq, Debug)]
enum Setup {
    /// A trusted shim the jail grants: the turn is wired.
    Wired,
    /// A trusted shim the jail refuses: the retry runs as text.
    RefusedShim,
    /// No shim program at all.
    NoShim,
    /// No jail (only for claude, whose policy is `Layered`).
    NoJail,
}

fn chat_system() -> String {
    use crate::core::ai::config::{Profile, Task};
    crate::core::ai::chat::prompts::builtin(Profile::Programmer, Task::Chat)
        .trim_end()
        .to_owned()
}

/// Runs a turn of `engine` built by its real adapter and returns what the fake
/// CLI saw: its argv and its stdin.
async fn real_turn(engine: &str, setup: Setup) -> (Vec<String>, String) {
    use crate::core::ai::chat::adapters::adapter_for;
    let lab = lab();
    let body = match engine {
        "claude" => format!("{MCP_CLAUDE}{GOOD}"),
        "codex" => format!("{MCP_CODEX}{CODEX_ANSWER}"),
        _ => format!("printf '%s\\0' \"$@\" > \"$REC/args\"\ncat > \"$REC/stdin\"\n{GEMINI_END}"),
    };
    script(&lab.bin.join(engine), &lab.rec, &body);
    let shim = match setup {
        Setup::RefusedShim => shim_in_ssh(&lab),
        _ => trusted_shim(&lab),
    };
    let mut env = with_shim(&lab, lab.env(), shim.to_str().unwrap());
    match setup {
        Setup::NoShim => env.shim = None,
        Setup::NoJail => {
            env.jail = JailSupport::Unavailable(JailOff::NotProbed);
        }
        _ => {}
    }
    let mut request = request();
    request.engine = engine.into();
    request.model = None;
    request.system = chat_system();
    request.messages = vec![ChatMessage {
        role: Role::User,
        content: "hi".into(),
    }];
    request.tools = Tools::On {
        origin: None,
        external: Default::default(),
        mcp: claude_job(&lab, true).mcp.map(Box::new),
    };
    let crate::core::ai::chat::adapters::Invocation::Process(job) =
        adapter_for(engine).unwrap().build_with(&request, &[engine])
    else {
        panic!("a process expected");
    };
    let _ = run_job(&env, job).await;
    (lab.args(), lab.recorded("stdin"))
}

const NO_ACCESS: &str = "no access to files, a shell or the terminal";
const HAS_TOOLS: &str = "run a command in the user's shell pane";

fn system_after(args: &[String], flag: &str) -> String {
    let at = args.iter().position(|a| a == flag).expect(flag);
    args[at + 1].clone()
}

#[tokio::test]
async fn the_real_wired_claude_argv_swaps_only_safe_mode_and_carries_the_tools_prompt() {
    let (args, _) = real_turn("claude", Setup::Wired).await;
    assert!(!args.contains(&"--safe-mode".to_owned()), "{args:?}");
    for kept in [
        "--disable-slash-commands",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--system-prompt",
    ] {
        assert!(args.contains(&kept.to_owned()), "{kept}: {args:?}");
    }
    let tools = args.iter().position(|a| a == "--tools").unwrap();
    // The recorder drops empty arguments: the empty list leaves the next flag.
    assert_eq!(
        args[tools + 1],
        "--strict-mcp-config",
        "--tools stays empty"
    );
    assert_eq!(args.last().unwrap(), "--setting-sources=");
    assert_eq!(
        args.iter().filter(|a| *a == "--setting-sources=").count(),
        1
    );
    let system = system_after(&args, "--system-prompt");
    assert!(
        system.contains(HAS_TOOLS) && !system.contains(NO_ACCESS),
        "{system}"
    );
}

#[tokio::test]
async fn a_claude_turn_that_ends_up_text_only_keeps_safe_mode_and_its_prompt() {
    for setup in [Setup::RefusedShim, Setup::NoShim, Setup::NoJail] {
        let (args, stdin) = real_turn("claude", setup).await;
        assert!(
            args.contains(&"--safe-mode".to_owned()),
            "{setup:?}: {args:?}"
        );
        assert!(
            !args.iter().any(|a| a.starts_with("--setting-sources")),
            "{setup:?}: {args:?}"
        );
        assert_eq!(
            system_after(&args, "--system-prompt"),
            chat_system(),
            "{setup:?}"
        );
        assert!(!stdin.contains(HAS_TOOLS), "{setup:?}");
    }
}

#[tokio::test]
async fn codex_and_gemini_get_the_tools_prompt_only_when_wired() {
    for engine in ["codex", "gemini"] {
        let (_, stdin) = real_turn(engine, Setup::Wired).await;
        assert!(
            stdin.contains(HAS_TOOLS) && !stdin.contains(NO_ACCESS),
            "{engine}: {stdin}"
        );
        for setup in [Setup::RefusedShim, Setup::NoShim] {
            let (_, stdin) = real_turn(engine, setup).await;
            assert!(
                stdin.starts_with(&format!("Instructions:\n{}\n\n", chat_system())),
                "{engine} {setup:?}: {stdin}"
            );
            assert!(!stdin.contains(HAS_TOOLS), "{engine} {setup:?}");
        }
    }
}

const OTHER_OS: &str = "the CLI jail needs Linux";
const OTHER_OS_HINT: &str = "; on this system use claude or Ollama for chat";

#[test]
fn a_refused_codex_turn_on_another_system_says_to_use_claude_or_ollama() {
    let mut env = lab().env();
    env.jail = JailSupport::Unavailable(JailOff::NotLinux);
    let planned = plan(&env, &codex_job(&[]));
    assert_eq!(
        planned.err(),
        Some(StreamEvent::Error(ChatError::Jail(format!(
            "{OTHER_OS}{OTHER_OS_HINT}"
        ))))
    );
}

#[test]
fn claude_on_another_system_keeps_the_neutral_notice() {
    let lab = lab();
    lab.claude("");
    let mut env = lab.env();
    env.jail = JailSupport::Unavailable(JailOff::NotLinux);
    let planned = plan(&env, &job(JailPolicy::Layered)).ok().unwrap();
    let text = format!("{:?}", planned.notice);
    assert!(text.contains(OTHER_OS), "{text}");
    assert!(!text.contains("use claude"), "{text}");
}

#[test]
fn linux_keeps_proc_self_exe_as_the_helper_whatever_current_exe_says() {
    let on = JailSupport::Available {
        backend: Backend::Landlock,
        abi: 6,
    };
    let gone = Err(io::Error::from(io::ErrorKind::NotFound));
    let (helper, jail) = helper_for(Os::Linux, on, gone);
    assert_eq!(helper, Path::new("/proc/self/exe"));
    assert_eq!(jail, on);
    let (helper, _) = helper_for(Os::Linux, on, Ok(PathBuf::from("/opt/lazarobox")));
    assert_eq!(helper, Path::new("/proc/self/exe"));
}

#[test]
fn elsewhere_the_helper_is_the_resolved_current_exe() {
    let on = JailSupport::Available {
        backend: Backend::Seatbelt,
        abi: 0,
    };
    let exe = PathBuf::from("/opt/homebrew/Cellar/lazarobox/1.0/bin/lazarobox");
    for os in [Os::Macos, Os::Other] {
        assert_eq!(helper_for(os, on, Ok(exe.clone())), (exe.clone(), on));
    }
}

#[test]
fn a_current_exe_that_cannot_be_resolved_makes_the_jail_unavailable_never_bare() {
    let gone = || Err(io::Error::from(io::ErrorKind::NotFound));
    let on = JailSupport::Available {
        backend: Backend::Seatbelt,
        abi: 0,
    };
    let (_, jail) = helper_for(Os::Macos, on, gone());
    assert_eq!(
        jail,
        JailSupport::Unavailable(JailOff::NoSeatbelt(SeatbeltFault::NoHelper))
    );
    // An already unavailable jail keeps its own reason.
    let off = JailSupport::Unavailable(JailOff::NoSeatbelt(SeatbeltFault::Leaked));
    assert_eq!(helper_for(Os::Macos, off, gone()).1, off);
    // And the launch for codex is a refusal, not a bare run.
    assert_eq!(launch(JailPolicy::Required, jail), Launch::Refuse);
}

const EXEMPT_NOTICE: &str = "claude runs without the jail here: claude keeps its macOS Keychain \
login, so it never runs jailed on macOS";

#[test]
fn an_exempt_claude_runs_bare_with_its_own_notice_and_no_tools_whatever_the_probe_said() {
    let lab = lab();
    lab.claude("");
    let available = JailSupport::Available {
        backend: Backend::Seatbelt,
        abi: 0,
    };
    let broken = JailSupport::Unavailable(JailOff::NoSeatbelt(SeatbeltFault::Leaked));
    for support in [available, broken] {
        let mut env = lab.env();
        env.jail = support;
        env.shim = Some(PathBuf::from("/nonexistent/shim"));
        let job = job(JailPolicy::Exempt);
        assert!(!runs_jailed(&env, &job), "{support:?}");
        let planned = plan(&env, &job).ok().unwrap();
        assert_eq!(
            planned.notice.as_deref(),
            Some(EXEMPT_NOTICE),
            "{support:?}"
        );
        assert!(!planned.wired, "{support:?}");
        assert_ne!(planned.program, env.helper, "{support:?}");
    }
}

#[test]
fn the_jail_note_of_claude_on_macos_says_it_is_not_jailed_there() {
    let mut env = lab().env();
    env.jail = JailSupport::Unavailable(JailOff::NoSeatbelt(SeatbeltFault::NotEnabled));
    let bin = Path::new("/bin/true");
    assert_eq!(
        jail_note_on(Os::Macos, &env, Cli::Claude, bin).as_deref(),
        Some("not jailed on macOS")
    );
    // codex and gemini keep their own note; claude elsewhere keeps its own.
    assert_eq!(
        jail_note_on(Os::Macos, &env, Cli::Codex, bin).as_deref(),
        Some("jail unavailable · use claude")
    );
    assert_eq!(
        jail_note_on(Os::Linux, &env, Cli::Claude, bin).as_deref(),
        Some("jail unavailable")
    );
}

#[test]
fn the_jail_note_of_another_system_points_codex_and_gemini_to_claude_only() {
    let mut env = lab().env();
    env.jail = JailSupport::Unavailable(JailOff::NotLinux);
    let bin = Path::new("/bin/true");
    assert_eq!(
        jail_note(&env, Cli::Codex, bin).as_deref(),
        Some("needs Linux · use claude")
    );
    assert_eq!(
        jail_note(&env, Cli::Gemini, bin).as_deref(),
        Some("needs Linux · use claude")
    );
    assert_eq!(
        jail_note(&env, Cli::Claude, bin).as_deref(),
        Some("needs Linux")
    );
}
