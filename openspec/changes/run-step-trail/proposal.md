# Proposal: Run step trail (a stopped run keeps a readable trail of its steps)

Issue #771. Tool calls are not stored in `chat.db`, so a run that stopped at
`max_turns`, was cancelled or hit an engine error cannot be diagnosed after the
fact or after a restart.

## Scope

- Each tool call of the coordinator's turn (name, one-line argument summary,
  outcome `ok`, `refused: <reason>` or `error: <text>`) and each handoff (unit,
  verdict, reason) is one line of a per-run trail.
- The trail is stored with the run, survives a restart, and is shown with the
  stop report (turn cap, run timeout, cancel) and with an interrupted run named
  after a restart.
- Capped at 60 steps of 160 bytes; the count of dropped steps is kept and the
  report says "N earlier steps not shown". Lines are cleaned and token-redacted
  with the same helpers as the transcript (`clean`, `redact_tokens`).
- Never silent: a failed run save now says the step trail may be missing.

Not in scope: worker tool calls (they have their own bounded transcript view;
changed by Slice 4 below),
a trail line for judge steps (acceptance-judges S2 can push through
`Trail::call` / `Trail::handoff` unchanged), showing the trail of a run that
finished cleanly.

## Design

- Pure `core/ai/orchestra/trail.rs`: `Trail` (capped deque of lines, open calls
  paired by `CallId`), `to_text` / `from_text` (first line `cut N`, then one line
  per step), `shown(n)` for reports.
- Storage: one nullable `trail TEXT` column on the existing `runs` table, added
  with the same additive `ALTER TABLE ... ADD COLUMN` as `token` and `folder`
  (no schema version bump: older builds never select it and ignore it). A save
  carrying no trail leaves the stored one (`COALESCE`). No new table: the trail
  is one value per run, replaced whole, and the run row already has retention
  (50 runs), recovery and the interrupted-run read.
- App: `RunKeep.trails` per pane (cleared when a fresh goal starts). A tool event
  of the live coordinator turn or a handoff event updates it and, once the run
  has a store row, sends a `SaveRun` with no workers and the trail. Steps made
  before the first worker row ride with the run's first save.
- Display: `Stopped`, `Expired`, a cancelled `Ended` and the interrupted-run
  lines of a restart append `Trail::shown(12)` as system lines.

Alternative rejected: storing tool calls as chat messages. It would send them
back as history and change the messages schema, which the rows-are-never-stored
rule of the transcript forbids.

## Tasks

See `tasks.md`.

## Slice 4 (amendment, 2026-11-19)

Slices A, B and the first "Later" items shipped (#777, #780, #801). Note: the
code caps a line at 180 bytes (`trail::LINE_BYTES`), not 160 as said above.
This slice changes one earlier non-goal: worker tool calls now enter the trail.

### Intent

A run that went wrong is still hard to diagnose in two cases:

- A resumed run (`Prefix r r`) starts an empty trail, so its stop report
  says nothing about what the interrupted run did before the restart.
- Worker tool calls only live in the worker transcript view, which is in
  memory and lost on restart. Handoff lines say a worker failed, not which
  call failed. The original reason to leave them out ("they have their own
  transcript view") does not hold after a restart, and the restart case is
  the one #771 is about.

### Scope

1. Resumed run carry-over: the new run's trail starts with the interrupted
   run's stored trail, then one separator line `resumed from run <id>`. Same
   cap (60 steps); the carried `cut` count adds to the new one, so the report
   still says "N earlier steps not shown". The old row keeps its own trail.
2. Worker tool calls: one line per call, `worker <id>: <tool> <arg summary>
   -> ok | refused: <fixed reason> | error`, from the worker's
   `OrchestraEvent::Trace` `Call` / `Result` items. Arguments go through
   the same `clean`, `scrub`, `redact_tokens` and byte caps as coordinator
   calls.
3. Worker lines never push coordinator steps or handoffs out: they have their
   own share of the cap (design picks the split), and the dropped count is
   still shown.

### Security rule (applies to both items)

- The stored trail holds only lazarobox-generated text: tool name, the
  argument summary lazarobox builds, the outcome class, and a reason taken
  from a fixed set of lazarobox strings. Never a worker's raw tool output,
  a result `summary` (it can carry file, issue or stderr text), or raw `gh`
  stderr. An unknown refusal reason is stored as plain `refused`.
- The trail is display-only. Checked today: it is shown only through
  `push_system_line` (never stored as a message, never sent to a model,
  `src/core/ai/chat/mod.rs:1289-1291`); `resume_message`
  (`src/core/ai/orchestra/record.rs:126`) does not read it; compaction and
  summary ignore tool events (`src/core/ai/chat/compact.rs:319`,
  `src/core/ai/chat/summary.rs:322`). Slice 4 must keep it so: the carried
  trail is never added to the resume seed. Same rule as the gh stderr
  notices (#810/#812).

### Non-goals

- Judge steps (acceptance-judges S2).
- Persisting the full worker transcript.
- Merging trails of two old runs (a resume of a resumed run carries its
  predecessor's trail, which already holds the earlier one, under the cap).
- Changing what the coordinator's own outcome details store.

### Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Workers flood the 60-step cap | High | Separate worker share, own cut count |
| Third-party text reaches the store | Med | Outcome class + fixed reasons only; tests with hostile results |
| Trail later fed to a model | Low | Test that `resume_message` holds no trail line |
| Older build reads new text | Low | Keep `to_text` format: `cut N` then plain lines |

### Success criteria

- [ ] A resumed run that stops lists the old run's steps, the separator,
      then its own steps; it survives another restart.
- [ ] A failed worker call shows as one line naming worker, tool and outcome
      after a restart.
- [ ] No stored trail contains a tool result's text (test with a result
      carrying a token and a fake instruction).
- [ ] Coordinator steps are never evicted by worker lines.

### Capabilities

- New: `run-step-trail` (no spec exists yet in `openspec/specs/`): the
  shipped trail behaviour plus the Slice 4 resume and worker requirements.
- Modified: none.

### Rollback

Revert the Slice 4 PR(s). No schema change: the `runs.trail` text format
stays the same, so rows written by Slice 4 still read in older builds.

### Assumptions (owner asleep, no question round)

- Separator text `resumed from run <id>`; only the direct predecessor's
  stored trail is read.
- Issue #771 and PRs #777/#780/#801 were not re-read with `gh` (no shell in
  this phase); scope rests on the existing proposal, tasks and code.
- Error outcomes store `error` with no detail for workers.
- Proposal question round to run when the owner is back: cap split between
  coordinator and workers; whether a carried trail is shown at resume time
  or only on the next stop.

### Size

About 200-280 production lines: resume seed ~50, worker lines in `Trail`
~70, wiring from `Trace` events ~60, fixed reason mapping and cap split
~60. Under the 400 budget; can ship as 4a (resume) and 4b (workers).
