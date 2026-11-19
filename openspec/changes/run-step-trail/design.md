# Design: Run step trail, Slice 4 (resume carry-over, worker tool calls)

Baseline (shipped in #777, #780, #801): `core/ai/orchestra/trail.rs` (`Trail`:
one capped deque, `MAX_STEPS` 60, `LINE_BYTES` 180, `to_text` = `cut N` and then
plain lines), `runs.trail` stored with COALESCE, `RunKeep.trails` keyed by
`RunId` in `src/app/run_store.rs`, and reports through `push_system_line` only.
This design covers Slice 4 only. It ships as 4a (resume) and 4b (workers),
stacked to main.

## Technical Approach

Both items stay inside the existing seams. The pure logic goes in `Trail`
(carry, kinds, per-kind cap, fixed reasons). The wiring goes in `run_store.rs`
(where it is seeded and captured) and in one hook in `src/app/orchestra.rs`.
There is no schema change and no change to the text format. Nothing in the
trail enters a request: the trail is read only by the report functions.

## Architecture Decisions

| # | Decision | Rejected | Rationale |
|---|---|---|---|
| D1 | The seed happens in `ensure_kept`, in the same branch that resolves `supersedes` (`resuming` / `awaiting` / `resume_of`). It takes the old `RunRecord.trail`, which the recovery read already loaded. For `resume_of`, the trail is read from `interrupted` before `retain`. | Seeding in `resume_run` | The new `RunId` is unknown in `resume_run`. `ensure_kept` is the single place where the new run gets linked to the old row, so the link and the trail cannot drift apart. |
| D2 | `Trail::carry(&mut self, earlier: Trail, from: u64)` pushes the plain line `resumed from run <from>` onto `earlier` and then calls `self.absorb(earlier)`. | Appending after the steps of the new run | `absorb` puts lines first, so the order is always old steps, then the separator, then new steps, whatever the order in which the run is named, adopted or seeded. The carried `cut` adds to the new one. |
| D3 | `keep_worker` and `save_trail` call `ensure_kept` *before* they read `trails[run].to_text()` (today they read it before). | Leaving the order as it is | Without the reorder, the first save of a resumed run would leave out the carried lines. |
| D4 | Chain of resumes: only the direct predecessor's stored text is read. It already holds its own predecessor's lines and separator. The same per-kind caps apply, and the cut counts add. | Walking the `supersedes` chain | That is a proposal non-goal, and it would mean extra store reads. |
| D5 | The carried trail is shown only at the next report (stop, cancel, error, clean end, interrupted after a restart). | Showing it when the resume starts | The interrupted-runs list (`on_runs_reply`) already showed that same trail just before `Prefix r r`, so showing it again would only repeat it. The report paths need no change. |
| D5b | The separator line counts toward `MAX_STEPS` like any other coordinator-share line, so a long carried trail can push out its oldest lines and the separator itself, and the cut counts it. `take_resumed` seeds in every path that makes a run's `Kept` (`ensure_kept` and `adopt_trail`). | A separate allowance for the separator | One cap keeps the stored trail bounded; seeding in one helper keeps the link and the trail together. |
| D6 | Worker lines are captured in `App::on_orchestra` from the `OrchestraEvent::Trace` `Call` / `Result` items, before `item` moves into `chat.trace`. They go through a new `keep_worker_step(id, run, worker, &item)` with the same guards as `keep_handoff` (`ended_runs`, `owns`). | Capturing from runtime stream text or from `chat.trace` | `Trace` is the structured, app-side event. Text and Notice items are ignored. |
| D7 | Line format: `worker <n>: <tool> <args> -> ok` / `-> refused: <reason>` / `-> refused` / `-> error`. `<tool> <args>` uses the coordinator pipeline (`flat` → `scrub` → `line(.., CALL_BYTES)`). The prefix is added after scrubbing. | A separate summariser | The args are the `ToolCall.summary` that lazarobox's host builds (`call.argument()`, clipped), so scrubbing and capping are the same as for coordinator calls. |
| D8 | The outcome is built only from `ToolStatus` and a static table (below). The `Result.summary` is used only as an equality key and is never copied. `Error` stores `error` with no detail. | Storing a cleaned detail | Under the security rule, the result summary can carry file text, issue text or gh stderr. |
| D9 | Open calls are keyed by `(Kind, CallId)`, where `Kind` is `Coord` or `Worker(u32)`. | Keying them by `CallId` alone | Worker call ids can repeat across workers and coincide with coordinator ids. |
| D10 | Cap split: the coordinator share (calls, handoffs, separators) stays at 60, and the worker share is 20, so a trail holds at most 80 lines. An overflow evicts the oldest line *of the same kind* and counts it in that kind's cut (`cut`, `worker_cut`). | Splitting 60 as 40/20 | 40/20 would cut what a coordinator-only run keeps, and a shared pool would let workers evict coordinator lines. |
| D11 | `to_text` writes `cut <cut + worker_cut>` and then the lines in chronological order. `from_text` classifies a line that starts with `worker <digits>: ` as a worker line and every other line as coordinator, and puts the whole stored cut into `cut`. The `shown` wording is unchanged. | A second header line | An older build reads `cut N` and then plain lines. Its own 60-step cap drops the oldest extra lines and counts them, so the report stays correct. |
| D12 | A worker `Ok` updates memory only. `refused` and `error` save at once. Every other save (a worker row, a coordinator step, a handoff, the end) carries the whole trail. | Saving on every result | This bounds SQLite writes from busy workers. The failures are what a diagnosis needs, and they persist immediately. |
| D13 | `shown` lists the last 12 coordinator-share lines and, separately, the last 4 worker lines, so a report lists at most 16 lines, in chronological order. The cut note counts both shares: `cut + worker_cut` plus the lines of either share that were stored but not listed. Before 4b there are no worker lines, so a 4a report is unchanged (12 lines). | `shown(12)` over the merged list | With busy workers the last 12 lines could all be worker lines and hide the last coordinator steps, which are what a diagnosis starts from. Decided by the orchestrator (owner asleep), resolving the open question. |
| D14 | The args summary of a call is model-authored text (a commit message, a command, a glob or grep pattern, MCP args). It is stored cleaned, scrubbed, redacted and capped at about 110 bytes, but it is not lazarobox-built. The trail is safe only because it is display-only (`Trail::to_text` carries the same warning). | Treating the stored args as trusted text | Feeding the stored trail to any model would turn it into a prompt-injection route. Any future change that wants to do that must first replace the args with lazarobox-built text. |

### Fixed refusal reasons (`trail.rs`, `REASONS: &[(&str, &str)]`)

The key is the result summary after an optional `denied: ` prefix is removed.
The two strings must be exactly equal.

| Key (source constant) | Stored |
|---|---|
| `the user declined` (`app::tool::DECLINED`) | `declined` |
| `cancelled` (`app::tool::CANCELLED`) | `cancelled` |
| `the user did not approve it` (`runtime::tools::host::NOT_APPROVED`) | `not approved` |
| `that call is not a command` (`host::NOT_COMMAND`) | `not a command` |
| `tools are off` (`app::tool`) | `tools off` |
| `the worker already ended` (`app::tool`) | `worker ended` |
| `chat::review::TOO_LARGE` | `too large to review` |
| `not reviewed` (`host/review.rs`) | `not reviewed` |

Any other key is stored as plain `refused`. This includes jail refusals and
dynamic reasons. Core cannot import app or runtime constants, so the keys are
literals, and a drift test in the runtime host tests asserts that each source
constant maps to its label.

## Data Flow

    Prefix r r ─→ resuming[pane]=RunRecord{trail}
    first event of new run ─→ ensure_kept ─→ supersedes + Trail::from_text(old)
                                         └─→ trails[new].carry(old, old_id)
    Trace{Call|Result} ─→ keep_worker_step ─→ Trail::worker_call/worker_result
                                         └─(refused|error)→ SaveRun{trail}
    Stopped/Expired/Ended ─→ report_* ─→ shown(12) ─→ push_system_line (display only)
    resume_message / coordinator prompt ── never reads trails ──X

## File Changes

| Slice | File | Change | Prod lines |
|---|---|---|---|
| 4a | `src/core/ai/orchestra/trail.rs` | Add `carry`. | ~15 |
| 4a | `src/app/run_store.rs` | `ensure_kept` resolves `(id, trail)` and seeds; reorder in `keep_worker` and `save_trail`. | ~40 |
| 4a total | | | **~55** |
| 4b | `src/core/ai/orchestra/trail.rs` | `Kind`, `Step.kind`, `worker_call`, `worker_result`, per-kind `fit` (shared by `push` and `absorb`), `worker_cut`, `from_text` classification, `REASONS` + `reason()`, per-share `shown` (D13). | ~140 |
| 4b | `src/app/run_store.rs` | `keep_worker_step`. | ~35 |
| 4b | `src/app/orchestra.rs` | Call it in the `Trace` branch, keep its effects. | ~8 |
| 4b total | | | **~185** |

## Interfaces

```rust
impl Trail {
    pub fn carry(&mut self, earlier: Trail, from: u64);
    pub fn worker_call(&mut self, worker: u32, id: CallId, name: &str, summary: &str);
    pub fn worker_result(&mut self, worker: u32, id: CallId, status: ToolStatus, summary: &str);
}
pub const WORKER_STEPS: usize = 20;
```

## Testing Strategy (strict TDD, `cargo test`)

| Slice | Test | Where |
|---|---|---|
| 4a | `carry` puts the old lines, then the separator, then the new ones; the cut counts add; with 60+ lines the oldest go first. | `trail_tests.rs` |
| 4a | Chain A→B→C: C's text holds A's lines, `resumed from run A`, B's lines and `resumed from run B`. | `trail_tests.rs` |
| 4a | A resumed run that stops lists the old lines and the separator. Its first `SaveRun` already carries them. Starting the resume pushes no trail line. | `app_tests/orchestra_trail.rs` |
| 4a | **No model input:** `resume_message()` of a record whose trail holds a marker and a token contains neither. | `record_tests.rs` |
| 4a | **No model input:** after `Prefix r r` with a marker in the stored trail, the resume turn's `ChatRequest` and the next continuation turn's request contain no marker. The stop report does. | `app_tests/orchestra_resume.rs` |
| 4b | 100 worker lines evict no coordinator line or handoff. `worker_cut` and `cut` count apart, and `to_text` writes the sum. | `trail_tests.rs` |
| 4b | A legacy-style parse (header, then at most 60 lines) of an 80-line text gives a correct total. A `from_text` round trip keeps the kinds. | `trail_tests.rs` |
| 4b | The same `CallId` for the coordinator and two workers resolves to each one's own line. | `trail_tests.rs` |
| 4b | **Hostile result:** a `Result` summary with a token, a fake instruction and gh-stderr text gives `-> error` or `-> refused`, and the stored `SaveRun` text holds none of that text. An unknown denial gives `refused`. Every `REASONS` key maps to its label. | `trail_tests.rs`, `app_tests/orchestra_trail.rs` |
| 4b | Reason drift: each source constant maps to its label. | runtime host tests |
| 4b | A failed worker call survives a restart: the interrupted line lists `worker 1: ... -> error`. | `app_tests/orchestra_trail.rs` |
| 4b | D13: with 30 coordinator and 30 worker lines, `shown` lists the last 12 coordinator and the last 4 worker lines (16), in order, and the cut note counts both shares. | `trail_tests.rs` |

## Migration / Rollout

No migration is needed. The text format and the schema are unchanged. Rows
written by Slice 4 read in older builds. Each slice can be reverted on its own.
MSRV is 1.88, so there are no `if let` guards in match arms. `reason()` uses
`strip_prefix` and an iterator `find`.

## Open Questions

- [x] `shown(12)` could be all worker lines when workers are busy. Resolved by
  D13: 12 coordinator-share lines plus 4 worker lines, at most 16.
