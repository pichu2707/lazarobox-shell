# Tasks: Reload in place after an update

Order follows Open question 5's recommendation: scope 1 first (S1, S2), then scope 2 (S3-S6). Each slice is one PR under about 400 production lines (tests and docs may exceed; the size is an advisory heuristic). Every RED/GREEN task writes the failing test first through the public interface, then the code. Every slice keeps the quit, the update install and the watcher behaving as today when no reload is asked; the characterization tasks pin that. Design references are `D#` in design.md; requirements are in specs/.

## S1 Handoff container and layout (pure core) — 250-330 lines

- [ ] 1.1 RED/GREEN: container encode/decode: magic, format version 1, writer version, header JSON, blobs; refused: wrong magic, unknown version, other pid, older than 30 s, over 64 MiB, truncated blob, header not JSON (reload-in-place, Handoff file; D3).
- [ ] 1.2 RED/GREEN: paths as text or bytes round trip (`0xff` folder).
- [ ] 1.3 RED/GREEN: layout export/import from `App`: tabs, `Node`/`Split` axis and weights, focus, zoom, `last_shell`, active tab, next pane id; pane kinds `shell`, `job`, `ended`, `chat` with folder, conversation id, draft, ORCH flag (src/app.rs:910-956, src/core/layout.rs:56-68).
- [ ] 1.4 RED/GREEN: the handoff never holds environment, tokens or messages (a test app with a token in its env and a chat with messages: none of them in the bytes).
- [ ] 1.5 RED/GREEN: the descriptor carrier: Linux memfd without close-on-exec, sealed after the write; other unix: `O_EXCL|O_NOFOLLOW` 0600 in the private run folder, unlinked at once; the reader's `fstat` checks (owner, mode, size).

## S2 Scope 1: restart keeping the layout — 300-380 lines

- [ ] 2.1 RED/GREEN: `__adopt` in `KNOWN` (src/core/cli.rs:36), dispatched in `main` before the guard (src/main.rs:22-30); `__adopt --formats` prints `1` and exits 0 (reload-in-place, Reload starts only from a probed, trusted new binary).
- [ ] 2.2 RED/GREEN: the new-binary probe: same canonical path as `InstallEnv.exe`, trust rule, `--formats` within 5 s; a binary without `__adopt` gives `X.Y.Z cannot reload in place: quit and start it again`; nothing is stopped on a refusal (D9).
- [ ] 2.3 RED/GREEN: Updates row and footer after an install: `X.Y.Z installed: Enter reloads`, `Updated to X.Y.Z · Enter reloads · Esc cancel`; Enter emits the reload effect; a refusal reason shows in the footer until the next key; `r` still does nothing (config-menu delta).
- [ ] 2.4 RED/GREEN: the scope 1 prompt `Restart? still running: <jobs> (y/n)` with the quit prompt's naming rules (modal-input "The quit prompt names what still runs").
- [ ] 2.5 RED/GREEN: `Outcome::Reload`: the quit's teardown without `restore()`, the host termios read before `ratatui::init()`, the handoff, `exec`; in the new process the termios set back, `ratatui::init()`, the modes, a full redraw (D1, D11).
- [ ] 2.6 RED/GREEN: the new process rebuilds tabs and layout, spawns shells in the saved folders (a gone folder falls back as a new pane does), reopens each chat's conversation in its folder with its draft, shows `restarted with X.Y.Z: the shells started again` and names the jobs left running.
- [ ] 2.7 RED/GREEN: scope 1 `exec` failure after the teardown rebuilds the layout in the old process (reload-in-place, Failures never lose panes silently).
- [ ] 2.8 Characterization: quit, the update install and its `installed` state, and `r` on Updates behave as before (src/app.rs:1986-2011, src/core/menu.rs:1562-1566).
- [ ] 2.9 [manual] Linux (Kitty) and WSL (Windows Terminal): update, reload, same tabs and folders; `stty -a` after the final quit equals the value before start.
- [ ] 2.10 Docs: README «Avisos de actualización» (Enter reloads; what scope 1 keeps and what it does not), CHANGELOG.

## S3 Screen snapshot core (pure) — 250-350 lines

- [ ] 3.1 RED/GREEN: snapshot and restore of the primary screen with history, attributes, wide and combining characters, wrap flags, cursor (terminal-emulation, Screen snapshot; D5).
- [ ] 3.2 RED/GREEN: input modes, focus reports, cursor shape, OSC 7 folder.
- [ ] 3.3 RED/GREEN: `Pane::feed` splits a chunk at `CSI ? 1049 h` / `1047 h` / `47 h` and keeps the primary `Screen` clone; the snapshot on the alternate screen holds that primary, then the alternate grid.
- [ ] 3.4 RED/GREEN: restore discards replies; the 8 MiB bound drops the oldest rows and writes the marker row.
- [ ] 3.5 Characterization: the existing `Pane` tests (scrollback counts, eviction, alt-screen span) pass unchanged.

## S4 PTY freeze and adoption — 280-380 lines

- [ ] 4.1 RED/GREEN: the reader is a `poll` loop on the master and a stop pipe; a stop delivers the current chunk, acknowledges and sends no `Exited`; the existing portable tests pass (src/core/pty/portable.rs:284-) (D6).
- [ ] 4.2 RED/GREEN: the writer's queued byte count; empty within 500 ms, else `pane <n> is not reading its input` and the readers restart.
- [ ] 4.3 RED/GREEN: `AdoptedHandle` over a raw master: write, resize, `shell_in_foreground`, probe, kill escalation, `waitpid` waiter with the exit status for `job` panes; a pid whose start time differs is `ended` (terminal-session delta).
- [ ] 4.4 RED/GREEN: clear close-on-exec on exactly the kept descriptors; set it back on any failure (reload-in-place, Only the kept descriptors cross exec; D4).
- [ ] 4.5 RED/GREEN integration (`tests/reload_exec.rs`): stage 1 child of the test binary spawns `/bin/sh`, freezes, writes a handoff, execs itself into stage 2; stage 2 adopts: same pid, `AFTER-42`, restored screen equals the snapshot, `exit 7` reported, `/proc/self/fd` holds only the kept descriptors; a close-on-exec master is not inherited and its shell ends with SIGHUP (D13).
- [ ] 4.6 RED/GREEN integration: `LAZAROBOX` equal before and after, for an adopted pane and a new one.

## S5 Scope 2 wiring — 300-400 lines

- [ ] 5.1 RED/GREEN: the busy check, one test per reason in D7 with its text, the first reason shown with its pane; an idle chat with history is not busy (reload-in-place, Busy refuses the reload).
- [ ] 5.2 RED/GREEN: before `exec`: stop the listed groups with the quit's polite stop, close the MCP hubs (sockets gone), check the list is empty, stop once more, else refuse (reload-in-place, The watcher stops nothing during a reload; D8).
- [ ] 5.3 RED/GREEN: the old watcher with an empty list exits on EOF and signals nothing (src/runtime/watch.rs:159-176); the new process starts its own.
- [ ] 5.4 RED/GREEN: the reload sequence: freeze, drain, snapshots, handoff, flags, `exec`; the new process adopts each live pane, restores its screen, nudges alternate-screen and mouse-mode panes with a one-row resize and back, shows `reloaded to X.Y.Z`.
- [ ] 5.5 RED/GREEN: scope 2 only on Linux (WSL2 included); other platforms run scope 1 (reload-in-place, Platforms).
- [ ] 5.6 Characterization: a quit after a reload leaves no orphan within 3 s; the quit prompt still names jobs of panes closed before the reload.
- [ ] 5.7 [manual] Linux and WSL: nvim, htop, `sleep 300 &`, an ssh session, a chat with history and a failed update tab across a reload (design D13, Manual).

## S6 Fallbacks and docs — 150-250 lines

- [ ] 6.1 RED/GREEN: a failed `exec` keeps the old process and every pane; flags restored; `reload failed: <error>; still running X.Y.Z` (reload-in-place, Failures never lose panes silently; D10).
- [ ] 6.2 RED/GREEN integration (linux): a stage 2 that refuses the handoff runs the old binary through its descriptor, which adopts the panes.
- [ ] 6.3 RED/GREEN: a panic during adoption is caught and handled as an unreadable handoff.
- [ ] 6.4 RED/GREEN: a pane that cannot be adopted keeps its last screen and `[the program in this pane ended during the reload]`; a live child behind a broken descriptor is killed like a closed pane.
- [ ] 6.5 RED/GREEN: without the old-binary fallback, the new process starts with one shell and shows, then prints after its next quit, `reload failed: the panes could not be kept (<reason>)`.
- [ ] 6.6 Docs: README «Avisos de actualización» (scope 2: what is kept, what refuses, what is lost: scroll region, title, selection) and «Al cerrar lazarobox» (the watcher during a reload); CHANGELOG.
- [ ] 6.7 [manual] macOS: scope 1 on a real Mac before scope 2 is offered there (Open question 4).
