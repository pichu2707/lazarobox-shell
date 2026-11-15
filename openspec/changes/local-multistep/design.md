# Design: Local multi-step (measure, then tune)

Evidence marks: **[R]** read in code on `docs/local-multistep-proposal` 7a2ef223 for this design; **[I]** inferred, to verify in the task that uses it. Decisions C1-C11 and "Future direction" of `proposal.md` are binding. Specs read: `specs/local-model-eval/spec.md`, `specs/chat-engines/spec.md` (written in parallel; three spec points are flagged in "Spec alignment").

## Technical Approach

Pure rules and data in `src/core/ai/` (scenarios, scorer, pass and acceptance rules, table renderer, later the model profiles and the step check); I/O only in test code and in the existing runtime loop. S1 adds no runtime code: the replay guard and the live harness are in-crate `#[cfg(test)]` modules next to `ollama_tests.rs`, so they reach the private `ollama` module and the `Fake` helpers without a public test-support API. Both drive the real loop (`ollama::tool_turn_with`) with a dispatch built by the already public `ToolHost::dispatch(TurnScope)` (host.rs:359 [R]), and both score the same thing: the tool exchanges carried by the last request body of the turn, plus the final answer text.

## Key Decisions

| # | C | Topic | Choice | Rejected, and why |
|---|---|---|---|---|
| D1 | C1, C11 | Scorer home and types | `src/core/ai/eval.rs` (types, `score`, `refused`, `asked_back`), `eval/scenarios.rs` (data), `eval/table.rs` (tally, pass rule, acceptance, render). Input is `&[ToolExchange]` (core type, adapters.rs:239 [R]) and the answer `&str`: no engine, wire, clock or I/O type | Scorer in runtime: would drag tokio types into a pure rule. A new exchange type: `ToolExchange` already holds name, arguments and result |
| D2 | C2, C3 | Scenario format | Rust `const` data: `Scenario { id, category, request, seed, steps, order, literals, forbid, ambiguous }`; a step is `Expect { label, tool, args: fn(&Value) -> bool }` (no result predicate: no step needs one) and `Literal { step, text }`. `Category { WriteAndTest, EditExisting, ReadOnly }` with `key()` = `write-and-test`/`edit-existing`/`read-only`. `seed` is the generated project (relative paths, file texts) both replay and live write | TOML/JSON scenario files: predicates are code; a file format would need a predicate language. Category as free text: the table summary groups by it |
| D3 | — | Transcript format | Replay transcripts are fixture files `tests/fixtures/ollama/eval/<scenario>-<case>.json`: `{ "scenario", "replies": [{ "text", "calls": [{ "name", "arguments" }] }] }`, parsed with `deny_unknown_fields`, turned into `Fake` steps (`text`, `calls_many`; `calls_many` becomes `pub(super)`, test-only). S1 ships `fib-good`, `fib-no-pytest`, `fib-wrong-message`, `fib-commit-first` (hand-written) | Transcripts as Rust literals: real curated runs could not be added without code. Storing results in fixtures: results come from the real dispatch at replay |
| D4 | — | What is scored | `exchanges_from_body(&Value) -> Vec<ToolExchange>` (test harness): pairs each assistant `tool_calls[i]` with the following `tool` messages, in order, from the LAST recorded request body. Answer = concatenated `Delta` events. A text call is never in `tool_calls`, so it never counts (spec "Text call does not count") | Scoring `StreamEvent::ToolCall`: it carries name and summary, no arguments (engine.rs:364-369 [R]), so the commit literal is invisible. A recording `Dispatch` decorator: it sees the parsed `Call`, not the JSON arguments |
| D5 | — | Refused exchange | `eval::refused(result)`: true for the host's refusal texts (`denied: ` prefix, `ollama::FAILED` "the tool failed", `ollama::DROPPED`, `TOO_MANY`, `paths::REFUSED`, the 13 `refusal_text` forms, pinned by a test against the real constants; unprefixed `ToolStatus::Error` texts are not covered, see tasks 1.2). A non-zero exit of `run_command` is NOT a refusal: the tool ran | Using `ToolStatus`: the body does not carry it |
| D6 | — | Replay feeds the real loop | In-crate `src/runtime/chat/eval_replay_tests.rs`: `Fake` + `tool_turn_with(Profile::Ollama, ..)` + `ToolHost::dispatch(TurnScope { approver: ScenarioApprover, families: coding set, stage_key, .. })` over a lab, wrapped in `Canned`, a test `Dispatch` that answers `run_command` and git calls with fixed non-refusal outcomes and forwards every other call (reads, writes, `propose_batch`) to the real dispatch | Public `Dispatch`/runner constructor for `tests/`: `ToolHost::dispatch` is already public, but `mod ollama` is private (chat.rs:1266 [R]) and the port override `adapter_runner_at` is `cfg(test)` (chat.rs:333-336 [R]); exporting the loop costs production lines and API for a test. Real exec and git in CI: needs the sandbox, python3, pytest and a lab outside `/tmp` on every CI runner |
| D7 | — | Approvals | `ScenarioApprover` (test code, implements `Approver`, host.rs:96 [R]) maps the ask variant to a tool (`Exec` -> `run_command`, `Batch` -> `propose_batch`, `Commit` -> `git_commit`, verified in `approval.rs:94`; `GitAdd` -> `git_add` is approved together with the commit step), answers `Decision::Sent` when that tool is an expected step, else `Denied("not part of the scenario")` and records it as `unexpected` | Approving everything: hides a model that writes in the read-only task. Driving through `Chats` and `decide`: the scenario rule would sit in a polling loop |
| D8 | C3, AGENTS.md | Live test location | In-crate `src/runtime/chat/ollama_live.rs`, `#[cfg(all(test, target_os = "linux"))]`, every test `#[ignore]`. Run: `cargo test --lib ollama_live -- --ignored --nocapture --test-threads=1`. No CI workflow passes `--ignored` | `tests/ollama_live.rs` like `claude_live.rs`: same public-API cost as D6 |
| D9 | C3 | Throwaway repo | `throwaway_root()`: `$LAZAROBOX_EVAL_DIR` or `$CARGO_MANIFEST_DIR/target/olive`; refuses a path under `/tmp` or `std::env::temp_dir()`. Each run gets `tempfile::Builder::new().prefix("r").tempdir_in(root)` (removed on drop, also on panic), `home/` and `home/repo/`; seeded by `git init`, local `user.name eval`, `user.email eval@example.invalid`, the scenario seed, one commit; seeding runs with `GIT_CONFIG_GLOBAL` pointing at an empty file in the run folder. `ToolHost::new(Some(home), ..)`, origin `repo` | `/tmp`: the jail refuses it. The owner's HOME: C4. One repo for 5 runs: runs would see each other's files |
| D10 | C2, C8 | Live run | `LAZAROBOX_EVAL_MODEL` (comma list, required; unset panics with usage), `LAZAROBOX_EVAL_RUNS` (default 5), optional `LAZAROBOX_EVAL_TASKS`, `LAZAROBOX_EVAL_HARDWARE`, `LAZAROBOX_EVAL_OUT` (file for the table). Pre-checks, each a panic naming the cause: `GET /api/version`, `GET /api/tags` holds every model (never pulls), `python3 -m pytest --version`. Requests go through a recording proxy (test code: accept, read one request, record body, forward to `127.0.0.1:11434`, copy the reply back) so D4 works live. Runs are sequential; each turn is `tokio::time::timeout(601 s, tool_turn_with(..))` timed with `Instant`; elapsed or `> 600 s` = failed by time (C8: exactly 600 s is judged by steps). Request: the production chat system prompt and the coding families of a default chat [I: the composing function], model from the env | Parallel runs: one GPU. A warm-up request: the median absorbs the first load |
| D11 | Future direction | Score table | `table::render(&Header, &[Row]) -> String`, plain ASCII, fixed columns, `\n` only, sorted by model then task. Header: `lazarobox local-model eval, format 1`, `date: YYYY-MM-DD` (UTC, days-to-civil helper, no new crate), `ollama: <version>`, `hardware: <env, else nvidia-smi name+memory, else cpu>`, `runs: N`, `lazarobox: <git short sha>`. Row: model, task, category, `passed/runs`, failure kinds with counts (`missing:pytest x1`, `order`, `literal`, `asked`, `forbidden`, `unexpected`, `time`), per-run seconds, median. Summary: per model x category, combinations passed / total. Baseline committed as `openspec/changes/local-multistep/scores/s1-baseline.txt` and pasted in the PR | JSON output: the PR and later guidance need text a person reads; a script can still split fixed columns |
| D12 | C2, C9 | Pass and acceptance | Pure in `table.rs`: `passes(passed, runs) = passed >= 4` for 5 runs; `accept(before, after) -> Verdict { Accepted, Refused(combos), Repeat(combos) }`: Repeat when any accepted-set combination sits at exactly 4/5 on one side of a change; Refused when one falls below 4/5; Accepted when one rose and none fell below. Informative models never block | Comparing totals only: a gain on one model would hide a loss on another |
| D13 | C4 | Fixture check | Test `fixtures_are_clean` over `tests/fixtures/ollama/eval/*.json`: fails on a token that starts with `/` and has two segments, `~/`, `[A-Za-z]:\`, the running `$HOME` and `$USER` values, an email-like or a token-like string (`sk-`, `ghp_`, 32+ hex/base64 chars); reports the file and the kind, never the value. Fixtures also capped at 16 KiB each. Live `LAZAROBOX_EVAL_DUMP=<dir>` (outside the repo) writes each run in the D3 format with the repo root replaced by `.`, as raw material for hand curation | A recorder in the app: out of scope. Checking only in CI: the `$USER`/`$HOME` checks matter on the machine that made the fixture |
| D14 | C6 | S2: 400 classification | `Profile::rejected` (ollama.rs:115-118 [R]) for Ollama: `text` starts with `HTTP 400: `, the rest parses as JSON, its `error` string contains `does not support tools` (case-insensitive). Anything else, including an empty, truncated or non-JSON body, takes the existing `Some(Err(error))` arm (:254-259 [R]): an `Error` event `HTTP 400: <flattened, sanitized body>`, already capped at 200 chars (`ERROR_MESSAGE_CAP`, loopback.rs:47 [R]) and shown in the pane. `OLLAMA_NO_TOOLS_NOTICE` and `REJECTED` docs lose "any 400". The exact phrase is pinned from a captured reply (fixture `tests/fixtures/ollama/400-no-tools.txt`) [I] | Status-only match (today): a `think` 400 would drop tools silently. A new notice before the error: two lines for one fault |
| D15 | C6, C7 | S3: profile table | `src/core/ai/chat/model_profiles.rs`: `ModelProfile { key: Key, think: Option<bool>, rounds: Option<usize>, addendum: Option<&'static str>, reminders: bool }`, `enum Key { Exact(&'static str), Family(&'static str) }`, `pub const PROFILES: &[ModelProfile] = &[]` until numbers justify a row. `profile_for(model)`: strip namespace (`library/`, `hf.co/<org>/`), Exact equals the full lowercase name; Family equals the name before `:`. Exact wins. Applied: `think` as top-level bool in `finish_body` for the Ollama wire only (adapters.rs:465 [R]); `rounds` in `max_rounds(set, model)` (ollama.rs:55 [R]); `addendum` where the system head and history budget are composed [I]; `reminders: false` turns off `go_on` and the step check. A 400 on a `think` request is D14's visible error, no retry | Prefix family match: `qwen2.5` would silently govern `qwen2.5-coder`, whose values were never measured. `config.toml` rows: C7 |
| D16 | C1, C2 | S4: step check | Pure `src/core/ai/chat/step_check.rs`: `check(request: &str, answer: &str, exchanges: &[ToolExchange]) -> Option<Missing { step: Step, why: Claimed \| Requested }>`, `Step { FileWrite, TestRun, Commit }`. Source of truth is the exchanges of the turn's `history` that `eval::refused` does not mark (refusal helper moves to `chat` then); claims and requests are phrase lists, as in `next_step.rs`. Never fires on an answer ending in `?`, without tools or in the no-tools retry. Hook: third arm of the reminder match in the no-calls branch (ollama.rs:266-289 [R]), gated by the same flag as `go_on` (`go_on_reminded`, :214, renamed `reminded`); order text call (own flag) > `go_on` > step check | A second budget: two extra 80-200 s rounds per turn. Deriving steps from free text only: the answer's claims are the cheaper, safer signal; requested steps cover "never ran pytest" |
| D17 | — | MSRV 1.88 | No `if let` guards in match arms (D14, D16 keep plain guards or nested `match`); let-chains are allowed (adapters.rs:472 uses them [R]) | — |

## Data Flow

    scenario (core const) ──seed──> lab / throwaway repo
       │ request                         │
       v                                 v
    Fake (fixture replies) | proxy ──> real Ollama
       │                      ^
    tool_turn_with ──calls──> Canned? ─> ToolHost::dispatch(TurnScope + ScenarioApprover)
       │ Delta events          (replay only)
       v
    last request body ──exchanges_from_body──> eval::score(scenario, exchanges, answer)
       └──> RunResult(+time, unexpected) ──> table::render / accept

## File Changes (production lines; tests and fixtures excluded)

| Slice | File | Action | Lines |
|---|---|---|---|
| S1 | `src/core/ai/eval.rs` | Create: types, `score`, `refused`, `asked_back` | ~170 |
| S1 | `src/core/ai/eval/scenarios.rs` | Create: tasks (a)-(c), seeds, predicates | ~90 |
| S1 | `src/core/ai/eval/table.rs` | Create: tally, median, `passes`, `accept`, `render` | ~110 |
| S1 | `src/core/ai/mod.rs`, `src/runtime/chat.rs` | `pub mod eval`; three `#[cfg(test)] mod` lines | ~4 |
| S1 tests | `src/runtime/chat/eval_harness.rs`, `eval_replay_tests.rs`, `ollama_live.rs`, `src/core/ai/eval/tests.rs`, `tests/fixtures/ollama/eval/*.json`, `scores/s1-baseline.txt` | Create | — |
| S2 | `src/runtime/chat/ollama.rs` | `rejected`, docs, notice text | ~40 |
| S3 | `src/core/ai/chat/model_profiles.rs` (new), `adapters.rs`, `ollama.rs`, head composer [I] | table, `think`, rounds, addendum, reminders gate | ~220 |
| S4 | `src/core/ai/chat/step_check.rs` (new), `ollama.rs` | check, texts, hook | ~240 |

S1 total ~374 estimated; measured at the 2.5 checkpoint: scorer and scenarios 246, table 189, so S1 is split: S1a (scorer, scenarios, refusal pin test) and S1b (table, replay guard, live harness). If apply passes it, the seeds move to fixture files read by the tests (−30).

## Testing Strategy (strict TDD, `cargo test`, no network)

| Layer | What |
|---|---|
| Unit (core) | Every scorer scenario of the spec; scenarios engine-neutral (no `ollama`, `http`, port, model text); `passes`, over-time edge (600 s vs 601 s), `accept` (raise, fall, 5->4 noise, Repeat); median of 10..50 = 30; render byte-identical twice, ASCII only, no `/` path |
| Replay | Good fixture passes; each bad fixture fails for exactly its reason; unexpected `propose_batch` in task (c) is denied and reported; runs offline |
| Harness | `throwaway_root` with `TMPDIR=/tmp` is not under `/tmp`; repo gone after a panicking run; seeding leaves the `GIT_CONFIG_GLOBAL` file empty; `exchanges_from_body` pairing; fixture scan passes on shipped files and fails on a planted `/home/x/y.py` without printing it |
| Regression | Existing recorded-body tests (adapters, `ollama_tests.rs`, `llamacpp_tests.rs:108` no `think`) unchanged in S1 |
| S2 | Refusal body -> retry without tools; invalid-option, empty, non-JSON, 1 MiB bodies -> one request, `Error` with `400`, ≤200 chars, no message echo |
| S3 | `profile_for` table (family, exact, none, namespaced); no row = byte-identical body; `think` false/true top-level; profile rounds only for its family |
| S4 | Spec scenarios via `Fake`; shared budget with `go_on` |
| Live | Manual baseline (4 models x 3 tasks x 5) |

## Migration / Rollout

No migration, no config key. S1 is test infrastructure; S2 lands before S3 (C6); S3 ships with an empty table and gains rows only with a before/after table; S4 ships only if its table is accepted (D12).

## Spec alignment (for the spec author)

1. chat-engines "Tools refusal still degrades silently" says the model "is remembered as without tools for the session". The loop decides per turn (ollama.rs:204-207 [R], "a per-pane memory is a follow-up"). This design keeps per-turn; the spec line should drop the memory or S2 grows.
2. "Per-model profiles" says the family is "matched by prefix"; D15 matches the family by equality of the name before `:` (rationale in D15).
3. "Replay guard" says "real `Dispatch` lab"; D6 keeps the real dispatch for files and batch and cans exec and git so CI needs no sandbox, python or git.

## Drift check (proposal references vs code)

| Reference | Now [R] |
|---|---|
| `TurnDispatch` private, host.rs:367, "needs a public Dispatch constructor" | true, but `ToolHost::dispatch` (:359) is public; the real barrier is private `mod ollama` (chat.rs:1266) and `cfg(test)` `adapter_runner_at` (chat.rs:333) |
| Fake ollama_tests.rs:105-190 | 105-196 |
| adapters.rs:164-167 | 165-167 |
| ollama.rs 49-68, 82, 115-118, 214, 245, 266-289; config.rs 473-495 | unchanged |

## Open questions

None blocking. [I] items are verified by the task that uses them: ask-variant names for batch and commit, host refusal prefixes, the exact Ollama refusal phrase, the function composing the chat system prompt and head budget.
