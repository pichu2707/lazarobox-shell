# Proposal: Panes and Tabs (Slice 2 of the modal multiplexer)

## Intent

Slice 1 "feels like plain Kitty": one shell, one pane. This slice makes lazarobox a real multiplexer with nvim muscle memory: arbitrary splits, geometric focus, sticky resize, zoom and tabs, all behind Ctrl+Space with which-key style groups (`w`, `t`, `g`, `b`). The prefix table becomes a described tree, so a later command viewer (`?`) can render it.

## Scope

### In Scope
- Pure layout tree. `PaneId` is never reused, splits use integer weights, siblings are divided by 1-cell separators, and the separator next to the focused pane uses the accent color.
- Prefix tree: `h/j/k/l` focus; `w v`/`w h` split right/below; `w q` close pane (`y`); `w r` RESIZE; `w z` zoom; `t n`/`t c` new/close tab (`y`); `g b`/`g B` next/prev tab; `b 1..9` go to tab; `[` COPY; `q` quit. `?` is reserved.
- Group rules: Esc, an unmapped key or Ctrl+Space inside a group cancels to TERMINAL, and the key is swallowed.
- Repeat events are ignored in PREFIX, group-pending and confirmations. They are allowed in TERMINAL, COPY and RESIZE.
- New panes and tabs inherit the focused pane's cwd.
- When a shell exits, its pane closes. The last pane closes the tab, and the last tab quits the app.
- COPY is per pane, and a focus change exits it. Only the focused pane draws the cursor.
- A tab bar is shown only when there is more than one tab. The statusline describes the focused pane.
- Per-pane cwd from OSC 7 (validated), with the `/proc` poll as fallback.
- `App` encapsulation: private fields, accessors, `take_dirty()`.

### Out of Scope
- The command viewer (only `?` is reserved), selection/yank, the media viewer, floating panes, session persistence, mouse, and a config file for the prefix.

## Capabilities

### New Capabilities
- `pane-layout`: layout tree, split, minimum-size refusal, close/collapse, geometric focus, RESIZE steps, zoom, separators.
- `tabs`: tab lifecycle, navigation, tab bar, and the last-tab-quits rule.

### Modified Capabilities
- `modal-input`: prefix groups, group-pending state, RESIZE mode, close confirmations, Repeat rules, the reserved `?`, focus change exiting COPY.
- `terminal-session`: multiple PTYs, per-pane spawn with the inherited cwd, shell exit closes the pane instead of quitting, kill off the event loop, per-pane resize.
- `terminal-emulation`: OSC 7 capture, rendering into a pane rect, cursor and shape only for the focused pane.
- `copy-mode-scrollback`: COPY state is per pane.
- `statusline`: the RESIZE label, group and confirmation prompts, and the focused pane's cwd and shell.
- `key-encoding`: unchanged.

## Approach

This follows exploration Option A.
- `core::layout` is pure and free of ratatui types.
- `App` holds `HashMap<PaneId, Pane>` and `Vec<Tab>`.
- Events and effects are addressed to a pane: `Pty(PaneId, ..)`, `WritePty(id)`, `ResizePty(id)`, `SpawnPane(id, spec)`, `ClosePane(id)`.
- The runtime owns the PTY registry and one shared tagged channel. It kills on a detached thread and drops events for closed ids.

## Affected Areas

| Area | Impact |
|---|---|
| `src/app.rs` | Modified: panes, tabs, modes, effects |
| `src/core/layout.rs` | New |
| `src/core/prefix.rs` | Modified: becomes a tree |
| `src/core/pane.rs` | Modified: OSC 7 |
| `src/runtime.rs` | Modified: registry, off-loop kill |
| `src/ui/components/{terminal_view,statusline,tab_bar}.rs` | Modified / New |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Kill blocks for about 2.3 s | High | Detached kill thread |
| SIGWINCH storm on layout change | Med | Resize only the panes whose size changed |
| Tiny terminals | Med | Refuse splits below the minimum size |
| Untrusted OSC 7 input | Med | Rejoin params, percent-decode, check host and absolute path |
| Test churn from encapsulation | High | S1 lands first with no behavior change |
| Over budget | High | Chained PRs (below) |

## Rollback Plan

Use a feature-branch chain on `feat/panes-tabs`. Only the tracker branch merges to main, so reverting that merge restores slice 1. There are no data or config migrations.

## Slicing

Slices merge bottom-up. Test lines are about 1.5x code lines.

| # | Content | Lines |
|---|---|---|
| S1 | Encapsulation, `PaneId` plumbing, no behavior change | ~350 |
| S2 | Pure `core::layout` | ~400 |
| S3 | Prefix tree, groups, cancel and Repeat rules; `q`/`[` migrated | ~300 |
| S4a | Multi-pane `App`: split, focus, close + confirm, exit collapse, per-pane COPY | ~400 |
| S4b | Runtime registry, tagged channel, spawn, off-loop kill, per-pane resize and poll | ~350 |
| S5 | UI: pane rects, separators, focused cursor. **First user-visible milestone** | ~300 |
| S6 | RESIZE mode and zoom | ~300 |
| S7a | Tabs core: `t n`, `t c`, `g b`/`g B`, `b N` | ~300 |
| S7b | Tab bar and statusline | ~200 |
| S8 | OSC 7 per-pane cwd | ~250 |

The total is about 3,150 lines across 10 PRs. S3 must land before S4a, because splitting needs `w v`.

## Accepted assumptions

The user's question round did not cover these. The user accepted them on 2026-10-06.
- RESIZE moves the focused pane's border by 1 cell per key, as tmux does. Holding the key repeats.
- The tab bar takes the top row.
- `g b`/`g B` wrap around.
- `b N` for a missing tab is a no-op.
- Splitting or moving focus while zoomed unzooms first.
- `w q` on the last pane of the last tab prompts "Quit? (y/n)" instead of "Close pane? (y/n)", because closing it quits the app. It quits after a lowercase `y`.
- The minimum pane size that still allows a split is left to the design (about 2 rows by 10 columns).

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
- [ ] Manual check: nvim runs in split panes; focus, close, resize and zoom work; `exit` collapses the pane; tabs and the tab bar behave as specified; `cd` updates the cwd; no orphan shells remain after quit.
