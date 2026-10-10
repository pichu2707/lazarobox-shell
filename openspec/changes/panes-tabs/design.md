# Design: Panes and Tabs (Slice 2)

## Technical Approach

This keeps the functional-core / imperative-shell split from slice 1 and follows exploration Option A.

- **Pure core.** A new pure `core::layout` owns the geometry: a binary split tree, integer weights, and its own `Rect`. `App` owns `HashMap<PaneId, PaneState>` plus `Vec<Tab>`, allocates ids, creates emulators, and computes every rect. `update` stays pure.
- **Shell.** `runtime.rs` owns a `Panes` registry (`HashMap<PaneId, Box<dyn PtyHandle>>`) and one shared tagged channel. It kills panes on detached threads. Effects and events are all addressed by `PaneId`.
- **UI.** The UI only draws what `App` computes, so geometry has one source of truth.
- **Prefix keys.** The prefix table becomes a static tree of groups and descriptions, which a later `?` viewer can render.
- **Boundaries.** vt100 stays inside `core/pane.rs`. portable-pty stays inside `core/pty/portable.rs`. ratatui does not enter `core::layout`.

> Note: the 800-word budget in the skill is exceeded on purpose. The orchestrator asked for concrete interfaces for 13 areas.

## Architecture Decisions

| # | Topic | Options | Decision / rationale |
|---|---|---|---|
| 1 | `App` encapsulation | (a) keep `pub` fields; (b) private fields + accessors | **(b)**. Fields become private. Accessors: `input()`, `take_dirty()`, `focused_pane()`, `pane(id)`, `view()`, `screen()`, `cwd_label()`, `shell_name()`, `cursor_shape()`. `take_dirty()` is `mem::take(&mut self.dirty)` and the runtime tick calls it. In-module tests also migrate to the public API, because `a.pane` disappears in S5a and they would break twice otherwise. The migration is mechanical: `a.input`→`a.input()`, `a.pane`→`a.focused_pane()`, `a.dirty=false`→`a.take_dirty();`, `a.size`→`a.focused_size()`. |
| 2 | `App::new` input | body `PaneSize` vs terminal size | `App::new(cols, rows)` takes the **terminal** size. `App` now computes the tab bar, the body and the statusline, so it needs the outer size. Tests use `App::new(80, 25)`, which keeps the old 24-row body. `pane_size()` is kept as the body size without a tab bar. |
| 3 | `PaneId` | index, `Uuid`, `u32` newtype | `pub struct PaneId(u32)`, `Copy + Eq + Hash + Ord`, in `core::layout`. It is allocated in `update` from `next_id`, starting at 1, with `checked_add(..).expect(..)`. Ids are never reused, so stale events can never hit a new pane. |
| 4 | Tree shape | n-ary vs binary | **Binary** `Split { axis, weights: [u16; 2], children: [Node; 2] }`. It matches nvim (`:vsplit` halves the current window). Close is a plain collapse. Each split has exactly one separator, which makes resize unambiguous. |
| 5 | Weights and rounding | floats, ratios, cells | Integer `u16` weights. `avail = extent - 1` (the separator); `first = avail * w0 / (w0 + w1)` in `u32`, clamped to `1..=avail-1` when `avail >= 2`; `second = avail - first`. The extent is degenerate when `avail < 2`: the first child gets everything and the second gets a zero-size rect. That case never panics, and the PTY size is clamped to at least 1x1. At split time the weights equal the real cell sizes: existing = `avail/2`, new = the rest. |
| 6 | Minimum pane | 1x1, 2x10, 3x20 | **2 rows x 10 cols** (`MIN_PANE`). 2 rows fit a prompt plus one output line. 10 cols fit a short prompt and `ls` names. Anything smaller is unusable, but tiny terminals still allow one split. A split needs `width >= 21` (side by side) or `height >= 5` (stacked). A refused split is a no-op. Resize steps refuse to shrink a subtree below its minimum extent. A terminal that shrinks below the minimum just degrades (rule 5): no panic or underflow, a zero-size layout rect is possible, and every PTY size is clamped to at least 1x1. |
| 7 | Neighbour h/j/k/l | MRU (tmux), center distance, overlap | **Geometric overlap.** A candidate is edge-adjacent across the 1-cell separator (for `h`: `c.x + c.width + 1 == f.x`) and must overlap on the perpendicular axis. Pick the largest overlap. **Tie-break:** the lowest perpendicular start (`y` for h/l, `x` for j/k). With no candidate the key is a no-op, with no wrap. This is deterministic and needs no history. |
| 8 | Resize step | tmux border preference, nearest ancestor | Use the separator of the **nearest ancestor split whose axis matches the key**. In a binary tree that separator always touches the focused pane. The key gives the direction the separator moves. Before the step, the weights are normalised to the current cell sizes, then moved by ±1, so a step is always exactly 1 cell even after a terminal resize. The step is refused if the shrinking side would fall below its `min_extent`. The focused pane may therefore grow or shrink: `h` on a left-hand pane moves its right border left. The step is a no-op when no ancestor split has a matching axis (for example a single pane, or `h` in a tab that only has stacked panes). |
| 9 | Zoom | separate tree vs flag | `Tab.zoom: bool`. While zoomed, `Tab::tiling` returns only the focused pane over the whole body, with no separators. Split, focus, `w r`, and any pane removal unzoom first. Panes hidden by zoom are not in the tiling, so their stored size is unchanged (no `ResizePty` for them). The statusline shows `[Z]` while the active tab is zoomed (`App::zoomed()`). |
| 10 | Close focus | sibling's first leaf, MRU, geometric | Close only when the focused pane is the one being removed. After the collapse, the new focus is `pane_at(old_rect.x, old_rect.y)`: the pane that now covers the removed pane's top-left cell. Removing a pane that is not focused keeps the focus. The same rule applies when a spawn failure removes a pane. |
| 11 | Prefix model | flat table + ad-hoc groups; nested static tree | **Static tree.** `Binding { chord, description, target: Target::{Action(PrefixAction), Group(&'static Group)} }` and `Group { label, bindings }`. It is all `const`, so it can be the single source for the future `?` viewer and for the key hint (ADR 28). `lookup(table, key) -> Step::{Run, Enter, Cancel}`. Group labels (shown in the mode block while the group is pending): `w` = `WINDOW`, `t` = `TAB`, `g` = `GO`, `b` = `BUFFER`. Binding descriptions are short because the hint reuses them: `v` "split right", `h` "split below", `q` "close", `r` "resize", `z` "zoom", `n` "new", `c` "close", `b` "next", `B` "prev", `1`..`9` "tab N". |
| 12 | Shift letters (`g B`) | require SHIFT in the table; ignore SHIFT on `Char` | `KeyChord::matches` **drops SHIFT for `Char` codes**, because the case already carries it. Terminals report `B` with and without SHIFT. Table entries use `Char('B')` + `NONE`. Ctrl chords stay exact. The `y` confirmation keeps its own strict check, `Char('y')` + `NONE`, so `Y` still declines. |
| 13 | `?` | leave unbound vs reserved entry | Reserved entry `PrefixAction::ShowCommands` with the description "Command viewer (coming soon)". For now it returns to TERMINAL. Being a table entry makes the uniqueness tests protect the key. |
| 14 | Mode model | separate variants per prompt; one parametrised variant | `InputMode::{Terminal, Prefix, Group(&'static Group), Copy(CopyState), Resize, Confirm(Confirm)}`, with `Confirm::{Quit, ClosePane, CloseTab}`. One variant means one Repeat rule, one `y` rule and one accent. Its label is derived. `w q` becomes `Confirm::Quit` when it closes the last pane of the last tab, and `t c` becomes `Confirm::Quit` when it is the only tab (prompt "Quit? (y/n)"). **Any pane removal cancels an open `Confirm`**, so `y` can never hit a pane that changed under the prompt. |
| 15 | Repeat and RESIZE key rules | — | Repeat is dropped in `Prefix`, `Group` and `Confirm`. It acts like Press in `Terminal`, `Copy` and `Resize`. Release is always ignored. In `Resize`: `h/j/k/l` step, Esc exits, and **every other key (including Ctrl+Space) is swallowed and the mode stays**. Entering RESIZE with a single pane is allowed; the steps are then no-ops (ADR 8). |
| 16 | COPY per pane; leaving COPY and RESIZE | per-pane store vs state in `InputMode` | `CopyState` stays in `InputMode::Copy` and always refers to the focused pane. **Any change of the focused pane or of the active tab exits both COPY and RESIZE** (mode becomes `Terminal`). Keys cannot cause this (COPY and RESIZE swallow them), so it happens through a removal that moves focus, such as the focused pane's shell exiting, or a spawn failure. The pane being left has its scrollback reset to 0. So "per pane" holds with no extra store. A removal that does not change focus leaves RESIZE untouched. |
| 17 | Events / effects | — | `AppEvent::{Key, Paste, Resize, Pty(PaneId, PtyEvent), Cwd(PaneId, PathBuf), SpawnFailed(PaneId, String)}`. `Effect::{WritePty(PaneId, Vec<u8>), ResizePty(PaneId, PaneSize), SpawnPane(PaneId, SpawnSpec), ClosePane(PaneId), Quit}`. `App` drops events for unknown ids (the single guard, tested purely). The runtime ignores effects for unknown ids. `ResizePty` is emitted only when a pane's stored size changes, across **all** tabs, so hidden tabs stay correctly sized. The diff runs after every `update` that can change geometry (split, close, resize step, zoom, host resize, tab bar appearing or disappearing); the stored size is the tile rect size clamped to at least 1x1. |
| 18 | Spawn failure | silent collapse, fatal, notice | The first pane is spawned before `ratatui::init` and a failure there is fatal, as today. Later failures feed `SpawnFailed(id, err)` back into `update`: the pane is removed (normal cascade) and `notice = "spawn failed: {err}"` replaces the cwd in the statusline path segment until the next key press (the notice is cleared when the next Press or Repeat key event arrives, before that key is handled; a newer notice replaces an older one). A failed `t n` removes the new tab through the same cascade and the previous tab stays active. A bad cwd is not a failure: portable-pty falls back to `$HOME`. |
| 19 | Kill | inline, detached, detached + bounded join | `ClosePane` removes the handle and moves it into a `std::thread` that runs `kill()` then drops it. The `JoinHandle`s are kept and finished ones are pruned. On quit, the runtime restores the terminal **first**, so the user gets the prompt back at once, then kills all remaining handles in parallel threads and polls `is_finished()` until `QUIT_KILL_BOUND = 3 s` (HUP 300 ms + reap 2 s + margin). `rx` is dropped before this, so readers blocked in `blocking_send` unblock. The panic path is unchanged: the hook restores, then unwinding drops the registry and `Drop` kills. The signal path reuses the quit path. |
| 20 | Channel | per pane vs shared | One `mpsc::channel::<(PaneId, PtyEvent)>(256)`. Each sink captures `id` and a `tx` clone. The runtime keeps one `tx` for future spawns, so `rx.recv() == None` cannot happen in practice and stays a defensive exit. `EVENT_BUDGET` is unchanged. |
| 21 | cwd source | OSC 7 only, `/proc` only, both | Both. `Pane::cwd()` returns the last **validated** OSC 7 path, and `PaneState.proc_cwd` holds the 1 s poll result for every pane. The effective cwd is `osc7.or(proc)`: OSC 7 wins once seen, so the poll runs for every pane but can never override an OSC 7 value. A poll that fails produces no event, so the last value is kept. It is the shell's own report, and the poll only sees the session leader anyway. New panes and tabs inherit the focused pane's effective cwd, falling back to the launch cwd. |
| 22 | OSC 7 parsing | `url`/`percent-encoding` crate vs hand-written | **Hand-written, no new dependency.** It lives in a pure `core/osc7.rs` that takes plain `&[&[u8]]`, so no vt100 type is involved. See the Interfaces section for the rules. Paths are bytes on Linux, so a decoded path that is not valid UTF-8 is accepted (displayed lossily). NUL and the other control bytes (below 0x20, and 0x7F) are rejected. |
| 23 | Hostname | `gethostname` crate, `/etc/hostname`, `libc` | `libc::gethostname`, already a dependency. It is resolved once in the runtime and passed in with `App::with_hostname`. The core stays IO-free. |
| 24 | Separator accent | fixed cyan vs mode accent | `theme.input_accent(app.input())` on the separator cells next to the focused pane. Other separator cells use `text_muted`. In RESIZE this highlights the border that is about to move. Glyphs are `│` and `─`, with no junction glyphs. |
| 25 | Tab placement | after the active tab vs at the end | **Appended at the end**, so `b N` numbers never shift when a tab is created. Closing tab *i* focuses the tab that takes index *i*, or the last tab. `g b`/`g B` wrap. `b N` beyond the count is a no-op. Closing an inactive tab (its last shell exited) keeps the active tab. |
| 27 | Focus after split | stay on the original; move to the new pane | **Move to the new pane** (nvim behaviour). The original pane keeps its content. |
| 28 | Key hint while PREFIX or a group is pending | none; hint built from the binding tables; full viewer | A one-line **minimal which-key** in the statusline path segment (user decision), shown both at the **root** (plain PREFIX) and inside a group. It is built by a pure `prefix::hint(bindings: &[Binding], max_cols) -> String`; a group passes `Group.bindings`, plain PREFIX passes `PREFIX_TREE`. Each entry is `{key label} {description}` (`KeyChord::label()`: the character itself for `Char`, so `v split right`), entries joined by ` · ` in table order. Example for `w`: `v split right · h split below · q close · r resize · z zoom`. **Clipping** is by whole entries, measured in terminal cells: entries are added while they fit; if some do not, the last fitting entry is followed by `…` (the `…` is reserved when counting). If not even the first entry fits, the hint is empty. Never a panic, never a partial entry. The mode block shows the group label in the PREFIX style and takes priority over the hint. **Root hint:** with plain PREFIX the same function renders the root of the tree, e.g. `w window · t tab · g go · b buffer · [ copy · q quit`. A group entry shows its description (`window`, `tab`, `go`, `buffer`); leaf entries show their description. The root hint lists only entries marked `hinted: true` on `Binding` (groups, `[`, `q`); focus keys `h/j/k/l`, the literal Ctrl+Space and the reserved `?` are `hinted: false` (all group bindings are `true`). `PREFIX_TREE` is ordered groups first, then `[`, `q`, then the unhinted entries, so hint order equals table order. The prefix tree stays a static table; keymap configuration stays out of scope, and so does the full `?` viewer. |
| 29 | Tab label | index only, cwd path, `N cwd-basename` | **`N cwd-basename`** of the tab's focused pane (effective cwd). Root is shown as `/`; with no known cwd the label is just `N`. Labels are clipped with `…` on overflow (see UI). |
| 30 | Statusline notice priority | — | In the path segment, precedence is: spawn-failure notice, then key hint (root hint in PREFIX, group hint in a group), then cwd. The notice is cleared by the next key, so it never coexists with a hint. |
| 26 | Screen geometry (superseded in part by ADR 32: bar and statusline edges are configurable) | ratatui `Layout` in the UI vs computed by `App` | `App::screen() -> ScreenLayout { tab_bar: Option<Rect>, body, status }` from `(cols, rows, tabs > 1)`. With the default positions the body is `y = bar as u16`, `height = rows - 1 - bar` (minimum 1), `width = cols` (minimum 1). The UI draws exactly these rects, so App and UI can never disagree. |
| 31 | COPY position indicator | `↑12/340`; `12/340`; percentage; `[12/340]` | **`COPY ↑{offset}/{total}`** inside the mode block, right after the label (before `[Z]`), like tmux's `[offset/total]` but keeping the label readable. `offset` = rows above the live bottom (`CopyState.offset`, synced from the emulator), `total` = `scrollback_len()`. At the bottom it shows `↑0/{total}` (explicit zero confirms that COPY is on and live). With no history (`total == 0`, e.g. alternate screen) just `COPY`, because `↑0/0` carries no information. The path budget shrinks by the indicator width; `App::copy_position()` is the pure source. |
| 32 | Config file and bar positions | hard-coded edges; CLI flags; `config.toml` | **`$XDG_CONFIG_HOME/lazarobox/config.toml`** (fallback `$HOME/.config/...`; empty or relative XDG or HOME ignored), read once at startup. A pure `core::config` (`Config::parse(&str)`, `Config::load(io::Result<String>) -> (Config, Option<notice>)`, `config_path(xdg, home)`) built on the already-declared `toml` + `serde`; the only IO (`fs::read_to_string`) is `runtime::load_config`. Schema: `[statusline] position`, `[tabbar] position` (`top`/`bottom`, defaults bottom/top), top-level `mouse` (reserved: parsed, validated, no effect, startup notice). Missing file is silent; an unreadable file or a TOML syntax error gives all defaults plus the `config: <short error>` notice through the existing notice slot (ADR 30). An invalid value or type falls back **per key** (user decision 2026-10-11, over whole-file fallback): the file is read as a `toml::Table` and each key is validated by hand, so a typo in one key never discards the rest; `Config::parse` returns `Parsed { config, problems }` and the notice names the first failing key (`statusline.position: expected "top" or "bottom"`) and, when several keys fail, the notice reads `config (N problems): <first problem>` so truncation never hides the count. Unknown keys are ignored (forward compatible). `App::with_config(&Config)` and `with_notice(..)` are builder-style like `with_env`, so `App::new(cols, rows)` and existing tests are untouched. `ScreenLayout::new(cols, rows, bar, BarPositions)` keeps ADR 26: the tab bar is the outermost row of its edge, the statusline sits next to the body, and the UI still draws exactly the rects of `screen()`, so the cursor offset follows `body.y` with no UI change. |

## Data Flow

```
crossterm ─┐                                  ┌─ tick 16ms: if app.take_dirty() → ui::render(&app)
PTY readers┼─(PaneId,PtyEvent)─> App::update ─┤
cwd poll 1s┘  (all panes)           │ pure    └─ Vec<Effect> → Panes::apply
                                    │              WritePty(id)  → handle.write
     PaneId alloc, Pane::new, layout│              ResizePty(id) → handle.resize   (changed sizes only)
     tiling diff → ResizePty        │              SpawnPane(id) → spawn(sink tagged id) ─ Err → AppEvent::SpawnFailed ─┐
                                    │              ClosePane(id) → remove + thread{kill; drop}                          │
                                    └──────────────────────────── feedback events ─────────────────────────────────────┘
Exit:  Pty(id, Exited) → remove leaf → collapse → (tab empty → remove tab) → (no tabs → Quit)
Quit:  loop returns (rx dropped) → restore() → Panes::shutdown(3 s bound) → exit
```

## Interfaces / Contracts

```rust
// core/layout.rs — pure, no ratatui
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)] pub struct PaneId(u32);
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)] pub struct Rect { pub x: u16, pub y: u16, pub width: u16, pub height: u16 }
pub enum Axis { X /* side by side: w v */, Y /* stacked: w h */ }
pub enum Direction { Left, Down, Up, Right }
pub const MIN_PANE: (u16, u16) = (2, 10); // (rows, cols)
pub enum Node { Leaf(PaneId), Split(Box<Split>) }
pub struct Split { pub axis: Axis, pub weights: [u16; 2], pub children: [Node; 2] }
pub struct Separator { pub axis: Axis /* line orientation */, pub x: u16, pub y: u16, pub len: u16 }
pub struct Tiling { pub panes: Vec<(PaneId, Rect)>, pub separators: Vec<Separator> }
pub enum Removal { NotFound, Removed, WasLast }
impl Node {
    pub fn split(&mut self, area: Rect, target: PaneId, new: PaneId, axis: Axis) -> Result<(), SplitError>; // TooSmall | NotFound
    pub fn remove(&mut self, id: PaneId) -> Removal;
    pub fn resize_step(&mut self, area: Rect, focus: PaneId, dir: Direction) -> bool; // S7
    pub fn leaves(&self) -> Vec<PaneId>;
}
pub fn tile(node: &Node, area: Rect) -> Tiling;
pub fn neighbour(t: &Tiling, from: PaneId, dir: Direction) -> Option<PaneId>;
pub fn pane_at(t: &Tiling, x: u16, y: u16) -> Option<PaneId>;
impl Separator { pub fn highlight(&self, focused: Rect) -> Option<(u16, u16)>; } // (offset, len) adjacent to focused

// core/prefix.rs
pub struct KeyChord { pub code: KeyCode, pub mods: KeyModifiers } // matches(): SHIFT ignored for Char; label() -> String for the hint
pub enum PrefixAction { SendPrefixLiteral, RequestQuit, EnterCopy, ShowCommands, Focus(Direction),
    SplitRight, SplitBelow, ClosePane, EnterResize, ToggleZoom, NewTab, CloseTab, NextTab, PrevTab, GotoTab(u8) }
pub struct Group { pub label: &'static str, pub bindings: &'static [Binding] }
pub enum Target { Action(PrefixAction), Group(&'static Group) }
pub struct Binding { pub chord: KeyChord, pub description: &'static str, pub target: Target, pub hinted: bool }
pub const PREFIX_TREE: &[Binding]; // w{v h q r z}, t{n c}, g{b B}, b{1..9}, [ q (hinted), then h j k l ? Ctrl+Space (unhinted)
pub enum Step { Run(PrefixAction), Enter(&'static Group), Cancel }
pub fn lookup(table: &'static [Binding], key: &KeyEvent) -> Step; // unknown/Esc ⇒ Cancel
pub fn hint(bindings: &[Binding], max_cols: u16) -> String; // ADR 28: "v split right · h split below · …" (group) or "w window · t tab · …" (root); hinted entries only, clipped by whole entries
// const fn helpers act()/group() keep the table to one line per binding.

// core/osc7.rs — pure
pub fn parse(params: &[&[u8]], local_host: Option<&str>) -> Option<PathBuf>;
fn percent_decode(bytes: &[u8]) -> Option<Vec<u8>>; // None on a bad escape
// Rules: params[0]==b"7"; raw = params[1..].join(b";"); reject if raw.len()+2 >= 1024 (the payload "7;<uri>" reaches the 1024-byte cap; vte may have truncated it);
// raw must start with "file://"; host = up to the first '/'; host ∈ {"", "localhost", local_host} (ASCII case-insensitive);
// path = from that '/', percent-decoded, starts with '/', no NUL and no other control byte (<0x20, 0x7F); non-UTF-8 bytes are accepted;
// PathBuf via OsStr::from_bytes. No canonicalisation (IO). Rejection keeps the previous cwd.

// core/pane.rs
impl Pane { pub fn with_host(self, host: Option<&str>) -> Self; pub fn cwd(&self) -> Option<&Path>; }
// Responder::unhandled_osc(&mut self, _: &mut Screen, params: &[&[u8]]) { if let Some(p) = osc7::parse(..) { self.cwd = Some(p) } }

// app.rs
pub enum InputMode { Terminal, Prefix, Group(&'static Group), Copy(CopyState), Resize, Confirm(Confirm) }
pub enum Confirm { Quit, ClosePane, CloseTab } // labels "Quit? (y/n)", "Close pane? (y/n)", "Close tab? (y/n)"
pub struct Tab { tree: Node, focus: PaneId, zoom: bool }  // tiling(area) is zoom-aware
struct PaneState { emu: Pane, size: PaneSize, proc_cwd: Option<PathBuf> }
pub struct ScreenLayout { pub tab_bar: Option<Rect>, pub body: Rect, pub status: Rect }
impl App {
    pub fn new(cols: u16, rows: u16) -> Self;
    pub fn with_env(self, shell: Option<&str>, home: Option<PathBuf>, cwd: PathBuf) -> Self; // also stores the shell path
    pub fn with_hostname(self, host: Option<String>) -> Self;                               // S9
    pub fn initial_spawn(&self) -> (PaneId, SpawnSpec);
    pub fn update(&mut self, ev: AppEvent) -> Vec<Effect>;
    pub fn take_dirty(&mut self) -> bool;
    pub fn input(&self) -> InputMode;
    pub fn screen(&self) -> ScreenLayout;
    pub fn view(&self) -> Tiling;            // active tab, absolute coords
    pub fn focused(&self) -> PaneId; pub fn focused_pane(&self) -> &Pane; pub fn pane(&self, id: PaneId) -> Option<&Pane>;
    pub fn tab_labels(&self) -> Vec<String>; pub fn active_tab(&self) -> usize;
    pub fn status_path(&self, max_cols: u16) -> String; // notice, else key hint (PREFIX or group pending), else cwd_label() of the focused pane
    pub fn zoomed(&self) -> bool;            // drives the `[Z]` statusline indicator
    pub fn shell_name(&self) -> &str; pub fn cursor_shape(&self) -> CursorShape;
}

// runtime.rs
type SpawnFn = Box<dyn FnMut(&SpawnSpec, PtySink) -> io::Result<Box<dyn PtyHandle>>>;
struct Panes { handles: HashMap<PaneId, Box<dyn PtyHandle>>, tx: mpsc::Sender<(PaneId, PtyEvent)>, spawn: SpawnFn, reapers: Vec<JoinHandle<()>> }
enum Outcome { Continue, Quit }
impl Panes {
    fn apply(&mut self, effects: Vec<Effect>) -> (Outcome, Vec<AppEvent> /* feedback */);
    fn close(&mut self, id: PaneId);               // detached kill
    fn poll_cwds(&self) -> Vec<AppEvent>;          // AppEvent::Cwd(id, path) per live pid
    fn shutdown(self, bound: Duration);            // parallel kill + bounded join
}
fn step(app: &mut App, ev: AppEvent, panes: &mut Panes) -> bool; // loops feedback until empty
fn local_hostname() -> Option<String>;            // libc::gethostname
```

The UI side:

- `ui/mod.rs` draws `screen()` and `view()`. It converts rects with `impl From<layout::Rect> for ratatui::Rect`.
- `SeparatorView` draws `│`/`─` and accents the cells in `highlight(focused_rect)`.
- Only the focused pane calls `cursor_position`.
- `TabBar` is drawn only when `screen().tab_bar` is `Some` (tabs > 1): `" N cwd-basename "` per tab (ADR 29), with the active tab on the accent. Overflow: a label wider than the bar is clipped with `…`; if the tabs together exceed the width, the bar is clipped on the right, but when that would hide the active tab, tabs are dropped from the left until the active one is visible (the active label is clipped if it alone is wider than the bar). Never a panic.
- The statusline mode block shows `TERMINAL`, `PREFIX`, `COPY`, `RESIZE`, a group label (`WINDOW`, `TAB`, `GO`, `BUFFER`) while a group is pending, or the confirmation prompt. `[Z]` follows the mode block while `zoomed()`. The path segment uses `status_path`.
- `theme.input_accent` gains: `Group` → `warning_orange` (same as `Prefix`), `Resize` → `ai_purple` (distinct from `success_green`, `warning_orange`, `primary_cyan` and `error_red`), `Confirm(_)` → `error_red`. `input_mode_style` follows the same mapping.

## Testing Strategy (strict TDD; `cargo test`, clippy, fmt per slice)

| Module | Approach |
|---|---|
| `layout` | Table-driven geometry. Rect sums equal the area and children nest inside parents. Rounding and degenerate extents (0, 1, 2) never panic. Split refused below the minimum. Collapse restores the parent rect. Neighbours, including the tie-break and no-wrap. `pane_at`. `highlight`. `resize_step` (exactly 1 cell, refusals, normalisation after a terminal resize). Zoom tiling. |
| `prefix` | Recursive walk: unique chords per group, non-empty descriptions and labels, every action reachable once, `PREFIX_KEY` absent inside groups, `?` reserved. Lookup: SHIFT-insensitive `Char`, exact Ctrl. `hint`: exact string for each group, clipping by whole entries with `…`, zero and tiny widths. The existing `q`/`[`/Ctrl+Space tests are kept. |
| `osc7` | Valid paths; `;` rejoin; `%20`/`%3B`; bad `%zz`; NUL and other control bytes; non-UTF-8 byte accepted; relative path; remote host rejected; empty host and `localhost` accepted; matching hostname (case-insensitive); 1024-byte cap; non-`file` scheme. |
| `pane` | `\e]7;file:///tmp\a` → `cwd()`; split across chunks; a rejected OSC keeps the previous value; no reply bytes. |
| `app` | Pure `update` scenarios: split/focus/close, exit cascades, stale ids dropped, `ResizePty` only on change, Repeat matrix per mode, group cancel rules, confirm kinds, prompt cancelled by a removal, COPY and RESIZE exit on a focus or tab change, RESIZE with one pane and unknown keys, focus moves to the new pane after a split, `t c` on the only tab asks Quit, cwd inheritance, `SpawnFailed` notice (shown, cleared by the next key), tabs. |
| `runtime` | `Panes` with a fake `SpawnFn` returning `FakePty` (or `Err`): spawn tags events, effects routed by id, unknown ids ignored, `close` kills off-thread (join the reapers in the test), `shutdown` kills all within the bound, feedback loop. The signal and restore tests are kept. |
| `ui` | `TestBackend` + insta: two-pane render, separator accent cells, focused-only cursor, tab bar present only with more than one tab, statusline labels for RESIZE/group/confirm, the key hint and its clipping, the `[Z]` indicator, the notice, tab labels and their overflow, separator accent per mode, body math with and without the bar. |

## File Changes

| File | Action | Description |
|---|---|---|
| `src/core/layout.rs` | Create | Tree, tiling, split/close, neighbours, resize, zoom view |
| `src/core/osc7.rs` | Create | OSC 7 parser + percent decoder |
| `src/core/mod.rs` | Modify | `pub mod layout; pub mod osc7;` |
| `src/core/prefix.rs` | Modify | Static tree, `Step` lookup, SHIFT-insensitive chars |
| `src/core/pane.rs` | Modify | `unhandled_osc`, `with_host`, `cwd()` |
| `src/core/pty/fake.rs` | Modify | Optional `pid` for poll tests |
| `src/app.rs` | Modify | Encapsulation, `PaneId` routing, panes/tabs, modes, effects |
| `src/runtime.rs` | Modify | `Panes` registry, tagged channel, feedback, off-loop kill, bounded shutdown, per-pane poll, hostname |
| `src/ui/mod.rs` | Modify | Render from `screen()`/`view()` |
| `src/ui/components/separators.rs` | Create | Separator widget |
| `src/ui/components/tab_bar.rs` | Create | Tab bar widget |
| `src/ui/components/{mod,statusline,terminal_view}.rs` | Modify | Register the widgets; statusline labels from the parametrised `InputMode` |
| `src/ui/theme.rs` | Modify | Accents for the new modes |

## Migration / Rollout — refined slicing (feature-branch-chain, merged bottom-up)

The runtime slice moves **before** the multi-pane app. It can be fully tested with a fake `SpawnFn`, and it means the app slices never emit effects that the runtime cannot handle.

| # | (proposal) | Content | Files | Est. lines | Risk |
|---|---|---|---|---|---|
| S1 | S1 | Encapsulation, `App::new(cols, rows)`, `PaneId` on events/effects (single pane), test migration. No behaviour change. | app, runtime, ui/mod, ui tests | ~330 | Medium |
| S2a | S2 | `layout`: types, `tile`, rounding, separators, `highlight`, `pane_at` | core/layout, core/mod | ~240 | Low |
| S2b | S2 | `layout`: `split` + minimum, `remove`/collapse, `neighbour` | core/layout | ~290 | Low |
| S3 | S3 | Prefix tree + `Group` mode, cancel/Repeat rules, `?`; new actions are no-ops | core/prefix, app, theme, statusline | ~380 | **High (near 400)** |
| S4 | S4b | `Panes` registry, tagged channel, `SpawnPane`/`ClosePane`/`SpawnFailed` variants, feedback, off-loop kill, bounded shutdown, per-pane poll | runtime, app (enums), pty/fake | ~370 | **High** |
| S5a | S4a | Multi-pane `App`: panes map, one `Tab`, split + focus, cwd inheritance, `ResizePty` diff | app | ~360 | Medium |
| S5b | S4a | Close: `Confirm(..)` (replaces `ConfirmQuit`), `w q`, exit collapse, stale drop, notice, COPY exit on focus change | app, theme, statusline | ~340 | Medium |
| S6 | S5 | UI: `screen()`/`view()` render, separators, focused cursor. **First visible milestone.** | ui/mod, separators, terminal_view | ~280 | Low |
| S7 | S6 | `resize_step`, RESIZE mode, zoom | core/layout, app, theme, statusline | ~330 | Medium |
| S8a | S7a | Tabs: `t n`/`t c`/`g b`/`g B`/`b N`, cascades, body math with the bar | app | ~330 | Medium |
| S8b | S7b | Tab bar widget, statusline wiring | ui/tab_bar, ui/mod | ~200 | Low |
| S9 | S8 | OSC 7: `osc7.rs`, responder hook, `Pane::cwd`, effective cwd, hostname | core/osc7, core/pane, app, runtime | ~320 | Medium |

That is 12 PRs and about 3,770 changed lines, with tests ≈1.5× code. S3 and S4 are the slices at risk. If S3 grows, move the table-walk tests and the `?` entry out. If S4 grows, split the bounded `shutdown` into its own PR.

## Resolved questions (user-approved reconcile decisions, 2026-10-06)

- [x] After a split, focus moves to the **new** pane (ADR 27).
- [x] In RESIZE, keys other than h/j/k/l/Esc are swallowed and the mode stays (ADR 15).
- [x] Separator accent uses the input-mode color (ADR 24).
- [x] Tab label is `N cwd-basename`, clipped on overflow (ADR 29).
- [x] Zoom indicator is `[Z]` in the statusline (ADR 9).
- [x] The spawn-failure notice shows until the next key (ADR 18); the spec covers it.
- [x] Key hint while a group is pending (ADR 28); `t c` on the only tab asks "Quit? (y/n)" (ADR 14); focus or tab change exits COPY and RESIZE (ADR 16); RESIZE with a single pane is allowed and a no-op (ADR 15).
