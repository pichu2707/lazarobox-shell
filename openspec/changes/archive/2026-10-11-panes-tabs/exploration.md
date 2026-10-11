# Exploration: panes-tabs

> Mirror of engram `sdd/panes-tabs/explore`. Splits, tabs, keybindings, OSC 7 cwd and `App` encapsulation.

## Current state (verified in code)

- **`App`** (`src/app.rs`) owns one `Pane`. Its fields `input` and `dirty` are `pub`, and it also holds `size`, `cwd`, `home` and `shell_name`.
  - `Effect` is `WritePty(Vec<u8>) | ResizePty(PaneSize) | Quit`. None of these say which pane they target.
  - `PtyEvent::Exited` maps to `Quit`.
  - `pane_size()` reserves one row for the statusline.
- **`runtime.rs`** runs one `spawn_portable`, one `mpsc(256)` and one `select!` loop. The loop handles crossterm events, PTY events (with `EVENT_BUDGET` 256), shutdown, a 1 s `/proc` cwd poll, and a 16 ms tick gated by `dirty`.
  - `run_effects` takes a single `&mut dyn PtyHandle`.
- **PTY adapter** (`portable.rs`) runs 3 std threads per PTY: reader, writer and waiter.
  - `kill()` sends SIGHUP, waits up to 300 ms, then sends SIGKILL with `killpg`. It also runs on `Drop`, and can block for about 2.3 s.
  - Closing a pane must therefore never kill on the event loop.
- **`Pane`** wraps a vt100 `Parser<Responder>`. `Responder` answers DA1 and DSR, and records DECSCUSR.
- **`prefix.rs`**: `PREFIX_BINDINGS` holds Ctrl+Space, `q` and `[`.
  - Lookup is single-key with exact modifiers.
  - Tests enforce a unique chord and a unique action per binding.
- **OSC 7 hook (verified in vt100 0.16.2)**: `Callbacks::unhandled_osc(&mut Screen, params: &[&[u8]])` receives OSC 7 as `[b"7", uri]`. Things to handle:
  - Params are split on `;`, so rejoin `params[1..]`.
  - Percent-decode the URI.
  - Validate the host (empty or local).
  - Require an absolute path.

## Options compared

| Option | Pros | Cons | Effort |
|---|---|---|---|
| **A. Layout tree in core, `HashMap<PaneId, Pane>`, `Vec<Tab>`, one shared tagged channel (recommended)** | Pure and testable. Matches tmux, WezTerm and zellij. Keeps the single `select!` branch and the event budget. | Large refactor. Test churn on `a.pane`, `a.input` and `a.dirty`. | High, but can be sliced |
| B. One mini-`App` per tab | Tabs are isolated | Duplicates input and prefix state. Cross-tab events are awkward. | Medium |
| C. Fixed grid or preset layouts | Trivial | No arbitrary nvim-like splits and no per-split resize | Low |
| D. One channel per pane | Isolation | Needs `StreamMap`. Loses ordering and the budget. No real gain. | Medium |
| E. Flat list of rects | Simple | Close and resize semantics are ill-defined | Medium |

**Prior art:**

- **tmux:** layout tree, pane ids that are never reused, windows act as tabs.
- **zellij:** tiled tree plus a sticky resize mode.
- **WezTerm:** each tab holds a binary pane tree with numeric pane ids.

The lesson from all three: keep the pane id independent of the pane's position in the tree.

## Recommended approach

1. **Layout.** A pure `core::layout` module, free of ratatui types.
   - Leaves are `PaneId`, a `u32` newtype that is monotonic and never reused.
   - Inner nodes are `Split { dir, integer weights }`.
   - `layout(tree, rect)` returns the pane rects and the separators.
   - The focus neighbour for h/j/k/l is chosen by geometry.
   - A split is refused when a child would end up below the minimum size.
2. **Borders.** 1-cell separators between siblings, tmux-style, with no outer border. The separator next to the focused pane uses the accent color.
3. **Events and effects.**
   - The PTY event becomes `AppEvent::Pty(PaneId, PtyEvent)`.
   - Effects become `Effect::{WritePty(id, ..), ResizePty(id, ..), SpawnPane(id, SpawnSpec), ClosePane(id), Quit}`.
   - `update` allocates ids and creates the emulator `Pane`. It stays pure.
   - The runtime owns `HashMap<PaneId, Box<dyn PtyHandle>>`. It spawns, and it kills on a detached thread.
   - Events for closed ids are dropped.
4. **Channel.** One shared mpsc. The sink closure captures the `PaneId`. A flooding pane shares the 256 slots with the others; that is FIFO and acceptable.
5. **Shell exit.**
   - The pane is removed and its sibling expands.
   - Focus moves to the nearest pane.
   - The last pane of a tab closes the tab.
   - The last tab quits the app.
6. **COPY mode.** Per pane, because scrollback is per emulator. Changing focus exits COPY.
7. **Cursor.** Only the focused pane sets the cursor and its DECSCUSR shape.
8. **Statusline.** Shows the cwd and shell of the focused pane in the active tab. How tabs are displayed is a product question.
9. **Redraw.** The 16 ms tick and a single `dirty` flag are enough.
   - Render only the active tab, but keep feeding the parsers of hidden tabs.
   - A per-pane dirty flag is deferred.
10. **`App` encapsulation.** Private fields, accessors, and `take_dirty()` called by the runtime tick. Diff-based redraw is rejected because ratatui already diffs.
11. **OSC 7.** `Responder` records the cwd. `Pane` exposes it per pane. The `/proc` poll stays as a per-pane fallback, because bash does not emit OSC 7 by default.

## Keybindings

All bindings follow the Ctrl+Space prefix and are single keys. None conflict with the existing `q`, `[` and Ctrl+Space.

| Key | Action |
|---|---|
| `s` / `v` | Split below / to the right (nvim `:split` / `:vsplit`). Optional aliases: `-` and `\|`. |
| `h` `j` `k` `l` | Move focus |
| `H` `J` `K` `L` | Resize. Chords need the SHIFT modifier. |
| `x` | Close pane |
| `z` | Zoom toggle |
| `c` | New tab |
| `n` / `p` | Next / previous tab |
| `1`..`9` | Go to tab N (`PrefixAction::GotoTab(u8)`) |
| `X` | Close tab |

A sticky resize mode would need a new `InputMode::Resize`; it is deferred. Repeat events stay ignored in PREFIX.

## Proposed slicing

Each slice is at most about 400 lines, tests included, under strict TDD.

| Slice | Content | Size |
|---|---|---|
| S1 | `App` encapsulation and `PaneId` plumbing, no behavior change | ~350 |
| S2 | Pure `core::layout` | ~400 |
| S3 | Multi-pane `App` and runtime registry. Bindings `s v h j k l x`. Off-loop kill. May split into 3a (app) and 3b (runtime). | — |
| S4 | UI: layout render, separators, focus highlight, focused-only cursor, per-pane resize | ~300 |
| S5 | Resize and zoom | ~250 |
| S6 | Tabs | ~400 |
| S7 | OSC 7 per-pane cwd, plus the `/proc` fallback | ~250 |

The first slice users can see is S1 to S4: split, focus and close.

## Risks

- Killing a pane blocks for about 2.3 s, so it must run off the event loop.
- Every layout change sends SIGWINCH to every pane's shell.
- In tiny terminals, splits must be refused.
- Encapsulation causes test churn, about 15 call sites.
- Every reader shares the same channel backpressure.
- OSC 7 input is untrusted and must be validated.

## Open product questions

1. Tab display: an always-visible bar, a bar only when there is more than one tab, or a statusline segment?
2. Split keys: nvim `s`/`v`, tmux `-`/`|`, or both?
3. Closing a pane with `x`: ask for confirmation or close immediately? What if a job is running?
4. Resize: one step per prefix press, or a sticky resize mode?
5. Working directory of a new pane or tab: inherit the focused pane's cwd, or use the launch directory or `$HOME`?
6. When a shell exits: collapse its pane, or keep a "dead pane" visible until the user closes it?
