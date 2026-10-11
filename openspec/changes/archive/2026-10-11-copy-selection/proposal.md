# Proposal: Copy Selection

## Intent

COPY mode is read-only: you can scroll the scrollback but not copy from it, so users fall back to mouse selection in the host terminal, which breaks across panes. This change adds vim-style `v`/`V` selection and `y` yank to the system clipboard via OSC 52 (works over ssh).

## Decisions (made by the orchestrator in automatic mode)

- D1: COPY gets a cursor. It starts at the live cursor's position (design picks the fallback). `h/j/k/l` and `0/$` move it. The viewport scrolls only when the cursor crosses an edge. `gg`/`G` and Ctrl+d/Ctrl+u move the cursor too.
- D2: `v` selects by character and `V` by line. Pressing the same key again clears the selection; pressing the other key switches the kind. Motions extend the selection.
- D3: `y` sends the selection with OSC 52, then exits to TERMINAL at the bottom. `y` with no selection does nothing.
- D4: `Esc` with a selection clears it and stays in COPY. `Esc` with no selection exits COPY. `q` and `i` exit and drop the selection.
- D5: The text keeps a `\n` between rows, but not after the last row. Soft-wrapped rows join with no newline. Trailing blanks are trimmed on each row. Wide characters are copied once.
- D6: The statusline shows the notice `Copied N characters`, using the existing notice slot that clears on the next key. If the base64 payload is over 100,000 bytes, nothing is sent and the notice says the selection is too large.
- D7: The selection is highlighted, and the statusline shows `VISUAL` or `V-LINE`.
- D8: A resize, a focus change or leaving COPY drops the selection.

## Scope

### In Scope
- COPY cursor, `v`/`V`/`y`/`Esc` behavior, text extraction across the viewport, OSC 52 encoder with the size cap, highlight, statusline marker and notice.

### Out of Scope
- A `wl-copy`/`xclip` fallback, reading the clipboard or pasting, and mouse selection.
- Word motions (`w`/`b`/`e`), search, and block selection (Ctrl-v).

## Capabilities

### New Capabilities
- `copy-selection`: selection states, extraction rules (D5), yank via OSC 52, the cap, when the selection is dropped.

### Modified Capabilities
- `copy-mode-scrollback`: Navigation now moves a cursor (D1). Exit COPY changes how `Esc` works (D4). Cursor in COPY now shows the block at the COPY cursor. The Purpose line no longer marks selection and yank as out of scope.
- `statusline`: the `VISUAL`/`V-LINE` marker, plus the copied and too-large notices.

## Approach

From the exploration:
- `src/core/copy.rs` is pure. It holds the cursor (absolute row, column), the selection anchor and kind, extraction and the base64/OSC 52 encoder. Design decides whether to hand-roll base64 or add a crate.
- `Pane` extracts absolute rows. `vt100::contents_between` only covers visible rows, so it pages with `set_scrollback`. It uses `row_wrapped` and skips wide-character continuation cells.
- A new `Effect::SetClipboard` is written to stdout by the runtime in the event loop, the same way the cursor style is written.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/copy.rs` | Modified: cursor, selection, encoder |
| `src/core/pane.rs` | Modified: extracts absolute rows |
| `src/app.rs` | Modified: `on_copy_key`, new `Effect::SetClipboard`, notice |
| `src/runtime.rs` | Modified: writes OSC 52 |
| `src/ui/components/terminal_view.rs`, `statusline.rs` | Modified: highlight, marker |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Host terminal ignores or blocks OSC 52 | Med | Document it; a fallback is a non-goal for now |
| j/k change breaks how users navigate | Med | Viewport moves only at the edges; spec delta |
| Paging the scrollback corrupts the viewport | Med | Restore the offset; tests |
| Output arriving moves the selection | Med | Anchor on absolute rows, the same as the stable viewport. Design must handle history eviction at the cap |

## Rollback Plan

Revert the merge of the tracker `feat/copy-selection` into main. There is no config or data format change.

## Delivery Slices

Feature-branch chain, 400-line budget per PR.

| # | Content | Lines |
|---|---|---|
| CS1 | Pure core: cursor and selection, extraction, encoder | ~300 |
| CS2 | App wiring, Effect, runtime emit, notice | ~300 |
| CS3 | Highlight, statusline marker, spec deltas | ~250 |

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
- [ ] Manual checks in Kitty:
  - [ ] `v`, a motion and then `y` pastes the exact text elsewhere
  - [ ] after a line yank, pasting into a shell runs nothing
  - [ ] a yank over ssh works
  - [ ] an oversized selection shows the too-large notice

## Proposal question round

The user could not be asked. These assumptions need review:
1. Should the cursor start at the live cursor, or on the bottom row?
2. Should `y` leave COPY (as decided), or stay in it like tmux `copy-selection-no-clear`?
3. Is 100 KB the right cap? Some terminals drop large OSC 52 payloads without any warning.
4. Should a line yank really leave out the trailing newline?
