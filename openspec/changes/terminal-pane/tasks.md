# Tasks: Terminal Pane (Slice 1)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,700 (range 1,400-1,800, incl. tests) |
| 400-line budget risk | High |
| Chained PRs recommended | Yes |
| Suggested split | PR 1 (~350) -> PR 2 (~300) -> PR 3 (~450) -> PR 4 (~450) -> PR 5 (~200) |
| Delivery strategy | ask-on-risk |
| Chain strategy | feature-branch-chain |

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

### Suggested Work Units

| Unit | Goal | Likely PR | Notes |
|------|------|-----------|-------|
| 1 | Deps + spike gate + `core/pane` | PR 1 | Base = tracker/main. Gate: spike accepted |
| 2 | `core/keys` encoder | PR 2 | Base = PR 1 (only needs `TermModes`) |
| 3 | prefix, copy, `App`, theme, statusline | PR 3 | Base = PR 2. PR 3 may exceed 400; split statusline off if so |
| 4 | pty, terminal_view, runtime, main (first runnable) | PR 4 | Base = PR 3 |
| 5 | Copy wiring + cwd polling + segments | PR 5 | Base = PR 4 |

Every PR must be green: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Strict TDD: RED (failing test) -> GREEN -> REFACTOR. [manual] = user-run in Kitty.

## PR 1: Deps, spike, pane

- [x] 1.1 SPIKE: edit `Cargo.toml` (portable-pty 0.9, vt100 0.16.2, crossterm 0.29 `event-stream`, futures); `cargo check` resolves a single crossterm.
- [x] 1.2 SPIKE: scratch tests in `examples/spike_vt100.rs` (or `#[test]`) confirming vt100 API shape: `Callbacks::unhandled_csi` signature; `\e[c`, `\e[6n` and DECSCUSR `\e[6 q` (intermediate `b' '`, Ps param) reach it; `set_scrollback(n)` clamps; `set_scrollback(usize::MAX)` + `scrollback()` gives length; offset > rows no panic. [auto]
- [x] 1.3 SPIKE: `examples/spike_vt100.rs` runs nvim in vt100 with the responder and a minimal view. [manual, Kitty]
- [ ] 1.4 GATE [manual]: user checks all accept criteria (LazyVim <1 s, no DA/DSR stall, truecolor, arrows in insert/normal, DECSCUSR bar/block and reset after exit, alt screen clean on `:q`, htop, Ctrl+Space = `Char(' ')`+CONTROL). ACCEPT -> continue. REJECT -> re-do 1.5-1.8 on alacritty_terminal 0.26 behind the same `core/pane.rs` API; nothing outside `pane.rs` changes. If DECSCUSR is not routed, use the pre-scanner fallback in `Pane::feed`.
- [ ] 1.5 RED: `src/core/pane.rs` tests: colors/text, mode tracking (Screen state); DA1/DSR 5n/6n replies and split-chunk single reply (Query responder).
- [ ] 1.6 GREEN: create `src/core/mod.rs`, `pane.rs` (`Pane`, `PaneSize`, `CellView`, `TermColor`, `TermModes`, responder callbacks); add `pub mod core;` to `src/lib.rs`.
- [ ] 1.7 RED: DECSCUSR tests for all 7 cursor-shape scenarios (bar steady/blinking, block steady, default, unknown Ps, split chunks, no reply).
- [ ] 1.8 GREEN: `CursorKind`, `CursorShape::from_decscusr`, `Pane::cursor_shape`; scrollback API (`set_scrollback`, `scrollback_len`) with a clamp test; delete the spike or keep it unwired.

## PR 2: Key encoder

- [ ] 2.1 RED: `src/core/keys.rs` table tests: basics (`a`, `é`, Ctrl+c, Alt+x, Enter, BS, Shift+Tab), Home/End/PgUp/PgDn/Del/Ins, F-keys.
- [ ] 2.2 RED: arrow tests for application cursor off/on.
- [ ] 2.3 RED: `encode_paste` tests, bracketed enabled/disabled.
- [ ] 2.4 GREEN: implement `encode_key`, `encode_paste`; register `keys` in `src/core/mod.rs`.

## PR 3: Modal core, theme, statusline

- [ ] 3.1 RED+GREEN: `src/core/prefix.rs` `PREFIX_KEY`, `PREFIX_BINDINGS`, `lookup` tests (Ctrl+Space, `q`, `[`, Esc/`z` -> None).
- [ ] 3.2 RED+GREEN: `src/core/copy.rs` `CopyState::on_key`, `apply` tests: j/k, half page 12, `gg`/`G`, clamping, lone `g`, exit keys, alt screen offset 0.
- [ ] 3.3 RED: `src/app.rs` tests for `App::update`: passthrough, enter PREFIX, literal `[0x00]`, cancel, confirm/decline quit, enter COPY, paste only in TERMINAL, Resize in all modes, resize math (39x100, 1x1 min), Pty Exited -> Quit, non-Press ignored, copy swallows keys.
- [ ] 3.4 GREEN: add `InputMode`, `AppEvent`, `Effect`, `App::update`; keep `AppMode`.
- [ ] 3.5 RED+GREEN: `App::cursor_shape()` tests (TERMINAL/PREFIX follow pane, COPY block steady, restore).
- [ ] 3.6 RED+GREEN: `src/ui/theme.rs` `input_mode_style` color-mapping test.
- [ ] 3.7 RED: `statusline.rs` insta snapshots per `InputMode`, "Quit? (y/n)", narrow width; confirm old snapshots unchanged.
- [ ] 3.8 GREEN: `StatusLine::input()` constructor with label/style fields.

## PR 4: PTY, view, runtime (first runnable)

- [ ] 4.1 RED+GREEN: `src/core/pty/{mod,fake}.rs` port, `FakePty`; spawn-spec tests (SHELL set/unset, TERM/COLORTERM).
- [ ] 4.2 RED: `pty/portable.rs` linux tests: echo round trip, `stty size` after resize, exit -> `Exited`, drop leaves no orphan.
- [ ] 4.3 GREEN: `spawn_portable` with detached reader thread and writer thread.
- [ ] 4.4 RED: `terminal_view.rs` TestBackend+insta: styled snapshot, wide chars, hidden cursor.
- [ ] 4.5 GREEN: `src/ui/components/terminal_view.rs`; `src/ui/mod.rs` `render(frame, &App, &theme)`.
- [ ] 4.6 RED: `runtime.rs` tests (fake PTY/`Vec<u8>` writer): effects executed; cursor style applied only on change; `restore()` emits `DefaultUserShape`.
- [ ] 4.7 GREEN: `src/runtime.rs` select loop, 16 ms tick, bounded channel 256, panic hook, `restore()`; `src/main.rs` -> `runtime::run()`; no mouse capture.
- [ ] 4.8 [manual]: nvim colors/arrows/`:q`, htop, resize reflow, `exit` and prefix `q` `y` restore the terminal, Kitty cursor bar/block and reset, native mouse selection, Ctrl+Space twice.

## PR 5: Copy wiring, cwd, segments

- [ ] 5.1 RED: `app.rs` anchoring test (output during COPY keeps content; 10 up + 5 lines).
- [ ] 5.2 GREEN: anchor offset by `scrollback_len()` delta around `feed`; entering COPY on alt screen shows the current screen.
- [ ] 5.3 RED+GREEN: `AppEvent::Cwd` updates cwd, lookup failure retains it; `~` for `$HOME`; shell basename.
- [ ] 5.4 GREEN: 1 s `/proc/<pid>/cwd` poll in `runtime.rs`; statusline cwd and right shell segments. Test: `cd /tmp` reflected [auto, unix PTY].
- [ ] 5.5 [manual]: COPY j/k/Ctrl+u/Ctrl+d/gg/G on `ls -R`, exit lands at the bottom; cursor block in COPY.
- [ ] 5.6 Final: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.

## Carried-over review notes (apply in PR 3, which already touches `theme.rs`/`statusline.rs`)

- [ ] R.1 `LazaroboxTheme`: add `#[derive(Debug, Clone)]`.
- [ ] R.2 Take `AppMode` by value in `mode_style` (it is `Copy`); add `accent(mode) -> Color` and drop the `unwrap_or(primary_cyan)` fallback in `statusline.rs`.
- [ ] R.3 `statusline.rs` tests: compute widths with `chars().count()` (or `Line::width`) instead of `str::len`.
