# Proposal: Acceptance judges (rule a run's change against the agreed standard, proportional to its scale, visibly)

## Intent

The owner (2026-11-15, verbatim):

> "hay que añadir dentro de la orquestación unos jueces que dictamine que que la tarea está correcta con el estandar acordado"

On scale:

> "si, pero también si el proyecto tiene una embergadura no tan asumible, por ejemplo una función es poco probable que lo haga mal, una hoja con ofunciones si puede haber más posibilidades"

On corrections:

> "Pueden corregir pero teniendo al usuario al tanto de lo que hacen"

> "no solo poner si un fallo y que están corrigiendo, si no marcar donde está el fallo y que parte está corrigiendo"

On the model:

> "El modelo que se pueda elegir en el menú si no el que tienen puesto por defecto"

The gap today (exploration, engram `sdd/acceptance-judges/explore`):

- The S7b receipt-driven review runs only for a `high` change or a cheap author (src/runtime/tools/host/review.rs `before_batch_with`), so most small and medium changes are never reviewed.
- Its lenses judge change quality (risk, resilience, readability, reliability). Nothing checks the change against the goal: the criteria fixed in resolve/track, the feature document, the tests, the repo norms.
- A `Finding` has no location field, only free text (src/core/ai/orchestra/rdd.rs `Finding`), so a correction cannot say where the fault is or what part it changes.
- No per-judge model exists; review workers use the `review` assignment.

## Decisions (owner, 2026-11-15, settled)

- **J1 Standard.** Three sources: the criteria from the resolve/track handoffs and the feature document (Large goals only; Small goals have only the resolve/classify reasons), the project's tests green, and the repo norms (`AGENTS.md`, `CLAUDE.md`).
- **J2 Proportional to scale, from data that exists.** The level is computed from classify's `Size`, the staged change (file count and bytes, `Change`/`StagedDiff`) and the S7b `Tier`. No function-level promise (no function count exists). Small → one judge. Large (`Size` Large, or more than one file, or `Tier` ≥ `medium`) → two independent judges that must agree. Read-only goals → none.
- **J3 May correct, user always informed.** Every verdict, failed criterion and correction is a visible transcript line. Each finding names WHERE (file:line or function, a parsed `at:` field). Each correction names WHAT it changes (file and range, or function). Disagreement or exhausted rounds → a verdict line with what is missing. Never silent.
- **J4 Model from the menu.** A new `judge` row in the Models tab (new `Task::Judge`); unassigned uses the default the menu already shows (`(default)`).
- **J5 One review system.** A judge mode INSIDE the existing S7b review at `propose_batch`, reusing the diff, correction worker, `Guard`, `MAX_CORRECTIONS`, receipt and reviewer notices. The gate widens from "high or cheap author" to "any writer except read-only goals".
- **J6 Tests evidence in two moments.** Pre-apply, judges check the diff against criteria and norms. Before the run closes, an evidence check reads the post-apply test commands the run already records (Commands, `attested()`, user-approved `run_command`). No test run or a failing one → a visible line before finishing. The judge never runs commands (stays read-only).
- **J7 Norms capped.** At most the first 8 KB of `AGENTS.md`/`CLAUDE.md` reach a judge, with a visible notice when cut.
- **J8 The user decides.** The user's batch review still decides; judges never approve.

## Scope

### In Scope

1. **Judge level (S1).** Pure `JudgeLevel` (`none`, `one`, `two`) from read-only flag, `Size`, staged files, bytes and `Tier` (J2).
2. **Verdict format (S1).** Judge prompts and a parser: verdict `pass`/`fail`, failed criteria, findings with a required `at:` field; `Finding` gains a location. A finding without `at:` is shown as `at: unknown`, never dropped.
3. **Standard bundle (S1, S4).** Criteria from the handoffs and feature document; repo norms capped (J7).
4. **Runtime hook (S2).** The judge mode in S7b: widened gate, one or two judges, agreement rule, correction through the existing bounded flow, transcript lines with where/what, judge section in the receipt.
5. **Menu row (S3).** `judge` task row, `[ai.tasks.judge]`, `Task::Judge`, reviewer notices (same model, local, diff leaving the machine) for judges.
6. **Tests evidence (S4).** Check before `finish` on recorded post-apply commands; visible line when absent or failing.

### Out of Scope

- Judges running commands or tests themselves (J6).
- Judges approving a batch or answering consent (J8).
- Function-level scale analysis (J2).
- A project test-command setting (none exists; J6 reads only what the run recorded).
- Changing the lenses, refuter or `MAX_CORRECTIONS` value.
- Local-model evaluation (#441, `local-multistep`).

## Capabilities

### New Capabilities

- `acceptance-judges`: judge level (J2), the standard and its sources (J1, J7), verdict format with `at:`, agreement of two judges, visible lines and corrections (J3), tests evidence before close (J6), never approves (J8).

### Modified Capabilities

- `agent-orchestrator`: "Receipt-driven review [S7b, W7]" gains the judge mode and the widened gate; "Mandatory review for a cheap author, optional for a strong one [W7]" (consent for the widened gate); "Worker contract" (judge workers on the `judge` assignment); "A run ends by itself" (evidence line before close).
- `orchestrator-handoffs`: "RDD receipts [S7b]" gains judge verdicts, failed criteria, locations and correction targets.
- `ai-task-models`: "Fixed task set" gains `judge`; fallback, "Set all excludes" and "Runtime use of every task assignment" cover it.
- `configuration`: `[ai.tasks.judge]` slot.
- `config-menu`: the Models tab gains the `judge` row (row lists and navigation scenarios).

## Approach

1. **S1 pure core.** `src/core/ai/orchestra/judge.rs` (new): level, standard bundle, prompts, parser; `at:` added to `Finding` (rdd.rs). Tested without processes.
2. **S2 runtime.** In `start_review`/`conduct` (src/runtime/orchestra/review.rs), when the level is not `none`, spawn one or two read-only judge workers over the staged diff; two that disagree → a verdict line naming what is missing and the batch goes to the user with blockers open. Failed criteria feed the existing correction worker, scoped to the named locations; lines go through `OrchestraEvent::Review`.
3. **S3 menu.** Add `Task::Judge` after `simple` (src/core/menu.rs Models rows, config tasks); the judge spawn reads it; `review_notices` covers the judge roster.
4. **S4 close.** Before `finish` (src/runtime/orchestra/run.rs), read the run's Commands records; no test command or a failed one → a visible line. Norms read from the run root, capped at 8 KB.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/orchestra/judge.rs` | New (S1) |
| `src/core/ai/orchestra/rdd.rs` (`Finding`, prompts) | Modified (S1) |
| `src/core/ai/orchestra/review.rs` (`Tier`, notices) | Read / Modified (S1, S3) |
| `src/runtime/orchestra/review.rs` (`start_review`, `conduct`, `correct`) | Modified (S2) |
| `src/runtime/tools/host/review.rs` (`before_batch_with` gate) | Modified (S2) |
| `src/runtime/orchestra/run.rs` (`finish`, `review_notices`) | Modified (S2-S4) |
| `src/core/menu.rs`, config `Task` enum | Modified (S3) |
| README orchestrator section | Modified (S4) |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Local models judge poorly: today `qwen3:14b` and `qwen2.5-coder:14b` failed live as coordinators (prose / JSON as text instead of tool calls) | High | Existing local/same-model notices apply to judges; an unparseable verdict is a visible `judge: no verdict` line, never a pass |
| Widened gate costs one or two workers on every write | High | Level `one` for small changes; read-only goals skip; cost shown per worker (S6 usage) |
| Consent prompt on every write becomes noise | Med | Open question 1 |
| Two judges on the same model are not truly independent | Med | Same-model notice; open question 2 |
| Pre-apply judges cannot see tests green | Certain | J6 second moment; stated in the receipt |
| Small goals have weak criteria (no feature document) | Med | Judge uses resolve/classify reasons and says criteria were thin |
| Norms over 8 KB lose rules | Low | J7 notice |

## Rollback Plan

- S1 is a pure module: revert the PR.
- S2 reverts the gate to "high or cheap author"; S3 removes the row (how an older build treats a leftover `[ai.tasks.judge]` table is for the design to verify); S4 removes the close check. Each slice reverts alone.

## Dependencies

- S7b review and W7 notices (shipped). `orchestrator-handoffs` receipts (shipped).

## Success Criteria

- [ ] A small single-file change gets one judge; a multi-file or `medium`+ change gets two; a read-only goal gets none.
- [ ] Every finding and correction line names a location (or `at: unknown`).
- [ ] Disagreement, exhausted rounds or no verdict always produce a visible line.
- [ ] A run with no recorded test command shows a line before finishing.
- [ ] No receipt or judge ever approves a batch.
- [ ] `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` pass on every slice.

## Slices (production lines; tests do not count; each PR under 400)

| Slice | Boundary | Decisions | Size |
|---|---|---|---|
| S1 | Pure judge module: level, standard bundle, prompts, verdict parser, `at:` in `Finding`. No runtime change | J1, J2, J3, J7 | 250-330 |
| S2 | Runtime hook in S7b: widened gate, one/two judges, agreement, correction targets, transcript lines, receipt section; judges use the `review` assignment until S3 | J3, J5, J8 | 300-390 |
| S3 | `judge` menu row, `Task::Judge`, config slot, notices | J4 | 120-200 |
| S4 | Tests evidence before close, norms read and capped, README | J6, J7 | 180-280 |

## Settled questions (owner, 2026-11-15)

- **J9 Consent once per run.** The judges ask consent once, when the run starts. After that they work without asking and report every step in the transcript. When the judge model is a cloud model and the diff leaves the machine, the existing notice is always shown.
- **J10 Independent second judge.** The second judge uses the `review` assignment when it is set and differs from `judge`. When only one model is available, both judges use it, and a visible notice says they are not independent.
- **J11 Default model.** An unassigned `judge` falls back to the run's default model, the same `(default)` rule as `general`. `Set all tasks` includes `judge`.
- **J12 High tier: both, one correction.** On a `high` change the lenses (code quality) and the judges (the agreed standard) both run, because they check different things. They share one correction round, so two correctors never edit the same change.
- **J13 No tests: the user confirms.** When no test ran, or one failed, after apply, the run waits before closing with a line such as `no tests ran · Enter to finish anyway · Esc to keep working`. A line that only scrolls past is not enough (never fail silently).
- **J14 Unknown criteria may pass, visibly.** A batch can pass with criteria the judges could not verify, as long as none is `not met`. The pass line names them, for example `passed · 2 met, 1 unverified (C3: tests not seen)`. Approved by the owner on 2026-11-15.
- **J15 Docs-only changes skip the tests wait.** When every applied file is documentation (Markdown, text, docs folders), the J13 wait does not apply; the close says `no tests needed (docs only)`. Approved by the owner on 2026-11-15.
- **J16 Worker cost on high tier accepted.** Up to 9 workers on a `high` change (lenses plus two judges plus corrector) is accepted; no cap to one judge when the lenses run. Approved by the owner on 2026-11-15.
