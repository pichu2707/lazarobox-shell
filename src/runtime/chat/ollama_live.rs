//! The live evaluation of local models (openspec `local-multistep`, S1): runs
//! the tasks of `core::ai::eval::scenarios` against the owner's Ollama on
//! `127.0.0.1:11434` and prints the score table. Opt-in and never in CI: every
//! test here is `#[ignore]`, and no workflow passes `--ignored`.
//!
//! Run (the whole run can take hours: 4 tasks x runs x models, each turn capped
//! at 10 minutes, the total is not capped):
//!
//! ```text
//! LAZAROBOX_EVAL_MODEL=qwen2.5:7b,qwen3:14b \
//!   cargo test --lib ollama_live -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Run `cargo build` first: commands and git run through the sandbox helper, the
//! `lazarobox` binary in `target/debug`.
//!
//! Environment: `LAZAROBOX_EVAL_MODEL` (comma list, required), `_RUNS` (default
//! 5), `_TASKS` (comma list of task ids, default all), `_HARDWARE` (text for
//! the table), `_OUT` (file the table is also written to), `_DUMP` (folder for
//! each run in the fixture format) and `_DIR` (where the throwaway folders go;
//! default `target/olive`, never `/tmp`). Ollama and the models must be there:
//! nothing is pulled, and a missing model, Ollama or `python3 -m pytest` fails
//! the test with a message.
//!
//! What is real and what is fixed: the model, the files, the batch, `run_command`
//! (the real sandbox, python3 and pytest) and every git tool run for real in the
//! run's throwaway repo, and the harness approves only what the scenario
//! expects. The one fixed answer is the orchestration tools of the `coordinate`
//! task (`handoff`, `spawn_agent`, ...): they answer `recorded`, so that task
//! measures whether the model CALLS the step handoffs and `spawn_agent`, not what
//! a real run manager does after them. Replay (CI) answers commands and git with
//! fixed text too; live never does.
//!
//! Fixture curation (C4): the repository is public. Take a run from the
//! `_DUMP` folder, read it, keep at most one per task, make sure it holds
//! generated content only, copy it to `tests/fixtures/ollama/eval/` and run
//! `cargo test --lib fixtures_are_clean`. A run from a throwaway repo only,
//! never from your projects.

use std::{
    path::Path,
    process::Command,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use super::eval_harness::{Folder, Mode, Rig, dump, live_helper, proxy, throwaway_root, write_dump};
use crate::{
    core::ai::eval::{
        Scenario,
        scenarios::ALL,
        table::{Header, Row, Run, date, hardware, render},
    },
    runtime::loopback::Timeouts,
};

const OLLAMA: &str = "127.0.0.1:11434";
/// One second over the 10 minutes a passing run may take (C8).
const TURN_CAP: Duration = Duration::from_secs(601);

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// A GET to Ollama as HTTP/1.0, so the body is never chunked.
async fn get(path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(OLLAMA)
        .await
        .map_err(|e| format!("Ollama is not reachable on {OLLAMA}: {e}"))?;
    let request = format!("GET {path} HTTP/1.0\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    text.split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .ok_or_else(|| format!("no body in the reply to GET {path}"))
}

/// Every pre-check, each a panic naming the cause. Returns the Ollama version.
async fn precheck(models: &[String]) -> String {
    let version = get("/api/version")
        .await
        .unwrap_or_else(|why| panic!("{why}"));
    let version: Value = serde_json::from_str(&version).expect("GET /api/version answers JSON");
    let tags = get("/api/tags").await.unwrap_or_else(|why| panic!("{why}"));
    let tags: Value = serde_json::from_str(&tags).expect("GET /api/tags answers JSON");
    let have: Vec<&str> = (tags["models"].as_array().into_iter().flatten())
        .filter_map(|m| m["name"].as_str())
        .collect();
    for model in models {
        let latest = format!("{model}:latest");
        assert!(
            have.iter().any(|name| name == model || *name == latest),
            "the model {model} is not installed; this test never pulls one"
        );
    }
    let pytest = Command::new("python3")
        .args(["-m", "pytest", "--version"])
        .output();
    assert!(
        pytest.is_ok_and(|out| out.status.success()),
        "python3 -m pytest does not run: the tasks need pytest"
    );
    version["version"].as_str().unwrap_or("unknown").to_owned()
}

/// One run of `scenario` on `model`: a fresh throwaway repo, one turn, scored.
async fn one(
    scenario: &'static Scenario,
    model: &str,
    port: u16,
    bodies: &Mutex<Vec<Value>>,
) -> Run {
    let folder = Folder::new(&throwaway_root(), scenario);
    folder.init_git();
    let mut rig = Rig::new(scenario, &folder, None, Mode::Live);
    rig.request.model = Some(model.to_owned());
    let judge = rig.judge.clone();
    bodies.lock().unwrap().clear();
    let cap = Timeouts {
        first_byte: TURN_CAP,
        idle: TURN_CAP,
        total: TURN_CAP,
        ..Timeouts::SERVER
    };
    let started = Instant::now();
    let answer = timeout(TURN_CAP, rig.drive(port, &cap))
        .await
        .unwrap_or_default();
    let time = started.elapsed();
    let last = bodies.lock().unwrap().last().cloned().unwrap_or(json!({}));
    if let Some(dir) = env("LAZAROBOX_EVAL_DUMP") {
        let name: String = model
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let text = dump(scenario, &last, &answer, &[&folder.repo, &folder.home]);
        if let Err(why) = write_dump(Path::new(&dir), scenario.id, &name, &text) {
            eprintln!("could not write the dump in {dir}: {why}");
        }
    }
    judge.run(&last, &answer, time)
}

fn gpu() -> Option<(String, String)> {
    let query = ["--query-gpu=name,memory.total", "--format=csv,noheader"];
    let out = Command::new("nvidia-smi").args(query).output().ok()?;
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .to_owned();
    let (name, memory) = line.split_once(", ")?;
    Some((name.to_owned(), memory.to_owned()))
}

fn sha() -> String {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();
    let out = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    out.ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The tasks x runs of each model in `LAZAROBOX_EVAL_MODEL`, as one table.
#[tokio::test]
#[ignore = "live: needs the owner's Ollama and its models; run with --ignored"]
async fn the_chosen_models_run_the_tasks() {
    let Some(models) = env("LAZAROBOX_EVAL_MODEL") else {
        panic!("set LAZAROBOX_EVAL_MODEL=<model>[,<model>...] (see the header of ollama_live.rs)");
    };
    let models: Vec<String> = models.split(',').map(|m| m.trim().to_owned()).collect();
    let runs: usize = env("LAZAROBOX_EVAL_RUNS")
        .map_or(5, |n| n.parse().expect("LAZAROBOX_EVAL_RUNS is a number"));
    let wanted = env("LAZAROBOX_EVAL_TASKS");
    let tasks: Vec<&'static Scenario> = ALL
        .into_iter()
        .filter(|s| {
            wanted
                .as_ref()
                .is_none_or(|w| w.split(',').any(|id| id.trim() == s.id))
        })
        .collect();
    assert!(
        !tasks.is_empty(),
        "LAZAROBOX_EVAL_TASKS names no known task"
    );
    live_helper();
    let version = precheck(&models).await;
    let (port, bodies) = proxy(OLLAMA).await;
    let mut rows = Vec::new();
    for model in &models {
        for scenario in &tasks {
            let mut list = Vec::new();
            for n in 1..=runs {
                let run = one(scenario, model, port, &bodies).await;
                eprintln!(
                    "{model} {} run {n}/{runs}: {} in {:.0?}",
                    scenario.id,
                    if run.passed() { "passed" } else { "failed" },
                    run.time
                );
                list.push(run);
            }
            rows.push(Row::new(model, scenario, list));
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let gpu = gpu();
    let header = Header {
        date: date(now),
        ollama: version,
        hardware: hardware(
            env("LAZAROBOX_EVAL_HARDWARE").as_deref(),
            gpu.as_ref().map(|(n, m)| (n.as_str(), m.as_str())),
        ),
        sha: sha(),
        runs,
    };
    let table = render(&header, &rows);
    println!("{table}");
    if let Some(out) = env("LAZAROBOX_EVAL_OUT") {
        std::fs::write(out, &table).expect("LAZAROBOX_EVAL_OUT is writable");
    }
}
