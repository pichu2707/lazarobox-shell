# Tasks: ODD flow hardening (no worker and no answer before authorize; skipped steps refused visibly; written calls re-asked)

Strict TDD (`cargo test`): every GREEN task is preceded by its RED test task, through the public interface of the module; refactor last. Decisions A1-A11 (design.md) and P1-P5 (proposal.md) are binding. MSRV 1.88: no `if let` guards in match arms. Every PR ends with `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`, then `rm -rf target` in the worktree (AGENTS.md). Never run the full suite from a worktree under `/tmp`.

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines (production) | ~200 (core 110, runtime 70, app/event 8, text call 14); tests, fixtures, comments, blanks and `.md` not counted |
| 400-line budget risk | Low |
| Chained PRs recommended | No |
| Suggested split | Single PR. Lever only if the count passes ~380 at task 6.1: move Phase 2 plus tasks 4.3/4.4 (`Tail`; A1 + A9, ~45 lines) to a follow-up PR |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main (used only if the lever fires) |

Decision needed before apply: No
Chained PRs recommended: No
Chain strategy: stacked-to-main
400-line budget risk: Low

### Suggested Work Units

| Unit | Goal | Likely PR | Notes |
|------|------|-----------|-------|
| 1 | Authorize spawn gate, answer gate, re-ask bound, text-call mark, prompt rule, eval fixture, CHANGELOG | PR 1 | base main; tests and docs included |
| 2 (lever) | Text-call turn-end mark (A1 + A9) | PR 2 | only if PR 1 > ~380; base main after PR 1 |

Merge-order dependency: acceptance-judges S2a edits `src/runtime/orchestra.rs` (judge consent fields, `review` args, `conduct`). This change edits `Next`, `turn_ended`, the `next_turn` arm, `LIFT_NOTE` and the `Waits` construction. The regions are disjoint; whichever merges second rebases onto main and re-runs `cargo test`.

## Phase 1: Verify first (read-only; record each result under the task)

- [ ] 1.1 Confirm no persisted state matches on `Gate` (grep `Gate` with serde); confirm line drift of run.rs:909 (`spawn_as`), :1545 (`finish` writer slot), :1585 (`next_turn`), chat.rs:563/:886, ollama.rs:275, text_call.rs:32, tools.rs:166, runtime/orchestra.rs:1255. Fix A1-A11 if any differs.

## Phase 2: Text call (A9; scenario: The `tool` key form)

- [ ] 2.1 RED `text_call` tests: `{"tool":"spawn_agent","arguments":{}}` found; non-object `arguments` refused; plain text and a tool-name-only text give none.
- [ ] 2.2 GREEN `src/core/ai/chat/text_call.rs`: `call_name` also reads `tool`.
- [ ] 2.3 RED `orchestra/tools` tests: `NAMES` has 7 entries; `written_call` returns the coordinator tool name for both keys and `None` for a non-coordinator name.
- [ ] 2.4 GREEN `src/core/ai/orchestra/tools.rs`: `NAMES: [&str; 7]` (also used by `parse`, :166) and `written_call(text) -> Option<&'static str>`.

## Phase 3: Core gates (pure; `steps.rs`, `run.rs`)

- [ ] 3.1 RED `gate_tests.rs` (A6, P2): `review` and read-only `simple` refused with `Gate(Authorize)` while authorize is open and start after `read_only`; a writer still refused at explore; refused read spawn then authorize leaves no stale gate (settles); `spawn_internal` never gated (A7).
- [ ] 3.2 GREEN `run.rs` `spawn_as`: gate target `Implement` for a writer, else `Explore`; `spawn_target` field; `close_step` re-checks `spawn_target`; a non-writer success clears a `Spawn` gate targeting `Explore`.
- [ ] 3.3 RED `steps` tests: `Gate::Answer` label `answer`; exact model text and pane line for authorize, implement step S, implement step (spawn the code worker), and written call T (with optional step sentence); all ASCII.
- [ ] 3.4 GREEN `steps.rs`: `Gate::Answer`, label, refusal arm, `reask()`, pane line builder.
- [ ] 3.5 RED `gate_tests.rs` gap table (A4): no grant; implement with no writer; implement after a writer; read_only; read_only + written; lifted; pending; not answered; standing spawn gate takes `Wait`.
- [ ] 3.6 RED bound tests (A5, P4): one `Reask` per step then `Wait`; a user reply never clears `reasked`; the cap line names `answer`; re-ask and wait each count a turn.
- [ ] 3.7 GREEN `run.rs`: `TurnEnd { answered, written }`, `NextTurn::Reask { step, written }`, `reasked`, `answer_gap`, `wrote()` (writer-slot test moved from `finish`), `next_turn_after`; `next_turn()` stays `#[cfg(test)]` as `next_turn_after(TurnEnd::default())`.
- [ ] 3.8 RED then GREEN `Run::undelegated()` (A11): false under a `read_only` grant, still true with gates lifted.

## Phase 4: Runtime and app (A1, A8)

- [ ] 4.1 RED `src/runtime/tests/gate_wait.rs`, `gates.rs`: answered `TurnEnd` sends `GateRefused(Answer)` then `Continue` whose text starts with the re-ask and holds the wrapped ledger (no `CONTINUE_HEAD`); second time sends `Waits { gate: Answer }`; no `Undelegated` on a re-ask; read_only inline answer settles with no notice; `LIFT_NOTE` reads `spawn, finish and turn ends are not gated now`.
- [ ] 4.2 GREEN `src/runtime/orchestra.rs`, `core/ai/orchestra/event.rs`: `Next::Reask { line, go }`, `turn_ended(.., end)`, `next_turn` arm, `Waits.gate` read from `last_gate()`, `LIFT_NOTE`; `Reask` never calls `tell_undelegated`.
- [ ] 4.3 RED `src/runtime/chat.rs` tests: `Tail` appends `Delta` (cap 16 KiB), resets on `ToolCall` and `Notice`; an errored or interrupted turn gives `answered = false`; a final written call sets `written`.
- [ ] 4.4 GREEN `run_turn` (chat.rs:563) `Tail { done, text }` and the `TurnEnd` built at :886.
- [ ] 4.5 RED `tests/orchestra_gate_wait.rs` (app): `Waits { gate: Answer }` offers the gates; Enter lifts them and names `answer` in the transcript; next text-only turn settles.
- [ ] 4.6 GREEN `src/app/orchestra.rs`: offer gates on `Waits`.

## Phase 5: Prompt rule, eval fixture, changelog

- [ ] 5.1 RED prompt-profiles test: both profiles' `orchestrate` prompt (`orchestrate.md` + `orchestrate_odd.md`) say nothing comes before `authorize`, a question takes `authorize` `read_only` and is answered directly with no `explore`, and stay at most 6 KiB.
- [ ] 5.2 GREEN add the question rule (not counted) to `src/core/ai/chat/prompts/programmer/orchestrate_odd.md` and `src/core/ai/chat/prompts/analyst/orchestrate_odd.md`; measure both sizes.
- [ ] 5.3 RED then GREEN eval: new `tests/fixtures/ollama/eval/coordinate-tool-key.json` (`{"tool": ..}` text call); replay shows no reminder before this change and one after; `coordinate-good` still passes all 5 steps; `coordinate-text-call` and `coordinate-prose` complete nothing.
- [ ] 5.4 CHANGELOG.md `[Unreleased]`: entry (Fixed/Changed) for the authorize spawn gate on every worker kind, the answer gate with its bounded re-ask, the `tool` key text call, and no `no worker ran` notice after an allowed `read_only` answer; ASCII, issue #503/#441.

## Phase 6: Verification

- [ ] 6.1 Production line count (no tests, blanks, comments, `.md`, fixtures); if over ~380 apply the lever in the forecast.
- [ ] 6.2 `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`; existing spawn/finish gate tests and the 20 `next_turn()` call sites unchanged and green; `rm -rf target`.
- [ ] 6.3 [manual, owner] Live check per `skills/run-lazarobox/SKILL.md`: isolated HOME and tmux session, never the owner's real config; local ollama with `qwen3:14b` and `qwen2.5-coder:14b`. Confirm (a) a text-only first turn shows the answer-gate line, then an `authorize` handoff or `run waits for your reply` with `Enter gates off | answer gate: step authorize`; (b) a question closes `authorize` `read_only`, answers inline, no notice; (c) a `review` spawn before authorize is refused with the spawn-gate line; (d) Enter lifts the gates and names `answer`. Record the transcript lines seen; a weak model failing to follow the re-ask is not a defect.
