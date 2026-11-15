//! Test support of the local-model evaluation (openspec `local-multistep`).
//! It lives in the chat module because it reaches the private Ollama loop.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{FutureExt, future::BoxFuture};
use serde_json::{Map, Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

use super::{
    PaneId, TURN_CHANNEL,
    ollama::{self, Profile, tool_turn_with},
    ollama_tests::{Step, end_line, lines},
};
use crate::{
    core::ai::{
        chat::{
            adapters::ToolExchange,
            approval::ApprovalAsk,
            engine::{
                Answer, CallId, ChatMessage, ChatRequest, Decision, Role, StreamEvent, Tools,
            },
            prompts,
        },
        config::{Profile as Persona, Task},
        eval::{Category, Scenario, refused, scenarios, scenarios::ALL, score, table::Run},
        orchestra::Toolset,
        tools::{
            Call, Locality, ToolError,
            mcp::refusal_text,
            paths::REFUSED,
            registry::{Family, FamilySet},
            stage::StageKey,
        },
    },
    runtime::{
        loopback::Timeouts,
        mcp::{Dispatch, Outcome},
        tools::{Approver, Limits, ToolHost, TurnScope, host::NOT_APPROVED},
    },
};

/// The exchanges of the LAST request body of a turn: each assistant
/// `tool_calls[i]` paired with the `tool` messages that follow, in order. A
/// call written as text is not in `tool_calls`, so it never counts.
pub(super) fn exchanges_from_body(body: &Value) -> Vec<ToolExchange> {
    let mut list = Vec::new();
    let mut calls: Vec<(String, Value)> = Vec::new();
    for message in body["messages"].as_array().map_or(&[][..], Vec::as_slice) {
        match message["role"].as_str() {
            Some("assistant") => {
                let entries = message["tool_calls"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice);
                calls = (entries.iter().rev())
                    .map(|c| &c["function"])
                    .map(|f| {
                        (
                            f["name"].as_str().unwrap_or_default().to_owned(),
                            f["arguments"].clone(),
                        )
                    })
                    .collect();
            }
            Some("tool") => {
                if let Some((name, arguments)) = calls.pop() {
                    let result = message["content"].as_str().unwrap_or_default().to_owned();
                    list.push(ToolExchange {
                        name,
                        id: None,
                        index: None,
                        arguments,
                        result,
                    });
                }
            }
            _ => {}
        }
    }
    list
}

/// A `fail` value for [`Rig::new`]: the canned `run_command` exits 1 (a
/// result, not an error), the way a failing pytest does.
pub(super) const EXIT_NONZERO: &str = "run_command exits 1";

/// One call that reached the dispatch, in order, and whether it failed.
pub(super) struct Done {
    pub(super) name: String,
    pub(super) error: bool,
}

/// The parse errors: their texts are answered by the loop, so the call never
/// reached the dispatch and has no record.
const PARSE_ERRORS: [ToolError; 13] = [
    ToolError::UnknownTool,
    ToolError::NotAnObject,
    ToolError::MissingArgument,
    ToolError::WrongType,
    ToolError::ExtraArgument,
    ToolError::TooLong,
    ToolError::PaneRange,
    ToolError::ScrollbackRange,
    ToolError::DraftKind,
    ToolError::DraftEmpty,
    ToolError::DraftTitleLong,
    ToolError::DraftTitleLines,
    ToolError::DraftBodyLong,
];

fn never_ran(result: &str) -> bool {
    result == ollama::TOO_MANY || PARSE_ERRORS.iter().any(|e| refusal_text(*e) == result)
}

/// Makes every exchange whose real outcome was an error read as refused: the
/// loop keeps the result text only, so a failure without a refusal prefix
/// (`nothing is staged`) would score as a completed step. The records pair
/// with the exchanges by order and tool name, never by text (a text may be
/// cut or equal to another); a call the parse refused has no record.
pub(super) fn mark_failed(exchanges: &mut [ToolExchange], calls: &[Done]) {
    let mut records = calls.iter().peekable();
    for exchange in exchanges {
        if never_ran(&exchange.result) {
            continue;
        }
        if let Some(done) = records.next_if(|d| d.name == exchange.name)
            && done.error
        {
            exchange.result = format!("{}: {}", ollama::FAILED, exchange.result);
        }
    }
}

/// What the harness saw while a run went on.
#[derive(Default)]
struct Seen {
    unexpected: bool,
    calls: Vec<Done>,
}

/// Answers the approvals of a run by its scenario, and keeps what the
/// dispatch decorator recorded. Approves the staged batch, the exec and the
/// commit (with its `git_add`) the scenario expects, and nothing else.
pub(super) struct Judge {
    scenario: &'static Scenario,
    seen: Mutex<Seen>,
}

const NOT_IN_SCENARIO: &str = "not part of the scenario";

impl Judge {
    pub(super) fn new(scenario: &'static Scenario) -> Arc<Self> {
        Arc::new(Self {
            scenario,
            seen: Mutex::default(),
        })
    }

    /// Whether the scenario has a step with `tool`; a tool it has not is
    /// recorded as unexpected.
    fn allows(&self, tool: &str) -> bool {
        let known = self.scenario.steps.iter().any(|step| step.tool == tool);
        if !known {
            self.seen.lock().unwrap().unexpected = true;
        }
        known
    }

    /// A call reached the dispatch: its slot, filled by [`Judge::ended`].
    fn began(&self, name: &str) -> usize {
        let mut seen = self.seen.lock().unwrap();
        seen.calls.push(Done {
            name: name.to_owned(),
            error: false,
        });
        seen.calls.len() - 1
    }

    fn ended(&self, slot: usize, error: bool) {
        self.seen.lock().unwrap().calls[slot].error = error;
    }

    /// Scores the run from the last request body and its final answer.
    pub(super) fn run(&self, body: &Value, answer: &str, time: Duration) -> Run {
        let seen = self.seen.lock().unwrap();
        let mut exchanges = exchanges_from_body(body);
        mark_failed(&mut exchanges, &seen.calls);
        let mut run = Run::new(score(self.scenario, &exchanges, answer), time);
        run.unexpected = seen.unexpected;
        run
    }
}

impl Approver for Judge {
    fn ask(&self, _: CallId, ask: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        let tool = match ask {
            ApprovalAsk::Exec { .. } => "run_command",
            ApprovalAsk::Batch(_) => "propose_batch",
            ApprovalAsk::Commit(_) | ApprovalAsk::GitAdd { .. } => "git_commit",
            _ => "",
        };
        let decision = match self.allows(tool) {
            true => Decision::Sent,
            false => Decision::Denied(NOT_IN_SCENARIO),
        };
        async move { decision.into() }.boxed()
    }
}

/// The dispatch of a run. Files and the batch always go to the real host. In
/// replay (`live` false) commands, git and the orchestration tools get fixed
/// answers, so no sandbox, python3 or git is needed; live, only the
/// orchestration tools do, and commands and git run for real in the lab (still
/// approved through the [`Judge`]). Every outcome is recorded for
/// [`mark_failed`] in both modes.
pub(super) struct Canned {
    real: Arc<dyn Dispatch>,
    judge: Arc<Judge>,
    live: bool,
    /// A tool whose canned answer is an error without a refusal prefix.
    pub(super) fail: Option<&'static str>,
}

impl Dispatch for Canned {
    fn call(&self, call: Call) -> BoxFuture<'static, Outcome> {
        let judge = self.judge.clone();
        let slot = judge.began(call.name());
        // The tool the scenario must expect, and the fixed text.
        let fixed = match &call {
            Call::Orchestra(_) => Some(("", "recorded")),
            _ if self.live => None,
            Call::RunCommand { .. } => Some(("run_command", "exit code 0 after 0.4s\n1 passed")),
            Call::GitAdd { .. } => Some(("git_commit", "staged 2 paths")),
            Call::GitCommit { .. } => Some(("git_commit", "committed abc1234")),
            git if git.uses_git() => Some(("", "nothing to report")),
            _ => None,
        };
        let Some((tool, text)) = fixed else {
            let real = self.real.call(call);
            return async move {
                let outcome = real.await;
                judge.ended(slot, outcome.is_error);
                outcome
            }
            .boxed();
        };
        let outcome = if !tool.is_empty() && !judge.allows(tool) {
            Outcome {
                text: format!("denied: {NOT_IN_SCENARIO}"),
                is_error: true,
            }
        } else if self.fail == Some(EXIT_NONZERO) && call.name() == "run_command" {
            Outcome {
                text: "exit code 1 after 0.4s\n1 failed".to_owned(),
                is_error: false,
            }
        } else if self.fail == Some(call.name()) {
            Outcome {
                text: "nothing is staged".to_owned(),
                is_error: true,
            }
        } else {
            Outcome {
                text: text.to_owned(),
                is_error: false,
            }
        };
        judge.ended(slot, outcome.is_error);
        async move { outcome }.boxed()
    }
}

/// Where the run folders go: `$LAZAROBOX_EVAL_DIR`, else `target/olive` of the
/// crate. Never `/tmp` or the system temp folder: the jail refuses them.
pub(super) fn pick_root(
    given: Option<&str>,
    manifest: &str,
    temp: &Path,
) -> Result<PathBuf, String> {
    let root = given.map_or_else(|| Path::new(manifest).join("target/olive"), PathBuf::from);
    match [Path::new("/tmp"), temp]
        .iter()
        .find(|bad| root.starts_with(bad))
    {
        Some(bad) => Err(format!(
            "the eval folder is under {}: set LAZAROBOX_EVAL_DIR elsewhere",
            bad.display()
        )),
        None => Ok(root),
    }
}

pub(super) fn throwaway_root() -> PathBuf {
    let given = std::env::var("LAZAROBOX_EVAL_DIR").ok();
    pick_root(
        given.as_deref(),
        env!("CARGO_MANIFEST_DIR"),
        &std::env::temp_dir(),
    )
    .unwrap_or_else(|why| panic!("{why}"))
}

/// One run's folder: `home/repo` seeded with the scenario's generated files.
/// Removed when dropped, also when the run panics.
pub(super) struct Folder {
    _dir: tempfile::TempDir,
    pub(super) home: PathBuf,
    pub(super) repo: PathBuf,
    /// An empty file standing in for the global git config.
    pub(super) global: PathBuf,
}

impl Folder {
    pub(super) fn new(root: &Path, scenario: &Scenario) -> Self {
        std::fs::create_dir_all(root).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("r")
            .tempdir_in(root)
            .unwrap();
        let base = dir.path().canonicalize().unwrap();
        let (home, global) = (base.join("home"), base.join("gitconfig"));
        let repo = home.join("repo");
        for (path, text) in scenario.seed {
            let at = repo.join(path);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(at, text).unwrap();
        }
        std::fs::write(&global, "").unwrap();
        Self {
            _dir: dir,
            home,
            repo,
            global,
        }
    }

    /// Runs git in the repo against the empty global config; the author is
    /// set in the repo's own config.
    pub(super) fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.repo)
            .env("GIT_CONFIG_GLOBAL", &self.global)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &self.home)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// `git init`, the author, and one commit of the seed.
    pub(super) fn init_git(&self) {
        self.git(&["init", "-q"]);
        self.git(&["config", "user.name", "eval"]);
        self.git(&["config", "user.email", "eval@example.invalid"]);
        self.git(&["config", "commit.gpgsign", "false"]);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "eval seed"]);
    }
}

/// The families a run of `scenario` offers: the coding set of a chat, or the
/// coordinator's.
pub(super) fn families(scenario: &Scenario) -> FamilySet {
    match scenario.category {
        Category::Coordinate => Toolset::Orchestrator.families(),
        _ => [Family::Write, Family::Exec, Family::Git, Family::GitWrite]
            .into_iter()
            .fold(FamilySet::LEGACY, FamilySet::with),
    }
}

/// How a run's commands and git are answered.
#[derive(PartialEq, Eq)]
pub(super) enum Mode {
    /// Fixed texts: CI needs no sandbox, python3 or git.
    Replay,
    /// For real, in the lab, through the sandbox helper (the built binary).
    /// Linux only, like the live harness that uses it.
    #[cfg(target_os = "linux")]
    Live,
    /// For real, through a script that does not sandbox: offline tests.
    StandIn(PathBuf),
}

/// The `lazarobox` binary that serves `__jail-exec`: the one `cargo build`
/// leaves next to the test binaries.
#[cfg(target_os = "linux")]
pub(super) fn live_helper() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let helper = exe
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("lazarobox");
    assert!(
        helper.is_file(),
        "the sandbox helper {} is missing: run `cargo build` first",
        helper.display()
    );
    helper
}

/// Everything one run needs: the request, the dispatch and the judge.
pub(super) struct Rig {
    pub(super) request: ChatRequest,
    pub(super) judge: Arc<Judge>,
    dispatch: Arc<dyn Dispatch>,
    tx: mpsc::Sender<StreamEvent>,
    rx: mpsc::Receiver<StreamEvent>,
}

impl Rig {
    pub(super) fn new(
        scenario: &'static Scenario,
        folder: &Folder,
        fail: Option<&'static str>,
        mode: Mode,
    ) -> Self {
        let home = folder.home.clone().into();
        let host = match &mode {
            Mode::Replay => ToolHost::new(Some(home), None, None, Limits::default()),
            #[cfg(target_os = "linux")]
            Mode::Live => ToolHost::with_helper(home, live_helper(), false),
            Mode::StandIn(helper) => ToolHost::with_helper(home, helper.clone(), true),
        };
        let (tx, rx) = mpsc::channel(TURN_CHANNEL);
        let judge = Judge::new(scenario);
        let mut scope = TurnScope::new(tx.clone(), Locality::Local, folder.repo.clone());
        scope.approver = judge.clone();
        scope.families = families(scenario);
        scope.stage_key = Some(StageKey::Pane(PaneId::for_test(1)));
        let dispatch = Canned {
            real: host.dispatch(scope),
            judge: judge.clone(),
            live: mode != Mode::Replay,
            fail,
        };
        let task = match scenario.category {
            Category::Coordinate => Task::Orchestrate,
            _ => Task::Chat,
        };
        let mut request = super::tests::request(prompts::builtin(Persona::Programmer, task));
        request.families = families(scenario);
        request.messages = vec![ChatMessage {
            role: Role::User,
            content: scenario.request.into(),
        }];
        request.tools = Tools::On {
            origin: Some(folder.repo.clone()),
            external: Default::default(),
            mcp: None,
        };
        Self {
            request,
            judge,
            dispatch: Arc::new(dispatch),
            tx,
            rx,
        }
    }

    pub(super) fn dispatch(&self) -> Arc<dyn Dispatch> {
        self.dispatch.clone()
    }

    /// Runs the turn against the server on `port`; returns the final answer:
    /// the text after the last tool call.
    pub(super) async fn drive(self, port: u16, timeouts: &Timeouts) -> String {
        let Self {
            request,
            dispatch,
            tx,
            mut rx,
            ..
        } = self;
        let run = async {
            tool_turn_with(Profile::Ollama, &request, port, dispatch, timeouts, &tx).await;
            drop(tx);
        };
        let drain = async {
            let mut answer = String::new();
            while let Some(event) = rx.recv().await {
                match event {
                    StreamEvent::ToolCall { .. } => answer.clear(),
                    StreamEvent::Delta(text) => answer.push_str(&text),
                    StreamEvent::Error(error) => {
                        eprintln!("the turn ended with an error: {error:?}")
                    }
                    _ => {}
                }
            }
            answer
        };
        tokio::join!(run, drain).1
    }
}

/// A scripted transcript: what the fake server answers, round by round.
pub(super) struct Fixture {
    scenario: String,
    replies: Vec<Reply>,
}

struct Reply {
    text: String,
    calls: Vec<(String, Value)>,
}

/// The object `value` with no key outside `keys`.
fn object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, String> {
    let map = value.as_object().ok_or("an object was expected")?;
    match map.keys().find(|key| !keys.contains(&key.as_str())) {
        Some(key) => Err(format!("a field outside the format: {key}")),
        None => Ok(map),
    }
}

fn text_of(map: &Map<String, Value>, key: &str) -> Result<String, String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(format!("{key} must be a string"))
}

impl Fixture {
    /// Reads `{ scenario, replies: [{ text, calls: [{ name, arguments }] }] }`;
    /// any other field is an error.
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let root: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let root = object(&root, &["scenario", "replies"])?;
        let mut replies = Vec::new();
        for item in root
            .get("replies")
            .and_then(Value::as_array)
            .ok_or("replies must be a list")?
        {
            let item = object(item, &["text", "calls"])?;
            let mut calls = Vec::new();
            for call in item
                .get("calls")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice)
            {
                let call = object(call, &["name", "arguments"])?;
                calls.push((
                    text_of(call, "name")?,
                    call.get("arguments")
                        .cloned()
                        .ok_or("arguments are missing")?,
                ));
            }
            replies.push(Reply {
                text: text_of(item, "text")?,
                calls,
            });
        }
        Ok(Self {
            scenario: text_of(root, "scenario")?,
            replies,
        })
    }

    pub(super) fn scenario(&self) -> &'static Scenario {
        ALL.into_iter()
            .find(|s| s.id == self.scenario)
            .expect("a known scenario")
    }

    pub(super) fn steps(&self) -> Vec<Step> {
        let step = |reply: &Reply| {
            let mut message = json!({"content": reply.text});
            if !reply.calls.is_empty() {
                let calls = reply.calls.iter().map(
                    |(name, arguments)| json!({"function": {"name": name, "arguments": arguments}}),
                );
                message["tool_calls"] = calls.collect();
            }
            lines(&[json!({"message": message, "done": false}), end_line()])
        };
        self.replies.iter().map(step).collect()
    }
}

/// Whether `name` is in `text` as a whole word.
fn has_word(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        let edge = |c: Option<char>| !c.is_some_and(char::is_alphanumeric);
        edge(before) && edge(after)
    })
}

/// The kinds of leak a curated fixture must not hold: never the matched value,
/// so a shared log shows no secret. `home` and `user` are the running values.
pub(super) fn leaks(text: &str, home: &str, user: &str) -> Vec<&'static str> {
    let words = || {
        let cut = |c: char| c.is_whitespace() || "\"'()[]{},;=".contains(c);
        text.split(cut).filter(|w| !w.is_empty())
    };
    let long_run = |word: &str| {
        let mut run = 0;
        word.chars().any(|c| {
            run = if c.is_ascii_alphanumeric() || "+/=".contains(c) {
                run + 1
            } else {
                0
            };
            run >= 32
        })
    };
    let windows = |word: &str| {
        let mut chars = word.chars();
        matches!((chars.next(), chars.next(), chars.next()), (Some(d), Some(':'), Some('\\')) if d.is_ascii_alphabetic())
    };
    let secret = |w: &str| w.contains("sk-") || w.contains("ghp_") || long_run(w);
    let checks: [(&'static str, bool); 9] = [
        ("size", text.len() > 16 * 1024),
        ("field", Fixture::parse(text).is_err()),
        (
            "absolute path",
            words().any(|w| w.starts_with('/') && w[1..].contains('/')),
        ),
        ("home path", words().any(|w| w.starts_with("~/"))),
        ("windows path", words().any(windows)),
        ("home", home.len() > 1 && text.contains(home)),
        ("user", user.len() > 4 && has_word(text, user)),
        (
            "email",
            words()
                .any(|w| w.contains('@') && w.rsplit('@').next().is_some_and(|d| d.contains('.'))),
        ),
        ("token", words().any(secret)),
    ];
    checks
        .into_iter()
        .filter(|(_, hit)| *hit)
        .map(|(kind, _)| kind)
        .collect()
}

/// A run in the fixture format, raw material for a hand-curated fixture: the
/// assistant side of the last request body plus the final answer, with every
/// `roots` path replaced by `.`.
pub(super) fn dump(scenario: &Scenario, body: &Value, answer: &str, roots: &[&Path]) -> String {
    let messages = body["messages"].as_array().map_or(&[][..], Vec::as_slice);
    let mut replies: Vec<Value> = (messages.iter())
        .filter(|m| m["role"] == "assistant")
        .map(|m| {
            let calls = m["tool_calls"].as_array().map_or(&[][..], Vec::as_slice);
            let calls: Vec<Value> = (calls.iter())
                .map(|c| json!({"name": c["function"]["name"], "arguments": c["function"]["arguments"]}))
                .collect();
            json!({"text": m["content"], "calls": calls})
        })
        .collect();
    replies.push(json!({"text": answer, "calls": []}));
    let mut text =
        serde_json::to_string_pretty(&json!({"scenario": scenario.id, "replies": replies}))
            .unwrap_or_default();
    for root in roots {
        text = text.replace(&*root.to_string_lossy(), ".");
    }
    text + "\n"
}

/// The result of running one trivial command (`program args`, allowed by the
/// run_command table) through `dispatch`, the same path every `run_command` of
/// a run takes. The live test passes `pytest --version`, so a pytest missing
/// from the PATH shows too. `Err` names the cause, so a
/// sandbox that cannot run anything (no Landlock on the kernel, a helper that
/// fails) stops the live test before any model call instead of scoring every
/// command as a failed step.
pub(super) async fn sandbox_probe(
    dispatch: &dyn Dispatch,
    program: &str,
    args: &[&str],
) -> Result<(), String> {
    let call = Call::RunCommand {
        args: json!({"program": program, "args": args}),
    };
    let out = dispatch.call(call).await;
    // A command that ran and failed is not an error outcome: it reads
    // `exit code N after ...`, which is how a helper that dies shows.
    if out.is_error || !out.text.starts_with("exit code 0 ") {
        return Err(format!(
            "the sandbox helper cannot run a command (is Landlock available on this kernel?): {}",
            out.text
        ));
    }
    Ok(())
}

/// A warning when `helper` is older than `exe` (the test binary): `cargo test
/// --lib` does not rebuild `target/debug/lazarobox`, so the live run would use
/// a stale sandbox helper. `None` when it is fresh or its age is unknown.
pub(super) fn helper_age_warning(helper: &Path, exe: &Path) -> Option<String> {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    (modified(helper)? < modified(exe)?).then(|| {
        format!(
            "WARNING: {} is older than the test binary: run `cargo build` or the sandbox helper may be stale",
            helper.display()
        )
    })
}

/// The live pre-check of the sandbox: a throwaway rig in `Mode::Live` runs one
/// command. Panics naming the cause. Linux only, like `live_helper`.
#[cfg(target_os = "linux")]
pub(super) async fn live_sandbox_check() {
    let scenario = &scenarios::WRITE_AND_TEST;
    let folder = Folder::new(&throwaway_root(), scenario);
    let rig = Rig::new(scenario, &folder, None, Mode::Live);
    if let Err(why) = sandbox_probe(&*rig.dispatch(), "pytest", &["--version"]).await {
        panic!("{why}");
    }
}

/// Writes one `_DUMP` run as `<scenario>-<model>-<n>.json` in `dir` (created
/// when missing): `n` is the first free number from 1, claimed with
/// `create_new`, so a run never overwrites another, even after files were
/// deleted from the folder or another process writes there.
pub(super) fn write_dump(
    dir: &Path,
    scenario: &str,
    model: &str,
    text: &str,
) -> std::io::Result<PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    for n in 1.. {
        let path = dir.join(format!("{scenario}-{model}-{n:03}.json"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(text.as_bytes())?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!("the counter does not end")
}

/// A proxy that records the body of every request and forwards it to Ollama,
/// so the exchanges of a run can be read from its last request.
pub(super) async fn proxy(upstream: &'static str) -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = bodies.clone();
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            tokio::spawn(forward(client, upstream, seen.clone()));
        }
    });
    (port, bodies)
}

async fn forward(mut client: TcpStream, upstream: &str, bodies: Arc<Mutex<Vec<Value>>>) {
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    let (end, length) = loop {
        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&raw[..end]).to_lowercase();
            let length = (head.lines())
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            break (end + 4, length);
        }
        match client.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
    };
    while raw.len() < end + length {
        match client.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
    }
    if let Ok(body) = serde_json::from_slice(&raw[end..end + length]) {
        bodies.lock().unwrap().push(body);
    }
    // The upstream closes after its reply only when asked to.
    let head = String::from_utf8_lossy(&raw[..end]);
    let kept = head
        .lines()
        .filter(|l| !l.is_empty() && !l.to_lowercase().starts_with("connection:"));
    let mut request = kept.collect::<Vec<_>>().join("\r\n").into_bytes();
    request.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
    request.extend_from_slice(&raw[end..end + length]);
    let Ok(mut server) = TcpStream::connect(upstream).await else {
        return;
    };
    if server.write_all(&request).await.is_ok() {
        let _ = tokio::io::copy(&mut server, &mut client).await;
    }
    let _ = client.shutdown().await;
}

/// Every text the host or the loop gives a call that did not run must be one
/// `eval::refused` knows, or a refused step would score as completed.
#[test]
fn the_refusal_prefixes_cover_the_real_constants() {
    for error in PARSE_ERRORS {
        assert!(refused(refusal_text(error)), "{error:?}");
    }
    for text in [
        ollama::FAILED,
        ollama::DROPPED,
        ollama::TOO_MANY,
        REFUSED,
        &format!("denied: {NOT_APPROVED}"),
    ] {
        assert!(refused(text), "{text}");
    }
}

fn exchange(name: &str, result: &str) -> ToolExchange {
    ToolExchange {
        name: name.to_owned(),
        id: None,
        index: None,
        arguments: json!({}),
        result: result.to_owned(),
    }
}

// Spec (Replay guard, D4): the exchanges are read from the last request body.
#[test]
fn each_call_is_paired_with_the_tool_messages_that_follow_in_order() {
    let body = json!({"messages": [
        {"role": "system", "content": "s"},
        {"role": "user", "content": "u"},
        {"role": "assistant", "content": "", "tool_calls": [
            {"function": {"name": "read_file", "arguments": {"path": "a"}}},
            {"function": {"name": "glob", "arguments": {"pattern": "*"}}}]},
        {"role": "tool", "content": "A", "tool_name": "read_file"},
        {"role": "tool", "content": "B", "tool_name": "glob"},
        {"role": "assistant", "content": "going on"},
        {"role": "user", "content": "go on"},
        {"role": "assistant", "content": "", "tool_calls": [
            {"function": {"name": "git_commit", "arguments": {"message": "m"}}}]},
        {"role": "tool", "content": "C", "tool_name": "git_commit"},
    ]});
    let list = exchanges_from_body(&body);
    let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["read_file", "glob", "git_commit"]);
    let results: Vec<_> = list.iter().map(|e| e.result.as_str()).collect();
    assert_eq!(results, ["A", "B", "C"]);
    assert_eq!(list[0].arguments, json!({"path": "a"}));
    assert_eq!(list[2].arguments, json!({"message": "m"}));
}

#[test]
fn a_call_written_as_text_never_appears() {
    let text = r#"{"name": "git_commit", "arguments": {"message": "m"}}"#;
    let body = json!({"messages": [
        {"role": "user", "content": "u"},
        {"role": "assistant", "content": text}]});
    assert!(exchanges_from_body(&body).is_empty());
    assert!(exchanges_from_body(&json!({})).is_empty());
}

// Task 3.0: the loop keeps the result text only, so a host failure without a
// refusal prefix is marked from the outcome the dispatch decorator recorded.
#[test]
fn a_recorded_failure_makes_its_exchange_refused() {
    let mut list = vec![
        exchange("git_commit", "nothing is staged"),
        exchange("git_commit", "committed abc"),
        exchange("git_commit", "nothing is staged"),
    ];
    mark_failed(
        &mut list,
        &[done("git_commit", true), done("git_commit", false)],
    );
    assert!(refused(&list[0].result), "{}", list[0].result);
    assert!(list[0].result.ends_with("nothing is staged"));
    assert!(!refused(&list[1].result));
    assert!(!refused(&list[2].result), "one record marks one exchange");
}

// Spec (Throwaway repository outside /tmp, Not under /tmp).
#[test]
fn the_throwaway_root_is_never_under_tmp() {
    let tmp = Path::new("/tmp");
    let root = pick_root(None, "/home/u/lazarobox", tmp).unwrap();
    assert_eq!(root, PathBuf::from("/home/u/lazarobox/target/olive"));
    assert!(pick_root(Some("/tmp/olive"), "/home/u/x", tmp).is_err());
    assert!(pick_root(None, "/tmp/worktree", tmp).is_err());
    let scratch = Path::new("/var/scratch");
    assert!(pick_root(Some("/var/scratch/olive"), "/home/u/x", scratch).is_err());
    let given = pick_root(Some("/home/u/olive"), "/home/u/x", tmp).unwrap();
    assert_eq!(given, PathBuf::from("/home/u/olive"));
}

// Spec (Removed on failure).
#[test]
fn the_run_folder_is_removed_when_the_run_panics() {
    let root = throwaway_root();
    let seen = std::sync::Mutex::new(PathBuf::new());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let folder = Folder::new(&root, &scenarios::WRITE_AND_TEST);
        *seen.lock().unwrap() = folder.repo.clone();
        assert!(folder.repo.join("README.md").is_file());
        panic!("the run failed");
    }));
    assert!(outcome.is_err());
    let repo = seen.lock().unwrap().clone();
    assert!(repo.starts_with(&root) && !repo.exists(), "{repo:?}");
}

// Spec (Global config untouched): the author lives in the repo's own config.
#[test]
fn seeding_a_repo_leaves_the_global_git_config_alone() {
    let folder = Folder::new(&throwaway_root(), &scenarios::WRITE_AND_TEST);
    folder.init_git();
    assert_eq!(std::fs::read(&folder.global).unwrap(), b"");
    let config = std::fs::read_to_string(folder.repo.join(".git/config")).unwrap();
    assert!(config.contains("eval@example.invalid"), "{config}");
    let log = folder.git(&["log", "--format=%an %s"]);
    assert_eq!(log.trim(), "eval eval seed");
}

// Spec (Ignored by default): no workflow runs ignored tests, and every live
// test is marked `#[ignore]`.
#[test]
fn live_tests_are_ignored_and_no_workflow_runs_ignored_tests() {
    let workflows = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    for entry in std::fs::read_dir(workflows).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(!text.contains("--ignored") && !text.contains("--include-ignored"));
    }
    let live = include_str!("ollama_live.rs");
    let lines: Vec<&str> = live.lines().map(str::trim).collect();
    let marks = lines
        .iter()
        .filter(|l| l.starts_with("#[tokio::test") || **l == "#[test]");
    assert!(marks.clone().count() > 0, "the live file has no test");
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("#[tokio::test") || *line == "#[test]" {
            assert!(lines[i + 1].starts_with("#[ignore"), "{line} at {i}");
        }
    }
}

// Task 6.6: a dump is a fixture with no path of the machine in it.
#[test]
fn a_dump_is_a_fixture_with_the_repo_root_replaced() {
    let home = Path::new("/home/u/target/olive/r1/home");
    let repo = home.join("repo");
    let body = json!({"messages": [
        {"role": "user", "content": "u"},
        {"role": "assistant", "content": "", "tool_calls": [
            {"function": {"name": "read_file", "arguments": {"path": "/home/u/target/olive/r1/home/repo/a.py"}}}]},
        {"role": "tool", "content": "x", "tool_name": "read_file"}]});
    let text = dump(&scenarios::READ_ONLY, &body, "It does x.", &[&repo, home]);
    assert!(!text.contains("/home/u"), "{text}");
    let fixture = Fixture::parse(&text).unwrap();
    assert_eq!(fixture.scenario().id, "read-only");
    assert_eq!(fixture.steps().len(), 2);
    assert!(text.contains("\"path\": \"./a.py\""), "{text}");
}

fn done(name: &str, error: bool) -> Done {
    Done {
        name: name.to_owned(),
        error,
    }
}

// Verify W2: records pair with exchanges by order, not by text.
#[test]
fn only_the_call_that_failed_is_marked_when_two_results_read_alike() {
    let mut list = vec![
        exchange("run_command", "exit 1"),
        exchange("run_command", "exit 1"),
    ];
    mark_failed(
        &mut list,
        &[done("run_command", false), done("run_command", true)],
    );
    assert!(!refused(&list[0].result));
    assert!(refused(&list[1].result));
}

#[test]
fn a_failure_is_marked_even_when_its_text_was_cut_or_replaced() {
    let mut list = vec![exchange("git_commit", "result dropped")];
    mark_failed(&mut list, &[done("git_commit", true)]);
    assert!(refused(&list[0].result));
    let mut cut = vec![exchange("git_commit", "nothing is st")];
    mark_failed(&mut cut, &[done("git_commit", true)]);
    assert!(refused(&cut[0].result));
}

#[test]
fn a_call_the_parse_refused_never_reached_the_dispatch() {
    let mut list = vec![
        exchange("write_file", refusal_text(ToolError::MissingArgument)),
        exchange("write_file", "staged a.py"),
        exchange("write_file", "path not available"),
    ];
    mark_failed(
        &mut list,
        &[done("write_file", false), done("write_file", true)],
    );
    assert_eq!(list[1].result, "staged a.py");
    assert!(refused(&list[2].result));
    assert_eq!(list[0].result, refusal_text(ToolError::MissingArgument));
}

// Verify W3: the user name is a whole word, and a short one is not checked.
#[test]
fn the_user_name_is_matched_as_a_word_and_short_names_are_skipped() {
    let body =
        |text: &str| format!(r#"{{"scenario": "read-only", "replies": [{{"text": "{text}"}}]}}"#);
    let prose = body("the user asked for a file");
    assert!(leaks(&prose, "/home/zzzz", "user").is_empty());
    assert!(leaks(&body("hello someone."), "/home/zzzz", "someone").contains(&"user"));
    assert!(leaks(&body("someones file"), "/home/zzzz", "someone").is_empty());
    assert!(leaks(&body("hello root"), "/home/zzzz", "root").is_empty());
}

// Hardening: the `_DUMP` name never collides, even with files deleted.
#[test]
fn a_dump_never_overwrites_another_run() {
    let dir = throwaway_root().join(format!("dump-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let first = write_dump(&dir, "write-and-test", "qwen3-14b", "one").unwrap();
    let second = write_dump(&dir, "write-and-test", "qwen3-14b", "two").unwrap();
    let third = write_dump(&dir, "write-and-test", "qwen3-14b", "three").unwrap();
    std::fs::remove_file(&first).unwrap();
    let fourth = write_dump(&dir, "write-and-test", "qwen3-14b", "four").unwrap();
    assert_eq!(std::fs::read_to_string(&second).unwrap(), "two");
    assert_eq!(std::fs::read_to_string(&third).unwrap(), "three");
    assert_eq!(std::fs::read_to_string(&fourth).unwrap(), "four");
    assert_ne!(fourth, second);
    assert_ne!(fourth, third);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Hardening: every `ToolError` is a parse error (the loop answers its text
/// and the call never reaches the dispatch), so `PARSE_ERRORS` must list them
/// all, or `mark_failed` would pair the records with the wrong exchanges. The
/// match below has no wildcard: a new variant does not compile until it gets a
/// slot here, and the test then fails until `PARSE_ERRORS` holds it there.
#[test]
fn parse_errors_list_every_tool_error_variant() {
    const SLOTS: usize = 13;
    let slot = |e: ToolError| match e {
        ToolError::UnknownTool => 0,
        ToolError::NotAnObject => 1,
        ToolError::MissingArgument => 2,
        ToolError::WrongType => 3,
        ToolError::ExtraArgument => 4,
        ToolError::TooLong => 5,
        ToolError::PaneRange => 6,
        ToolError::ScrollbackRange => 7,
        ToolError::DraftKind => 8,
        ToolError::DraftEmpty => 9,
        ToolError::DraftTitleLong => 10,
        ToolError::DraftTitleLines => 11,
        ToolError::DraftBodyLong => 12,
    };
    assert_eq!(PARSE_ERRORS.len(), SLOTS, "PARSE_ERRORS misses a variant");
    for (at, error) in PARSE_ERRORS.into_iter().enumerate() {
        assert_eq!(slot(error), at, "{error:?} is not where the match says");
    }
}
