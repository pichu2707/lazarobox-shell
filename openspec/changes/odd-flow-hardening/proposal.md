# Proposal: ODD flow hardening (no worker and no answer before authorize; skipped steps refused visibly; written calls re-asked)

## Intent

Issue #503, slice 1 of the hybrid plan (exploration: engram `sdd/app-driven-odd/explore`). It hardens the model-driven flow that both future modes share. The app-driven driver comes later (slices 2-3).

Symptoms today (code at origin/main, worktree `odd-hardening`):

- **Steps are skipped.** The spawn gate only guards workers that write. It checks `writer && gated && !gates_lifted` (src/core/ai/orchestra/run.rs:909-916). A `review`, `summarize`, `general`, `chat` or read-only `simple` worker starts before `authorize`. So does an inline answer.
- **An inline answer is only disclosed.** If a turn ends with text and no worker, `NextTurn::Settle` (run.rs:1595) leads to `tell_undelegated` (src/runtime/orchestra.rs:931). The pane then says `no worker ran, nothing written` (src/core/ai/chat/guidance.rs:36). Nothing makes the coordinator take the flow. The spec forbids an automatic retry (agent-orchestrator, "An undelegated coordinator reply is disclosed [W1]").
- **#441: local coordinators write calls as text.** Two local models failed live. `qwen2.5-coder:14b` wrote `{"name": "spawn_agent", "arguments": {...}}` as text (fixture tests/fixtures/ollama/eval/coordinate-text-call.json). `qwen3:14b` described the calls in prose (coordinate-prose.json). The Ollama/llama.cpp loop reminds once per turn (src/runtime/chat/ollama.rs:273-307, src/core/ai/chat/text_call.rs:18). A second text call ends the turn as `Done`, and the run settles with no worker. `written_call` misses the `{"tool": ..}` form, which `orchestra::textcall::is_call` already accepts (src/core/ai/orchestra/textcall.rs:51).

## Decisions (owner, 2026-11-15, settled)

- **D1** With ORCH on, nothing may come before `authorize` closes: no worker of any kind and no inline answer. After `authorize` closes `read_only`, the coordinator may answer itself.
- **D2** A skipped step is refused, not fatal. The model gets a clear reply and the user sees a short line. The model can correct itself. The shape is today's `Gate::refusal` (src/core/ai/orchestra/steps.rs:96).
- **D3** A coordinator call written as text is never run. It gets a visible re-ask.
- **D4** The override stays as it is today: gates off (Enter / `lift_gates`).
- **D5** The undelegated notice becomes enforcement where D1 applies, with a bounded number of re-asks.

## Proposal decisions (this phase, for review)

- **P1 An inline answer needs `authorize` only, not `explore`.** The answer is itself what explore would find. The coordinator keeps its read tools. Asking for an `explore` handoff too would add a second step to every question and adds no safety (nothing is written). That breaks the "at most one extra step" bound for strong models. A read-only question then costs exactly one handoff (`authorize`, scope `read_only`), which the ODD prompt already asks for.
- **P2 Read workers need `authorize` only.** Explore is where read workers are used, so the rule cannot require `explore` before them. Writers keep the full gate (every step before `implement`).
- **P3 An inline end under an `implement` grant is refused too.** This is a turn that ends with no tool call, with no writer started and with steps still open. It is the case where `qwen2.5-coder` closes `authorize` and then writes the rest as text. The re-ask names the next open step (`Steps::next_open`, steps.rs:244).
- **P4 Bound: one re-ask per open step, inside `max_turns`.** Each re-ask is one orchestrator turn and counts against `max_turns`. When the re-ask for that step is spent, the run behaves like today's standing spawn gate (`NextTurn::Wait`, run.rs:106). The run waits for the user's reply and shows the line and the gates offer. When `max_turns` is reached first, the existing stop line names the gate. Waiting is also the right path for a clarifying question: the user's reply continues the same run (W3 "The reply attaches to the waiting run").
- **P5 New gate label `answer`.** `Gate::Answer` sits next to `Spawn` and `Finish`. The existing gates hint, gates offer and lift work with it unchanged.

## Scope

### In Scope

1. Spawn gate: any public spawn (`gated`) is refused with `Refusal::Gate(Step::Authorize)` while `authorize` is open. Internal workers (lenses, refuter, corrector, judges) are not touched.
2. Answer gate at turn end. It applies to a run that is `Open`, has nothing pending, has no spawn gate standing and has gates not lifted. Two cases: `authorize` is open (D1), or the grant is `implement`, no writer ran and steps are still open (P3). The run gets a re-ask continuation and a visible line, bounded by P4.
3. Written coordinator calls (D3). `written_call` also accepts `tool` as a key. The re-ask text says that text does nothing. When the coordinator writes an orchestration call as text and its reminder is spent, the turn end re-asks (the answer gate path), even after a `read_only` grant.
4. Prompt line (both `orchestrate_odd.md`): a question takes `authorize` `read_only` and is then answered directly. Nothing comes before `authorize`.
5. Spec deltas (see Capabilities).

### Out of Scope

- The app-driven driver, the `drive` setting, persisting and resuming run state (slices 2-3).
- A new override key (D4). Auto-start of ORCH (#763). Per-role models (#765).
- Re-asking automatically after a standing spawn-gate refusal. W3 option B stays: the refusal is the tool reply in the same turn (open question 2).
- Running anything parsed from text.

## Capabilities

### New Capabilities

- None

### Modified Capabilities

- `agent-orchestrator`: "Gates on step state are visible and bounded [W3]" gains the authorize spawn gate (every worker kind), the answer gate, the re-ask bound and its wait. "An undelegated coordinator reply is disclosed [W1]" drops "no automatic retry" where the answer gate applies. The notice stays everywhere it applies except after an allowed `read_only` inline answer (open question 3, decided). "Spawn refusals that create no worker" lists the authorize gate.
- `chat-engines`: "A tool call written as text gets one reminder" accepts the `tool` key.
- `prompt-profiles`: the ODD orchestrate prompt has the question rule.

## Approach

1. **Pure core** (`steps.rs`, `run.rs`):
   - `Gate::Answer` with a label and a refusal text.
   - In `spawn_as`, an authorize check before the writer gate.
   - `Run::answer_gap() -> Option<Step>`.
   - A `reasked` step set.
   - `NextTurn::Reask(Step)` in `next_turn`, before `Settle`; when the re-ask is spent it falls back to `Wait` with `last_gate = (Answer, step)`.
2. **Runtime** (`runtime/orchestra.rs`): `Reask` maps to `Next::Go(Continue)`, with the re-ask text above the wrapped ledger, plus `OrchestraEvent::Reask { run, step, n }` for the pane line. `tell_undelegated` still fires only on `Settle`.
3. **Text call** (`text_call.rs`, `ollama.rs`):
   - The `tool` key.
   - The loop marks a turn whose final text is an unanswered written call.
   - That mark reaches `turn_ended`. Today the call at src/runtime/chat.rs:889 carries no text, so design picks the route.
4. **App** (`app/orchestra.rs`): render the `Reask` line. The gates offer covers `answer`.

### User-visible wording (drafts, ASCII)

| When | Model gets | Pane line |
|---|---|---|
| Spawn before authorize | `spawn gate: step authorize is not closed (closed: none). Hand off authorize; spawn again only after it is handed off. Only the user can lift the gate (Enter gates off)` (existing text) | existing refused-spawn notice + `Enter gates off \| spawn gate: step authorize` |
| Answer before authorize | `answer gate: your turn ended without a tool call and step authorize is not closed (closed: none). Text, prose or a call written as JSON does nothing. Call the handoff tool now: step authorize, scope read_only to answer yourself or implement to change files.` | `answer gate: the coordinator answered before handing off authorize; asked it to hand off authorize first` |
| Stopped under `implement` | `answer gate: your turn ended without a tool call and step <next> is not closed (closed: <list>). Call handoff for <next> now.` | `answer gate: the coordinator stopped before step <next>; asked it to go on` |
| Written call, reminder spent | `answer gate: you wrote a call to <tool> as text; nothing was run. Make a real tool call.` | `the coordinator wrote a <tool> call as text; nothing was run; asked it to make a real tool call` |
| Re-ask spent | (none) | existing `run waits for your reply: step <step> is not closed; closed: <list> (Enter gates off)` + `Enter gates off \| answer gate: step <step>` |

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/orchestra/steps.rs` (`Gate` :75-112) | Modified |
| `src/core/ai/orchestra/run.rs` (`NextTurn` :92, `undelegated` :455, `spawn_as` :888-922, `next_turn` :1570-1603) | Modified |
| `src/runtime/orchestra.rs` (`turn_ended` :904, `next_turn` :941-987) | Modified |
| `src/runtime/chat.rs` (:886-891), `src/runtime/chat/ollama.rs` (:273-307) | Modified |
| `src/core/ai/chat/text_call.rs` (:18-42), `src/core/ai/orchestra/event.rs` | Modified |
| `src/app/orchestra.rs` (`Waits` :417, `on_undelegated` :563) | Modified |
| `src/core/ai/chat/prompts/{programmer,analyst}/orchestrate_odd.md` | Modified (not counted) |

## Review Workload Forecast

| Field | Value |
|---|---|
| Estimated production lines | ~200-280: core ~90, runtime ~50, text call ~40, app ~40, event ~10 |
| 400-line budget risk | Low |
| Chained PRs recommended | No (one PR) |
| Lever if over ~380 | move the text-call turn-end mark (item 3, ~50) to a follow-up PR |

Decision needed before apply: No
Chained PRs recommended: No
400-line budget risk: Low

## Coexistence

- **acceptance-judges S2a.** Judges are internal spawns (`spawn_internal`, run.rs:870) inside `conduct`, and the gate change leaves them alone. Judge consent is asked at the `authorize` handoff, and this slice does not change that handoff. Both slices edit `runtime/orchestra.rs`: rebase whichever merges second. Nothing conflicts in what they mean.
- **#763 / #765.** With ORCH off there is no run and no gate. Per-role models do not change the gate. The later `drive` setting is not part of this slice.

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| A strong model asks a clarifying question before authorize and pays one re-ask turn | Med | P4: one re-ask, then the run waits for the reply in the same run; prompt line 4 |
| A legitimate read-only answer still shows "no worker ran" | n/a | Resolved: the notice is dropped after an allowed `read_only` inline answer (Q3) |
| A weak model loops re-ask → text | Med | One per step, `max_turns`, then wait; every step visible |
| The final-text route touches the turn plumbing | Med | Design chooses; lever moves it out |
| A turn has already streamed the refused answer | Certain | The line says it was not taken as the run's answer; no hiding |
| Eval cannot see run-level gates (`Canned` answers orchestra calls `recorded`, src/runtime/chat/eval_harness.rs:239) | Certain | Runtime tests + live check; see Success Criteria |

## Rollback Plan

Revert the single PR. Spawn gating goes back to writers only and turn end goes back to `Settle` plus the notice. The added `tool` key and prompt line go too. No config, schema or stored data changes.

## Dependencies

- W3 gates, gates offer and wait (shipped). The text-call reminder (shipped). The `coordinate` eval and its fixtures (shipped, local-multistep).

## Success Criteria

- [ ] With authorize open, a `review` spawn and a read-only `simple` spawn are refused with the spawn-gate text and a pane notice. After `authorize` `read_only`, both start.
- [ ] A text-only turn before authorize gets exactly one re-ask and its line. A second one waits for the reply with the gates offer. `max_turns` still stops the run and names the gate.
- [ ] After `authorize` `read_only`, an inline answer settles with no re-ask (one extra step in total).
- [ ] A Small write flow with a compliant model makes the same calls as before (no added step).
- [ ] Gates off lifts the answer gate, visibly.
- [ ] `coordinate` eval in replay. Before and after, the `coordinate-good` fixture passes all 5 steps in order. `coordinate-text-call` and `coordinate-prose` complete nothing in both: nothing written as text ever runs. A new `{"tool": ..}` fixture gets no reminder before this change and one after. Live (owner's machine, `qwen2.5-coder:14b`): the transcript showed `no worker ran, nothing written` before. After, it shows the answer-gate line and then either an `authorize` handoff or `run waits for your reply`. The eval score is not expected to rise from this slice.
- [ ] `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` green. A live check following `skills/run-lazarobox/SKILL.md`.

## Open questions

All three resolved by the owner on 2026-11-15:

1. **P3 stays (decided).** A turn that ends with no tool call under an `implement` grant, with no writer started and steps still open, gets the re-ask (the #441 `qwen2.5-coder:14b` case).
2. **A standing spawn-gate refusal gets no automatic re-ask (decided).** It already waits for the user (W3 option B unchanged).
3. **The `no worker ran` notice is not shown after an allowed `read_only` inline answer (decided; changes the earlier "keep").** It would be noise after an answer the run allowed. It still shows wherever it applies otherwise (for example gates lifted, or an `implement` grant with no worker spawned). The spec delta drops the notice for that case only; the "legitimate answer gets the same notice" scenario of W1 is replaced accordingly.

Consequences for the sections above: In Scope item 2 and the Modified Capabilities line on W1 read "notice stays" as "notice stays except after an allowed `read_only` inline answer"; the risk row "A legitimate read-only answer still shows 'no worker ran'" no longer applies; the Success Criteria line "an inline answer settles with no re-ask" also means no notice.
