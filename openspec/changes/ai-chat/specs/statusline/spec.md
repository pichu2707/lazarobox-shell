# Delta for Statusline

Tags: [auto] = `cargo test`; scenarios without a tag are [auto]. Existing statusline snapshots MUST remain valid except the root hint snapshot, which now also includes `c chat`.

## MODIFIED Requirements

### Requirement: Mode block
The statusline MUST show the current mode label (TERMINAL, PREFIX, COPY, RESIZE, MENU, or the group label while a group is pending) styled with that mode's color from `mode_style`. While the mode is TERMINAL and the focused pane is a chat pane, the label MUST be `CHAT`, drawn with the TERMINAL color; in every other mode the label is unchanged.

Mode colors (reusing the theme palette, via `input_mode_style`): TERMINAL → `success_green`, PREFIX → `warning_orange`, COPY → `primary_cyan`, any confirmation → `error_red`. RESIZE → `ai_purple`, which no other mode uses. A pending group uses the PREFIX style. The separator accent next to the focused pane follows the same mapping (see pane-layout). `CHAT` is a label only, not an input mode: the input mode stays TERMINAL and `input_mode_style` is unchanged.

MENU MUST have its own accent color from the theme palette, different from the colors of TERMINAL, PREFIX, COPY, RESIZE and the confirmations (the specific palette entry is a design decision). The separator accent next to the focused pane follows the same mapping. All existing mode colors are unchanged. While MENU is active the path segment shows the usual cwd or notice; the root hint is not shown.
(Previously: no MENU label; no CHAT label)

#### Scenario: Label and color [auto, TestBackend + insta]
- GIVEN each mode, including RESIZE
- WHEN rendered
- THEN the label text and mode color are shown

#### Scenario: Mode color mapping [auto]
- GIVEN each input mode
- WHEN `input_mode_style` is evaluated
- THEN TERMINAL uses `success_green`, PREFIX `warning_orange`, COPY `primary_cyan`, every confirmation `error_red`, and RESIZE `ai_purple`

#### Scenario: Existing snapshots [auto]
- GIVEN the pre-existing statusline snapshots
- THEN they still pass unchanged, except the root hint snapshot which now includes `m menu` and `c chat`

#### Scenario: MENU label and color [auto, TestBackend + insta]
- GIVEN MENU
- WHEN rendered
- THEN the label `MENU` is shown in its accent color

#### Scenario: Mode color mapping includes MENU [auto]
- GIVEN each input mode
- WHEN `input_mode_style` is evaluated
- THEN MENU's style differs from every other mode's style and the other mappings are unchanged

#### Scenario: CHAT label [auto, TestBackend + insta]
- GIVEN TERMINAL with a chat pane focused
- WHEN rendered
- THEN the mode block reads `CHAT` in the TERMINAL color, and after focus moves to a shell pane it reads `TERMINAL`

#### Scenario: Other modes keep their label over a chat pane [auto]
- GIVEN a chat pane focused and PREFIX pending
- WHEN rendered
- THEN the mode block reads `PREFIX`

### Requirement: Group-pending key hint
While a group is pending (after `w`, `t`, `g` or `b`) the mode block MUST show the group label (`WINDOW`, `TAB`, `GO`, `BUFFER`) in the PREFIX color, and the path segment MUST show a one-line key hint generated from the prefix tree: each binding of the group as `{key} {description}`, in table order, joined by ` · `. For group `w` the hint is `v split right · h split below · q close · r resize · z zoom`. When the hint does not fit the available width it MUST be clipped by whole entries and end with `…`; it MUST never show a partial entry. The hint is minimal: the full `?` command viewer is out of scope.

With plain PREFIX (no group yet) the path segment MUST show the same kind of hint built from the root of the prefix tree, using the same function and clipping rule: `w window · t tab · g go · b buffer · [ copy · m menu · c chat · q quit`. A group entry shows its short description; focus keys, the literal Ctrl+Space and the reserved `?` are not listed.
(Previously: the root hint was `w window · t tab · g go · b buffer · [ copy · m menu · q quit`)

#### Scenario: Root hint with chat [auto, TestBackend + insta]
- GIVEN PREFIX with no group pending on a wide terminal
- WHEN rendered
- THEN the mode block shows `PREFIX` and the path segment shows `w window · t tab · g go · b buffer · [ copy · m menu · c chat · q quit`, and still omits focus keys, the literal Ctrl+Space and `?`

#### Scenario: Root hint clipped and cleared [auto]
- GIVEN PREFIX and a path segment too narrow for the full root hint
- WHEN rendered, and then Esc is pressed
- THEN the hint shows only whole entries followed by `…`, and after Esc the statusline shows TERMINAL and the cwd

#### Scenario: Group hint [auto, TestBackend + insta]
- GIVEN group `w` pending on a wide terminal
- WHEN rendered
- THEN the mode block shows `WINDOW` and the path segment shows `v split right · h split below · q close · r resize · z zoom`

#### Scenario: Hint is generated from the table [auto]
- GIVEN each group `w`, `t`, `g` and `b`
- WHEN its hint is built
- THEN it contains exactly the key and description of every binding in that group, in table order

#### Scenario: Hint clipped by whole entries [auto]
- GIVEN group `w` pending and a path segment too narrow for the full hint
- WHEN rendered
- THEN the hint shows only the entries that fit followed by `…`, with no partial entry

#### Scenario: Hint gone after the group resolves [auto]
- GIVEN group `w` pending
- WHEN `v` is pressed
- THEN the statusline shows the TERMINAL label and the cwd again

#### Scenario: Narrow width [auto]
- GIVEN group pending on a very narrow terminal
- WHEN rendered
- THEN the mode block takes priority, the hint may be empty, and rendering does not panic

#### Scenario: Root hint stays table-generated [auto]
- GIVEN the root of the prefix tree
- WHEN its hint is built
- THEN it contains exactly the root entries that are listed (groups and leaf bindings, including `m menu` and `c chat`), in table order

## ADDED Requirements

### Requirement: Chat pane segments
While the focused pane is a chat pane, the path segment MUST show its origin cwd (unless a notice or a key hint replaces it) and the right-hand segment MUST show the effective chat engine and model as `<engine> › <model>` (only `<engine>` when the model is empty), or `no engine` when the effective `chat` assignment is empty. The right-hand segment MUST NOT show a shell name for a chat pane. Focus changes MUST switch the segments immediately. The mode block keeps priority at narrow widths and nothing panics.

#### Scenario: Engine shown [auto, TestBackend + insta]
- GIVEN a focused chat pane opened from `/srv/app` and `chat` assigned to `ollama` / `llama3:8b`
- WHEN rendered
- THEN the path segment shows `/srv/app` and the right side shows `ollama › llama3:8b`

#### Scenario: No engine [auto]
- GIVEN a focused chat pane and nothing assigned
- WHEN rendered
- THEN the right side shows `no engine`

#### Scenario: Falls back to general [auto]
- GIVEN `chat` unassigned and `general` `claude` / `sonnet`
- WHEN rendered over a focused chat pane
- THEN the right side shows `claude › sonnet`

#### Scenario: Focus switches segments [auto]
- GIVEN a shell pane (cwd `/a`, shell zsh) focused next to a chat pane
- WHEN focus moves to the chat pane and back
- THEN the right side shows the engine, then `zsh`

#### Scenario: Narrow width [auto]
- GIVEN a focused chat pane on a terminal of any width from 0 to 40
- WHEN rendered
- THEN rendering does not panic
