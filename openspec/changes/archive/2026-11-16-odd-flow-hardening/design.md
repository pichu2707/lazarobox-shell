# Design: ODD flow hardening

Read on origin/main (worktree `odd-hardening`). Owner answers (2026-11-15): P3 stays; no auto re-ask on a standing spawn-gate refusal; no `no worker ran` notice after an allowed `read_only` inline answer.

## Technical Approach

All decisions stay in the pure core (`run.rs`, `steps.rs`). The runtime only passes in a small `TurnEnd` fact and maps one new `NextTurn` variant. Read workers reuse the existing spawn-gate path (`Refusal::Gate` -> `gate_refused`, runtime/orchestra.rs:1255), so nothing new is needed in the runtime for them. The answer gate reuses `GateRefused` (pane line + gates offer) and `Continue` (the next turn), so the app gains only the gate on `Waits`.

## Architecture Decisions

| # | Topic | Choice | Rejected, and why |
|---|---|---|---|
| A1 | Final text at turn end | `run_turn` (chat.rs:563) keeps a `Tail { done, text }` from the events after the pipe. `Delta` appends, capped at 16 KiB. `ToolCall` and `Notice` reset it, so it holds only the last round and a reminded call drops out. `done` = the turn ended in `Done`. The task at chat.rs:886 builds `TurnEnd { answered: done, written: tools::written_call(&text) }` and calls `turn_ended(pane, run, end)` | A new `StreamEvent` variant: every match on it changes, and it would reach the app. A flag set by the Ollama loop: it misses CLI coordinators and needs a side channel from inside `DISPATCH` |
| A2 | Error turns | No answer gate unless `answered`. An engine error or interrupt keeps today's path | Re-asking after a failure would be an automatic retry of an error |
| A3 | Where the gate lives | `Gate::Answer` (steps.rs:75), label `answer`. `Run::next_turn_after(TurnEnd)` adds `NextTurn::Reask { step, written }` after the spawn-gate `Wait` and before `Settle` (run.rs:1585-1596). `next_turn()` stays `#[cfg(test)]` as `next_turn_after(TurnEnd::default())` (answered = false, today's behaviour), so 20 existing call sites keep their meaning | Changing `next_turn`'s signature: churn in 3 test files with no gain |
| A4 | Gap rule | `answer_gap(written)`: `None` if gates are lifted. Grant `None` -> `Authorize` (D1). `Implement` with no writer slot -> `steps.next_open()` (P3). Otherwise, with a written call -> `next_open().unwrap_or(Close)` (D3). Otherwise `None`. Applies only when the run is `Open` and nothing is pending. A standing `Gate::Spawn` takes the existing `Wait` first. The "writer slot" test is moved from `finish` (run.rs:1545) into `fn wrote()` so both use it | Requiring `explore` before an inline answer: rejected in P1 |
| A5 | Bound (P4) | `Run.reasked: Vec<Step>`. On a gap it sets `last_gate = (Answer, step)` first, so the cap line names the gate. Then: if `turns >= limits.turns` -> `Capped`. Then `turns += 1`. A step already in `reasked` -> `Wait(step)`. Otherwise push it -> `Reask`. A user reply never clears `reasked`, so the run waits again rather than looping | A per-run counter: a weak model would spend it on authorize and then nothing would bound the later steps |
| A6 | Read workers (D1, P2) | In `spawn_as` (run.rs:909) the gate target becomes `if writer { Implement } else { Explore }`. `steps.check(Explore)` fails only while `authorize` is open. New field `spawn_target: Step`. `close_step` (run.rs:1427) re-checks `spawn_target`, not `Implement`. A non-writer success clears a `Spawn` gate whose target was `Explore`. Without this, a refused read spawn would leave a stale `(Spawn, explore)` and wrongly turn a settle into a wait | A separate authorize branch: duplicates the refusal and the bookkeeping |
| A7 | Internal workers | Untouched. `spawn_internal` passes `gated = false` (run.rs:876). Lenses, refuter and corrector go through review.rs:1021, and the S2a judges go through the same call in `conduct` | — |
| A8 | Runtime mapping | `Next::Reask { line, go }`. `line` = `GateRefused { gate: Answer, step, reason: pane_line }` (existing app arm: line + `offer_gates`). `go` = `Continue(Continuation { text: "{reask}\n\n{wrapped ledger}" })`, with no `CONTINUE_HEAD`, which talks about workers. `turn_ended` treats `Reask` like `Go` for `turn_over`, sends `line` then `go`, and never calls `tell_undelegated`. `NextTurn::Wait` reads its gate from `last_gate()`. `Waits` gains `gate: Gate` and the app offers the gates there | A new `Reask` event and app arm: duplicates `GateRefused` |
| A9 | Text calls | `text_call::call_name` (text_call.rs:32) also reads `tool`. `orchestra::tools::NAMES: [&str; 7]` (it also replaces the list in `parse`, tools.rs:166). `tools::written_call(text) -> Option<&'static str>` wraps `text_call::written_call` | Calling `textcall::is_call`: it accepts non-object `arguments`, which text_call.rs:68 refuses on purpose |
| A10 | Double reminder | The in-turn reminder stays as it is (one per turn, ollama.rs:275). The turn-end re-ask is the next turn's opening message. So the order is reminder (turn n), re-ask (turn n+1), wait. There are never two in one turn | Suppressing the in-turn reminder on a re-ask turn: more state, and it changes the shipped chat-engines behaviour |
| A11 | Undelegated (owner Q3) | `Run::undelegated()` (run.rs:455) also needs `grant() != Some(ReadOnly)`. It still fires with gates lifted | Suppressing it in the app: the app does not know the grant |

## Data Flow

    run_turn -> Tail{done,text} -> TurnEnd{answered,written} -> turn_ended
      -> Run::next_turn_after: Limit | Wait(spawn) | answer_gap?
           gap & not asked -> Reask -> GateRefused(Answer) + Continue(reask+ledger)
           gap & asked     -> Wait  -> Waits{gate:Answer}
           at cap          -> Capped (names the answer gate)
           none            -> Settle (+Undelegated unless read_only) | Continue | Capped

## Wording (ASCII, never silent)

| Case | Model gets | Pane line (+ `Enter gates off \| answer gate: step S`) |
|---|---|---|
| Authorize open | `answer gate: your turn ended without a tool call and step authorize is not closed (closed: none). Text, prose or a call written as JSON does nothing. Call the handoff tool now: step authorize, scope read_only to answer yourself or implement to change files.` | `answer gate: the coordinator answered before handing off authorize; asked it to hand off authorize first` |
| Implement, step S open | `answer gate: your turn ended without a tool call and step S is not closed (<closed>). Call handoff for S now.` For S = implement: `... Spawn the code worker now.` | `answer gate: the coordinator stopped before step S; asked it to go on` |
| Written call | `answer gate: you wrote a call to T as text; nothing was run. Make a real tool call.` If there is a step gap, the step sentence follows | `answer gate: the coordinator wrote a T call as text; nothing was run; asked it to make a real tool call` |
| Re-ask spent | none | existing `run waits for your reply: step S is not closed; <closed> (Enter gates off)` |
| Spawn before authorize | existing `Gate::Spawn` refusal | existing refused-spawn line |

`LIFT_NOTE` becomes `... spawn, finish and turn ends are not gated now ...`.

## File Changes (production lines)

| File | Change | ~Lines |
|---|---|---|
| `src/core/ai/orchestra/steps.rs` | `Gate::Answer`, label, refusal arm, `reask()`, pane line builder | 40 |
| `src/core/ai/orchestra/run.rs` | `TurnEnd`, `NextTurn::Reask`, `reasked`, `spawn_target`, `answer_gap`, `wrote`, `next_turn_after`, `undelegated` | 70 |
| `src/core/ai/orchestra/tools.rs` | `NAMES`, `written_call` | 12 |
| `src/core/ai/chat/text_call.rs` | `tool` key | 2 |
| `src/runtime/chat.rs` | `Tail` in `run_turn`, `TurnEnd` at :886 | 30 |
| `src/runtime/orchestra.rs` | `Next::Reask`, `turn_ended(.., end)`, `next_turn` arm, `Waits.gate`, `LIFT_NOTE` | 40 |
| `src/core/ai/orchestra/event.rs` | `Waits.gate` | 4 |
| `src/app/orchestra.rs` | offer gates on `Waits` | 4 |
| `prompts/{programmer,analyst}/orchestrate_odd.md` | question rule (not counted) | — |

## Testing Strategy (strict TDD, `cargo test`)

| Layer | Tests (RED first) |
|---|---|
| Core `gate_tests.rs` | `review` and read-only `simple` refused with `Gate(Authorize)` while authorize is open, and they start after `read_only`. A writer is still gated at explore. A refused read spawn then authorize gives no stale gate (settles). `spawn_internal` is never gated. Gap table: no grant, implement with no writer, implement after a writer, read_only, read_only + written, lifted, pending, not answered. One `Reask` per step, then `Wait`. Cap names `answer`. `undelegated()` is false under read_only |
| Core `steps`/`tools`/`text_call` | Exact reask and pane strings, ASCII. `{"tool":"spawn_agent","arguments":{}}` is found, and a non-coordinator name is not |
| Runtime `tests/gate_wait.rs`, `gates.rs` | `turn_ended` with an answered `TurnEnd` sends `GateRefused(Answer)` then `Continue` whose text starts with the reask. The second time it sends `Waits{gate:Answer}`. Nothing is `Undelegated` on a reask. A read_only inline answer settles with no notice. `Tail` resets on `ToolCall`/`Notice`, and an errored turn gives `answered = false` |
| App `orchestra_gate_wait.rs` | `Waits{gate:Answer}` offers the gates, and Enter lifts them |
| Eval | New fixture `tests/fixtures/ollama/eval/coordinate-tool-key.json`: no reminder before, one after |
| Regression | `coordinate-good` passes all 5 steps; existing spawn/finish gate tests unchanged |

Existing runtime tests that end a text turn with authorize open stay green: the `#[cfg(test)]` wrappers default to `answered = false`.

## Coexistence with S2a

S2a touches `runtime/orchestra.rs` (~25 lines: judge consent fields, `review` args) and `conduct`. This change touches `Next`, `turn_ended`, the `next_turn` arm, `LIFT_NOTE` and the `Waits` construction. Those regions are disjoint, so whichever merges second rebases. S4's later `Hold` sits in the same `next_turn_after` order. It cannot meet P3: Hold needs an applied writer, and P3 needs no writer.

## Review Workload Forecast

| Field | Value |
|---|---|
| Estimated production lines | ~200 (tests excluded) |
| Split lever | A1 + A9 (`Tail`, `written_call`, `tool` key, ~45 lines) to a follow-up PR if the count passes ~380 |

Decision needed before apply: No
Chained PRs recommended: No
400-line budget risk: Low

## Migration / Rollout

No migration. `last_gate`, `reasked` and `spawn_target` live only in memory. Revert the single PR.

## Open Questions

None blocking. To check during apply: no persisted state matches on `Gate` (grep shows no serde today).
