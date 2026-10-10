# Tasks: Panes and Tabs (Slice 2)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~3,800 total (range 3,500-4,200, tests ~1.5x code), 12 PRs of 200-410 lines |
| 400-line budget risk | High (S3 and S4) |
| Chained PRs recommended | Yes |
| Suggested split | S1 -> S2a -> S2b -> S3a -> S3b -> S4 -> S5a -> S5b -> S6 -> S7 -> S8a -> S8b -> S9 |
| Delivery strategy | ask-on-risk |
| Chain strategy | feature-branch-chain |

Decision needed before apply: Resolved (2026-10-06). The user chose to split S3 into S3a/S3b up front, giving 13 PRs. S4 stays whole and only splits (S4b shutdown) if it exceeds 400 lines during apply.
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

HIGH-risk slices and fallback splits:
- **S3 (~410 with the root hint, over budget)**: split into **S3a** (`prefix.rs` tree, `lookup`, SHIFT rule, `hint` incl. root, table-walk tests; ~230) and **S3b** (`Group` mode in `app`, cancel/Repeat rules, theme, statusline label; ~200). Branches `feat/panes-tabs-03a-prefix-table` -> `feat/panes-tabs-03b-group-mode`.
- **S4 (~370)**: if over 400, move the bounded `Panes::shutdown` (task S4.7-S4.8) to its own PR `feat/panes-tabs-04b-shutdown` based on S4.

The "Decision needed" is Yes because `ask-on-risk` applies: the orchestrator must confirm S3/S4 handling (pre-split S3 into 3a/3b, or accept `size:exception`) before apply. The chain strategy is already chosen.

### Suggested Work Units (merge BOTTOM-UP into the tracker `feat/panes-tabs`)

| Slice | Branch | Base | Est. lines | Spec coverage |
|---|---|---|---|---|
| S1 | `feat/panes-tabs-01-encapsulation` | `feat/panes-tabs` | ~330 | modal-input: Passthrough (WritePty to focused pane), Paste/resize (single pane); no behaviour change |
| S2a | `feat/panes-tabs-02a-layout-tile` | S1 branch | ~240 | pane-layout: Ids never reused, Rects tile, Tiny terminal never panics, separators geometry |
| S2b | `feat/panes-tabs-02b-layout-ops` | S2a branch | ~290 | pane-layout: Split (all 5), Geometric focus (3), Close and collapse (4) |
| S3a | `feat/panes-tabs-03a-prefix-table` | S2b branch | ~230 | modal-input: Prefix tree (table, lookup, SHIFT); statusline: hint function (group and root) |
| S3b | `feat/panes-tabs-03b-group-mode` | S3a branch | ~200 | modal-input: Group cancellation, Prefix key, Repeat/Release; statusline: Mode color mapping |
| S4 | `feat/panes-tabs-04-runtime-panes` | S3b branch | ~370 (HIGH) | terminal-session: Per-pane PTY registry, Spawn failure (runtime part), Non-blocking close, Per-pane cwd poll, Quit leaves no orphan |
| S5a | `feat/panes-tabs-05a-multipane` | S4 branch | ~360 | pane-layout: Split via keys; terminal-session: Split inherits cwd, Resize (changed panes only), Stale events dropped; modal-input: Focus keys, Focus follows new pane |
| S5b | `feat/panes-tabs-05b-close` | S5a branch | ~340 | modal-input: Close confirmations, Quit confirmation, Focus change exits COPY; copy-mode: all; terminal-session: Failed split, Notice clears, Shell exit |
| S6 | `feat/panes-tabs-06-ui` | S5b branch | ~280 | pane-layout: Separators; terminal-emulation: Focused-only cursor, Two panes clipped; statusline: Group/Root hint, Notice, Confirmation prompts, Live cwd, Shell segment; [manual] milestone |
| S7 | `feat/panes-tabs-07-resize-zoom` | S6 branch | ~330 | pane-layout: Resize, Zoom; modal-input: RESIZE mode; statusline: Zoom indicator, RESIZE label; terminal-session: Zoom resizes only the zoomed pane |
| S8a | `feat/panes-tabs-08a-tabs` | S7 branch | ~330 | tabs: Tab lifecycle, Tab navigation, Tab state independent, Last tab quits; terminal-session: Hidden tabs stay sized; Failed new tab |
| S8b | `feat/panes-tabs-08b-tab-bar` | S8a branch | ~200 | tabs: Tab bar (all), Tab label; [manual] tabs milestone |
| S9 | `feat/panes-tabs-09-osc7` | S8b branch | ~320 | terminal-emulation: OSC 7 (all); terminal-session: OSC 7 wins, Fallback for plain shell; tabs: New tab inherits cwd |

Every PR must be green: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. Strict TDD: RED (failing test) -> GREEN -> REFACTOR. [manual] = user-run in Kitty. Each PR states start/end, parent, follow-up, out of scope, and a dependency diagram with the current PR marked (chained-pr skill). Work-unit commits: tests and code of one slice stay together; no cross-slice commits.

## S1: Encapsulation, `App::new(cols, rows)`, `PaneId` (single pane)

- [x] S1.1 RED: `src/core/layout.rs` (new, only `PaneId(u32)` + `Rect` for now) test: `PaneId` is `Copy + Eq + Hash + Ord`; register in `src/core/mod.rs`.
- [x] S1.2 GREEN: define `PaneId`, `Rect`.
- [x] S1.3 RED: `src/app.rs` tests via accessors only (`input()`, `take_dirty()`, `focused_pane()`, `screen()`), `App::new(80, 25)` body is 24 rows [modal-input: Passthrough, Resize in any mode].
- [x] S1.4 GREEN: make `App` fields private; add accessors, `take_dirty`, `screen()` (no bar), `initial_spawn()`; `AppEvent::Pty(PaneId, ..)`, `Effect::{WritePty,ResizePty}(PaneId, ..)` with one pane id 1.
- [x] S1.5 REFACTOR: migrate in-module tests (`a.input`->`a.input()`, `a.dirty=false`->`a.take_dirty();`, `a.size`->`a.focused_size()`).
- [x] S1.6 RED+GREEN: `src/runtime.rs` uses `take_dirty()` and tagged events (single handle); `src/ui/mod.rs` uses accessors; existing snapshots unchanged.
- [x] S1.7 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S2a: Layout tile (pure)

- [x] S2a.1 RED: `src/core/layout.rs` table tests for `tile`: rect sums equal the area incl. separators; `avail = extent - 1`; rounding; degenerate extents 0/1/2 [pane-layout: Rects tile the area, Tiny terminal never panics].
- [x] S2a.2 GREEN: `Axis`, `Node`, `Split`, `Separator`, `Tiling`, `tile`. `Direction` and `MIN_PANE` were intentionally moved to S2b, where they are first used.
- [x] S2a.3 RED+GREEN: `pane_at`, `Separator::highlight(focused)` (offset,len adjacent to focus) tests.
- [x] S2a.4 RED+GREEN: PaneId allocation helper test: ids start at 1, never reused [pane-layout: Ids are never reused].
- [x] S2a.5 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S2b: Layout split, remove, neighbour

> Note: RED evidence was compile-only for the initial tasks. Behaviour coverage was confirmed by review mutation testing (41/45 killed); the survivors were fixed in a follow-up.

- [x] S2b.1 RED: `Node::split` tests: right/below, weights equal cell sizes, 21 cols / 5 rows applied, one less refused (`TooSmall`) [pane-layout: Split right, Split below, Refused below minimum, Minimum split sizes].
- [x] S2b.2 GREEN: `Direction`, `MIN_PANE`, `Node::split` + `SplitError`.
- [x] S2b.3 RED: `Node::remove` tests: sibling expands, `WasLast`, `NotFound`, parent rect restored [pane-layout: Sibling expands, Last pane closes the tab].
- [x] S2b.4 GREEN: `Node::remove`, `leaves`.
- [x] S2b.5 RED: `neighbour` tests: adjacency across 1-cell separator, largest overlap, lowest-start tie-break, no wrap [pane-layout: Move focus, Edge no-op, Geometry beats tree order]; close-focus rule via `pane_at(old.x, old.y)` [Focus goes to the pane covering the old top-left].
- [x] S2b.6 GREEN: `neighbour`; it skips zero-area rects (legal per ADR 5/6) so focus never lands on an invisible pane (test included).
- [x] S2b.7 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S3: Prefix tree + Group mode

Split up front: S3a covers S3.1–S3.5 (+ S3.9 final checks) on `feat/panes-tabs-03a-prefix-table`; S3b covers S3.6–S3.8 (+ S3.9) on `feat/panes-tabs-03b-group-mode`. Root hint shows only hinted entries: groups, `[` and `q` (user decision).

- [x] S3.1 RED: `src/core/prefix.rs` recursive walk tests: unique chords per group, non-empty descriptions/labels, `PREFIX_KEY` absent in groups, `?` reserved, every action reachable once [modal-input: Table integrity].
- [x] S3.2 GREEN: `KeyChord` (SHIFT dropped for `Char`, `label()`), `PrefixAction`, `Group`, `Target`, `Binding { .., hinted }`, `PREFIX_TREE` (groups, `[`, `q`, then unhinted), `Step`, `lookup`.
- [x] S3.3 RED: `lookup` tests: `B` with/without SHIFT, exact Ctrl, Esc/unknown -> Cancel; existing `q`/`[`/Ctrl+Space tests kept [modal-input: Shift on `g B`, Cancel/unknown].
- [x] S3.4 RED: `hint(bindings, max_cols)` exact strings for `w`, `t`, `g`, `b` and the ROOT `w window · t tab · g go · b buffer · [ copy · q quit`; clipping by whole entries with `…`; width 0 and tiny; unhinted entries absent [statusline: Hint is generated from the table, Hint clipped by whole entries, Root hint clipped, Narrow width].
- [x] S3.5 GREEN: `prefix::hint` (same function for group and root).
- [x] S3.6 RED (S3b): `src/app.rs` tests: `InputMode::Group`, group opens, binding in a group returns to TERMINAL (new actions are no-ops), Esc/unmapped/Ctrl+Space cancel swallowed, Repeat ignored in Prefix/Group/Confirm, `?` returns to TERMINAL with no effect [modal-input: Group opens, Binding in a group, Group cancellation x3, Reserved `?`, Repeat ignored].
- [x] S3.7 GREEN: `InputMode::{Group, Resize, Confirm(Confirm)}` enum shape (`Confirm::Quit` replaces `ConfirmQuit`), `App::update` group handling.
- [x] S3.8 RED+GREEN: `src/ui/theme.rs` `input_accent`/`input_mode_style`: Group `warning_orange`, Resize `ai_purple`, Confirm `error_red`; `statusline.rs` label snapshots for `WINDOW`/`TAB`/`GO`/`BUFFER`/`RESIZE` [statusline: Label and color, Mode color mapping, Existing snapshots].
- [x] S3.9 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S4: Runtime `Panes` registry (fake `SpawnFn`)

- [x] S4.1 RED: `src/runtime.rs` tests with fake `SpawnFn`: spawn tags events by id; `WritePty`/`ResizePty` routed by id; unknown ids ignored [terminal-session: Output routed by id, Stale events dropped].
- [x] S4.2 GREEN: `AppEvent::{Cwd, SpawnFailed}`, `Effect::{SpawnPane, ClosePane}` variants (app emits none yet); `Panes { handles, tx, spawn, reapers }`, `apply`, one `mpsc::channel::<(PaneId, PtyEvent)>(256)`.
- [x] S4.3 RED: spawn `Err` yields `AppEvent::SpawnFailed` fed back; `step` loops feedback until empty [terminal-session: Spawn failure].
- [x] S4.4 GREEN: feedback loop in `step`; `src/core/pty/fake.rs` optional `pid`.
- [x] S4.5 RED: `Panes::close` kills off-thread (join reapers in the test); a blocking kill does not stall `apply` [terminal-session: UI stays responsive].
- [x] S4.6 GREEN: detached kill thread + pruning of finished `JoinHandle`s.
- [x] S4.7 RED: `shutdown(bound)` kills all handles in parallel within the bound; slow kill does not exceed it [terminal-session: Quit leaves no orphan].
- [x] S4.8 GREEN: `Panes::shutdown` (restore terminal first, `QUIT_KILL_BOUND = 3 s`, `rx` dropped before); panic and signal paths unchanged.
- [x] S4.9 RED+GREEN: `poll_cwds` emits `AppEvent::Cwd(id, path)` per live pid; failed poll emits nothing [terminal-session: Fallback for plain shell].
- [x] S4.10 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S5a: Multi-pane `App`: split + focus

- [ ] S5a.1 RED: `src/app.rs` tests: `w v`/`w h` split, new pane focused, `SpawnPane` carries cwd of focused pane, refused split is a no-op, focus keys h/j/k/l [pane-layout: Split right/below, Refused below minimum; terminal-session: Split inherits cwd, Unknown cwd falls back; modal-input: Focus follows the new pane, Focus keys].
- [ ] S5a.2 GREEN: `HashMap<PaneId, PaneState>`, one `Tab { tree, focus, zoom }`, `next_id` with `checked_add`, split and focus handling.
- [ ] S5a.3 RED: `ResizePty` only for panes whose stored size changed; unaffected pane untouched; stale ids dropped by `update` [terminal-session: Only changed panes resized, Unaffected pane untouched, Stale events dropped; modal-input: Resize only changed panes; Input goes to the focused pane only].
- [ ] S5a.4 GREEN: tiling diff after geometry-changing updates, size clamped to >= 1x1 (every PTY size goes through a `PaneSize` helper built from a layout rect, since zero-size rects are legal per ADR 5/6); WritePty/Paste to the focused pane.
- [ ] S5a.5 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S5b: Close, confirmations, exit cascade

- [ ] S5b.1 RED: `w q` prompts "Close pane? (y/n)" (or "Quit? (y/n)" for the last pane of the only tab); `y` closes, `n`/Esc/`x`/`Y` decline [modal-input: Close pane confirmed/declined, Last pane prompts quit, Confirm, Decline].
- [ ] S5b.2 GREEN: `Confirm::{ClosePane, CloseTab}` handling, strict lowercase `y`.
- [ ] S5b.3 RED: `Pty(id, Exited)` collapses the pane; unfocused removal keeps focus; focused removal focuses `pane_at(top-left)`; last shell emits Quit; any removal cancels an open `Confirm` [terminal-session: One of two shells exits, Last shell exits; modal-input: Removal cancels the prompt, Last pane of one of two tabs prompts close pane].
- [ ] S5b.4 GREEN: remove-pane cascade, `ClosePane` effect, confirm cancel.
- [ ] S5b.5 RED: focus change by removal exits COPY (pane scrollback reset to 0) and RESIZE; removal that keeps focus leaves RESIZE [modal-input: Pane close in COPY, Focus change in RESIZE, Removal that keeps focus leaves RESIZE; copy-mode: Focus move exits COPY, Focused pane exits while in COPY, Cursor shape restored, Offsets are independent, Other panes keep running, Keys act on the focused pane].
- [ ] S5b.6 GREEN: mode exit on focus change.
- [ ] S5b.7 RED: `SpawnFailed(id, err)` removes the pane and sets `notice`; notice cleared by the next Press/Repeat; newer replaces older [terminal-session: Failed split removes the pane, Notice clears on next key; statusline: Notice shown, Notice cleared by next key].
- [ ] S5b.8 GREEN: `notice` field and clearing in `update`; statusline `status_path` (notice first).
- [ ] S5b.9 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S6: UI renders `screen()`/`view()` (first visible milestone)

- [ ] S6.1 RED: `src/ui/components/separators.rs` TestBackend+insta: two panes with separator `│`/`─`, accent cells only next to the focused pane, recolor on focus change, accent follows mode [pane-layout: Separators x3].
- [ ] S6.2 GREEN: `SeparatorView`; `impl From<layout::Rect> for ratatui::Rect`.
- [ ] S6.3 RED: terminal_view in two panes: clipped, cursor offset to the pane rect, only the focused pane sets the cursor, shape follows focus [terminal-emulation: Two panes clipped, Cursor offset, Unfocused pane has no cursor, Shape follows focus, Unfocused shape change ignored].
- [ ] S6.4 GREEN: `src/ui/mod.rs` renders from `screen()`/`view()`; per-pane `terminal_view`; focused-only `cursor_position`; rendering skips zero-size pane rects and separators with `len == 0`.
- [ ] S6.5 RED: statusline render snapshots: root hint in PREFIX `w window · t tab · g go · b buffer · [ copy · q quit`, group hint, clipped hint, hint gone after resolve, notice precedence, prompts, cwd/shell of the focused pane [statusline: Root hint in PREFIX, Root hint clipped and cleared, Group hint, Hint gone after the group resolves, Narrow width, Notice shown, Close pane/tab/Quit prompt, Focus switches cwd, Shell name].
- [ ] S6.6 GREEN: `App::status_path(max_cols)` (notice, else `prefix::hint` root/group, else cwd), `shell_name()` of the focused pane; wire in `ui/mod.rs`.
- [ ] S6.7 [manual] Kitty: `w v`/`w h` split, focus with prefix `h/j/k/l` (separator accent follows), nvim in two panes (typing goes only to the focused one), `w q` then `y`, `exit` collapses a pane; PREFIX shows the root hint, `w` shows the group hint, hint disappears on resolve [pane-layout: nvim in split panes, Focus/close, `exit` collapses its pane; statusline hint scenarios].
- [ ] S6.8 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S7: Resize and zoom

- [ ] S7.1 RED: `resize_step` tests: exactly 1 cell, normalisation after terminal resize, refusal at minimum, no-op without a matching axis, single pane [pane-layout: Border moves one cell, Focused pane shrinks, Clamped at minimum, No border on that axis, Single pane].
- [ ] S7.2 GREEN: `Node::resize_step`.
- [ ] S7.3 RED: `w r` enters RESIZE; `h/j/k/l` sticky and Repeat-allowed; Esc exits; other keys and Ctrl+Space swallowed; single pane allowed [modal-input: Sticky, Repeat allowed, Esc exits, Keys swallowed, Single pane RESIZE].
- [ ] S7.4 GREEN: `InputMode::Resize` handling in `update`; `ResizePty` diff after each step; accent `ai_purple`.
- [ ] S7.5 RED: `w z` toggles `Tab.zoom`; zoomed tiling is only the focused pane, no separators; split/focus/`w r`/removal unzoom first; only the zoomed pane gets `ResizePty` [pane-layout: Zoom and restore, Split/Focus change/RESIZE while zoomed; terminal-session: Zoom resizes only the zoomed pane].
- [ ] S7.6 GREEN: zoom-aware `Tab::tiling`, `App::zoomed()`.
- [ ] S7.7 RED+GREEN: statusline `[Z]` shown only while zoomed; `RESIZE` label [statusline: Zoomed, Unzoomed].
- [ ] S7.8 [manual] Kitty: `w r` + `h/j/k/l` moves the border visibly, nvim reflows, `w z` zoom/unzoom keeps nvim intact with `[Z]` [terminal-session: Reflow in nvim; pane-layout: Focus, close, resize, zoom].
- [ ] S7.9 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S8a: Tabs in `App`

- [ ] S8a.1 RED: `t n` appends and activates a tab, `SpawnPane` has the focused cwd; `b N` unaffected; `t c` then `y` closes, next tab takes index i; declined with `n`/Esc/`Y`; only tab asks Quit [tabs: New tab, Close tab confirmed, Next tab becomes active, New tab is appended, Close tab declined, Close the only tab asks Quit/quits].
- [ ] S8a.2 GREEN: `Vec<Tab>`, `active`, `NewTab`/`CloseTab` handling, `Confirm::CloseTab`.
- [ ] S8a.3 RED: `g b`/`g B` wrap; `b N` and missing tab no-op; single tab no-op [tabs: Next wraps, Previous wraps, Go to tab N, Missing tab is a no-op, Single tab navigation].
- [ ] S8a.4 GREEN: `NextTab`, `PrevTab`, `GotoTab`.
- [ ] S8a.5 RED: tab state preserved; background output parsed; inactive tab removal keeps the active tab; last pane of a tab closes it, last tab quits; tab switch exits COPY/RESIZE; failed `t n` removes the new tab [tabs: State preserved, Background output, Last shell exits, Last pane of a tab with sibling tabs; modal-input: Tab switch in RESIZE; copy-mode: Tab switch exits COPY; terminal-session: Failed new tab].
- [ ] S8a.6 GREEN: tab cascade; `screen()` with a bar (body `y=1`, `rows-2`); `ResizePty` diff across ALL tabs [tabs: Body math, Bar disappears on close; terminal-session: Hidden tabs stay sized].
- [ ] S8a.7 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S8b: Tab bar widget

- [ ] S8b.1 RED: `src/ui/components/tab_bar.rs` TestBackend+insta: hidden with one tab; two tabs with the second active; narrow width clips and keeps the active tab; no panic [tabs: Hidden with one tab, Shown with two tabs, Narrow width].
- [ ] S8b.2 RED: `App::tab_labels()`: `1 proj`, `2 tmp`, `/` for root, number only without cwd [tabs: Tab label, Label without cwd].
- [ ] S8b.3 GREEN: `TabBar` widget, `tab_labels()`, `active_tab()`, register in `components/mod.rs`, draw in `ui/mod.rs` when `screen().tab_bar` is `Some`.
- [ ] S8b.4 [manual] Kitty: `t n` shows the bar, `g b`/`g B`/`b 1` navigate and wrap, `t c` then `y`, the bar vanishes with one tab [tabs: Tabs and tab bar].
- [ ] S8b.5 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## S9: OSC 7 cwd

- [ ] S9.1 RED: `src/core/osc7.rs` `parse` tests: BEL/ST, `%20`/`%3B`, `;` rejoin, bad `%zz`, NUL/control bytes, non-UTF-8 accepted, relative path, remote host rejected, empty/`localhost`/matching host (case-insensitive), 1024-byte cap, non-`file` scheme [terminal-emulation: BEL/ST terminator, Percent-decoding, Semicolon in path, Remote host rejected, Malformed ignored, Local hosts accepted, Non-UTF-8 path bytes].
- [ ] S9.2 GREEN: `osc7::parse`, `percent_decode`; register in `core/mod.rs`.
- [ ] S9.3 RED: `Pane` feeds `\e]7;file:///tmp\a` -> `cwd()`; split across chunks; rejected OSC keeps the previous value; no reply bytes [terminal-emulation: Split across reads, No query reply].
- [ ] S9.4 GREEN: `Responder::unhandled_osc`, `Pane::with_host`, `Pane::cwd()`.
- [ ] S9.5 RED: effective cwd `osc7.or(proc)`; OSC 7 wins over the poll; poll fills for plain shells; new pane/tab inherits it [terminal-session: OSC 7 wins, Fallback for plain shell; statusline: OSC 7 cwd shown; tabs: New tab inherits cwd].
- [ ] S9.6 GREEN: `PaneState.proc_cwd`, `App::with_hostname`, `runtime::local_hostname()` via `libc::gethostname`, wiring in `runtime.rs`.
- [ ] S9.7 [manual] Kitty: `cd /tmp` in a shell with OSC 7 (or the poll) updates the statusline and the tab label; `t n` starts in `/tmp` [terminal-emulation: cwd follows `cd`; tabs: New tab inherits cwd].
- [ ] S9.8 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## Deferred (out of scope)

- The `?` command viewer (the `?` entry stays reserved; `PREFIX_TREE` is already its data source).
- Keymap configuration (the prefix tree stays a static table).
- Per-pane dirty tracking (a single `dirty` flag redraws everything).
