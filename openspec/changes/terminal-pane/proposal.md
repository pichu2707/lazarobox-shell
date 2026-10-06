# Proposal: Terminal Pane (Slice 1 of the modal multiplexer)

## Intent

lazarobox-shell today renders only a theme preview. The product is a keyboard-only, nvim-like, MODAL terminal multiplexer: the user must be able to run their real `$SHELL` (and nvim/LazyVim on real projects) inside it. Slice 1 delivers one full-screen PTY pane plus the modal input model (TERMINAL / PREFIX / COPY) that every later feature (splits, tabs, AI panes) builds on. No mouse support, ever.

## Scope

### In Scope
- One full-screen pane: `$SHELL` (fallback `/bin/sh`) via portable-pty 0.9, launch cwd, `TERM=xterm-256color`, `COLORTERM=truecolor`.
- vt100 0.16.2 emulation + query responder (DA1, DSR 5n/6n, ...) via `Callbacks`; FIRST task is a spike against real nvim; fallback alacritty_terminal behind the same seam.
- Hand-written `TerminalView` (vt100 Screen -> ratatui Buffer, real cursor).
- Pure key encoder honoring application cursor mode; bracketed paste passthrough. No Kitty keyboard flags.
- Cursor shape (DECSCUSR `CSI Ps SP q`): captured from the inner app (nvim insert = bar, normal = block) and applied to the outer Kitty terminal via crossterm `SetCursorStyle`; block in COPY; reset to default on exit/panic.
- Modes in StatusLine (lualine-style colors: TERMINAL success_green, PREFIX warning_orange, COPY primary_cyan, ConfirmQuit error_red, via `input_mode_style`):
  - TERMINAL: all keys to PTY.
  - PREFIX (Ctrl+Space, NUL 0x00): Ctrl+Space again sends literal; `q` -> "Quit? (y/n)"; `[` -> COPY; Esc/unknown cancels. Key->action table as data.
  - COPY: scrollback (~10k lines) with j/k, Ctrl+u/Ctrl+d, gg/G; q/Esc/i -> TERMINAL at bottom. On the alternate screen (nvim, htop) there is no scrollback: COPY shows the current screen and motions clamp (accepted limitation, same as tmux).
- Async tokio loop (EventStream + one `AppEvent` channel), pure `App::update -> Vec<Effect>`.
- Resize (pane = rows - statusline). Shell `exit` closes the app.
- StatusLine: mode block, live cwd (`/proc/<pid>/cwd` polling), shell name on the right (model segment reserved).
- `cargo run` = multiplexer; `cargo run --example theme_preview` unchanged. Linux only.

### Out of Scope
Selection/yank + OSC 52 (next change, with splits/tabs), mouse (never), detach/attach, prefix config file, macOS/Windows, AI panes, `sh -c` tool runner, OSC 8, image protocols, Kitty keyboard protocol inside.

## Capabilities

### New Capabilities
- `terminal-session`: PTY spawn, env, I/O, resize, shell exit, shutdown.
- `terminal-emulation`: vt100 parsing, query responder, TerminalView rendering.
- `key-encoding`: KeyEvent -> bytes, application cursor, bracketed paste.
- `modal-input`: TERMINAL/PREFIX/COPY state machine, prefix action table, quit confirmation.
- `copy-mode-scrollback`: vim-motion scrollback navigation.
- `statusline`: mode block, cwd, shell segment.

### Modified Capabilities
None (no existing specs).

## Approach

Ports-and-adapters: `core/pty` (trait `PtyHandle` + portable adapter + fake), UI-independent `core/pane.rs` (parser + responder), pure `core/keys.rs`, pure `App::update` emitting `Effect { WritePty, ResizePty, Quit }`, thin `runtime.rs`. Reader on a detached `std::thread`. Mode enum extends/sits beside `AppMode` (design decides).

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `Cargo.toml` | Modified | portable-pty, vt100, crossterm 0.29 (event-stream), futures |
| `src/main.rs`, `src/runtime.rs` | Modified/New | async loop, terminal setup/restore |
| `src/app.rs` | Modified | modes, `App`, `AppEvent`, `Effect` |
| `src/core/{pty,pane,keys,copy}` | New | PTY seam, emulation, encoder, scrollback |
| `src/ui/components/terminal_view.rs` | New | renderer |
| `src/ui/components/statusline.rs`, `src/ui/theme.rs` | Modified | mode block/colors, cwd, shell |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| vt100 query gaps break nvim | Med | responder + spike first; alacritty fallback |
| Application cursor mishandled | Med | table-driven encoder tests |
| Ctrl+Space NUL not reported as Ctrl+Space | Low-Med | verify in spike; data-driven prefix key |
| Reader thread blocks shutdown | Med | detached thread, no `spawn_blocking` |
| Orphan shells on quit/panic | Med | kill child on drop + panic hook restore |
| Outer scrollback lost (alt screen) | High (accepted) | COPY mode is the replacement |
| DECSCUSR not delivered by vt100 `unhandled_csi` (intermediate b' ') | Low-Med | verify in spike; fallback: pre-scan bytes for `\e[<n> q` in `Pane::feed` |
| Cursor shape left changed in Kitty after exit/panic | Med | reset in `restore()` and panic hook |
| Diff exceeds 400-line budget | High | chained PRs (see estimate) |

## Rollback Plan

Additive change on a feature branch. Revert the merge commit(s); `main.rs` returns to the theme preview. No data or config migrations.

## Dependencies

portable-pty 0.9, vt100 0.16.2, crossterm 0.29 (`event-stream`, unified single version), futures.

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` pass.
- [ ] Manual: nvim/LazyVim with correct colors; arrows work in nvim; `:q` returns to shell; htop renders; resize with nvim open reflows; Ctrl+Space twice reaches shell; COPY j/k/Ctrl+u/Ctrl+d/gg/G work and exit to bottom; prefix+q shows "Quit? (y/n)" and honors y/n; `exit` closes the app with terminal restored.

## Size Estimate

~1,400-1,800 changed lines (incl. tests) vs 400 budget -> chained PRs recommended (~5 slices: spike+pane, encoder, modes/App, view+runtime, copy+statusline). Decision needed before apply.
