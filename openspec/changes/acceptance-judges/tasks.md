# Tasks: Acceptance judges (rule a run's change against the agreed standard, proportional to its scale, visibly)

Order: S1a -> S1b -> S2a -> S2b -> S3 -> S4, one PR each, stacked-to-main (each base = main after the previous PR merges). Each slice is under 400 production lines (tests, fixtures, snapshots, comments, blank lines and docs do not count). Strict TDD (`cargo test`): in every slice the RED test task comes before its GREEN task, through the public interface of the module; refactor last. MSRV 1.88: no `if let` guards in match arms (D18). Every slice ends with `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`, then `rm -rf target` in the worktree (AGENTS.md). Never run the full suite from a worktree under `/tmp`.

Decisions J1-J17 and D1-D18 are binding. J17 (2026-11-15) changed S3 only: the judges are the rows `judge-a` and `judge-b` of the `ODD + RDD roles` section of #765 (D13, D14), and J4, J10, J11 are amended in place; the delta specs `config-menu`, `ai-task-models`, `configuration`, `acceptance-judges` and the one scenario of `agent-orchestrator` follow it. J14 (unknown criteria may pass, visibly) lives in S1a (parser carries unverified criteria) and S2a (pass line and receipt). J15 (docs-only skips the tests wait) lives in S4. Both changed spec text on 2026-11-15 (this tasks phase): `specs/acceptance-judges/spec.md` (verdict format, visible-line forms, tests-evidence outcome, 4 new scenarios), `specs/orchestrator-handoffs/spec.md` (receipt: `unverified:` and `tests: no tests needed (docs only)`), `specs/agent-orchestrator/spec.md` (docs-only scenario in "A run ends by itself"). J16 (up to 9 workers on `high`) needs no task; it is a recorded risk.

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines (production) | S1a ~250, S1b ~220, S2a ~340, S2b ~160, S3 ~260, S4 ~330 (total ~1560) |
| 400-line budget risk | Medium: S2a (85%) and S4 (83%) are the tight ones; the rest Low |
| Chained PRs recommended | Yes (six PRs) |
| Suggested split | S1a -> S1b -> S2a -> S2b -> S3 -> S4 (stacked) |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main |

Decision needed before apply: Yes (the guard fires because the total passes 400 and chained PRs are recommended; the owner has already chosen stacked-to-main, so the question is only to confirm it per `ask-on-risk`; no slice needs `size:exception`)
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Medium

Per slice, any risk of passing 400? S1a no (~250, prompts `.md` ~70 not counted as production Rust). S1b no. S2a possible: ~340 estimate plus the J14 pass line (~10) and the consent fallback; checkpoint at task 7.9, and if the running total passes ~380, move the lines tasks 7.7/7.8 (D11) into S2b (S2b has ~240 of room). S2b no. S3 no (~260, 65%; J17 added the section header, `Set all ODD + RDD roles` and the second `Task` variant, about +50). S4 possible: ~330 including `docs_only` (~25); checkpoint at 14.6, lever: move `guidance.rs` hint and `CloseOffer` rendering into a follow-up if over 380.

### Suggested Work Units

| Unit | Goal | Likely PR | Notes |
|------|------|-----------|-------|
| S1a | Pure core: level, standard, norm cap, prompt, `parse_ruling`, `Finding.at`, `Lens::Judge` | PR 1 | base main; no runtime change; prompts `.md` included |
| S1b | Pure core: `agree`, `changed`, `Review::judged/rejudged/contested`, receipt section, `ApprovalAsk::JudgeConsent` | PR 2 | base main after PR 1; still no runtime caller |
| S2a | Runtime hook: gate, consent, judges in `conduct`, lines | PR 3 | base main after PR 2; judges use `review` assignment |
| S2b | Correction lines, re-judge, exhausted line | PR 4 | base main after PR 3 |
| S3 | `Task::JudgeA/JudgeB`, slots, `ODD + RDD roles` section (header, `Set all ODD + RDD roles`, 2 rows), `resolve_judges`, notices, norms I/O | PR 5 | base main after PR 4 |
| S4 | Tests evidence, docs-only (J15), `Hold`, `CloseWait`, Enter/Esc, README | PR 6 | base main after PR 5 |

## S1a Pure core: level, standard, prompt, verdict parser (~250 lines)

Files: `src/core/ai/orchestra/judge.rs` (new, ~240), `src/core/ai/orchestra/rdd.rs` (`Finding.at`, `Lens::Judge`, ~25), `src/core/ai/chat/prompts.rs` (`judge_builtin`, ~15), `prompts/{analyst,programmer}/judge.md` (new, not counted), `src/core/ai/orchestra/mod.rs` (`pub mod judge`).

### 1. Verify first (read-only; record each result under the task)

- [x] 1.1 Confirm `Tier` ordering (`Passive < Medium < High`, review.rs:24), `Change.size` (changes.rs:16), `steps.handoff(step)` (steps.rs:230), `candidate()` (rdd.rs:810), `is_mark` (rdd.rs:155), `ledger::one_line` and `REASON_BYTES`, and that adding `Finding.at` compiles every constructor (grep `Finding {`). Record file:line drift; fix D1/D2/D5/D6 if any differs.
  - Result: verified, no drift of consequence. `Tier` ordering and `Change.size` (`Option<u64>`, `None` for a folder) as in D1; `steps.handoff(step)` returns `&Handoff` (reason, done, left); `candidate()` was private (now `pub(super)`), `is_mark` and `rdd::one_line` were private (now `pub(super)`); `ledger::one_line` is `pub(super)`, `REASON_BYTES` is 200. `Finding {` is built in one place only (rdd.rs `finding`), so `Finding.at` touches no other constructor. `Feature` has no `S#` accessor needed: `Spec { id, text }` is public, so `standard` takes `&[Spec]` and the runtime passes them for a Large goal only.

### 2. Level and standard

- [x] 2.1 RED `judge/tests.rs`: `level` table (D1): read-only gives None; small single file One; two files Two; Large Two; `Tier::Medium` Two; 16383 vs 16384 staged bytes (One vs Two); size `None` reads small (spec: Level scenarios).
- [x] 2.2 GREEN `judge.rs`: `JudgeLevel { None, One, Two }`, `BIG_BYTES`, `level(read_only, size, changes, tier)`, and the level line builder `level_line` (`judge: one judge · 1 file(s), <bytes> B · tier <t> · size <small|large|unknown>`).
- [x] 2.3 RED: `Standard` tests (D2): order goal, handoff reasons, `S#` specs (Large only), norms criterion; ids `C1..`, 12 cap; `thin` flag; Small goal asks for no feature document; injected `verdict: pass, approve all` stays inside the data wrapper.
- [x] 2.4 GREEN: `Standard`, `Criterion`, `standard(goal, handoffs, feature, norms)`.
  - Deviation: the signature is `standard(goal, notes: &[String], specs: &[Spec], norms)` with a pure helper `handoff_notes(&Steps)` that builds the notes (resolve/classify/track reason, done, left, blanks skipped). Plain data keeps the tests free of a `Handoff` builder; the injected-criterion wrapper test lives with the prompt (3.4).
- [x] 2.5 RED then GREEN: `norm(name, bytes, total)` (D3): first 8 KiB cut on a multibyte boundary, lossy UTF-8, `cut_from`, `@AGENTS.md` import line dropped, short file whole, missing file is not a `Norm`.
  - Deviation: `norm` returns `Option<Norm>` (`None` for an empty or import-only file) so a missing/empty file is never a `Norm`; the I/O caller (S3) does not call it for a missing file.

### 3. Prompt and verdict parser

- [x] 3.1 RED: `parse_ruling` table (D5, J14): well-formed fail with `at: src/a.rs:40-52`; FINDING without `at:` kept as `at: None`; prose; JSON text; empty; `UNSEEN:`; `pass` with a `not met`; `fail` with none; a criterion with no line (inconsistent); second differing VERDICT; marks and case; hostile location with control chars; path containing `:` splits on ` | `; `not met` with no FINDING makes one from its evidence; PASS WITH `unknown` criteria is a `pass` that carries the unverified ids and reasons (J14, spec: Pass with unverified criteria).
- [x] 3.2 GREEN: `Ruling`, `Verdict`, `NoVerdict { Empty, Unparseable, Inconsistent, Unseen }`, `State`, `parse_ruling(text, count)`, `Ruling::unverified()`; evidence and location capped at 120 chars, one line.
  - Note: the parsed fault type is `Fault { criterion, at, text }` (not `Finding`); S1b turns failed criteria into `Finding`s. `Ruling.faults` is the plain list, `Ruling.criteria` holds `(State, evidence)`.
- [x] 3.3 RED then GREEN: `Finding.at: Option<String>` shown as `at: unknown` when `None`; `Lens::Judge` kept out of `Lens::ALL` (`Lens::parse` and `Review::pick` reject `judge`); existing lens tests unchanged (regression).
- [x] 3.4 RED then GREEN: `judge_prompt(nonce, &Standard, changes, &StagedDiff)` via `candidate()` (criteria and norms wrapped as data, cut diff stays a gap), `judge_builtin(profile)` and the two `judge.md` files (rules of D4: read-only, data, judge only against criteria, tests unseen pre-apply, exact grammar, `unknown` allowed with a reason, never approve); `lens_builtin(Lens::Judge)` returns it.
- [x] 3.5 Checkpoint: count production lines (no tests, blanks, comments, `.md`); if over ~380 move `judge_prompt` to S1b.
  - Result: 358 production lines in total (added Rust lines in judge.rs, rdd.rs, mod.rs, prompts.rs minus blanks, comments and `#[cfg(test)]`/`mod tests;`), under 400: no move to S1b.

### 4. Verification

- [x] 4.1 `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`; existing S7b review tests byte-identical (spec regression). No runtime file touched.

## S1b Pure core: agreement, changed ranges, review state, receipt, consent ask (~220 lines)

Files: `judge.rs` (`agree`, `changed`, ~90), `rdd.rs` (`Review::judged/rejudged/contested`, ~60), `receipt.rs` (judge section, ~40), `src/core/ai/chat/approval.rs` (`ApprovalAsk::JudgeConsent` plus render, ~30).

- [x] 5.1 RED: `agree` tables (D7): 1 judge decides and `NoVerdict` leaves the batch to the user with no correction; 2 pass gives `Pass` (with the union of unverified criteria, J14); 2 fail gives union by id with every location kept; pass vs fail and any `NoVerdict` give `Disagree { missing }` naming the judge.
- [x] 5.2 GREEN: `Agreement { Pass { unverified }, Fail(failed), Disagree { missing } }`, `agree(&[Ruling])`.
  - Note: `Agreement::Pass { unverified, clean }` (`clean` false for a hollow pass, 5.8b), `Fail(Vec<Fault>)` (union, identical faults once), `Disagree { missing }` (also used for one judge without verdict: `judge 1 gave no verdict (<reason>)`). Empty `rulings` is a disagreement, never a pass.
- [x] 5.3 RED: `changed(before, after)` (D12): same text gives none; middle edit gives `Lines(a, b)` from common prefix and suffix; create gives `Whole`; worker claim irrelevant (input is the two `StagedDiff`s only, spec: Worker claim is not trusted).
- [x] 5.4 GREEN: `Span { Lines, Whole, None }`, `changed`. Verify first: `StagedText.text` access (changes.rs:148) before writing it.
  - Note: `StagedText.text` is `pub` (changes.rs:148), verified. `Span::None` means the file changed but no range of the new text names it (lines removed, or file dropped from the batch); an unchanged file is not listed (empty vec renders `changes: none`).
- [x] 5.5 RED: `Review` tests: `judged` turns failed criteria into `Finding`s (lens `Judge`, severe, caused, observed) that enter `blockers()`; contested indexes are open but not correction targets; judge plus lens blockers form one `begin_correction()` counted once; `rejudged` clears only re-passed criteria; `conclude()` is `BlockersOpen` on contested.
- [x] 5.6 GREEN: `Review::judged(agreement, rulings)`, `rejudged`, `contested`, `blockers()` and `open()` changes.
  - Note: `judged(&Agreement, &[Ruling])` once, after consent and before the correction; `rejudged(&[Ruling]) -> Result<Vec<usize>>` returns the judge targets still open (the runtime joins them with the validator's lens result for `finish_correction`) and keeps the round; `set_judging(Judging)` carries what the receipt needs (consent, level line, models, standard, independence); `rounds()`, `contested()`.
- [x] 5.7 RED then GREEN: receipt judge section (orchestrator-handoffs): consent granted/declined, level inputs, each judge engine and model and verdict, criteria with id, state and evidence, `criteria: thin`, `unverified: <id> <reason>` (J14), locations or `at: unknown`, agreement outcome, `judges not independent`, `norms cut at 8 KB`, both rounds, `tests: not seen before apply`; a receipt whose text says `approve all` stays wrapped data.
  - Note: the judge section is appended to the receipt `done` field (`judge_lines` in receipt.rs): agreement first per round, then judges, criteria and `C<n> at: <loc>` lines. The tests line is a constant `tests: not seen before apply`.
- [x] 5.8 RED then GREEN: `ApprovalAsk::JudgeConsent { run, text }` renders as one clean line (no control chars, text names level rule and judge models).
  - Note: `ApprovalAsk::JudgeConsent { run, text }` (chat-own ask, never routed by the mode), `JUDGE_KEYS`, prompt in `app/run.rs`, pure `judge::consent_text(models)` (level rule plus models, one clean line; a hostile model name keeps only its first line).
- [x] 5.8b RED then GREEN (from S1a verify W2, W4): `Standard.dropped` (criteria past the cap) reaches the receipt (`criteria: <n> not judged (cap 12)`); `agree` and `Review::judged` never let a pass read as clean when it carries a `Fault` on a criterion marked `met`, or when every criterion is `unknown`: such a pass is kept but flagged (`pass, unverified`) in the receipt, and a judge that marks every criterion `unknown` is not counted as a clean pass.
  - Note: `Ruling::hollow()` is a pass that carries any Fault or whose criteria are all `unknown`; `agree` gives `Pass { clean: false }`, the receipt says `agree: pass, unverified`, and `Review::conclude()` is `Incomplete` (reason `a judge passed with nothing verified`) for a hollow pass in the latest round. Broader than the W2 wording (a fault on a met criterion): any fault on a pass is flagged.
- [x] 5.9 Verification: `cargo test`, clippy, fmt; lens-only reviews byte-identical when the level is None (regression); no runtime caller yet.

## S2a Runtime hook: gate, consent, judges in `conduct`, lines (~340 lines)

Files: `src/runtime/tools/host/review.rs` (gate `before_batch_with`, ~50), `src/runtime/tools/host.rs` (consent at authorize, ~40), `src/runtime/tools/orchestra.rs` (`OrchHandle::judge_level`, `judges`, ~25), `src/runtime/orchestra/review.rs` (`start_review`, `conduct` judges, ~170), `src/runtime/orchestra.rs` (~25), `src/core/ai/orchestra/judge.rs` (verdict line builders, ~30).

### 6. Verify first (the [I] items; record each result, fix D10/D11 or spec alignment 6 if wrong)

- [ ] 6.1 [I] How `Live` exposes the run id to the coordinator's host scope for D10 consent: read `host.rs:1199` (`OrchCall::StepHandoff` authorize), the `TurnScope` and `Live`, and decide how `judges(run)` gets the run id and where `Live.judges` is stored. Record the exact call path. If the run id is not reachable at the authorize step, the fallback (ask at the first judged batch through the worker scope, D10) becomes the primary path: update D10 and the task 7.x below.
- [ ] 6.2 [I] Whether the app keeps non-ASCII (`·`, `›`) in review lines: read `OrchestraEvent::Review` rendering and any sanitizer in `src/app/` plus guidance tests (guidance.rs:249). If it sanitizes, build the lines with the sanitizer's accepted separator and record the form; the spec text keeps `·`, so note the deviation under spec alignment 6.
- [ ] 6.3 Read `before_batch_with` (host/review.rs:78), `consent()` (:160), `start_review`/`conduct`/`finished()` (runtime/orchestra/review.rs) and the test seams (`diff`, `restage`, :949, :970); record any signature drift against D8/D9.

### 7. Gate, consent, judges

- [ ] 7.1 RED: gate tests (spec: The gate is wider than before; Read-only goal gets none): low-tier one-file batch by a strong author runs one judge; read-only goal runs none and asks no consent; lens review keeps its own gate and ask.
- [ ] 7.2 GREEN: `before_batch_with` runs the review when `cheap || tier >= High || level != None`; `OrchHandle::judge_level` and `judges(run) -> BoxFuture<JudgeConsent>`; `review(..)` gains `judges: JudgeLevel`; judge-only review is `Consent::Granted` plus `no_lenses_needed()`.
- [ ] 7.3 RED: consent tests (D10): asked once over three batches; declined gives `not judged` on every batch, receipt `consent: declined`, one decline line; cancelled ask records nothing and the next judged batch asks again; fallback ask for a run whose authorize never closed; lens consent and judge consent never answer each other (spec: Declining the lens consent does not cancel the judges).
  - Note (S1b verify): `JUDGE_KEYS` today reads `y judge the changes · n skip the judges`; it must also say what Esc does, and a cancelled ask (Esc, expiry) must differ from a declined one: cancelled records nothing and asks again, declined is `not judged`.
- [ ] 7.4 GREEN: ask at authorize (per 6.1) and fallback at first judged batch; `Live.judges: Option<JudgeConsent>`; `before_batch` waits on `judges(run)` with the worker paused.
- [ ] 7.5 RED: `conduct` tests with the existing seams (D8): level 1 spawns one read-only `Role::Reader` judge (toolset has no write tool and no `run_command`); level 2 spawns two over the same diff; judges and lenses start together and one `finished()` wait covers all ids; a refused judge is `NoVerdict(refused)` and does not halt the review; the orchestrator's `spawn_agent` with kind `judge` is refused; cloud-diff notice shown whatever the consent; high change gives one correction worker for lens blockers and failed criteria, round counted once (spec: High tier runs both).
- [ ] 7.6 GREEN: judges in `conduct` (judges use the `review` assignment until S3, same not-independent line when both share a model); `Review::judged` after lens reports; refuter only on inferred lens blockers.
- [ ] 7.7 RED: lines (D11, J14, spec: visible line): level line before any judge; thin-criteria line; per judge `pass · <m> criteria met`, `pass · 2 met, 1 unverified (C3: tests not seen)`, `fail · k of c criteria not met`, `no verdict · <reason>`; one `<id> not met · at: <loc> · <evidence>` per criterion (max 8 per judge then `+k more in the receipt`); `judges agree: pass` (with unverified union), `judges disagree · judge 1 pass, judge 2 fail · missing: ...` with no correction started; `verdict()` adds open judge criteria with locations; batch still waits for `y` after all pass.
- [ ] 7.8 GREEN: line builders in `judge.rs` and emission through `RunManager::say` (`OrchestraEvent::Review`).
- [ ] 7.8b RED then GREEN (from S1a verify W2, W4): transcript line when `Standard.dropped > 0` (`judge: <n> criteria not judged (cap 12)`); the pass line (J14) never reads as a clean pass when the ruling carries Faults on met criteria or every criterion is `unknown`: it says `pass, nothing verified` or names the contradiction.
- [ ] 7.8c RED then GREEN (from run-step-trail, archived): each judge spawn and its verdict enter the run step trail through `Trail::call` / `handoff`, display-only like the other trail lines (run-step-trail D14).
- [ ] 7.9 Checkpoint: production line count; if over ~380 move task 7.7/7.8 lines to S2b.

### 8. Verification

- [ ] 8.1 `cargo test`, clippy, fmt; lens-only high and cheap-author reviews byte-identical when level is None.
- [ ] 8.2 [manual] Live check on the real binary per `skills/run-lazarobox/SKILL.md` (isolated home and tmux session, never the user's config): a one-file write goal shows the consent once, the level line, one judge result with `at:` or an `unverified` pass, and the batch waits for `y`; decline once and confirm `not judged`; a read-only goal shows no judge line and no consent. Record the transcript lines seen.

## S2b Correction lines, re-judge, exhausted rounds (~160 lines)

Files: `src/runtime/orchestra/review.rs` (~160); `judge.rs` line builders if not placed in S2a.

- [ ] 9.1 RED: correction tests (D12, spec: Corrections say what they change): `correction 1/3 · fixes C2 · at: src/a.rs:40-52` before; `correction 1/3 done · changes: src/a.rs 44-48` after, from `changed()` over the re-read `diff_text` (use the S2a seams); a worker text claiming `src/b.rs` is ignored; a no-op gives `changes: none`; an unknown range gives `changes: <file>`.
- [ ] 9.2 GREEN: target lines, `diff_text` re-read, `changed` lines; `r` is the worker's round count after the increment.
- [ ] 9.3 RED: re-judge (D8 step 9): after a correction the judges that failed rule again and show their own lines; the receipt lists both rounds; lens targets still use the validator.
- [ ] 9.4 GREEN: fresh run of the failing judges for judge targets, `Review::rejudged`.
- [ ] 9.5 RED then GREEN: exhausted rounds: `judge: rounds exhausted (3) · open: C2 · missing: <what>`, receipt `correction: limit reached (3 rounds)`, batch reaches the user with `C2` open.
- [ ] 9.6 Verification: `cargo test`, clippy, fmt. [manual] per `skills/run-lazarobox/SKILL.md` (isolated home): provoke a failed criterion (a goal whose change breaks an explicit criterion) and read the target line, the `changes:` line and the re-judge verdict; note that a weak local judge may give `no verdict` (record it, not a defect).

## S3 `ODD + RDD roles` section, `judge-a` / `judge-b`, `resolve_judges`, notices, norms I/O (~260 lines)

Files: `src/core/ai/config.rs` (`Task::JudgeA/JudgeB`, ids, tables, `ROLES`, `resolve_judges`, roles aggregate, ~75), `src/core/menu.rs` (section title, `Set all ODD + RDD roles`, two rows, picker and Set-all handling, ~50), `src/core/ai/chat/prompts.rs` (~5), `src/core/ai/orchestra/run.rs` (`review_notices` per judge task, `set_judges`, ~40), `src/app/orchestra.rs` (roster with `resolve_judges`, ~40), `src/runtime/orchestra/review.rs` (spawn tasks, norms read, ~60). Scope limit (J17): only the section header, its Set-all row and the two judge rows; the other roles of #765 and the sub-headers are a later change.

### 10. Verify first

- [ ] 10.1 Verify D14 for the previous schema: load a config with `[ai.tasks.judge-a]` and `[ai.tasks.judge-b]` through the schema of `main` before S3 (the build of the S2b parent) and record: earlier keys load unchanged and the notice `unknown key: ai.tasks.judge-a` appears as a visible configuration problem, not silently. If the notice does not appear, fix the configuration spec scenario "Older build reports the table and keeps working" and D14, and record under this task.
- [ ] 10.2 Verify `Task::ALL` consumers (grep `Task::ALL`, `Task::Orchestrate as usize == 5`, config.rs:452, :3743, menu.rs:594, app/orchestra.rs:825, run.rs:497) and list the exact tests that hard-code 7 or `ALL[..5]` (the `ALL[..5]` test stays: `SET_ALL` is unchanged). Verify the menu renderer: `Row::Title` inside a tab (menu.rs:1178), navigation passing over a title, and the popup sizing rule with the taller Models tab; record whether the popup height follows the tallest tab.

### 11. Implementation

- [ ] 11.1 RED: `Task::ALL.len() == 9`, `JudgeA as usize == 7`, `JudgeB as usize == 8`, `Orchestrate as usize == 5`, ids `judge-a` / `judge-b`, tables `ai.tasks.judge-a` / `ai.tasks.judge-b`; both parsed, absent means unassigned, wrong type flagged by key; `SET_ALL` still the five tasks; `Task::ROLES == [JudgeA, JudgeB]`; untouched file gains no table (byte for byte); `builtin(profile, JudgeA | JudgeB)` is `judge_builtin`.
- [ ] 11.2 GREEN: the two variants at indexes 7 and 8, `slots: [Assignment; 9]`, ids, tables, `ROLES`, builtin arms.
- [ ] 11.3 RED then GREEN: `resolve_judges` table (J10, J11, J17): `judge-a` own, else `review`, else `general`; `judge-b` own, else `review` when set and different from `a`, else `a` with `independent = false`; an own `judge-b` equal to `a` is not independent; roles aggregate (`(not set)`, shared value, `(mixed)`).
- [ ] 11.4 RED then GREEN: menu: row order `... simple`, title `ODD + RDD roles`, `Set all ODD + RDD roles`, `judge-a`, `judge-b`, `Hugging Face`; the title is not selectable and navigation passes over it; Enter opens the task picker with `(default)` first; unassigned reads `(default)`; no `not recommended` mark; `Set all ODD + RDD roles` writes only the judges; `Set all tasks` writes no judge; `TestBackend` plus insta snapshot: popup size unchanged with the section (config-menu spec).
- [ ] 11.5 RED: second-judge rule (D13, J10 table): judge 1 on `Task::JudgeA`; judge 2 on `Task::JudgeB`; when `resolve_judges` says not independent, the line `judge: judges are not independent · both on <engine › model>`; notices evaluated per judge task (local engine says local models are unreliable as judges, same model as author warns, diff leaving the machine); an unusable judge model ends `refused`.
- [ ] 11.6 GREEN: `Run::set_judges`, app roster, `review_notices(.., task)` once per distinct judge task, spawn tasks in `conduct`.
- [ ] 11.7 RED: norms read (D3 I/O): `AGENTS.md` then `CLAUDE.md` from the run origin; skips a symlink and a missing file; `take(8 KiB)`; once per run kept in `Live`; one cut line `judge: norms cut at 8 KB · <file> is <n> KB`; 3 KB `AGENTS.md` and no `CLAUDE.md` gives no cut line and no error.
- [ ] 11.8 GREEN: norms read on the blocking pool; standard rebuilt with norms.
- [ ] 11.9 RED then GREEN (from S1a verify W3): norms notices. (a) The J7 line builder `judge::norms_cut_line(&Norm) -> Option<String>` returning `judge: norms cut at 8 KB · <file> is <size> KB` from `Norm.cut_from`, shown once per run through `RunManager::say` (`OrchestraEvent::Review`) when the standard is built (spec: Long norms are cut with a notice). (b) A visible line for a norm file that is present but empty or import-only (`norm()` returns `None`): `judge: <file> has no norms of its own` , never silent. (c) Consideration: when the staged text is at or above `BIG_BYTES`, cut the norms to fit the prompt (the diff budget is what the norms leave), and say so in the same line.

### 12. Verification

- [ ] 12.1 `cargo test`, clippy, fmt; every `ALL`-length test updated and passing. Checkpoint: if the running production total passes ~380, move the norms I/O (11.7, 11.8, ~60) to its own follow-up slice (S3b).
- [ ] 12.2 [manual] Per `skills/run-lazarobox/SKILL.md` (isolated home, never the user's config): open Config, Models tab, confirm the section `ODD + RDD roles` after `simple` with `judge-a` and `judge-b` showing `(default)`, assign `judge-a`, use `Set all ODD + RDD roles`, and confirm the written `config.toml` in the isolated home gains `[ai.tasks.judge-a]` and `[ai.tasks.judge-b]` only then and that `Set all tasks` writes neither. Run a two-judge goal with `review` and `judge-b` unset and read the not-independent line.

## S4 Tests evidence before close, docs-only, norms docs (~330 lines)

Files: `src/core/ai/orchestra/judge.rs` or `orchestra/evidence.rs` (`is_test`, `docs_only`, `evidence`, ~120), `src/core/ai/orchestra/run.rs` (`seq`, `last_apply`, `tests`, `applied_paths`, `finish`, `next_turn`, `Hold`, `confirm_close`, ~90), `src/runtime/orchestra.rs` (`Hold` to `Wait(CloseWait)`, `OrchClose`, ~30), `src/core/ai/orchestra/event.rs` (~10), `src/app/orchestra.rs` (`CloseOffer`, `answer_close`, ~50), `src/core/ai/chat/mod.rs`, `guidance.rs` (hint, ~20); README (not counted).

### 13. Verify first (the [I] items and J15 inputs; record each result)

- [ ] 13.1 [I] The exact key-dispatch position for `answer_close`: read the app key handler (`src/app/`) order for `answer_gates_off`, cancel (Esc), the composer and the orchestra panes; decide where an Enter/Esc on an empty composer is taken so Esc does not also cancel a live turn (D17 says before `answer_gates_off` and cancel). Record the call chain and fix D17 if it cannot go first.
- [ ] 13.2 [I] Non-ASCII in the close line and hint: reuse the result of task 6.2; guidance tests assert ASCII (guidance.rs:249), so the hint is `Enter finish anyway | Esc keep working` and the `·` form is used only if the line path keeps non-ASCII.
- [ ] 13.3 Applied-file source for J15: find where `Run::applied` (run.rs) is called and whether the applied paths (relative, per batch) are available there (`Change.path`). Record how the path list is stored for the pure `docs_only` predicate. If paths are not available at `applied`, add them to its argument (note the extra lines in the checkpoint).
- [ ] 13.4 Verify `RunOutcome::facts` prefix `exit code 0 ` (tools/run.rs:185) and `Commands`/`Ran` shapes (run.rs:619, commands.rs:9).

### 14. Evidence and docs-only (pure)

- [ ] 14.1 RED: `is_test` table (D16): `pytest`, `cargo test`, `cargo nextest`, `npm run test:unit`, `python -m pytest`, `make test`, `./gradlew check`, basename contains `test` are hits; `cargo check`, `cargo build`, `npm run build`, `ls` are misses.
- [ ] 14.2 GREEN: `is_test(command)`.
- [ ] 14.3 RED: `docs_only(paths)` table (J15): all `.md`/`.markdown`/`.txt`/`.rst`/`.adoc` is true; files under `docs/` or `doc/` true (any extension); `README.md` plus `src/a.rs` false; empty list false (nothing applied is a separate outcome); case-insensitive extensions; `docs` as a substring of a name (`mydocs/a.rs`) false.
- [ ] 14.4 GREEN: `docs_only(&[path]) -> bool`.
- [ ] 14.5 RED: `evidence(last_apply, &tests, applied) -> Evidence { NothingApplied, DocsOnly, Passed(cmd), NoTests, Failed(cmd, why) }` (D15, J15): a test before the last apply does not count; the last test wins; `Denied` skipped; `Error` is a failure; `Ended` passes only on `exit code 0 ` prefix, pinned against a real `RunOutcome`; `exit code 1` gives `exit 1`; nothing applied gives `NothingApplied`; all-docs applied with no test gives `DocsOnly`; docs-only with a passing test still gives `Passed` (the passing line wins); mixed docs and code with no test gives `NoTests`.
- [ ] 14.6 GREEN: `Evidence`, `evidence`. Checkpoint: production lines; lever if over ~380 is deferring the guidance hint.

### 15. Run wiring

- [ ] 15.1 RED: `Run` tests: `ran()` records into `tests` (cap 8, run-wide, apart from `Commands`) only for `is_test`; `applied` increments `seq`, sets `last_apply` and stores applied paths; `finish` returns `FinishError::Evidence(gap)` while a gap stands, keeping `pending_summary`; `DocsOnly` and `NothingApplied` and `Passed` never hold; `next_turn` gives `NextTurn::Hold(gap)` for an `Open` run with nothing pending; `confirm_close` writes `tests: none ran, finished by the user` or `tests: failed, finished by the user` into the close report and the receipt ledger line; `DocsOnly` writes `tests: no tests needed (docs only)`; `Passed` writes `tests: <cmd> passed`.
- [ ] 15.2 GREEN: `Run` fields, `finish`, `next_turn`, `confirm_close`.
- [ ] 15.3 RED: runtime and app tests (spec: A run ends by itself, Tests evidence): `Hold` maps to `Next::Wait(OrchestraEvent::CloseWait { run, text })` so settle does not end the run; the line reads `no tests ran · Enter to finish anyway · Esc to keep working` and Ctrl+O is not yet available; Enter sends `Effect::OrchClose { run, finish: true }` and the run ends once; Esc drops the offer, keeps the run open and the check runs again at the next close; failed test line `tests failed · <cmd> · exit 1 · ...`; docs-only change ends by itself with no prompt; key order against cancel (per 13.1).
- [ ] 15.4 GREEN: `Hold` mapping, `CloseWait` event, `CloseOffer`, `answer_close`, hint in `chat/mod.rs` and `guidance.rs` (ASCII fallback per 13.2).

### 16. Docs and verification

- [ ] 16.1 README orchestrator section (not counted): judges, level rule, the `judge-a` / `judge-b` rows, norms cap, tests wait and its docs-only exception, J14 unverified passes.
- [ ] 16.2 `cargo test`, clippy, fmt; a run with nothing applied is never held (regression); lens-only reviews unchanged.
- [ ] 16.3 [manual] Per `skills/run-lazarobox/SKILL.md` (isolated home, tmux, never the user's config): (a) apply a code change and finish with no test: confirm the wait line, Esc keeps the run open, Enter closes with `tests: none ran, finished by the user` in the report; (b) apply a Markdown-only change: confirm `tests: no tests needed (docs only)` and no wait; (c) approve a `cargo test` `run_command` after the apply: confirm `tests: cargo test passed` and no wait. Confirm Esc here does not cancel a live turn (13.1).

## Overall acceptance (after S4)

- [ ] 17.1 Walk the proposal's Success Criteria against the merged slices: one judge small, two multi-file or `medium`+, none read-only; every finding and correction line has a location or `at: unknown`; disagreement, exhausted rounds and no verdict always visible; no recorded test shows a line before finishing; nothing approves a batch; gates green on each slice.
