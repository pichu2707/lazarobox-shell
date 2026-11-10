# Delta for Modal Input (agents overview)

## Base of this delta

This delta targets the owning spec `openspec/specs/modal-input/spec.md` as it stands on 2f20e9df. It adds requirements only. The prefix root binds `w t g b [ m c n q` (hinted) and `h j k l o s e a T W r u ?` (unhinted) (src/core/prefix.rs:230-275); the root hint is full, so the new key is unhinted, and `a` is the approval toggle, so the overview takes the capital, as `T` (tags) and `W` (worker transcript) did (prefix.rs:261-263). The view's keys follow LINKS mode ("LINKS mode", openspec/specs/modal-input/spec.md:426). Everything here is pending Q1 of the proposal (an overlay on a prefix key); if the owner picks the menu tab or a sidebar, this delta is rewritten.

Slice tag: [S1], except where marked [S2].

## ADDED Requirements

### Requirement: Agents overview key [S1] (pending Q1)
`Ctrl+Space A` (capital: `a` is the approval toggle) MUST open the agents overview (see agents-overview, The overview view) from any pane, a chat or a shell. It is a root leaf of the prefix table, left out of the hint (the root hint is full) and bound nowhere else; chords stay unique in every group. With the AI section off it MUST say `No AI section` and open nothing, unless a CLI agent row exists [S4] (assumption: CLI agents in panes need no AI section). While the worker transcript is open, the key MUST refuse with `Close the worker transcript first (Esc)` as `Prefix T` does today. No key of the sequence reaches a PTY.

#### Scenario: The key opens the view [auto]
- GIVEN a chat in tab 1 and a shell pane focused in tab 2
- WHEN Ctrl+Space then `A` are pressed
- THEN the overview is drawn over the focused pane and lists the chat

#### Scenario: No collision [auto]
- GIVEN the prefix tree
- WHEN the uniqueness test of every group runs
- THEN `A` appears once at the root and the root hint text is unchanged

#### Scenario: Lower case unchanged [auto]
- GIVEN a chat pane in `ask` mode
- WHEN Ctrl+Space then `a` are pressed
- THEN the approval mode toggles as before and the overview does not open

### Requirement: Agents overview keys [S1] (pending Q1)
While the overview is open: `1` to `9` and then letters MUST jump to their row (see agents-overview, Jump); Enter MUST jump to the selected row; `j`/`k` and Down/Up MUST move the selection; Esc, `q` and Ctrl+C MUST close the view and return to TERMINAL (Ctrl+C does not cancel a chat turn here). Every other key MUST be swallowed with the notice `Esc closes the agents list`. A held key MUST NOT pick a row. No key reaches a PTY or a composer. An approval prompt in the focused chat keeps its place and its keys, answered before the view's (assumption, as the worker transcript does).

#### Scenario: Jump by row key [auto]
- GIVEN the view with three rows
- WHEN `2` is pressed
- THEN the second row's pane is focused and the mode is TERMINAL

#### Scenario: Esc closes, nothing typed [auto]
- GIVEN the view open over a shell pane
- WHEN `x` then Esc are pressed
- THEN the notice `Esc closes the agents list` was shown, the view is closed and the PTY received no byte
