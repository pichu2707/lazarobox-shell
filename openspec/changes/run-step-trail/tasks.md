# Tasks

Slice A, `feat/run-trail-core` (pure, nothing wired):

- [x] `Trail` type: cap, one line per step, call text and outcome cut apart so
      a long call keeps how it ended, text round trip, `shown` report,
      `absorb` of steps taken before the run was known.
- [x] `scrub`: best-effort masking of JSON members, `name=value`,
      `--name value`, headers, URL query values, curl `-u`, `sshpass -p` and
      `mysql -pVALUE` for secret-looking name segments; quoted values go whole.
      Handoff reasons are scrubbed too.

Slice B, `feat/run-step-trail` (stacked on A):

- [x] `runs.trail` nullable column, save with COALESCE, read on recovery.
- [x] Trail keyed by run id; steps of a fresh goal's turn wait until an event
      names its run (not the run the pane had before); a run with no worker
      still gets its store row; only the coordinator's turn feeds the trail.
- [x] Steps of a turn that has not named its run are held per turn, so a
      coordinator-only run ending after the next goal keeps its steps and goal.
      Only an end or stop may name an earlier turn's run (the run's end can
      arrive after the next goal: `Done` is sent before `turn_ended`, and
      swapping them would break the Undelegated/Waits notices that wait for
      an idle pane). Limits: a worker-less run that ends after the next turn
      already took steps of its own is misfiled, and held steps of a turn that
      never made a run are dropped, with a notice, two goals later.
- [x] Show the trail on stop (turn cap, timeout, cancel) and on interrupted runs.
- [x] Failed run save says the step trail may be missing; steps of a run that
      never started are not dropped silently.

Later:

- [x] Engine error of the coordinator turn with the run still open (the
      ipfing-like case): the trail is listed under the error.
- [x] Show the trail of a run that finished cleanly (listed on its end; once
      with a stop report; not for a replaced run).
- [x] A resumed run (Prefix r r) starts an empty trail: see Slice 4a below.
- [ ] Worker tool calls enter the trail: see Slice 4b below.
- [ ] Acceptance-judges S2: push judge steps through `Trail::call` / `handoff`
      (out of scope for Slice 4).

## Slice 4 (spec: `specs/run-step-trail/spec.md`, design: `design.md`)

Delivery: `delivery_strategy: ask-on-risk` (owner asleep, so `auto-chain` when
there is a risk), `chain_strategy: stacked-to-main`. 4a first, then 4b, one PR
each; 4b's base is main after 4a merges. PR budget: 400 production lines each
(tests, docs and comments do not count).

Rules for every task: strict TDD (each RED test task comes before its GREEN
task), MSRV 1.88 (no `if let` guards in match arms), English artifacts.

Decision D13 (recorded here and in `design.md`, orchestrator, owner asleep): a
report could show only worker lines and hide the last coordinator steps. Reports
list the last 12 coordinator-share lines and, separately, the last 4 worker
lines, so at most 16 lines are listed, in chronological order. The cut note
counts both shares. Spec scenarios: "Busy workers", "Cut note counts both
shares". It lands in 4b, because 4a has no worker lines.

### Slice 4a: resume carry-over (`feat/run-trail-resume`, ~55 production lines)

Spec: "Resumed run carries the predecessor's trail", "Display only, never sent
to a model". Design: D1-D5.

RED (write first, all fail):

- [x] 4a.1 `trail_tests.rs`: `carry` puts the old lines, then
      `resumed from run <id>`, then the new lines; the cut counts add (5 + 3
      says "8 earlier steps not shown"); with 60+ lines the oldest go first.
- [x] 4a.2 `trail_tests.rs`: chain A -> B -> C: C's text holds A's lines,
      `resumed from run A`, B's lines and `resumed from run B` (D4). A
      predecessor with an empty trail adds no separator.
- [x] 4a.3 `app_tests/orchestra_trail.rs`: a resumed run that stops lists the
      old lines, the separator, then its own steps; its first `SaveRun` already
      carries them; starting the resume pushes no trail line (D5); the
      predecessor row's stored trail is unchanged; a restart reads the carried
      lines, separator and own steps; a predecessor with no trail gives no
      separator.
- [x] 4a.4 `record_tests.rs`: `resume_message()` of a record whose trail holds
      a marker and a token contains neither.
- [x] 4a.5 `app_tests/orchestra_resume.rs`: after `Prefix r r` with a marker in
      the stored trail, the resume turn's `ChatRequest` and the next
      continuation request hold no marker; the stop report does.

GREEN (each follows its RED):

- [x] 4a.6 `trail.rs`: add `Trail::carry(&mut self, earlier: Trail, from: u64)`
      (push the separator on `earlier`, then `absorb`). Makes 4a.1, 4a.2 pass.
- [x] 4a.7 `run_store.rs`: `ensure_kept` resolves `(id, trail)` in the
      `supersedes` branch (`resuming` / `awaiting` / `resume_of`; for
      `resume_of` read from `interrupted` before `retain`) and seeds
      `trails[new].carry(old, old_id)`.
- [x] 4a.8 `run_store.rs`: in `keep_worker` and `save_trail` call `ensure_kept`
      before reading `trails[run].to_text()` (D3). Makes 4a.3 pass.
- [x] 4a.9 Confirm 4a.4 and 4a.5 pass with no production change (the trail is
      never read by `resume_message` or a request). If one fails, fix the
      leak, not the test.

Close-out:

- [x] 4a.10 README.md and README.es.md: where they describe the step trail,
      say a resumed run keeps the old run's steps, then a
      `resumed from run <id>` line (same sections, same order; code and
      printed texts unchanged).
- [x] 4a.11 CHANGELOG `[Unreleased]` entry (resumed run keeps the interrupted
      run's step trail).
- [x] 4a.12 `cargo test`, `cargo clippy --all-targets -- -D warnings`,
      `cargo fmt --check` all green (not from a worktree under `/tmp`).
- [ ] 4a.13 `rm -rf target` in the worktree. Open the PR, merge after green CI
      and clean review.

Order: 4a.1-4a.5 (parallel, independent test files) -> 4a.6 -> 4a.7 -> 4a.8 ->
4a.9 -> 4a.10-4a.11 (parallel) -> 4a.12 -> 4a.13.

### Slice 4b: worker tool calls (`feat/run-trail-workers`, ~185 production lines)

Base: main after 4a merges. Spec: "Worker tool calls enter the trail", "Only
generated text is stored", "Worker lines never evict coordinator steps",
"Reports keep the last coordinator steps visible", "Stored format stays
readable by older builds", "Display only". Design: D6-D13.

RED (write first, all fail):

- [ ] 4b.1 `trail_tests.rs`: 100 worker lines evict no coordinator line or
      handoff; `worker_cut` and `cut` count apart; `to_text` writes the sum
      (D10, D11).
- [ ] 4b.2 `trail_tests.rs`: a legacy-style parse (header, then at most 60
      lines) of an 80-line text gives a correct total; a `from_text` round
      trip keeps the kinds (D11).
- [ ] 4b.3 `trail_tests.rs`: the same `CallId` for the coordinator and two
      workers resolves to each one's own line (D9).
- [ ] 4b.4 `trail_tests.rs`: hostile result (token, fake instruction, gh
      stderr text) gives `-> error` or `-> refused`, and the stored text holds
      none of it; an unknown denial gives plain `refused`; every `REASONS`
      key maps to its label, with and without the `denied: ` prefix (D7, D8).
- [ ] 4b.5 `trail_tests.rs` (D13): with 30 coordinator and 30 worker lines,
      `shown` lists the last 12 coordinator-share lines and the last 4 worker
      lines (16), in chronological order, and the cut note counts both
      shares. With no worker lines, `shown` is unchanged (12 lines).
- [ ] 4b.6 runtime host tests: reason drift, each source constant
      (`app::tool::DECLINED`, `CANCELLED`, `host::NOT_APPROVED`,
      `NOT_COMMAND`, the `tools are off` and `the worker already ended`
      strings, `chat::review::TOO_LARGE`, `not reviewed`) maps to its label.
- [ ] 4b.7 `app_tests/orchestra_trail.rs`: a failed worker call survives a
      restart (the interrupted line lists `worker 1: ... -> error`); a refused
      worker call stores its fixed reason; the stored `SaveRun` text of a
      hostile result holds none of its text; an `ok` worker result saves
      nothing by itself, `refused` and `error` save at once (D12); a call
      from an ended run or a run the pane does not own is not kept.
- [ ] 4b.8 `app_tests/orchestra_resume.rs` and `record_tests.rs`: with worker
      lines in the stored trail, `resume_message()` and the next requests
      still hold no trail line.

GREEN (each follows its RED):

- [ ] 4b.9 `trail.rs`: add `Kind` and `Step.kind`, per-kind `fit` shared by
      `push` and `absorb`, `WORKER_STEPS = 20`, `worker_cut`, and open calls
      keyed by `(Kind, CallId)`. Makes 4b.1, 4b.3 pass.
- [ ] 4b.10 `trail.rs`: `from_text` classification (`worker <digits>: ` prefix
      = worker line, else coordinator; whole stored cut into `cut`) and
      `to_text` writing `cut <cut + worker_cut>`. Makes 4b.2 pass.
- [ ] 4b.11 `trail.rs`: `worker_call`, `worker_result`, `REASONS` and
      `reason()` (`strip_prefix` plus iterator `find`, no `if let` guards;
      the result summary is only an equality key, never copied; `Error`
      stores `error`). Makes 4b.4 pass; 4b.6 passes once the constants match.
- [ ] 4b.12 `trail.rs`: per-share `shown` (last 12 coordinator-share, last 4
      worker, cut note counts both) (D13). Makes 4b.5 pass.
- [ ] 4b.13 `run_store.rs`: `keep_worker_step(id, run, worker, &item)` with the
      same guards as `keep_handoff` (`ended_runs`, `owns`); `Ok` updates
      memory only, `refused` and `error` send a `SaveRun` with the trail
      (D6, D12).
- [ ] 4b.14 `orchestra.rs`: in `App::on_orchestra`, call it from the `Trace`
      branch for `Call` / `Result` items before `item` moves into
      `chat.trace`; ignore Text and Notice items. Makes 4b.7 pass.
- [ ] 4b.15 Confirm 4b.8 passes with no production change.

Close-out:

- [ ] 4b.16 README.md and README.es.md: step trail sections say worker tool
      calls appear as `worker <n>: <tool> <args> -> ok | refused: <reason> |
      error`, the separate worker share, and that a report lists the last 12
      coordinator steps and the last 4 worker steps (same sections, same
      order).
- [ ] 4b.17 CHANGELOG `[Unreleased]` entry (worker tool calls in the step
      trail, per-share report).
- [ ] 4b.18 `cargo test`, `cargo clippy --all-targets -- -D warnings`,
      `cargo fmt --check` all green.
- [ ] 4b.19 `rm -rf target` in the worktree. Open the PR, merge after green CI
      and clean review.

Order: 4b.1-4b.8 (parallel, independent test files) -> 4b.9 -> 4b.10 ->
4b.11 -> 4b.12 -> 4b.13 -> 4b.14 -> 4b.15 -> 4b.16-4b.17 (parallel) -> 4b.18
-> 4b.19. 4b.9-4b.12 all touch `trail.rs`: sequential.

## Review Workload Forecast

| Slice | Est. production lines | 400-line budget risk | Chained PRs recommended | Decision needed before apply |
|---|---|---|---|---|
| 4a resume carry-over | ~55 | Low | Yes (2 slices, 4a then 4b) | No |
| 4b worker tool calls | ~185 (design ~165 + ~20 for D13) | Low | Yes (stacked on 4a merged to main) | No |
| Total | ~240 | Low | Yes | No |

Each slice is far under 400 lines. The split is kept because 4b depends on
nothing in 4a for correctness but both edit `trail.rs` and `run_store.rs`, and
independent revert is cheap. `delivery_strategy` is `ask-on-risk` with no risk
found, `chain_strategy` is `stacked-to-main`: no decision is pending.
