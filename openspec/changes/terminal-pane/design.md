# Design: Terminal Pane (Slice 1)

## Technical Approach

Ports-and-adapters around a pure core. `App` owns the emulator (`Pane`) and input state and is driven by `App::update(AppEvent) -> Vec<Effect>`. It does no IO. `runtime.rs` is the only impure layer. It owns the `PtyHandle`, the terminal, and the timers, and it runs the effects. No vt100 types leak past `core/pane.rs`, so the alacritty fallback only replaces that file.

## Architecture Decisions

| # | Topic | Options | Decision / rationale |
|---|---|---|---|
| 1 | Mode model | (a) add TERMINAL/PREFIX/COPY to `AppMode`; (b) a separate `InputMode` axis | **(b)**. `AppMode` answers "what am I viewing" and `InputMode` answers "where do my keys go". Mixing them causes N×M variants once AI views arrive. `AppMode` stays as it is (the theme preview uses it, and future views will too). The multiplexer does not use it yet. |
| 2 | Mode colors | new theme fields vs. reuse the palette | Reuse, following the vim analogy: TERMINAL≈insert→`success_green`, PREFIX≈pending→`warning_orange`, COPY≈normal→`primary_cyan`, CONFIRM_QUIT→`error_red`. New method `input_mode_style`. `mode_style(&AppMode)` is unchanged. |
| 3 | StatusLine API | change `new()` signature vs. add a constructor | Keep `new(theme, AppMode, path, model)`. Add `StatusLine::input(theme, InputMode, path, right)`. Internally the widget stores `label: &str, style: Style`. Existing tests and snapshots pass byte-for-byte. New snapshots get new names. |
| 4 | Emulator seam | `trait Emulator` vs. a concrete `Pane` with its own cell DTOs | Concrete `Pane` exposing `CellView`/`TermColor` (our own types). Only one implementation exists at any time, so a trait is YAGNI. The seam is the module boundary. |
| 5 | Reader thread | `spawn_blocking` vs. a detached `std::thread` | Detached thread: `spawn_blocking` would hang runtime shutdown. |
| 6 | Reader → loop | unbounded vs. bounded channel | Bounded tokio mpsc (256) with `blocking_send`. Floods like `cat big` get backpressure instead of unbounded memory growth. |
| 7 | Writer | direct write in the loop vs. a writer thread | Writer thread fed by `std::sync::mpsc`. A big paste to a stalled child must never block the UI loop. |
| 8 | Redraw | draw per event vs. dirty flag + tick | Drain all ready events (budget 256), set `dirty`, draw on a 16 ms tick (`MissedTickBehavior::Skip`). This caps output at about 60 fps under floods. |
| 9 | cwd | OSC 7 vs. `/proc/<pid>/cwd` | Poll `read_link` every 1 s in the runtime, then send `AppEvent::Cwd(PathBuf)`. The OSC 7 override comes later. |
| 10 | Prefix key | Ctrl+G (exploration) vs. Ctrl+Space (approved proposal) | **Ctrl+Space**, as approved. It is data (`PREFIX_KEY`), so changing it later is one line. |
| 11 | Copy viewport on new output | drift vs. anchored | Anchored. Before and after `feed` in COPY, compare `scrollback_len()` and add the difference to the offset (clamped). Once the 10k cap is reached the view drifts, which is accepted. |
| 12 | Cursor shape (DECSCUSR) | (a) `Effect::SetCursorShape` emitted by `update`; (b) pure derived value applied at render time | **(b)**. `Pane` stores the last requested `CursorShape` (pure DTO, no crossterm/vt100 types). `App::cursor_shape()` is a pure function: COPY → Block steady, otherwise `pane.cursor_shape()`. The runtime compares it with the last applied shape after each draw and emits `SetCursorStyle` only on change. No new Effect, `update` stays pure, and the rule is unit-testable without a terminal. |
| 13 | Cursor shape restore | restore only on clean exit vs. also on panic | Both: `restore()` emits `SetCursorStyle::DefaultUserShape` (also chained into the panic hook). Ps 0 from the child maps to `Default` and is applied the same way. |
| 14 | COPY cursor | show child's shape vs. fixed block | Fixed steady block: in COPY the viewport can be away from the child's cursor, and block matches vim normal mode. TERMINAL restores the child's last request automatically since the value is derived. |
| 15 | Alt screen in COPY | special-case vs. natural clamp | Natural clamp: there is no scrollback on the alt screen (`scrollback_len() == 0`), so offsets stay 0. Accepted limitation, same as tmux. |

## Data Flow

```
 crossterm EventStream ─┐                        ┌─> terminal.draw(ui::render(&app))  [on tick if dirty]
 PTY reader thread ─────┼─> AppEvent ─> App::update ─> Vec<Effect> ─> runtime executes:
   (blocking_send)      │   (select!, drain)  │                       WritePty -> writer thread -> PTY
 tick 16ms / cwd 1s ────┘                     │                       ResizePty -> master.resize (SIGWINCH)
                                              │                       Quit -> break loop
                         Pane.feed(bytes) -> responder replies -> Effect::WritePty
                         Pane.feed(bytes) -> DECSCUSR -> Pane.cursor_shape (state, no effect)
 draw tick: want = App::cursor_shape() (COPY => Block steady); if want != last_applied
            -> execute!(SetCursorStyle::from(want)); last_applied = want

 Shutdown: break -> handle.kill() + drop (closes master => SIGHUP) -> restore() (incl. DefaultUserShape) -> exit
 Panic:    hook = restore() then previous hook; unwinding drops PtyHandle (kill)
```

## File Changes

| File | Action | Description |
|---|---|---|
| `Cargo.toml` | Modify | `portable-pty = "0.9"`, `vt100 = "0.16.2"`, `crossterm = { version = "0.29", features = ["event-stream"] }`, `futures = "0.3"`. The lock already resolves a single crossterm 0.29.0. The direct dep only turns on `event-stream`, and the code keeps importing through `ratatui::crossterm`. |
| `src/lib.rs` | Modify | `pub mod core; pub mod runtime;` |
| `src/core/mod.rs` | Create | `pty`, `pane`, `keys`, `prefix`, `copy` |
| `src/core/pty/{mod,portable,fake}.rs` | Create | port, adapter (writer/reader threads), fake |
| `src/core/pane.rs` | Create | vt100 parser, responder `Callbacks` (DA1/DSR + DECSCUSR capture), `CellView`, `CursorShape` |
| `src/core/keys.rs` | Create | pure encoder |
| `src/core/prefix.rs` | Create | prefix key and action table |
| `src/core/copy.rs` | Create | `CopyState`, motions |
| `src/app.rs` | Modify | add `InputMode`, `App`, `AppEvent`, `Effect`; keep `AppMode` |
| `src/runtime.rs` | Create | `ratatui::init` + `EnableBracketedPaste`; `restore()` = `SetCursorStyle::DefaultUserShape` + `DisableBracketedPaste` + `ratatui::restore` (also chained into the panic hook); select loop; effect executor; applies `App::cursor_shape()` via `SetCursorStyle` when it changes |
| `src/ui/components/terminal_view.rs` | Create | `Pane` → Buffer, cursor |
| `src/ui/components/statusline.rs` | Modify | `input()` constructor, label/style fields |
| `src/ui/theme.rs` | Modify | `input_mode_style` |
| `src/ui/mod.rs` | Modify | `render(frame, &App, &theme)` |
| `src/main.rs` | Modify | `#[tokio::main]` → `runtime::run()` |

## Interfaces / Contracts

```rust
// core/pty
pub struct SpawnSpec { pub program: PathBuf, pub cwd: PathBuf, pub env: Vec<(String, String)>, pub size: PaneSize }
pub enum PtyEvent { Output(Vec<u8>), Exited }
pub type PtySink = Box<dyn FnMut(PtyEvent) -> bool + Send>; // false = receiver gone, stop
pub trait PtyHandle: Send {
    fn write(&mut self, bytes: Vec<u8>);              // non-blocking enqueue
    fn resize(&mut self, size: PaneSize) -> io::Result<()>;
    fn pid(&self) -> Option<u32>;
    fn kill(&mut self);                               // idempotent; also on Drop
}
pub fn spawn_portable(spec: &SpawnSpec, sink: PtySink) -> io::Result<Box<dyn PtyHandle>>;

// core/pane
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub struct PaneSize { pub rows: u16, pub cols: u16 }
pub enum TermColor { Default, Idx(u8), Rgb(u8, u8, u8) }
pub struct CellView<'a> { pub text: &'a str, pub fg: TermColor, pub bg: TermColor,
    pub bold: bool, pub italic: bool, pub underline: bool, pub inverse: bool, pub wide_cont: bool }
pub struct TermModes { pub application_cursor: bool, pub bracketed_paste: bool }
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)] pub enum CursorKind { #[default] Default, Block, Underline, Bar }
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)] pub struct CursorShape { pub kind: CursorKind, pub blinking: bool }
// DECSCUSR Ps: 0 Default | 1 Block blink | 2 Block steady | 3 Underline blink | 4 Underline steady | 5 Bar blink | 6 Bar steady
impl CursorShape { pub fn from_decscusr(ps: u16) -> Option<Self>; }   // None for unknown Ps => ignored
impl Pane {
    pub fn new(size: PaneSize, scrollback: usize) -> Self;
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8>;   // returns query replies
    pub fn resize(&mut self, size: PaneSize);
    pub fn cell(&self, row: u16, col: u16) -> Option<CellView<'_>>;
    pub fn cursor(&self) -> Option<(u16, u16)>;        // None if hidden or scrolled back
    pub fn modes(&self) -> TermModes;
    pub fn cursor_shape(&self) -> CursorShape;         // last DECSCUSR requested by the child
    pub fn set_scrollback(&mut self, offset: usize);
    pub fn scrollback_len(&mut self) -> usize;
}

// core/keys
pub fn encode_key(key: KeyEvent, modes: TermModes) -> Option<Vec<u8>>;
pub fn encode_paste(text: &str, modes: TermModes) -> Vec<u8>;

// core/prefix
pub struct KeyChord { pub code: KeyCode, pub mods: KeyModifiers }
pub enum PrefixAction { SendPrefixLiteral, RequestQuit, EnterCopy }
pub const PREFIX_KEY: KeyChord = KeyChord { code: KeyCode::Char(' '), mods: KeyModifiers::CONTROL };
pub const PREFIX_BINDINGS: &[(KeyChord, PrefixAction)] = &[
    (PREFIX_KEY, PrefixAction::SendPrefixLiteral),            // sends 0x00
    (KeyChord { code: KeyCode::Char('q'), mods: KeyModifiers::NONE }, PrefixAction::RequestQuit),
    (KeyChord { code: KeyCode::Char('['), mods: KeyModifiers::NONE }, PrefixAction::EnterCopy),
];
pub fn lookup(key: &KeyEvent) -> Option<PrefixAction>;   // None (incl. Esc) => cancel

// core/copy
pub enum CopyMotion { LineUp, LineDown, HalfUp, HalfDown, Top, Bottom }
pub enum CopyCommand { Move(CopyMotion), Exit, Ignore }
pub struct CopyState { pub offset: usize, pending_g: bool }
impl CopyState { pub fn on_key(&mut self, key: &KeyEvent) -> CopyCommand;
                 pub fn apply(&mut self, m: CopyMotion, max: usize, rows: u16); }

// app
pub enum InputMode { #[default] Terminal, Prefix, Copy(CopyState), ConfirmQuit }
pub enum AppEvent { Key(KeyEvent), Paste(String), Resize { cols: u16, rows: u16 },
                    Pty(PtyEvent), Cwd(PathBuf) }
pub enum Effect { WritePty(Vec<u8>), ResizePty(PaneSize), Quit }
pub struct App { pub pane: Pane, pub input: InputMode, pub cwd: PathBuf,
                 pub shell_name: String, pub dirty: bool, home: Option<PathBuf> }
impl App { pub fn update(&mut self, ev: AppEvent) -> Vec<Effect>;
           pub fn cursor_shape(&self) -> CursorShape; }   // pure: Copy => Block steady, else pane.cursor_shape()
// runtime only: fn to_crossterm(CursorShape) -> SetCursorStyle  (Default => DefaultUserShape)
```

Rules: the pane size is `rows - 1` (min 1×1). `Key` events with `kind != Press` are ignored. ConfirmQuit: `y` → `Quit`, any other key → Terminal. COPY exit (`q`/`Esc`/`i`) sets offset 0. The shell name is the basename of `$SHELL` (fallback `/bin/sh`). The cwd is shown with `$HOME` replaced by `~`.

The responder implements `vt100::Callbacks::unhandled_csi` for DA1 (`\e[?62;c`), DSR 5n (`\e[0n`), and 6n (`\e[{r};{c}R`, 1-based). Replies collect in a `Vec<u8>` inside the callbacks struct, and `feed` drains them.

The same callback captures DECSCUSR: final byte `q` with intermediate `b' '` and one numeric param. The callbacks struct stores `cursor_shape: CursorShape`; unknown Ps leaves it unchanged and DECSCUSR never produces reply bytes. If vt100 does not route this sequence to `unhandled_csi`, the fallback is a small stateful pre-scanner in `Pane::feed` (must handle sequences split across chunks).

## Spike (task 1, gates slice 1)

Scope: a throwaway `examples/spike_vt100.rs` (deleted or kept unwired) that runs `nvim` in vt100 with the responder and draws through a minimal view.

Accept if all of these hold:
- LazyVim starts in under 1 s with no DA/DSR timeout stall.
- Truecolor theme is correct.
- Arrows work in insert and normal mode (application cursor).
- nvim insert/normal switches emit DECSCUSR (`\e[6 q` / `\e[2 q`) and the captured shape changes accordingly; Kitty shows bar/block; the cursor returns to default after exit.
- Alt screen enters and exits cleanly on `:q`.
- htop renders.
- crossterm reports Ctrl+Space as `Char(' ')` + CONTROL.

The spike must also confirm these vt100 0.16 APIs (the crate is not in the local registry):
- `Callbacks::unhandled_csi` signature, and that 6n and `c` reach it.
- DECSCUSR (`CSI Ps SP q`) reaches `unhandled_csi` with intermediate `b' '` and the Ps param exposed (otherwise use the pre-scanner fallback).
- `screen_mut().set_scrollback(n)` semantics: offset in rows from the bottom, clamped.
- `set_scrollback(usize::MAX)` followed by `scrollback()` gives the buffer length.
- No panic when offset > rows (known 0.15 issue).

Reject if any stall or corruption cannot be fixed with callbacks. Fallback: re-implement `core/pane.rs` on alacritty_terminal 0.26 with the same public API. Nothing outside `pane.rs` changes.

## Testing Strategy (strict TDD, `cargo test`)

| Module | Approach |
|---|---|
| `keys` | table-driven: chars, Ctrl, Alt, Enter/BS/Tab/BackTab, arrows × app-cursor, F-keys, paste ± bracketed |
| `prefix`, `copy` | pure tables: lookup, motions, `gg`, clamping, exit |
| `pane` | feed bytes → cells/colors/cursor; `\e[6n`/`\e[c`/`\e[5n` → reply bytes; scrollback anchoring; DECSCUSR `\e[0..6 q` → `CursorShape` (incl. unknown Ps, split chunks, no reply bytes) |
| `app` | `update` → effects and mode transitions; resize math; quit flow; Pty Exited → Quit; `cursor_shape()`: TERMINAL/PREFIX follow the pane, COPY → Block steady, back to TERMINAL restores |
| `terminal_view` | TestBackend + insta (colors, wide chars, cursor, scrolled view) |
| `statusline` | existing tests untouched; new snapshots per `InputMode`; cwd/shell segments |
| `runtime` executor | `FakePty` records writes and resizes; cursor-style application only on change; `restore()` emits `DefaultUserShape` (writer-generic helper, tested against a `Vec<u8>`) |
| `pty/portable` | `#[cfg(all(test, target_os = "linux"))]`, 2–3 tests with `recv_timeout`: echo, `stty size` after resize, exit → `Exited` |

## Migration / Rollout

No migration required. `examples/theme_preview.rs` keeps using `AppMode` + `run_preview`.

Suggested chained PRs (each one green on test, clippy, and fmt):
1. deps + spike + `core/pane` (responder + `CursorShape`/DECSCUSR capture)
2. `core/keys`
3. `core/prefix` + `core/copy` + `InputMode`/`App::update` + `App::cursor_shape` + theme/statusline
4. `core/pty` + `terminal_view` + `runtime` (applies cursor style, restores on exit/panic) + `main` (first runnable)
5. copy-mode wiring (anchoring) + cwd polling + statusline segments

## Open Questions

- [ ] Right StatusLine segment: shell name only for now. Does the reserved "model" slot stay hidden?

## Resolved

- DECSCUSR cursor shape is IN scope for slice 1 (user decision). Size estimate rises by about 100 lines, to ~1,400-1,800 changed lines, still ~5 chained PRs.
- Mode colors approved: TERMINAL success_green, PREFIX warning_orange, COPY primary_cyan, ConfirmQuit error_red.
- Alt-screen COPY: no scrollback, clamped motions, accepted limitation.
