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
use tokio::sync::mpsc;

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

/// Makes every exchange whose real outcome was an error read as refused: the
/// loop keeps the result text only, so a failure without a refusal prefix
/// (`nothing is staged`) would score as a completed step. One record marks one
/// exchange.
pub(super) fn mark_failed(exchanges: &mut [ToolExchange], errors: &[String]) {
    let mut errors = errors.to_vec();
    for exchange in exchanges {
        if let Some(at) = errors.iter().position(|text| *text == exchange.result) {
            errors.remove(at);
            exchange.result = format!("{}: {}", ollama::FAILED, exchange.result);
        }
    }
}

/// What the harness saw while a run went on.
#[derive(Default)]
struct Seen {
    unexpected: bool,
    errors: Vec<String>,
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

    fn failed(&self, text: &str) {
        self.seen.lock().unwrap().errors.push(text.to_owned());
    }

    /// Scores the run from the last request body and its final answer.
    pub(super) fn run(&self, body: &Value, answer: &str, time: Duration) -> Run {
        let seen = self.seen.lock().unwrap();
        let mut exchanges = exchanges_from_body(body);
        mark_failed(&mut exchanges, &seen.errors);
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

/// The dispatch of a run: files and the batch go to the real host; commands,
/// git and the orchestration tools get fixed answers, so no sandbox, python3
/// or git is needed. Records every error outcome for [`mark_failed`].
pub(super) struct Canned {
    real: Arc<dyn Dispatch>,
    judge: Arc<Judge>,
    /// A tool whose canned answer is an error without a refusal prefix.
    pub(super) fail: Option<&'static str>,
}

impl Dispatch for Canned {
    fn call(&self, call: Call) -> BoxFuture<'static, Outcome> {
        let judge = self.judge.clone();
        // The tool the scenario must expect, and the fixed text.
        let fixed = match &call {
            Call::RunCommand { .. } => Some(("run_command", "exit code 0: 1 passed")),
            Call::GitAdd { .. } => Some(("git_commit", "staged 2 paths")),
            Call::GitCommit { .. } => Some(("git_commit", "committed abc1234")),
            git if git.uses_git() => Some(("", "nothing to report")),
            Call::Orchestra(_) => Some(("", "recorded")),
            _ => None,
        };
        let Some((tool, text)) = fixed else {
            let real = self.real.call(call);
            return async move {
                let outcome = real.await;
                if outcome.is_error {
                    judge.failed(&outcome.text);
                }
                outcome
            }
            .boxed();
        };
        let outcome = if !tool.is_empty() && !judge.allows(tool) {
            Outcome {
                text: format!("denied: {NOT_IN_SCENARIO}"),
                is_error: true,
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
        if outcome.is_error {
            judge.failed(&outcome.text);
        }
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
    ) -> Self {
        let host = ToolHost::new(
            Some(folder.home.clone().into()),
            None,
            None,
            Limits::default(),
        );
        let (tx, rx) = mpsc::channel(TURN_CHANNEL);
        let judge = Judge::new(scenario);
        let mut scope = TurnScope::new(tx.clone(), Locality::Local, folder.repo.clone());
        scope.approver = judge.clone();
        scope.families = families(scenario);
        scope.stage_key = Some(StageKey::Pane(PaneId::for_test(1)));
        let dispatch = Canned {
            real: host.dispatch(scope),
            judge: judge.clone(),
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
        ("user", user.len() > 2 && text.contains(user)),
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

/// Every text the host or the loop gives a call that did not run must be one
/// `eval::refused` knows, or a refused step would score as completed.
#[test]
fn the_refusal_prefixes_cover_the_real_constants() {
    let errors = [
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
    for error in errors {
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
    mark_failed(&mut list, &["nothing is staged".to_owned()]);
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
