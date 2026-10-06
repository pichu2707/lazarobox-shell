# Exploration: command-executor

> Source of truth also stored in engram topic `sdd/command-executor/explore` (project `lazarobox-shell`).

## Executive summary

Move to an async tokio event loop (crossterm `EventStream` + one mpsc `AppEvent` channel) and add a channel-based `CommandRunner` port in `core/` (tokio::process adapter + fake). The port streams `OutputEvent`s so PTY and MCP can reuse it later.

## Current state

- `src/app.rs` only has `AppMode`. `src/ui/preview.rs` runs a synchronous loop (`ratatui::run` + blocking `event::read()`). No state struct, no async, no `core/`.
- crossterm is only available through `ratatui::crossterm` (no `event-stream` feature).
- crossterm 0.29.0 provides an `event-stream` feature (not default). Using `EventStream` requires a direct `crossterm = { version = "0.29", features = ["event-stream"] }` dependency (unified with ratatui's copy) plus `futures`. Verify a single crossterm with `cargo tree -d`.

## Affected areas

- `Cargo.toml`: add crossterm (event-stream) and futures.
- `src/app.rs`: `App` state (input, output blocks, cwd, running flag, scroll), pure `update(event)`, async `run`.
- `src/ui/preview.rs`: kept as the theme preview, separate path.
- `src/main.rs`: wire the new loop.
- New `src/core/{mod,runner,builtins,output}.rs`: port, `OutputEvent`, `cd`/`~` builtin, tokio adapter.
- New `src/ui/components/{input,output_view}.rs`.
- `src/ui/components/statusline.rs`: real cwd and running/exit indicator.

## 1. Event loop

| Option | Pros | Cons |
|---|---|---|
| Blocking `event::read` (today) | No change | Cannot stream output or handle Ctrl+C while a child runs |
| `event::poll(timeout)` + `try_recv` | No new feature | Hand-rolled polling, tick latency |
| tokio `select!` over EventStream + mpsc + tick | Idiomatic, immediate, scales to LLM streams and PTY | Needs crossterm `event-stream` + `futures` |

**Recommendation:** tokio `select!`. Single `enum AppEvent { Key, Resize, Tick, Command(OutputEvent), .. }`, pure `App::update`, then redraw. Use `ratatui::init()/restore()` inside `#[tokio::main]`. Keep filtering `KeyEventKind::Press`.

## 2. Process execution

- `tokio::process::Command` with `sh -c <line>`, piped stdout/stderr, read line by line in two tasks into one mpsc. Interleaving is best effort (PTY fixes it later).
- Send `Exit` last; signal kills reported as 128+n.
- Cancel: Ctrl+C while running kills the child's whole process group (`process_group(0)` + group kill), plus `kill_on_drop(true)`.
- No default timeout (long-running commands are legitimate).
- stdin = `Stdio::null()`; interactive programs show a clear "not supported yet" message.
- Lossy UTF-8; cap retained output (ring buffer) to avoid floods.
- `cd` builtin in core: `cd`, `cd ~`, `cd ~/x`, `cd -`, relative, nonexistent (error + exit 1). App owns `cwd`; never call `std::env::set_current_dir`.
- `sh -c` is a real shell by design. Future AI/MCP execution MUST go through an approval layer.

## 3. Port design

- `CommandRunner::spawn(CommandRequest { line, cwd, env }) -> RunningCommand { events: mpsc::Receiver<OutputEvent>, cancel: CancelHandle }`.
- `OutputEvent = Stdout | Stderr | Exit(ExitInfo)` (+ `Started` / `Error(SpawnError)`). Consider byte chunks (`Output { stream, bytes }`) so PTY is not a breaking change.
- Sync, object-safe method: no async-trait, works as `Box<dyn CommandRunner>`; the fake pushes scripted events.

## 4. ANSI colors

- Set `CLICOLOR_FORCE=1`; do not rewrite user commands.
- Render with `ansi-to-tui` 8.0.1 (depends on ratatui-core ^0.1, matching ratatui 0.30) after a compile spike; fallback: strip ANSI.
- SGR state can span lines; drop `\r`-overwrite and unsupported CSI safely.

## 5. Layout and input

- Hand-rolled single-line input (cursor, insert, delete, home/end, history, Ctrl+U/W/A/E). Revisit `tui-textarea-2` when AI chat needs multiline.
- Output view: `Vec<OutputBlock>` with scroll offset and auto-scroll unless the user scrolled up.
- Screen: output on top, input (`cwd ❯`) below, statusline at the bottom.

## 6. Testability

- Pure tests: input editing, `App::update` transitions, builtins.
- TestBackend + insta for the output view.
- `FakeRunner` for app tests; a few real `#[tokio::test]` runner tests (echo, stderr + exit code, cwd, cancel `sleep 30`). Unix-only.

## 7. Risks

crossterm / ratatui-core version unification; orphan processes; output floods; stdin hangs; ANSI edge cases; scope creep into PTY; future AI execution security.

**Defer:** PTY, history persistence, tab completion, aliases, job control, timeouts, multiline input, MCP approval layer, Windows support.

## Suggested slices

1. Input component · 2. Builtins/cd · 3. Port + fake + tokio adapter · 4. Output view · 5. App state/update · 6. Loop wiring in main
