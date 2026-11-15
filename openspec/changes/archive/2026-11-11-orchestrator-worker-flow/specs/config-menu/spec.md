# Delta for Config Menu

Slice tag: [W8]. The Models tab gains a `simple` row. Its position is an assumption of this delta (after `orchestrate`, before the static `Hugging Face` row, which stays last); design confirms it.

## ADDED Requirements

### Requirement: Simple row and picker [W8]
The Models tab MUST show a `simple` row after `orchestrate`. Enter on it MUST open the same two-level task picker as the other tasks, with `(default · use general)` as its first entry. Choosing an entry changes only `simple`. Its row reads `(default)` when unassigned and `<engine> › <model>` when assigned. The row MUST NOT carry `not recommended` on server engines. `Set all tasks` does not touch it and its aggregate ignores it. The row MUST NOT be reachable only by an undocumented key: the same navigation keys visit it.

#### Scenario: Row is present and ordered [W8, auto]
- GIVEN the Models tab
- WHEN the rows are built
- THEN they read `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `Hugging Face`

#### Scenario: Picker changes only simple [W8, auto]
- GIVEN `general` is `codex` and the picker for `simple`
- WHEN `ollama` / `llama3:8b` is chosen
- THEN `simple` is `ollama` / `llama3:8b` and the other tasks are unchanged

#### Scenario: Navigation visits the row [W8, auto]
- GIVEN the Models tab with `orchestrate` selected
- WHEN `j` is pressed
- THEN `simple` is selected, and one more press selects `Hugging Face`

#### Scenario: Set all leaves simple alone [W8, auto]
- GIVEN `simple` assigned and `Set all tasks` applied
- WHEN the draft is read
- THEN `simple` is unchanged

## Stale statements to amend at apply or archive (not rewritten here)
The following existing statements name the Models rows and become false with the new row; each must be reworded to include `simple` before `Hugging Face`:
- the tab-contents scenario "Models reads `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `Hugging Face`";
- the navigation scenario "the selection visits `general`, ..., `orchestrate`, `Hugging Face` in that order and the eighth press wraps to `Set all tasks`" (the ninth press wraps);
- the rows scenario that lists `orchestrate` `(default)` then `Hugging Face` `not connected`;
- the `Hugging Face` row requirement ("after the `orchestrate` row") and its scenario "after `orchestrate`".
