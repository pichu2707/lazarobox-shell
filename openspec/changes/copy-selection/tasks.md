# Tasks: Copy Selection

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,150 total (range 1,000-1,500, tests ~1.5x code), 4 PRs of 190-360 lines |
| Per-PR estimate | CS1 ~340, CS2a ~230, CS2b ~190, CS3 ~360 |
| 400-line budget risk | Medium (CS1 and CS3 are tightest; both have a 30-60% test overrun history) |
| Chained PRs recommended | Yes |
| Suggested split | CS1 -> CS2a -> CS2b -> CS3 |
| Delivery strategy | auto-chain |
| Chain strategy | feature-branch-chain |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: Medium

The design sized CS2 at ~390, so it is pre-split into CS2a (pure `CopyState`) and CS2b (App and UI wiring), as the design allows. Tests are never trimmed to fit a budget; if a slice grows past 400 during apply, it splits further using the fallbacks below.

Fallback splits (apply only if the slice exceeds 400 during apply):
- **CS1** (~340): CS1-i `Pane` line model (probe, `first_line`, `line_generation`, `live_cursor`, `line_end`) then CS1-ii `read_lines` + `LineSpan`/`LineText`/`join`.
- **CS3** (~360): CS3a `feat/copy-selection-03a-yank` (`encode`/`osc52`, `Effect::SetClipboard`, yank flow, notices, runtime `host_out`/`flush_host`) then CS3b `feat/copy-selection-03b-highlight-marker` (`TerminalView::highlight`, statusline marker).

### Suggested Work Units (merge BOTTOM-UP into the tracker `feat/copy-selection`)

Tracker `feat/copy-selection` is already pushed and equals main `88282ed`. The first slice PR targets the tracker; each later slice PR targets the previous slice branch; only the tracker merges to main.

| Slice | Branch | Base | Est. lines | Content |
|---|---|---|---|---|
| CS1 | `feat/copy-selection-01-line-model` | `feat/copy-selection` | ~340 | `Pane` probe, `first_line`, `line_generation`, `live_cursor`, `line_end`, `read_lines`; `LineSpan`, `LineText`, `join` |
| CS2a | `feat/copy-selection-02a-copystate-core` | CS1 branch | ~230 | pure `CopyState`: `Point`, `Selection`, `Geometry`, motions, edge scrolling, key map and Repeat |
| CS2b | `feat/copy-selection-02b-app-wiring` | CS2a branch | ~190 | App enter, `on_copy_key`, `sync_copy`, `copy_cursor`, COPY cursor render, existing copy tests rewritten |
| CS3 | `feat/copy-selection-03-yank-highlight-marker` | CS2b branch | ~360 | `encode`/`osc52`, `SetClipboard`, runtime flush, notices, highlight, statusline marker |

Every PR must be green: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. Strict TDD: RED is an assertion-level failing test written against compiling stubs (the stub exists and returns a wrong or default value, so the failure is a failed assertion, not a compile error); then GREEN, then REFACTOR. All assertions use exact equality (`assert_eq!` on full values, whole strings, exact byte vectors, exact cell styles); never `.all`, `.any` or `contains` to loosen a check. Characterization tests that pass immediately are labelled as such. [manual] = user-run in Kitty. Each PR states start/end, parent, follow-up, out of scope, and a dependency diagram with the current PR marked. Tests and code of one slice stay in the same commit set (work-unit commits, conventional commits, no AI attribution); no cross-slice commits.

## CS1: Pane line model and extraction (pure)

- [x] CS1.1 Stubs (compile only; pane stubs in CS1-i, `copy.rs` and `read_lines` in CS1-ii): in `src/core/copy.rs` add `LineSpan { first, last, first_col, last_col }` (`contains(line, col)` returns `false`), `LineText { text, wrapped }`, `join(&[LineText]) -> String` returning `String::new()`. In `src/core/pane.rs` add `first_line()` (returns `0`), `line_generation()` (returns `0`), `live_cursor() -> Option<(u16, u16)>` (returns `None`), `line_end(line) -> u16` (returns `0`), `read_lines(span) -> Vec<LineText>` (returns empty).
- [x] CS1.2 Characterization (expected to pass immediately): all existing scrollback tests in `core::pane` stay green before the probe lands; pin them as the safety net for CS1.4 [CS-2 risk].
- [x] CS1.3 RED: `first_line` counts evictions with `Pane::new(.., cap 5)` + `feed_lines`: below the cap `first_line == 0`; after N overflow lines at offset 0 `first_line == N`; with offset > 0 the same count, and the user's offset is pinned exactly as vt100 does natively (assert exact offset value) [copy-selection: Eviction of the anchor (pane part), Output keeps the selection on its content (pane part)].
- [x] CS1.4 GREEN: probe in `Pane::feed` (CS-2): on the main screen set offset to 1 before `process`, compute `scrolled = offset_after - 1`, `first_line += (h0 + scrolled) - h1`, restore offset `0` if it was `0` else `o0 + scrolled`.
- [x] CS1.5 RED: `line_generation` is unchanged by plain output, bumped by a screen flip (main -> alternate and back) and bumped by `resize`; bumped when the probe is lost (offset 0 afterwards) [copy-selection: Screen switch drops it (pane part), Resize drops it (pane part)].
- [x] CS1.6 GREEN: bump `line_generation` on screen flip, probe loss and resize.
- [x] CS1.7 RED: `live_cursor` returns the child's cursor `(row, col)` ignoring the scrollback offset (set offset > 0 and assert the same value); hidden cursor returns `None` [copy-mode-scrollback: Cursor starts at the live cursor (pane part)].
- [x] CS1.8 GREEN: `live_cursor`.
- [x] CS1.9 RED: `line_end(line)` on `echo hi` returns `6`; on a blank row returns `0`; on a row in history returns its last non-blank column; the offset is restored exactly afterwards [copy-mode-scrollback: Horizontal motion (pane part), End of a blank row (pane part)].
- [x] CS1.10 GREEN: `line_end` by paging `set_scrollback` and restoring.
- [x] CS1.11 RED: `LineSpan::contains` table test: inside, both end cells inclusive, before first, after last, middle rows full-width, single-row span, reversed anchor ordering normalised by constructor/`span` helper [copy-selection: Motion extends backwards (span part)].
- [x] CS1.12 GREEN: `LineSpan::contains` and the ordering helper.
- [x] CS1.13 RED: `read_lines` over history plus screen returns exact `LineText` values (text and `wrapped` flag); cells in `[from, to]` inclusive; empty cell counts as `" "`; the viewport offset is identical before and after, including for a span entirely above the viewport [copy-selection: Selection spans scrollback (pane part), Character selection within a row (pane part)].
- [x] CS1.14 GREEN: `read_lines` pages at most once per pane height and restores the offset (CS-6).
- [x] CS1.15 RED: wide characters: row `a漢b` with a range starting on the continuation cell (col 2) yields `漢b`; a range ending on the head cell yields the character once; continuation cells never emit text [copy-selection: Wide character once (pane part)].
- [x] CS1.16 GREEN: continuation skip and head snapping.
- [x] CS1.17 RED: `copy::join`: `foo` + `bar` (not wrapped) -> `foo\nbar`; wrapped first row joins with no separator -> `abcdefghijklmn`; trailing blanks trimmed on logical-line ends and on the last row; a soft-wrapped non-last row keeps its trailing spaces (`hello` + 5 spaces + `world` -> `hello     world`); no trailing `\n` [copy-selection: Multi-row line selection (join part), Soft-wrapped rows, Soft-wrapped row keeps trailing spaces, Trailing blanks trimmed].
- [x] CS1.18 GREEN: `join` per CS-6.
- [x] CS1.19 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## CS2a: Pure `CopyState` (cursor, selection, motions, keys)

- [x] CS2a.1 Stubs (compile only): `Point { line, col }`, `SelectionKind::{Char, Line}`, `Selection { anchor, kind }`, `Geometry { first_line, history, rows, cols }`, `CopyCommand::{Move(CopyMotion), Yank, Exit, Ignore}`; `CopyState` gains `cursor`, `selection`, `generation` and `enter(geo, live_cursor, generation)`; new methods return defaults. The old `apply` stays as a thin adapter so CS2b can land separately and the crate keeps compiling (design CS2 split).
- [x] CS2a.2 Characterization (expected to pass immediately): pin the current copy key map tests that remain valid after the rewrite (`gg`, `G`, Ctrl+u/d offsets) so the adapter is proven behaviour-neutral for existing callers.
- [x] CS2a.3 RED: `CopyState::enter`: cursor on the live cursor clamped into the pane (row 5, col 3 -> `Point { line: first_line + history + 5, col: 3 }`); hidden cursor (`None`) uses the bottom screen row at column 0; offset `0`; no selection [copy-mode-scrollback: Cursor starts at the live cursor].
- [x] CS2a.4 GREEN: `enter` (CS-3).
- [x] CS2a.5 RED: horizontal motions: `h`/`l` move one column and clamp at 0 and `cols-1`; `0` goes to column 0; `$` uses the supplied `line_end` value; with `line_end` = 6, the sequence `h 0 h $ l` from col 3 gives cols exactly `2, 0, 0, 6, 7`; `$` with `line_end` = 0 on a blank row gives col 0 [copy-mode-scrollback: Horizontal motion, End of a blank row].
- [x] CS2a.6 GREEN: horizontal motion and the `$` hook (the caller resolves `line_end`, `Move` carries the request).
- [x] CS2a.7 RED: vertical motions with edge scrolling: cursor on the top visible row at the bottom viewport, `k` gives offset exactly 1 and the cursor line decreased by 1 (still on the top visible row), `j` returns; cursor mid-viewport `k` changes only the line, offset unchanged; clamping at `first_line` and the last line for `j`/`k` leaves cursor and offset unchanged [copy-mode-scrollback: Line motion, Cursor moves inside the viewport, Clamping (j/k)].
- [x] CS2a.8 GREEN: `j`/`k` plus `ensure_visible` (CS-3).
- [x] CS2a.9 RED: Ctrl+u/Ctrl+d on a 24-row pane move the cursor line and the offset by exactly 12; clamp at both bounds (Ctrl+u at the top row leaves state unchanged); `gg` gives `(first_line, 0)` at offset = history; `G` gives the bottom row at column 0 and offset 0; a lone `g` followed by any non-`g` key discards the pending `g` [copy-mode-scrollback: Half page, Top and bottom, Clamping (Ctrl+u), Lone g].
- [x] CS2a.10 GREEN: half page, `gg`/`G`, pending-`g` handling.
- [x] CS2a.11 RED: selection start and toggling: `v` at (5, 3) gives `Selection { anchor: (5,3), kind: Char }`; `V` gives `kind: Line`; same key again clears; `v` then `V` switches kind and keeps the anchor line; `V` then `v` likewise [copy-selection: Start character selection, Start line selection, Same key clears, Other key switches kind].
- [x] CS2a.12 GREEN: `v`/`V` handling (CS-4).
- [x] CS2a.13 RED: motions extend from the anchor in either direction: `v` at (5,3), `h` twice gives `span()` exactly `(5,1)..=(5,3)`; moving down then up past the anchor reorders the span; a Line selection spans `(first, 0)..=(last, cols-1)` [copy-selection: Motion extends backwards, selection kinds extend].
- [x] CS2a.14 GREEN: `span()` (ordering and line-kind widening).
- [x] CS2a.15 RED: key map: `y` with a selection gives `Yank`, `y` without gives `Ignore`; `q` and `i` give `Exit`; Esc with a selection clears it and returns `Ignore`, Esc without gives `Exit`; unlisted keys are `Ignore`, never forwarded [copy-selection: Yank without selection (core part), Esc clears then exits (core part); copy-mode-scrollback: Esc with selection, `q` with selection, Exit (core part)].
- [x] CS2a.16 GREEN: `y`/Esc/`q`/`i` mapping.
- [x] CS2a.17 RED: `on_key(&key, repeat)`: Repeat of `v V y Esc q i` returns `Ignore` and leaves selection and cursor unchanged; Repeat of `j`, `h`, `l`, `k`, Ctrl+d moves exactly like Press [modal-input: Repeat of selection keys in COPY (core part), Repeat of motions in COPY (core part)].
- [x] CS2a.18 GREEN: Repeat filter (CS-4).
- [x] CS2a.19 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## CS2b: App wiring, revalidation, COPY cursor

- [x] CS2b.1 Characterization (expected to pass immediately): pin the current `cursor_position`/effective cursor-shape behaviour in COPY (Block steady while COPY, child shape restored on exit) before touching render [copy-mode-scrollback: Block in COPY, Restore on exit].
- [x] CS2b.2 Stubs: `App::enter_copy` builds `CopyState` via `Pane::live_cursor`/`first_line`/`line_generation`; `on_copy_key` delegating to `CopyState::on_key` (Yank is a no-op until CS3); `sync_copy` empty; `copy_cursor() -> Option<(u16, u16)>` returning `None`; `copy_highlight` not yet present.
- [x] CS2b.3 RED: entering COPY through the prefix places the COPY cursor at the child's cursor (row 5, col 3) and at the bottom row, column 0 when hidden; `copy_cursor()` returns the viewport-relative `Some((5, 3))` and `None` when the cursor scrolled off screen [copy-mode-scrollback: Cursor starts at the live cursor, Block follows the COPY cursor (app part)].
- [x] CS2b.4 GREEN: `enter_copy`, `copy_cursor` (CS-3, CS-11 cursor part).
- [x] CS2b.5 RED: app-level keys in COPY via `update`: `v`/`V`/`Esc` states observable through `selection()`; Esc with a selection clears and stays COPY, second Esc gives TERMINAL with offset 0; `q` with a selection gives TERMINAL and re-entering shows no selection; no key in COPY emits `WritePty` [copy-selection: Esc clears then exits, `q` drops the selection; copy-mode-scrollback: Esc with selection, `q` with selection, Exit; modal-input: Input goes to the focused pane only (characterization)].
- [x] CS2b.6 GREEN: `on_copy_key` mapping to mode transitions and offset reset.
- [x] CS2b.7 RED: Repeat events through `update`: Repeat `v V y Esc q i` with a selection leave selection, mode (COPY) and effects unchanged (exactly `vec![]`, no `SetClipboard`, no `WritePty`); Repeat `j` moves the cursor one row; Release ignored in every mode [modal-input: Repeat of selection keys in COPY, Repeat of motions in COPY, Release ignored, Repeat ignored in decision states (characterization)].
- [x] CS2b.8 GREEN: thread `repeat` from the key event to `CopyState::on_key`.
- [x] CS2b.9 RED: `sync_copy` revalidation: new output (5 lines) keeps the selection on `foo` (selected span lines unchanged); eviction at the cap clamps the anchor to `(first_line, 0)` and keeps the selection; a `line_generation` change (screen flip) drops the selection and keeps COPY; host Resize or a `relayout` that emits `ResizePty(focus)` (sibling exit, tab bar toggle) drops the selection and keeps COPY; focus change drops COPY to TERMINAL with no selection [copy-selection: Output keeps the selection on its content, Eviction of the anchor, Resize drops it, Screen switch drops it, Focus change drops it].
- [x] CS2b.10 GREEN: `sync_copy` (CS-5) invoked after output on the focused pane, after `relayout` and on resize.
- [x] CS2b.11 RED: `ui` draws the real cursor at `copy_cursor()` while COPY (even when the child hid its cursor) and uses the child's cursor unchanged outside COPY (characterization); TestBackend asserts the exact cursor position for pane cell (row 2, col 4) [copy-mode-scrollback: Block follows the COPY cursor].
- [x] CS2b.12 GREEN: cursor placement in `src/ui/mod.rs`.
- [x] CS2b.13 REFACTOR: rewrite existing copy tests that encoded viewport-only `j`/`k` onto the cursor model; remove the CS2a adapter `apply` if no caller remains.
- [x] CS2b.14 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## CS3: Encoder, yank, runtime emit, notices, highlight, marker

- [x] CS3.1 Stubs: `copy::encode(&[u8]) -> String` (returns empty), `osc52(&str) -> Result<Vec<u8>, TooLarge>` (returns `Ok(vec![])`), `OSC52_MAX_PAYLOAD = 100_000`; `Effect::SetClipboard(Vec<u8>)`; `Panes::host_out: Vec<u8>`; `flush_host<W: Write>(&mut W, &mut Vec<u8>)` (no-op); `TerminalView::highlight(Option<Highlight>)` (no-op); `App::copy_highlight()` returning `None`; statusline marker absent.
- [x] CS3.2 RED: `encode` RFC 4648 section 10 vectors (`""`, `f`, `fo`, `foo`, `foob`, `fooba`, `foobar`) exact strings, plus multi-byte UTF-8 [copy-selection: Yank emits OSC 52 (encoder part)].
- [x] CS3.3 GREEN: hand-written `encode` (CS-7).
- [x] CS3.4 RED: `osc52("hello")` equals exactly `b"\x1b]52;c;aGVsbG8=\x07".to_vec()`; boundary: 75,000 bytes (base64 length 100,000) is `Ok`; 75,001 bytes (length 100,004) is `Err(TooLarge)`; the check happens before encoding (99,999-length and 100,000-length payloads covered) [copy-selection: Yank emits OSC 52 (bytes), At the cap, Over the cap (core part)].
- [x] CS3.5 GREEN: `osc52` with `4 * ceil(n/3)` pre-check (CS-8).
- [x] CS3.6 RED: App yank with a selection of `hello`: `update` returns exactly `[SetClipboard(b"\x1b]52;c;aGVsbG8=\x07")]`, mode TERMINAL, offset 0, no selection, no `WritePty` [copy-selection: Yank emits OSC 52, Character selection within a row (`v`, `l` x4 gives `hello`), Multi-row line selection (`V`, `j` gives `foo\nbar`), Wide character once, Selection spans scrollback (app part: offset restored)].
- [x] CS3.7 GREEN: yank flow `read_lines` -> `join` -> `osc52` -> effect, mode and offset reset, selection dropped (CS-10).
- [x] CS3.8 RED: `y` without a selection emits `vec![]` and stays COPY; selection of exactly 75,000 bytes emits `SetClipboard`; 75,001 bytes emits no effect, mode stays COPY, selection kept, notice exactly `Selection too large to copy` [copy-selection: Yank without selection, At the cap, Over the cap].
- [x] CS3.9 GREEN: cap handling and notice (CS-8).
- [x] CS3.10 RED: notices through the notice slot: `hello` -> `Copied 5 characters` with mode block `TERMINAL`; `foo\nbar` -> `Copied 7 characters`; `x` -> `Copied 1 character`; over-cap -> `Selection too large to copy`; any next key restores the cwd; a newer notice (spawn failure) replaces it [statusline: Copied notice, Multi-row count, Singular notice, Too-large notice, Notice cleared by next key].
- [x] CS3.11 GREEN: notice text with `text.chars().count()` and singular rule.
- [x] CS3.12 RED: runtime: `Panes::apply(SetClipboard(bytes))` appends exactly `bytes` to `host_out` and writes nothing else; `flush_host` into a `Vec<u8>` writes the exact bytes, calls flush, and clears the buffer; an empty buffer writes nothing [copy-selection: Yank emits OSC 52 (runtime part)].
- [x] CS3.13 GREEN: `host_out` buffering and `flush_host` in `event_loop` tick after the draw and `apply_cursor_style` (CS-9).
- [ ] CS3.14 RED: highlight via TestBackend: `v` + `l` x2 on `hello` -> cells `h e l` have `Modifier::REVERSED` and `l o` do not (exact per-cell style equality); `V` -> every cell of the row reversed; no selection -> no cell reversed; an inverse cell under the selection is un-reversed (XOR); highlight follows scrolling; applied to the focused pane only [copy-selection: Character highlight, Line highlight, No selection].
- [ ] CS3.15 GREEN: `copy_highlight`, `TerminalView::highlight` XOR REVERSED via `LineSpan::contains` (CS-11).
- [ ] CS3.16 RED: statusline marker: 340-row history scrolled 12 up with `v` -> mode block exactly `COPY ↑12/340 VISUAL`; alternate screen with `V` -> exactly `COPY V-LINE`; Esc clears -> `COPY ↑12/340` with no marker; marker sits before `[Z]` when zoomed; widths 0..=40 never panic; `InputMode::label` still returns `COPY`; existing snapshots unchanged when no selection or notice [statusline: Marker with indicator, Line marker without history, Marker gone, Narrow width].
- [ ] CS3.17 GREEN: marker rendering after the indicator, path budget reduced by marker width (CS-12); accept new snapshots with `cargo insta review`, no other snapshot changes.
- [ ] CS3.18 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## Verify prep: manual Kitty checks

Run after CS3 is merged into the tracker, on a build of the tracker branch.

- [ ] VP.1 [manual] Paste elsewhere: `Ctrl+Space [`, `v`, a motion, `y`; paste into another application; the pasted text equals the selection exactly [copy-selection: Paste elsewhere].
- [ ] VP.2 [manual] Line yank runs nothing: `V` on a shell command line, `y`, paste at a shell prompt; the text appears with no trailing newline and nothing runs [copy-selection: Line yank runs nothing].
- [ ] VP.3 [manual] Yank over ssh: run lazarobox inside an ssh session, select and `y`; the text reaches the local clipboard [copy-selection: Yank over ssh].
- [ ] VP.4 [manual] Oversized selection: select well over 75 KB of scrollback, `y`; the footer notice reads `Selection too large to copy` and the clipboard is unchanged [copy-selection: Oversized in a real terminal].
- [ ] VP.5 [manual] Real session: long `ls -R`; `j`/`k`/Ctrl+u/Ctrl+d/`gg`/`G` navigate it with a visible block cursor, exit lands at the bottom [copy-mode-scrollback: Real session].
- [ ] VP.6 [manual] tmux note: confirm OSC 52 under tmux needs `set -g set-clipboard on` (or `external`); record the result for user docs (design risk).
- [ ] VP.7 Verify prep: run `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` on the tracker; confirm every row of the coverage table below has a passing test or a ticked [manual] item; hand the tracker to `sdd-verify`.
- [ ] VP.8 Archive prep: the Purpose line of the main copy-mode-scrollback spec ("Selection/yank is out of scope.") MUST be edited by hand at archive, as noted in the delta.

## Spec Coverage Table

Every scenario in the four deltas maps to at least one task.

### copy-selection

| Scenario | Tasks |
|---|---|
| Start character selection | CS2a.11, CS2a.12, CS3.16 |
| Start line selection | CS2a.11, CS2a.12, CS3.16 |
| Same key clears | CS2a.11, CS2a.12 |
| Other key switches kind | CS2a.11, CS2a.12 |
| Motion extends backwards | CS1.11, CS2a.13, CS2a.14 |
| Character selection within a row | CS1.13, CS3.6 |
| Multi-row line selection | CS1.17, CS3.6 |
| Soft-wrapped rows | CS1.17 |
| Soft-wrapped row keeps trailing spaces | CS1.17 |
| Trailing blanks trimmed | CS1.17 |
| Wide character once | CS1.15, CS3.6 |
| Selection spans scrollback | CS1.13, CS3.6 |
| Yank emits OSC 52 | CS3.2, CS3.4, CS3.6, CS3.12 |
| Yank without selection | CS2a.15, CS3.8 |
| Paste elsewhere [manual] | VP.1 |
| Line yank runs nothing [manual] | VP.2 |
| Yank over ssh [manual] | VP.3 |
| At the cap | CS3.4, CS3.8 |
| Over the cap | CS3.4, CS3.8 |
| Oversized in a real terminal [manual] | VP.4 |
| Esc clears then exits | CS2a.15, CS2b.5 |
| `q` drops the selection | CS2b.5 |
| Resize drops it | CS1.5, CS2b.9 |
| Screen switch drops it | CS1.5, CS2b.9 |
| Focus change drops it | CS2b.9 |
| Output keeps the selection on its content | CS1.3, CS2b.9 |
| Eviction of the anchor | CS1.3, CS2b.9 |
| Character highlight | CS3.14 |
| Line highlight | CS3.14 |
| No selection (highlight) | CS3.14 |

### copy-mode-scrollback

| Scenario | Tasks |
|---|---|
| Line motion | CS2a.7 |
| Cursor moves inside the viewport | CS2a.7 |
| Half page | CS2a.9 |
| Top and bottom | CS2a.9 |
| Clamping | CS2a.7, CS2a.9 |
| Lone g | CS2a.9 |
| Cursor starts at the live cursor | CS1.7, CS2a.3, CS2b.3 |
| Horizontal motion | CS1.9, CS2a.5 |
| End of a blank row | CS1.9, CS2a.5 |
| Block in COPY | CS2b.1 (characterization) |
| Restore on exit | CS2b.1 (characterization) |
| Block follows the COPY cursor | CS2b.3, CS2b.11 |
| Exit | CS2a.15, CS2b.5 |
| Esc with selection | CS2a.15, CS2b.5 |
| `q` with selection | CS2a.15, CS2b.5 |
| Real session [manual] | VP.5 |

### modal-input

| Scenario | Tasks |
|---|---|
| Passthrough | unchanged; existing test stays green (CS2b.5 characterization) |
| Repeat ignored in decision states | CS2b.7 (characterization) |
| Release ignored | CS2b.7 |
| Input goes to the focused pane only | CS2b.5 (characterization) |
| Repeat in MENU | unchanged; existing config-menu tests stay green (CS2b.14) |
| Repeat of selection keys in COPY | CS2a.17, CS2b.7 |
| Repeat of motions in COPY | CS2a.17, CS2b.7 |

### statusline

| Scenario | Tasks |
|---|---|
| Marker with indicator | CS3.16 |
| Line marker without history | CS3.16 |
| Marker gone | CS3.16 |
| Narrow width | CS3.16 |
| Copied notice | CS3.10 |
| Multi-row count | CS3.10 |
| Singular notice | CS3.10 |
| Too-large notice | CS3.10 |
| Notice cleared by next key | CS3.10 |

## Deferred (out of scope)

- Mouse selection, block (rectangular) selection, word/search motions.
- Paste-from-clipboard (OSC 52 read) and clipboard history.
- Configurable cap or terminator; OSC 52 chunking beyond the 100,000-byte payload cap.
- Automatic tmux `set-clipboard` configuration (documented only).
