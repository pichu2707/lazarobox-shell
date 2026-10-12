# Delta for Statusline

Tags: [auto] = `cargo test`; scenarios without a tag are [auto]. Existing statusline snapshots MUST remain valid except the root hint snapshot, which now also includes `c chat`; snapshots rendered with no running chat stream are unaffected by the thinking indicator.

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

### Requirement: Thinking indicator
While at least one chat pane in any tab has a running stream, the statusline MUST show a thinking indicator, in every input mode and whichever pane or tab is focused. With no running stream it MUST show nothing and every statusline snapshot without a stream stays unchanged. The indicator MUST be a ratatui widget drawn from pure state (animation frame, phase, model, elapsed) in the normal frame; it MUST NOT write to the terminal outside ratatui (the `indicatif` crate is not used). Its parts, in order, are:
- a scanner bar: a track of `━` cells with a lit segment of `▰` cells sweeping across it, whose position depends only on the animation frame (track width and sweep style are design decisions, the lit segment never leaves the track);
- a spinner: the braille frames `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` in this order, advancing one frame per animation frame and wrapping after the last;
- the phase: `thinking` while the stream has produced only reasoning or nothing, `answering` once answer text has arrived (see chat-pane, Stream phase);
- the model of the request as sent for the shown stream (omitted when empty);
- the elapsed time since that stream was sent, as `Ns` below 60 seconds and `MmSSs` from 60 seconds (for example `1m05s`), computed from the start instant and not from the number of frames drawn.

The spinner and the lit scanner segment MUST use the theme's `ai_purple`; every other part (track, phase, model, elapsed, count) MUST use `text_muted`. When several streams are running, the indicator MUST show the stream of the focused chat pane if that pane is streaming, otherwise the most recently started stream, and when more than one stream is running it MUST append `+N` where N is the number of other running streams. The indicator is added to the existing segments: the mode block (including `CHAT`), the path segment and the chat engine segment are unchanged by it, and the mode block keeps priority at narrow widths. When the width is not enough the indicator MUST drop whole parts in this order: scanner bar, model, elapsed, `+N`; the spinner and the phase go last, and a part is never shown partially.

#### Scenario: Indicator while streaming [auto, TestBackend + insta]
- GIVEN one chat pane streaming with model `llama3:8b`, no byte received, 4 seconds after the send
- WHEN rendered at animation frame 0
- THEN the statusline shows the scanner bar, the spinner `⠋`, `thinking`, `llama3:8b` and `4s`

#### Scenario: Phase answering [auto, TestBackend + insta]
- GIVEN the same stream after its first text delta
- WHEN rendered
- THEN the phase reads `answering` and the model and elapsed time are still shown

#### Scenario: Phase thinking with reasoning only [auto]
- GIVEN a stream that delivered only reasoning
- WHEN rendered
- THEN the phase reads `thinking`

#### Scenario: Nothing without a stream [auto, TestBackend + insta]
- GIVEN no running stream in any pane, with idle chat panes open
- WHEN rendered
- THEN the statusline equals the one rendered without this requirement, in every mode

#### Scenario: Colors [auto]
- GIVEN the indicator rendered
- WHEN the buffer cell styles are inspected
- THEN the spinner glyph and the lit `▰` cells have the `ai_purple` foreground, and the track, phase, model, elapsed time and `+N` have the `text_muted` foreground

#### Scenario: Spinner sequence [auto]
- GIVEN animation frames 0 to 10
- WHEN the spinner glyph is computed
- THEN frames 0 to 9 give `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` in order and frame 10 gives `⠋` again

#### Scenario: Scanner stays on the track [auto]
- GIVEN any animation frame from 0 to 1000
- WHEN the scanner is computed
- THEN the lit segment lies entirely inside the track, and consecutive frames give at least two distinct positions over one sweep

#### Scenario: Elapsed formats [auto]
- GIVEN a stream sent 0, 59, 60 and 65 seconds ago (injected clock)
- WHEN rendered
- THEN the elapsed time reads `0s`, `59s`, `1m00s` and `1m05s`

#### Scenario: Elapsed does not depend on frames [auto]
- GIVEN a stream sent 7 seconds ago for which only 2 animation frames were drawn
- WHEN rendered
- THEN the elapsed time reads `7s`

#### Scenario: Focused streaming pane wins [auto]
- GIVEN two streaming chat panes A (started first, model `m1`) and B (started later, model `m2`) with A focused
- WHEN rendered
- THEN the indicator shows model `m1` and `+1`

#### Scenario: Most recent stream when the focus is elsewhere [auto]
- GIVEN the same two streams with a shell pane focused, and then with an idle chat pane focused
- WHEN rendered each time
- THEN the indicator shows model `m2` (the most recently started) and `+1`

#### Scenario: Count of other streams [auto]
- GIVEN three streaming chat panes
- WHEN rendered
- THEN the indicator shows `+2`, and with a single stream no `+N` is shown

#### Scenario: Streams ending [auto]
- GIVEN two streams of which the shown one finishes, then the other is cancelled
- WHEN rendered after each event
- THEN the indicator first shows the remaining stream without `+N`, then disappears; a closed pane or an error ends a stream the same way

#### Scenario: Shown in every mode and in a background tab [auto]
- GIVEN a stream running in a chat pane of another tab, then PREFIX, COPY and MENU in turn with a shell pane focused
- WHEN rendered each time
- THEN the indicator is shown with the usual mode label

#### Scenario: Coexists with the chat segments [auto, TestBackend + insta]
- GIVEN a focused chat pane streaming, opened from `/srv/app`, assigned `ollama` / `llama3:8b`
- WHEN rendered on a wide terminal
- THEN the statusline shows `CHAT`, `/srv/app`, the engine segment `ollama › llama3:8b` and the indicator, none overlapping

#### Scenario: Narrow widths drop whole parts [auto]
- GIVEN a streaming chat pane and terminal widths from 0 to 80
- WHEN rendered at each width
- THEN nothing panics, the mode block is intact whenever it fits, parts are dropped in the order bar, model, elapsed, `+N`, and no part is cut in the middle

#### Scenario: Real terminal look [manual, Kitty]
- GIVEN a real chat streaming from a local model in Kitty
- WHEN the user watches the bottom bar
- THEN the bar sweeps, the braille spinner turns, the phase flips from `thinking` to `answering`, and the glyphs render without gaps

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
