# Proposal: Local multi-step (measure, then tune, small local models finishing real coding tasks)

## Intent

Issue #441 (summarized): make small local models (Ollama, llama.cpp; 4-8B in the issue, up to 14B in the acceptance set of C2) finish multi-step coding tasks reliably. The owner (2026-10-30, verbatim):

> "marcamos como mejora esto dentro de los issues para los modelos locales, ahí es donde tenemos que hilar más fino"

Product framing (owner, 2026-11-15, verbatim):

> "lanzalo, piensa que al final es trabajar desde lazarobox en vez de el CLI"

The goal is that the owner does daily coding work from lazarobox's chat with a local model instead of an external coding CLI. Success means "I can trust lazarobox + a local model to finish the job".

What still fails today (issue #441):

- skips steps (pytest never run, a file forgotten);
- ignores the user's literal words (the commit message);
- calls tools out of order (commit before `propose_batch`);
- answers with instructions instead of tool calls;
- asks for input it could decide itself;
- is slow (80-200 s of reasoning per round on qwen3:4b).

There is no way to tell whether a change helps: no score, no baseline, no live harness for Ollama. Every tweak so far (#439 reminders, coding prompt) is unmeasured. This change measures first, then tunes only what the numbers justify.

## Decisions (owner, 2026-11-15, settled)

- **C1 Measure first.** Slice S1 changes no loop, prompt, `think` or profile behaviour. Every later slice is accepted by numbers (C2).
- **C2 Acceptance bar.** Models: `qwen2.5:7b`, `qwen2.5-coder:14b`, `qwen3:14b` (owner's RTX 4080 16 GB, Ollama 0.30.10). Tasks: (a) Fibonacci function + pytest test + run it + commit with the user's literal message; (b) edit an existing file without breaking its tests; (c) a read-only question that must not invent write steps. 5 runs per model x task; a combination passes at 4 of 5. A change is accepted if it raises the score without lowering any model x task combination.
- **C3 Real work, not toys.** Tasks must represent daily coding in lazarobox (the 2026-11-15 framing).
- **C4 Fixtures.** Real transcripts may be stored as fixtures, curated and trimmed: messages and tool calls only; no absolute paths, user names or real file contents; produced from a throwaway repo, never the owner's projects (the repo is public).
- **C5 Developer-only.** The evaluation is a developer tool in this change. Feeding the tool mark (#430) is later, once stable.
- **C6 Reasoning per model, by numbers.** Reasoning off is NOT a default. It is decided per model from the evaluation (`qwen3:14b` vs `qwen3:14b-nothink`). Prerequisite before lazarobox ever sends `think`: tell "tools refused" apart from any other HTTP 400 (today `Profile::rejected` treats every 400 as tools refused, src/runtime/chat/ollama.rs:115-118, 245). Never fail silently.
- **C7 Profiles in code first.** Per-model profiles are a built-in table; a `config.toml` override only if requested later.

Repo rules that bind the live runs (not owner decisions): the throwaway repo lives outside `/tmp` (the jail refuses `/tmp` roots, AGENTS.md); live tests are `#[ignore]` and never run in CI; they never pull models.

## Scope

### In Scope

1. **Scenario scorer (S1).** A pure `eval` module: a scenario is the request text, expected steps with argument predicates, order constraints and required literals (the commit message). `score(&[ToolExchange])` returns completed, missing, out of order and ignored literal.
2. **Replay guard (S1).** An in-crate test that replays scripted transcripts through `tool_turn_with` with the `Fake` server and a real `Dispatch` lab: a good run, skipped pytest, wrong commit message, commit before `propose_batch`. CI-safe.
3. **Live harness (S1).** An `#[ignore]` test against local Ollama: model from an env var, tasks (a)-(c), N runs, prints a score and latency table. Throwaway repo outside `/tmp`, created and removed by the test.
4. **Baseline (S1).** The first table for the three C2 models plus `qwen3:14b-nothink` (informative for C6), recorded in the PR.
5. **HTTP 400 disambiguation (S2).** "Tools refused" vs other 400s; the other case shows a visible notice, never a silent retry.
6. **Per-model profiles and `think` (S3).** Built-in table keyed by model family (prompt addendum, round limit, `think`, reminders), seeded only with values the evaluation supports.
7. **Step checklist (S4).** Before ending a turn, compare what the user asked or the answer claims against the tool exchanges shown ("claimed vs shown"), with one reminder. Measured.
8. **#442 follow-ups (S5+).** Small separate PRs in `next_step` (courtesy closings, emoji tail, `Finish::Length`, loop tests).

### Out of Scope

- A user-visible score or feeding the tool mark (#430) (C5).
- A `config.toml` profile override (C7).
- Reasoning off as a global default (C6).
- llama.cpp `think` control and llama.cpp live runs (llama.cpp keeps today's behaviour; the replay guard covers its loop only where shared).
- A transcript recorder in the app; fixtures are curated by hand (C4).
- Fine-tuning, model downloads, or changes to CLI engines.

## Capabilities

### New Capabilities

- `local-model-eval`: scenarios, the scorer, the replay guard, the live harness (models, tasks, runs, pass rule, table), the throwaway-repo rule and the fixture policy (C2-C5).

### Modified Capabilities

- `chat-engines` (S2): "Ollama native tools" — "degrade silently to a text turn" applies only to a confirmed tools refusal; any other 400 gives a visible notice.
- `chat-engines` (S3): ADDED "Per-model profiles" and "Reasoning control per model"; "Ollama round limit with coding tools" may take a per-profile limit only if measured; "Reasoning events" notes turns sent with `think` off.
- `chat-engines` (S4, S5): "A next step announced in prose gets one reminder" gains the checklist reminder and the #442 cases.

S1 modifies no existing capability.

## Approach

1. **S1 measure.** Pure scorer in core; replay test in-crate because the `Fake` helpers are `pub(super)` (src/runtime/chat/ollama_tests.rs:105-190) and `TurnDispatch` is private (src/runtime/tools/host.rs:367). Design decides whether the live test is in-crate or in `tests/ollama_live.rs` (the latter needs a public `Dispatch` constructor).
2. **S2 safety.** Parse the 400 body; only a tools-refusal message keeps the silent no-tools retry (ollama.rs:82, 115-118, 245).
3. **S3 tune.** Add `think` to the Ollama body per profile (today it is absent, adapters.rs:164-167); profiles keyed by family prefix; each value justified by an S1 table.
4. **S4 checklist.** Hook in the no-calls branch of `rounds()` (ollama.rs:266-289), sharing the turn's reminder budget (`go_on_reminded`, ollama.rs:214).

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/eval*` (new) | New (S1) |
| `src/runtime/chat/ollama_tests.rs`, live test file | New tests (S1) |
| `tests/fixtures/ollama/` | New curated fixtures (S1, C4) |
| `src/runtime/chat/ollama.rs` (`Profile::rejected`, `rounds()`) | Modified (S2, S4) |
| `src/runtime/chat/adapters.rs` (`finish_body`) | Modified (S3) |
| `src/core/ai/chat/next_step.rs`, `prompts.rs` | Modified (S3-S5) |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Live runs are non-deterministic (no seed or temperature in the body) | High | 5 runs, 4/5 rule (C2); live is `#[ignore]`, never CI |
| Replay checks loop logic, not model quality | Certain | Live table is the acceptance; replay only guards regressions |
| Sending `think` to a non-thinking model returns 400, misread as tools refused | High if unguarded | S2 lands before S3 (C6) |
| A fixture leaks a path, name or file content into a public repo | Med | C4 curation; throwaway repo only |
| Live lab runs write, exec and git | Med | Throwaway repo outside `/tmp`, removed after |
| Checklist adds an 80-200 s round per turn | Med | Shares the single reminder; accepted only if C2 numbers rise |
| Owner's GPU and models differ from the issue's (qwen3:4b, 8 GB) | Med | C2 fixes the acceptance set; smaller models are informative |

## Rollback Plan

- S1 is a pure module plus tests: revert the PR.
- S2-S5 revert on their own; removing a profile row returns that model to today's generic behaviour.

## Dependencies

- Local Ollama with the C2 models (installed on the owner's machine, engram `sdd/local-multistep/live-env`).
- #442 for the S5 follow-ups.

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
- [ ] The replay guard fails each scripted bad transcript for the right reason and passes the good one.
- [ ] A baseline table exists for 3 models x 3 tasks x 5 runs.
- [ ] Every slice after S1 attaches a before/after table that raises the score and lowers no combination (C2).
- [ ] No 400 is ever handled silently unless it is a tools refusal.

## Slices (production lines; tests and fixtures do not count; each PR under 400)

| Slice | Boundary | Decisions | Size |
|---|---|---|---|
| S1 | Scorer + replay guard + `#[ignore]` live harness + baseline table. No loop, prompt, `think` or profile change | C1-C5 | 250-380 |
| S2 | 400 disambiguation with visible notice. No `think` sent yet | C6 | 80-180 |
| S3 | Profiles table + `think` per model, seeded from S1/S2 numbers | C6, C7 | 200-330 |
| S4 | Claimed-vs-shown checklist reminder in `rounds()` | C1, C2 | 220-360 |
| S5+ | #442 `next_step` follow-ups, one small PR each | C2 | 30-120 each |

## Settled questions (owner, 2026-11-15)

- **C8 Latency is measured, not a pass bar.** The score table shows turn time per run, and the median per model x task. A turn over 10 minutes counts as a failed run. Otherwise time does not decide acceptance; it is the evidence for S3.
- **C9 Noise.** 5/5 to 4/5 on a combination that still passes is not "lowering" under C2. Dropping below 4/5 is. When a result is borderline, the 5 runs of that combination are repeated before deciding.
- **C10 Asking back.** A clarifying question is a pass only in task (c) when the request is genuinely ambiguous. In tasks with every detail given (for example, the commit message), asking back is a failure ("Please provide a commit message" is the defect the evaluation exists to catch).
- **C11 llama.cpp later.** Out of this change. The scorer and the scenario format stay engine-neutral, so adding llama.cpp models later is configuration, not new code.

## Future direction (owner, 2026-11-15, out of scope here)

The owner, verbatim: "más adelante para poder mejorar el UX tenemos que hacer comprobaciones de modelos locales para poder comentar o dar a conocer que modelos pueden ser útiles y en que situaciones para ayudar a estos agentes".

A later change turns the evaluation into user-facing guidance: which local models are useful, and for which kinds of work (writing and testing code, editing existing code, read-only questions). That could be through the tool mark (#430), the README or the model picker. This change prepares it without shipping it:

- scenarios carry a task category, and the score table reports per model x category, so results can later be published as "good for X, not for Y";
- the score table is a stable, plain-text format with the date, the Ollama version and the hardware, so published guidance can say what it was measured on.
