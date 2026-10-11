# Design: Copy Selection

## Technical Approach

The design keeps the functional core and imperative shell split that copy-mode-scrollback and config-menu already use.

- **Pane (`core::pane`)** gives every row a **stable line number**: `first_line + index`, where index 0 is the oldest retained history row. `first_line` only grows, and it counts rows evicted at the scrollback cap. The emulator stays behind DTOs. Rows are read by paging `set_scrollback` and then restoring the offset. `vt100::contents_between` is never used.
- **Pure core (`core::copy`)** holds the cursor and the selection as line numbers. It also holds the motions, edge scrolling, the join and trim rules, and the base64 and OSC 52 encoder with its size cap.
- **App** wires the keys, revalidates the selection after output and resizes, and emits `Effect::SetClipboard(bytes)` and notices.
- **Runtime** buffers the bytes in `Panes` and writes them to stdout in the tick, next to the cursor style.
- **UI** reverse-highlights the selection, puts the real cursor on the COPY cursor and shows the `VISUAL`/`V-LINE` marker after the position indicator.

ADRs use the `CS-` prefix. The text is over the 800-word budget on purpose, because the orchestrator asked for 7 decision areas.

## Architecture Decisions

| # | Topic | Options | Decision / rationale |
|---|---|---|---|
| CS-1 | Coordinates | (a) viewport-relative (row, col); (b) index from the oldest history row; (c) **stable line number** `first_line + index` | **(c)**. (a) shifts on every scroll. (b) is stable below the cap, but at the 10,000-row cap (`SCROLLBACK_LINES`) every new line drops the oldest row and shifts all indices. With (c) a line keeps its number until it is evicted, and `line < first_line` means it is gone. `Point { line: usize, col: u16 }`. The visible top is `first_line + history - offset`. |
| CS-2 | Counting evictions | infer from offset deltas in App; a scroll callback (vt100 has none); **probe pin in `Pane::feed`** | **Probe.** Before `process`, on the main screen, the offset is set to 1. vt100 then pins every scroll (`grid.rs:571`), so `scrolled = offset_after - 1` is exact, even at the cap and even when the user's offset was 0. Then `first_line += (h0 + scrolled) - h1`. The offset is restored to `0` if it was 0, else to `o0 + scrolled` (vt100 clamps it). That is the same result as vt100's native pin, so the "Output during COPY" scenarios hold. Inferring from deltas cannot tell an eviction from a repaint at offset 0 at the cap. **Safety:** entering the alternate screen resets the main offset to 0 (`screen.rs:652`), so the probe never leaks. If the screen flipped during the feed, or the probe was lost (offset 0 afterwards), the count is unknown. `line_generation` is then bumped. A resize also bumps it. |
| CS-3 | Cursor, entry, motions | keep `InputMode::Copy(CopyState)`; side field like `App.menu` | **Keep it.** The new fields (`cursor: Point`, `selection: Option<Selection>`, `generation: u32`) are all `Copy`. `CopyState::enter(geo, live_cursor, generation)` puts the cursor on the **live cursor** (new `Pane::live_cursor`, which ignores the scrollback). If the cursor is hidden it uses the bottom screen row at column 0. The offset is 0. `h/l` move one column, clamped. `0` goes to column 0 and `$` to `Pane::line_end(line)`, the last non-blank column. `j/k` move one line, clamped to the valid range, and keep the column. Ctrl+u/d move the cursor and the offset by half a page each. `gg` goes to `(first_line, 0)` at offset = history, and `G` to the bottom row at column 0 and offset 0. **Edge scrolling:** after a motion, `ensure_visible` moves the offset only as far as needed to bring the cursor into view. New output never moves the cursor or the viewport, so with offset 0 the cursor can scroll off screen, and the next motion brings it back. |
| CS-4 | Keys and Repeat | — | `v`/`V` toggle the selection or switch its kind, with the anchor at the cursor. `y` gives `Yank` only with a selection, otherwise it is ignored. Esc with a selection clears it, and Esc without one gives `Exit`. `q`/`i` give `Exit`. `on_key(&key, repeat)`: **Repeat is dropped for `v V y Esc q i`** and is kept for motions. A held `y`, `q` or `i` must not exit COPY and then leak into TERMINAL as typed text. |
| CS-5 | Revalidation (D8) | — | `App::sync_copy` runs after output on the focused pane, after `relayout` and on resize. (1) If `generation` differs from the pane's, the selection is dropped. (2) The cursor and the anchor are clamped to `first_line..=last_line`. An evicted anchor becomes `(first_line, 0)`, so the selection shrinks and is not lost. (3) If `relayout` emitted `ResizePty(focus)`, the selection is dropped. That covers a terminal resize, a sibling exit and the tab bar toggling. Focus and tab changes already leave COPY, which drops the state. |
| CS-6 | Extraction | `contents_between`; **`Pane::read_lines(LineSpan)` plus pure `copy::join`** | `contents_between` sees only the visible rows and is end-exclusive on a single row. `read_lines` maps each line to a visible row by paging, at most once per pane height, then restores the original offset. It returns `LineText { text, wrapped }`. Rules: cells in `[from, to]`, both inclusive. An empty cell counts as `" "`. Continuation cells are skipped. A range that starts on a continuation cell starts at its head, so a wide character is copied exactly once. `join` (pure): if the previous row is soft-wrapped it joins with nothing, otherwise with `\n`. There is no trailing `\n`. Trailing blanks are trimmed on rows that end a logical line and on the last row. **A soft-wrapped row keeps its trailing spaces**, because they are real content at the wrap point. N = `text.chars().count()`. |
| CS-7 | base64 | `base64 = "0.22"` (already in the lockfile through `termwiz`, so no new crate); hand-written | **Hand-written** `encode` (RFC 4648 standard alphabet with padding, about 20 lines), tested against the RFC §10 vectors. The crate would add no new code, but it would tie a direct dependency to `portable-pty`'s transitive pin. We only need to encode. |
| CS-8 | OSC 52 and cap | BEL vs ST terminator | `ESC ] 52 ; c ; <b64> BEL`. **BEL** is what tmux emits, and Kitty, xterm, Alacritty, WezTerm and tmux all accept it. `osc52(text) -> Result<Vec<u8>, TooLarge>` checks `4 * ceil(n/3) > 100_000` **before** encoding. If the selection is too large, nothing is sent, COPY stays open with the selection kept and the notice reads `Selection too large to copy`. |
| CS-9 | Emission plumbing | write directly in `Panes::apply`; **buffer in `Panes` and flush in the tick** | `Effect::SetClipboard(Vec<u8>)` carries the finished sequence. `Panes::apply` appends it to `host_out: Vec<u8>`. `event_loop`'s tick runs, after the draw and `apply_cursor_style`, `flush_host<W: Write>(out, &mut host_out)`: `write_all`, then `flush`, then clear. It is the same single task, and nothing is written in the middle of a ratatui draw. OSC 52 does not move the cursor or print anything, so ratatui's back buffer stays valid. Delay ≤ `TICK`. Writing directly would force a writer into `Panes` and its tests. |
| CS-10 | Yank flow | — | `read_lines(span)`, then `join`, then `osc52`. On success: `[SetClipboard]`, the offset goes back to 0, the mode goes to TERMINAL and the notice reads `Copied N characters`, or `Copied 1 character` when N is 1. It uses the existing notice slot, which is cleared on the next key. |
| CS-11 | Highlight and cursor | theme accent background; **reverse video (XOR)** | **XOR `REVERSED`** on the cells where `LineSpan::contains(line, col)`. It works with any colours and any theme, and an inverse cell stays distinct. A line selection is the span `(first, 0)..=(last, cols-1)`, so one predicate covers both kinds. `TerminalView::highlight(Option<Highlight { span, top_line }>)` is applied only to the focused pane. The real cursor goes to `App::copy_cursor()` (viewport-relative, `None` when off screen) instead of the child's cursor. It is shown even if the child hid its cursor. The shape is the existing steady block. |
| CS-12 | Statusline | replace the label with `VISUAL`/`V-LINE`; **keep the label and render a marker after the indicator** | `InputMode::label` is UNCHANGED: it returns `COPY` for `Copy(_)`. The statusline renders the position indicator right after the label, then the marker `VISUAL` or `V-LINE` (from `s.selection()`), separated by one space and before `[Z]`, e.g. `COPY ↑12/340 VISUAL`; without history, `COPY V-LINE`. This keeps the main statusline rule that the indicator follows the mode label. The marker width is subtracted from the path budget, and narrow widths clip it without panic. |

## Data Flow

```
key ─> App::on_key ─ COPY ─> CopyState::on_key(&key, repeat)
        Move(m) ─> line_end? ─> apply(m, geo) ─> ensure_visible ─> pane.set_scrollback
        Yank    ─> pane.read_lines(span) ─> copy::join ─> copy::osc52
                     Ok  ─> [Effect::SetClipboard] + notice + TERMINAL
                     Err ─> notice "too large", stay
Pty output ─> Pane::feed (probe: first_line/generation) ─> App::sync_copy (CS-5)
Panes::apply(SetClipboard) ─> host_out ─tick─> flush_host(stdout)
render: TerminalView(+highlight) ─ cursor at copy_cursor ─ marker VISUAL/V-LINE after indicator
```

## File Changes

| File | Action | Description |
|---|---|---|
| `src/core/pane.rs` | Modify | probe in `feed`, `first_line`, `line_generation`, `live_cursor`, `line_end`, `read_lines` |
| `src/core/copy.rs` | Modify | `Point`, `Selection(Kind)`, `Geometry`, `LineSpan`, `LineText`, the rewritten `CopyState`, `join`, `encode`, `osc52` |
| `src/app.rs` | Modify | enter, `on_copy_key`, `sync_copy`, `SetClipboard`, notices, `copy_cursor`/`copy_highlight` |
| `src/runtime.rs` | Modify | `host_out`, `flush_host`, SetClipboard arm |
| `src/ui/components/terminal_view.rs`, `src/ui/mod.rs`, `statusline.rs` | Modify | highlight, COPY cursor, marker snapshots |

## Interfaces / Contracts

```rust
// core::copy (pure)
pub struct Point { pub line: usize, pub col: u16 }
pub enum SelectionKind { Char, Line }
pub struct Geometry { pub first_line: usize, pub history: usize, pub rows: u16, pub cols: u16 }
pub struct LineSpan { pub first: usize, pub last: usize, pub first_col: u16, pub last_col: u16 }
pub struct LineText { pub text: String, pub wrapped: bool }
pub enum CopyCommand { Move(CopyMotion), Yank, Exit, Ignore }
pub fn join(rows: &[LineText]) -> String;
pub fn osc52(text: &str) -> Result<Vec<u8>, TooLarge>; // OSC52_MAX_PAYLOAD = 100_000
// core::pane
fn first_line(&self) -> usize; fn line_generation(&self) -> u32;
fn live_cursor(&self) -> Option<(u16, u16)>; fn line_end(&mut self, line: usize) -> u16;
fn read_lines(&mut self, span: LineSpan) -> Vec<LineText>;
// app: Effect::SetClipboard(Vec<u8>)
```

## Testing Strategy (strict TDD; `cargo test`, `clippy --all-targets`, `fmt --check` on every slice)

| Layer | What | Approach |
|---|---|---|
| Pane | the probe keeps every existing scrollback test green; `first_line` counts evictions at a small cap with offset 0 and >0; a screen flip and a resize bump the generation; `read_lines` across history and screen, with offset restored, wide characters at both edges, and wrap flags | `Pane::new(.., cap 5)` + `feed_lines` |
| Copy core | key map and Repeat, each motion with clamps and edge scrolling, v/V/Esc states, `span` ordering, `contains`, `join` (wrap, trim, no trailing `\n`), RFC 4648 vectors, the cap boundary (99,999 / 100,000 / 100,004) | plain `#[test]` |
| App | entering at the live or fallback cursor, yank effect bytes, exit and notice, too large stays, Repeat `y` swallowed, revalidation (eviction, generation, resize, sibling exit) | `update` |
| Runtime | apply buffers bytes; `flush_host` into a `Vec<u8>` | unit |
| UI | highlight cells, inverse XOR, cursor placement, marker after indicator | `TestBackend` + insta |

## Migration / Rollout

No migration is needed. Feature-branch chain, 400-line budget per slice:

| # | Content | Est. | Depends |
|---|---|---|---|
| CS1 | Pane line model (probe, `first_line`, generation, `live_cursor`, `line_end`, `read_lines`) + `LineSpan`/`LineText`/`join` | ~340 | — |
| CS2 | `CopyState` cursor and selection, motions, keys, edge scrolling; App enter, `on_copy_key` (Yank is a no-op), `sync_copy`; COPY cursor render; existing copy tests rewritten | ~390 | CS1 |
| CS3 | `encode`/`osc52`, `SetClipboard`, runtime buffer and flush, notices, highlight, marker | ~360 | CS2 |

CS2 is close to the limit. If it goes over, split it into CS2a (pure `CopyState` with the old `apply` kept as a thin adapter) and CS2b (App and UI wiring).

## Risks

- **OSC 52 under tmux** needs `set -g set-clipboard on` (or `external`) in tmux; with `off` the sequence is dropped and the yank silently copies nothing. Document it for users.
- The probe in `Pane::feed` (CS-2) depends on vendored vt100 behavior (`grid.rs:571`, `screen.rs:652`). The existing scrollback tests must stay green; a vt100 bump needs a re-check.
- Terminals that cap or disable OSC 52 may lose the copy silently. The 100,000-byte cap is a heuristic, not a guarantee.
- CS2 is close to the 400-line budget (see Migration).

## Open Questions

- [x] Spec alignment resolved by the orchestrator: wrapped rows keep trailing spaces (CS-6), Repeat is dropped for `v V y Esc q i` (CS-4), `Copied 1 character` singular (CS-10), the label stays `COPY` with the marker after the indicator (CS-12).
- [x] Screen switch: CS-5 drops the selection through `line_generation`; the spec has the scenario "Screen switch drops it".
- [x] `$`: resolved by the orchestrator in favour of CS-3, the last non-blank column (column 0 on a blank row). The copy-mode-scrollback delta now says so, with the scenarios "Horizontal motion" (`echo hi`: col 6) and "End of a blank row".
