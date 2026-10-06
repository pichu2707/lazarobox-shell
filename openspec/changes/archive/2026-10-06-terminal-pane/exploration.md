# Exploration: terminal-pane

> Also stored in engram topic `sdd/terminal-pane/explore` (project `lazarobox-shell`).
> Supersedes `openspec/changes/command-executor/exploration.md` for the user-facing terminal; that exploration is kept as reference for the future AI/MCP tool runner.

## Executive summary

Slice 1 is one full-screen pane running the user's `$SHELL` in a PTY, with the existing StatusLine and a tmux-style prefix key. It uses portable-pty 0.9 and vt100 0.16.2 (user decision), a hand-written Screen→Buffer widget, a hand-rolled key encoder, and a pure `App::update -> Vec<Effect>`. vt100 does not answer terminal queries, so a responder callback is mandatory. An early spike with real nvim gates the emulator choice; the fallback is alacritty_terminal.

## Kept from command-executor

Async tokio `select!` loop over crossterm `EventStream` plus an mpsc `AppEvent` channel and a pure `App::update`. Requires a direct `crossterm 0.29` dependency with `event-stream` plus `futures`; check `cargo tree -d` for a single crossterm.

## Affected areas

- `Cargo.toml`: `portable-pty 0.9`, `vt100 0.16`, `crossterm 0.29` (event-stream), `futures`.
- `src/main.rs` + new `src/runtime.rs`: async loop, `ratatui::init`, EnableBracketedPaste, restore.
- `src/app.rs`: `App`, `AppEvent`, `Effect`, prefix state machine (keeps `AppMode`).
- New `src/core/pty/{mod,portable,fake}.rs`, `src/core/pane.rs`, `src/core/keys.rs`.
- New `src/ui/components/terminal_view.rs`.
- `src/ui/components/statusline.rs`: prefix indicator via builder flag (existing snapshots survive).
- `src/ui/preview.rs`: stays as the `theme_preview` example.

## 1. PTY

- **portable-pty 0.9.0** (recommended), alternatives pty-process 0.5.3 (tokio-native, Unix only) and raw `nix::openpty`.
- Blocking reader on a **detached `std::thread`** (not `spawn_blocking`: it would hang runtime shutdown) → `AppEvent::PtyOutput` / `PtyExited`.
- Writer via a thread + channel (or direct write for bounded sizes in slice 1). Keep `MasterPty` alive.
- Env: `TERM=xterm-256color`, `COLORTERM=truecolor`, optional `TERM_PROGRAM=lazarobox`, `LAZAROBOX=1`. Inherit the rest (verify in spike).
- Shell: `$SHELL`, fallback `/bin/sh`; cwd: launch directory.

## 2. Emulation: vt100 0.16.2

- Supports alt screen, truecolor, wide chars, mouse-mode query, bracketed paste, application cursor, scrollback.
- **Gaps:** no replies to DA1/DA2/XTVERSION/DSR 6n/DECRQM/OSC 10-11 queries; no cursor shape (DECSCUSR) tracking; no kitty keyboard state; no OSC 8.
- **Mitigation:** `Callbacks::unhandled_csi` / `unhandled_osc` (receive `&mut Screen`) → responder that writes replies back to the PTY (DA1 `\e[?62;c`, DSR 5n `\e[0n`, 6n cursor position). DECSCUSR captured and applied with crossterm `SetCursorStyle` (optional). Verify 6n reaches the callback in the spike.
- Performance: drain events before drawing, dirty flag + ~16 ms redraw tick.
- Fallback: alacritty_terminal 0.26 if the spike shows breakage callbacks cannot fix.

## 3. Rendering

- tui-term 0.3.4 is ratatui 0.30-compatible (ratatui-core ^0.1, vt100 ^0.16.2), but a hand-written `TerminalView` (~80–120 lines) is recommended: needed later for scrollback/selection, fewer third-party deps. tui-term stays as reference/fallback.
- Map vt100 colors (Default→theme, Idx, Rgb) and attributes to ratatui; skip wide continuations; place the real cursor with `frame.set_cursor_position` unless hidden.
- Risk: unicode-width vs Kitty disagreement on some emoji.

## 4. Input encoding

- Pure `encode_key(KeyEvent, &TermModes) -> Vec<u8>` (~100–150 lines): UTF-8 chars, Ctrl+letter 0x01–0x1A, Alt as ESC prefix, Enter `\r`, Backspace 0x7F, Shift+Tab `\e[Z`, xterm modified keys and F-keys.
- **Application cursor mode is critical**: arrows send `\eOA` instead of `\e[A` when enabled (nvim and zsh enable it).
- Do NOT enable Kitty keyboard enhancement flags in slice 1 (same as tmux defaults).
- Paste: EnableBracketedPaste outside; wrap in `\e[200~ … \e[201~` when the inner app enabled bracketed paste.
- Mouse: deferred (Kitty native selection keeps working).

## 5. Prefix key

| Key | Conflicts | Verdict |
|---|---|---|
| Ctrl+B | nvim page-up, zsh backward-char | Familiar, frequent double-press |
| Ctrl+A | zsh beginning-of-line, nvim increment | Worst |
| Ctrl+Space | completion / incremental selection in common setups, zsh set-mark | Risky |
| Ctrl+\ | nvim `<C-\><C-n>`, AltGr on Spanish layout | Awkward |
| **Ctrl+G** | zsh send-break, nvim file info; in the user's setup only opencode.nvim maps it (`toggle_scope`) | **Recommended** |

State machine: `Passthrough → PrefixPending`; prefix again sends the literal key; `q` requests quit; Esc/unknown cancels. Key→action table as data so splits/tabs add rows later.

## 6. Resize

`Event::Resize` → pane area = rows − statusline (min 1×1) → `parser.set_size` then `PtyHandle::resize` (TIOCSWINSZ → SIGWINCH).

## 7. Architecture

- `core/pty`: `trait PtyHandle { write, resize, pid }` + spawner returning `(Box<dyn PtyHandle>, Receiver<PtyEvent>)`; portable adapter + fake.
- `core/pane.rs`: `Pane { id, parser, handle }`, UI-independent; `feed(bytes)` returns reply bytes.
- `core/keys.rs`: pure encoder.
- `app.rs`: `App::update(AppEvent) -> Vec<Effect>` with `Effect = WritePty | ResizePty | Quit`. No trait objects in App.
- `runtime.rs`: executes effects and redraws.
- Later: `Vec<Pane>` + layout tree for splits; AI panes as another pane kind. Client/server (detach/attach) is NOT in slice 1, but the UI-independent `Pane` keeps the door open.

## 8. Testability

- Table-driven encoder tests (application cursor on/off), prefix state machine, `App::update` effects.
- TestBackend + insta for TerminalView by feeding bytes to vt100 (no real PTY).
- Responder tests (`\e[6n`, `\e[c` → reply bytes).
- 2–3 `#[cfg(unix)]` real PTY integration tests with timeouts (echo, resize + `stty size`, `cat` echo).
- Manual acceptance checklist: nvim with colors, `:q`, htop, resize with nvim open, Ctrl+G Ctrl+G, shell exit closes the app.

## 9. Statusline

- PREFIX indicator while pending (transient input state, not an `AppMode`).
- cwd: poll `/proc/<pid>/cwd` on a ~1 s tick (Linux); OSC 7 as a later override.
- Right segment: "model" is meaningless now (product question).

## 10. Risks and deferrals

Risks: missing query replies, no cursor shape, single-maintainer vt100, emoji width, application cursor mode, reader thread shutdown, terminal restore, orphan shells, output floods, lost outer scrollback (alt screen), crossterm unification, no kitty keyboard protocol inside.

Deferred: scrollback/copy mode UI, mouse forwarding, splits, tabs, detach/client-server, OSC 52/8, config file for the prefix, macOS/Windows, `sh -c` AI tool runner, synchronized output, image protocols.

## Suggested tasks (TDD order)

1. Spike vt100 + responder against real nvim.
2. Key encoder.
3. Prefix state machine + `App::update`.
4. TerminalView + snapshots.
5. PTY adapter + integration tests.
6. Runtime loop, resize, statusline prefix indicator, cwd polling.
