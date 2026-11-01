# Delta for Config Menu

Applies on top of the unarchived deltas for config-menu (`agent-tools`, `auto-execute`). Tags as in the main spec.

## ADDED Requirements

### Requirement: Usage row and view
The Settings tab MUST hold a static row `Usage (alpha)` after `llama-server idle` and before `Updates` (placement per ai-menu-tabs, "Usage row belongs to Settings"), found by its own `ItemKind`, never by position. Its value reads a summary of the draft: `line on · bar on` (the two toggles as `on`/`off`). `Enter`, `Space`, `h`, `l`, Left or Right open the Usage view (a closed row never ignores a key), a sub-level of Settings; the row adds one line to Settings, so the fixed popup is 17 rows high and its minimum is 24 columns by 16 rows (see Popup height is fixed in ai-menu-tabs), and the view reuses the popup. The view lists, in this order, static rows found by their own kinds:
1. `Usage line` (`ai.usage.line`, `on`/`off`);
2. `Usage in bar` (`ai.usage.statusline`, `on`/`off`);
3. `claude billing`, `codex billing`, `gemini billing` (`ai.usage.billing.<id>`, values `subscription`, `per-token`);
4. (Slice 3, not shipped in slice 2b: the thresholds stay in the config file until then.) `Turn tokens` (`ai.usage.turn_tokens`), `Day tokens` (`ai.usage.day_tokens`), `Day $` (`ai.usage.day_usd`).
Toggles and billing rows cycle with `h`, `l`, Left, Right, Space and Enter (an Enter repeat is ignored). `s` saves from inside the view. Any other key (Tab, digits, letters) shows the notice `Esc first to leave Usage` and does nothing else. Limit rows (slice 3) show `off` when empty. `Enter` on a limit row starts editing: digits are accepted (and one `.` for `Day $`, at most 2 decimals), Backspace deletes, `Enter` confirms, `Esc` cancels the edit and restores the previous value; any other character is ignored. A limit is at most 12 digits; `0` or an empty value means `off`. There is no row for local engines. The footer is `j/k move · h/l change · s save · Esc back`; slice 3 adds `Enter edit` once limit rows exist (and while editing a limit `type digits · Enter ok · Esc cancel`). Esc at the view's top level returns to Settings keeping the draft; the AI draft is saved or reverted with the menu as for other AI rows (`s` saves, Esc at the top level restores). Changing a draft MUST have no live effect on open chat panes until a successful save; after a save the usage line, the status segment and the labels take effect at the next frame. The descriptor integrity rules extend to the view's rows (no duplicated key path).

#### Scenario: Row and summary [auto, TestBackend + insta]
- GIVEN the Settings tab with the defaults
- WHEN rendered
- THEN the row after `llama-server idle` is `Usage (alpha)` with the value `line on · bar on`

#### Scenario: Open and back [auto]
- GIVEN `Usage (alpha)` selected
- WHEN `Enter` then `Esc` are pressed
- THEN the view opens, then Settings returns with the draft kept and no file touched

#### Scenario: Toggle cycle [auto]
- GIVEN the view on `Usage line` with the draft on
- WHEN `l` is pressed twice
- THEN the draft is off, then on, and no save is requested

#### Scenario: Billing cycle [auto]
- GIVEN `claude billing` showing `subscription`
- WHEN `l` is pressed
- THEN it shows `per-token`, and there is no billing row for `ollama`

#### Scenario: Edit a limit [auto, slice 3]
- GIVEN `Turn tokens` showing `off`
- WHEN `Enter`, `4`, `0`, `a`, `0`, `0` and `Enter` are pressed
- THEN the row shows `4000` (the `a` was ignored)

#### Scenario: Escape cancels an edit [auto, slice 3]
- GIVEN a limit showing `4000` and an edit in progress typing `9`
- WHEN `Esc` is pressed
- THEN the row still shows `4000` and the view stays open

#### Scenario: Zero and empty mean off [auto, slice 3]
- GIVEN a limit showing `4000`
- WHEN it is edited to `0`, and in another run to empty
- THEN it shows `off` both times

#### Scenario: Dollar limit decimals [auto, slice 3]
- GIVEN `Day $` in edit
- WHEN `2`, `.`, `5`, `0`, `9` are typed
- THEN the field reads `2.50` or `2.5` and the extra digit is ignored

#### Scenario: No live effect [auto]
- GIVEN an open chat with the usage line on and the draft toggled off
- WHEN no save is made
- THEN the open chat still shows the usage line and no WritePty or ResizePty is emitted

#### Scenario: Save writes only changes [auto]
- GIVEN the draft `Usage line` off and `claude billing` `per-token`
- WHEN `s` is pressed
- THEN the edit list holds `ai.usage.line` and `ai.usage.billing.claude` only

#### Scenario: Esc at top level reverts [auto]
- GIVEN usage drafts changed
- WHEN Esc is pressed on the AI task list
- THEN the saved values are unchanged and no file is read or written

#### Scenario: Footer and sizes [auto, TestBackend + insta]
- GIVEN the Usage view, a terminal of 80x24 and one of exactly 24x16
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · s save · Esc back`, the popup is the fixed 17 rows (clipped at 24x16 without panic), and one column or row less shows the one-line fallback

#### Scenario: Keys the view does not use [auto]
- GIVEN the Usage view open
- WHEN Tab, a digit or another unlisted key is pressed
- THEN the tab did not change, the view stays and the footer shows `Esc first to leave Usage`

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN the usage rows exist with the key paths above and no key path is duplicated

#### Scenario: Usage settings in Kitty [manual, Kitty, limit part in slice 3]
- GIVEN the menu in Kitty
- WHEN `Usage line` is turned off and `Turn tokens` set to 1000 and saved, and a chat answers
- THEN no usage line appears, the turn note `turn used <N> tokens (limit 1000)` appears when exceeded and the file holds `line = false` and `turn_tokens = 1000` under `[ai.usage]`
