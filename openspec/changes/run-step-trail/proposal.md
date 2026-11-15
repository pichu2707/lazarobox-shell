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

Not in scope: worker tool calls (they have their own bounded transcript view),
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
