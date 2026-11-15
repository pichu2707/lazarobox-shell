# Design: Reload in place after an update

Traceability: issue #598 and the owner's comment of 2026-11-10 (quoted in proposal.md). Scope 2 is the goal; scope 1 is the stepping stone and the fallback.

Naming. The **reload state** is what the old process passes to the new one across `exec` (D3). It is unrelated to the orchestrator's `handoff` module (src/app/handoff.rs), which saves a run's handoffs to engram; this change never uses "handoff" for its own state.

Evidence marks. **V** = checked in the code on `main` at 3b9548c2 (1.27.0) (cited `path:line`), or in a dependency's source in `~/.cargo/registry` at the version locked in `Cargo.lock`. **P** = probed on this machine (Linux 6.18, WSL2) with the throwaway Python probes listed at the end. **I** = inferred, not checked. **U** = OS or terminal behaviour not probed.

## 1. What exists today

### 1.1 The update flow (V)

- The notice is pure in `src/core/update.rs` (module doc, :1-8); the request and cache in `src/runtime/update.rs` (:1-8, `cache_path` :44).
- Enter on the Updates row runs `cargo install lazarobox-shell --locked --version <v> --root <root>` in a new tab (README.md:454-458; the plan, src/core/update.rs:291-317). Starting it closes the menu as Esc does (src/app.rs:2070-2076).
- When the pane's job ends with 0, `update_ended` sets `installed` (`update_ended`, src/app.rs:2084-2108): the notice `Updated to X.Y.Z. Restart lazarobox to use it.` (src/core/update.rs:375-378), the row `X.Y.Z installed: restart` (:380-383), the footer `Updated to X.Y.Z · restart lazarobox to use it` (src/core/menu.rs:3290-3291). After that, `r` does nothing (`can_check_update`, src/core/menu.rs:1789-1792) and Enter has no effect (src/core/menu.rs:2516-2522; config-menu spec "Notify, Sound and Updates keys", openspec/specs/config-menu/spec.md:919-935).
- `InstallEnv.exe` is the running binary's canonical path, read once at start-up (`InstallEnv`, src/core/update.rs:321-337); after a `cargo install` the same path holds the new binary. `std::env::current_exe()` on Linux would read `/proc/self/exe`, which then ends in ` (deleted)` (P, probe 3), so the reload uses `InstallEnv.exe`.
- A failed update tab stays open with its last line (README.md:460): a pane whose child has ended but which is still shown.

### 1.2 Panes and PTYs (V)

- `SpawnSpec` (src/core/pty/mod.rs:14-33) sets `TERM`, `COLORTERM` and `LAZAROBOX=<std::process::id()>` (:47-54). `exec` keeps the PID (P, probe 1), so the marker stays true without a change.
- The adapter is portable-pty 0.9.0 (Cargo.toml:40; src/core/pty/portable.rs). `spawn_portable` (:207, through `spawn_with_grace`, :212-295) opens the PTY, spawns the child (portable-pty calls `setsid` and `TIOCSCTTY`, portable-pty `src/unix.rs:257-271`), drops the slave (:232-233), then starts three detached threads: the reader on a clone of the master (:235, :247-256), the writer fed by a `std::sync::mpsc` channel (:236, :258-270) and the waiter that owns the `Child` and blocks on `wait()` (:271-285).
- **CLOEXEC.** portable-pty sets `FD_CLOEXEC` on the master and the slave right after `openpty` (portable-pty `src/unix.rs:60-65`). `try_clone_reader` and `take_writer` clone the master with `F_DUPFD_CLOEXEC` (portable-pty `src/unix.rs:352-363`; `filedescriptor` `src/unix.rs:144-166`). The child closes every descriptor above 2 before its `exec` (`close_random_fds`, portable-pty `src/unix.rs:152-176`, called at :276). So today **no PTY descriptor survives an `exec` of lazarobox**, and none leaks into a pane.
- The master's raw descriptor is reachable: `MasterPty::as_raw_fd` (portable-pty `src/unix.rs:366-368`). There is no constructor from a raw descriptor (the fields are private), so adoption needs its own handle (D6).
- `PortableHandle` kills the child's group on drop: SIGHUP, 300 ms, SIGKILL (src/core/pty/portable.rs:131-160, 180-187). `exec` runs no destructors, so the handles must simply stay alive until `exec` (I, standard: `execve` replaces the image).
- `Panes` (src/runtime.rs:1479) holds `handles: HashMap<PaneId, Box<dyn PtyHandle>>` (:1480) and `sessions: Vec<jobs::Session>` (:1560), the session of every pane spawned, closed ones included, which the quit prompt names (`spawn_pane`, :1907-1918).
- Each PTY pane's screen is a `Pane` around `vt100::Parser<Responder>` (src/core/pane.rs:174-183) with `SCROLLBACK_LINES = 10_000` (src/app.rs:101, :938). `Responder` keeps state `vt100` does not: the DECSCUSR cursor shape, the last OSC 7 folder, the hostname and mode 1004 focus reports (src/core/pane.rs:113-122, 159-160). `feed` already notices a switch of the alternate screen (:232, :241).
- App-side pane state: `PaneState` (src/app.rs:929-932), `PtyPane` (:847-865: `emu`, `proc_cwd`, `update`, `line_pending`, `run`, `user_dirty`, `after_menu`), `Tab` (:996-1003: `tree`, `focus`, `zoom`, `last_shell`), `App.tabs`, `active`, `ids` (:1038-1042). The layout tree is `Node`/`Split` with `axis` and `weights` (src/core/layout.rs:56-68); `PaneId` is a `u32` never reused (:5-6, :109-113).

### 1.3 The watcher (V)

- `lazarobox-shell __watch` is started first thing in `run` (src/runtime.rs:245, `watch::start`, src/runtime/watch.rs:69-77), in its own session (:90-119), from `/proc/self/exe` on Linux (:121-128).
- lazarobox writes `+ <pgid> <start>` for each group a `GroupKill` arms and `- <pgid>` when it disarms (src/runtime/watch.rs:5-7, 132-150; src/runtime/proc.rs:65-72, 98-102). Users of `GroupKill`: chat CLI engines, AI detection probes, the managed llama-server, MCP servers, Hugging Face downloads, the llama-server install and the tool runner (`grep -rln GroupKill src`).
- **When its stdin ends**, the watcher sends SIGHUP and SIGTERM to every listed group, waits `GRACE` (3 s) and SIGKILLs the rest (src/runtime/watch.rs:8-10, 159-176, 217-250). Groups 0, 1 and its own are never signalled, nor a group whose leader changed (:11-14, 199-211).
- The pipe's write end is a `ChildStdin` made by `std::process::Command` (src/runtime/watch.rs:92-109). Rust's standard library creates descriptors close-on-exec (I, documented std behaviour; the probe confirmed the effect for a close-on-exec pipe: the reader sees EOF about 10 ms after the writer's `exec`, P probe 4).
- **Pane shells are not listed** (src/runtime.rs:1907-1918 arms no `GroupKill`; README.md:168 "Los shells de los paneles no pasan por el vigilante"). A pane shell dies with lazarobox only through its PTY: the master closes and the kernel hangs up the session (P, probe 2: SIGHUP).

### 1.4 Quit and the terminal (V)

- `main` dispatches the hidden subcommands before the nesting guard and before any thread exists (src/main.rs:9-30), then classifies the command line (:36-50; an unknown word exits 2, src/core/cli.rs:39-52, 56-73), runs the guard (:51-56) and builds the tokio runtime under a `QuitGuard` (:57-66; src/runtime.rs:210-240).
- `run` (src/runtime.rs:242) spawns the first pane before the UI (:857), then `ratatui::init()` (:860), `enter_modes` (bracketed paste, focus change, modifyOtherKeys off; :865, :2743-2747) and the loop (:867). After the loop: `restore()` (:873, :2749-2760), then the quit order (:875-914): polite stop of every listed group (`watch::stop_listed(QUIT_GRACE)`, 2 s, :893), store drain, memory flush and chat shutdown, PTY teardown, then the jobs left running are printed.
- `ratatui::init` enables raw mode and enters the alternate screen (ratatui 0.30.2 `src/init.rs:397-400`). crossterm keeps the terminal mode from **before** raw mode in a process-wide static and only captures it when raw mode is off (crossterm 0.29.0 `src/terminal/sys/unix.rs:30, 108-121`). A new process that inherits a raw terminal would capture raw as "original" and leave the terminal raw on its final quit (I, from that code).
- SIGTERM and SIGHUP are handled by tokio (src/runtime.rs:169-190).
- The quit prompt names the jobs still running in the panes' sessions (modal-input spec, openspec/specs/modal-input/spec.md:776-797; archived change `2026-10-31-quit-names-pane-jobs`) and a shared llama-server that is answering (:799-815).

### 1.5 Other state (V unless marked)

| State | Where | Persisted today |
|---|---|---|
| Tabs, layout, focus, zoom, last shell | `App.tabs` (src/app.rs:1038), `Tab` (:996-1003) | no |
| Pane folders | OSC 7 in `Responder.cwd` (src/core/pane.rs:117), `/proc` poll `proc_cwd` (src/app.rs:850) | no |
| Chat conversations | SQLite store; `ChatStore::conversation()` (src/core/ai/chat/store.rs:471) | yes, messages; resume by id (chat-history "Resume", openspec/specs/chat-history/spec.md:254) |
| Chat in-flight turn, approvals, staged writes | `ChatState` (`folder_busy`, src/core/ai/chat/folder.rs:287-309) | no (a partial answer is saved on close) |
| Orchestrator runs | the run of each orchestrating chat in `RunManager.runs: Mutex<HashMap<PaneId, Live>>` (src/runtime/orchestra.rs:586, :620; its worker turns are tokio tasks and engine process groups), `orch_run()` (src/core/ai/chat/mod.rs:685), `run_stores: HashMap<RunId, handoff::RunStore>` (src/app.rs:1069; the orchestrator's engram saves, src/app/handoff.rs, unrelated to this change's reload state) | rows in the chat store; a restart leaves a live run "interrupted" and offers a resume (src/app/run_resume.rs:1-12) |
| MCP hub sockets | `$XDG_RUNTIME_DIR/lazarobox` else `<data>/run` (src/runtime/mcp/mod.rs:158-169), `s-<pid>-<tail>.sock` 0600 (src/runtime/mcp/session.rs:225-236); swept at start by `comm` (:176-193) | no |
| User MCP servers | started on demand (mcp-servers "Lifecycle") | no |
| Managed llama-server | `chat::Managed` (src/runtime.rs:474-495), stopped on quit | no; restarted by the next llamacpp turn |
| Engram link | memory worker (`memory_for`, src/runtime.rs:374-385); `engram serve` is never listed (src/runtime/watch.rs:17-18) | the notes are in engram |
| WSL drive write grant | in memory until the app closes (README.md:693) | no |

### 1.6 Platforms (V)

lazarobox is Unix-only (`std::os::unix` throughout, e.g. src/runtime/watch.rs:29); 1.0 is Linux-only and macOS is not officially supported (README.md:94, :178); Windows means WSL (README.md:95). WSL2 runs a Linux kernel: the probes below ran on WSL2. WSL1: U. macOS: same POSIX `execve` rules (PID kept, descriptors without `FD_CLOEXEC` inherited, other threads destroyed), U on a real Mac; no `/proc`, so no D10 fallback there.

## 2. Design

### D1. The reload is an `exec` of the installed binary, from the main thread

The event loop returns a new `Outcome::Reload` (next to `Quit`, `Outcome`, src/runtime.rs:1473-1477; the app emits `Effect::Reload { scope }`, handled where `Effect::Quit` is, :2510). `run` then writes the reload state and execs instead of running the quit order. The future of `run` is driven by `block_on` on the main thread (src/main.rs:62-66), so `exec` is called there (I: tokio's multi-thread `block_on` runs the future on the calling thread).

`execve` from a multi-threaded process destroys the other threads and keeps the PID (P, probe 1, with three extra threads). The `QuitGuard` drop (src/runtime.rs:233-239) never runs: nothing waits for the runtime.

Command line: `<InstallEnv.exe> __adopt --state-fd <n> [--fallback-exe-fd <m>]`. `__adopt` is a hidden subcommand added to `KNOWN` (src/core/cli.rs:36) and dispatched in `main` before the nesting guard, like `__watch` (src/main.rs:23-26); it then builds the tokio runtime and calls `runtime::run_adopted`. Being a subcommand, it is exempt from the guard (terminal-session "Nesting guard"), which matters for a lazarobox started with `--force` inside a pane.

### D2. Two scopes, one path

| Step | Scope 1 (restart keeping the layout) | Scope 2 (reload in place) |
|---|---|---|
| Busy check | D7, plus the running-jobs prompt like quit | D7; running pane programs are fine |
| Probe the new binary | D9 | D9 |
| Children of lazarobox (chats, MCP, llama-server) | stopped as at quit | stopped as at quit (D8) |
| Pane children | torn down as at quit | frozen (D6), masters kept |
| Reload state | layout, folders, chats | layout, folders, chats, masters, screens |
| Terminal | stays raw on the alternate screen (D11) | same |
| New process | new shells in the saved folders | adopts each pane (D6) |

Scope 2 falls back to scope 1 per pane (D10).

### D3. The reload state file

**Location and life.**

- Linux: `memfd_create("lazarobox-reload-state", MFD_ALLOW_SEALING)` without `MFD_CLOEXEC`: no name on any filesystem, never on disk. `memfd_create` gives the file mode 0777 (probed: `fstat` reads `0o100777`), which the reader's own check below would refuse; the writer calls `fchmod(fd, 0o600)` **before** sealing (sealing does not block `fchmod`, but doing it first keeps one order to test). Sealed (`F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK`) once written.
- Elsewhere (macOS): `open(<private dir>/reload-state-<pid>-<16 random hex>, O_CREAT|O_EXCL|O_NOFOLLOW|O_RDWR, 0600)` in the 0700 private folder the MCP sockets already use (`$XDG_RUNTIME_DIR/lazarobox`, else `<data>/run`; `candidate_dirs`, src/runtime/mcp/mod.rs:158-169; `ensure_private_dir`, session.rs:180-183), written, then **unlinked at once**, before `exec`. Only the descriptor remains.
- The descriptor is the only way to it. The new process reads it whole, checks it (below) and never writes it back. It keeps the descriptor open until adoption has fully succeeded (D10) and only then closes it; on a refusal or a failed adoption the fallback path rewinds it with `lseek(fd, 0, SEEK_SET)` before `execveat`, because a read leaves the offset at EOF. When the process ends for any reason, the kernel frees it.
- Mode 0600 and owner: the reader checks with `fstat` that it is a regular file owned by its own uid with no group or other bits, and at most `STATE_CAP` (64 MiB) long; anything else is refused as unreadable (D10).

**Format** (binary container, version 1):

```
"LAZAROBOX-RELOAD-STATE\n"                      23 bytes, magic
u32 LE  format version                     1
u32 LE  header length H
H bytes JSON header (UTF-8)
u32 LE  blob count B, then B × (u32 LE length, bytes)
```

The JSON header is built by hand with `serde_json::Value` (the crate has `serde_json` but no serde derive, Cargo.toml:47; `src/core/update.rs` does the same). Blobs hold the screen snapshots (D5), referenced by index, so screen bytes are never escaped into JSON. Header fields:

```json
{
  "writer": "1.27.0",              // CARGO_PKG_VERSION of the writer, for messages only
  "pid": 12345,                    // must equal getpid() of the reader
  "created": 1762790400,           // seconds; older than 30 s is refused
  "scope": 2,
  "term": {"cols": 200, "rows": 50, "termios": "<hex of struct termios before raw mode>"},
  "active": 0, "next_pane_id": 9,
  "tabs": [{"tree": {"split": {"axis": "x", "weights": [1, 1],
                                "children": [{"leaf": 1}, {"leaf": 4}]}},
            "focus": 4, "zoom": false, "last_shell": 1}],
  "panes": [
    {"id": 1, "kind": "shell", "rows": 49, "cols": 99,
     "fd": 7, "pid": 23001, "start": 8123456, "status_wanted": false,
     "cwd": "/home/u/p", "screen": 0},
    {"id": 3, "kind": "ended", "rows": 49, "cols": 99, "screen": 1},
    {"id": 4, "kind": "chat", "conversation": 812, "folder": "/home/u/p",
     "draft": "and then", "orch": false}
  ],
  "sessions": [{"sid": 23001, "start": 8123456}],
  "notice": null
}
```

- `kind`: `shell` (a live child, scope 2 adopts it), `job` (a live child whose exit status is reported, `report_status`, src/core/pty/mod.rs:29), `ended` (the child is gone, the pane is kept with its screen, like a failed update tab), `chat`.
- Scope 1 omits `fd`, `pid`, `start` and `screen`; scope 2 has them for live panes. `start` is the child's start time (`proc::start_time`), so the new process never adopts a recycled pid.
- `sessions` carries the quit prompt's list (src/runtime.rs:1434) so jobs of panes closed before the reload are still named after it.
- `termios` is the host terminal's mode read once before `ratatui::init()` (D11).
- Paths are JSON strings when they are UTF-8, else an array of bytes (a folder name that is not text still round-trips).
- Never in the reload state: the environment, tokens, `secrets.toml` values, the MCP session tokens, chat messages (they are in the store), approvals.

**Version rule.** The reader accepts exactly the format versions it lists (version 1 at first). A later reader may add versions it can read; it never guesses at one it does not know. The writer's binary version is recorded for the messages, not for the decision: the decision is made before `exec` by asking the new binary (D9).

### D4. Which descriptors cross `exec`

Every descriptor lazarobox and its dependencies create is close-on-exec (portable-pty, `filedescriptor`, Rust std; §1.2, §1.3). The reload clears `FD_CLOEXEC` (`fcntl(fd, F_SETFD, flags & !FD_CLOEXEC)`) on exactly:

1. the master of each live pane (`MasterPty::as_raw_fd`), one per pane; the reader and writer clones stay close-on-exec and vanish;
2. the reload state descriptor;
3. on Linux, the old-binary descriptor (D10).

Plus 0, 1, 2 (the host terminal). If clearing fails for any of them, the reload is abandoned before `exec` and the flags already cleared are set back. If `exec` itself fails, the flags are set back too (D10). In the new process these descriptors arrive with `FD_CLOEXEC` cleared and it MUST keep them so on every adopted master, the state descriptor and the old-binary descriptor until adoption has fully succeeded (D10, point of no return): the fallback `execveat` must carry them again. Only then does it set `FD_CLOEXEC` on each adopted master and close the state and old-binary descriptors, so that no later child (a new shell, a probe, an engine) inherits a master. The integration test (D13) lists `/proc/self/fd` after `exec` and fails on any other descriptor.

### D5. The screen snapshot

What `vt100` 0.16.2 offers (V, `~/.cargo/registry/.../vt100-0.16.2`): `Screen` is `Clone` (`src/screen.rs:54`), `Parser` is not (`src/parser.rs:3-6`). `state_formatted` = `contents_formatted` + `input_mode_formatted` (`src/screen.rs:224-229`): it clears, draws the visible rows of the **active** grid with their attributes and wrap flags, puts the cursor, then the cursor visibility, application keypad and cursor, bracketed paste, and mouse mode and encoding (:249-259, :379-405; `src/grid.rs:217-252`). It does not include the scrollback, the inactive grid (the primary screen while an application uses the alternate one), the scroll region, origin mode, the saved cursor (`src/grid.rs:4-16` keeps them private) or the window title. `set_scrollback` moves the view into history (`src/screen.rs:113-115`) and `rows_formatted` draws the rows in view (:273-298).

**Snapshot (written by the old process), per PTY pane:**

1. The **primary grid with its history**: on a `Screen` clone, page through the history with `set_scrollback` and `rows_formatted`, oldest first, then the visible rows; rows are joined so that a parser of the same width, fed them, pushes the old ones into its own history and keeps the wrap flags.
2. If the alternate screen is active, the primary grid in step 1 comes from the **clone taken when the application entered it**: `Pane::feed` splits a chunk at `CSI ? 1049 h` / `CSI ? 1047 h` / `CSI ? 47 h`, feeds the part before, clones the `Screen` (the primary is complete at that moment and does not change while the alternate screen is active), then feeds the rest. Then `CSI ? 1049 h` and the alternate grid's `contents_formatted`.
3. `input_mode_formatted` of the active screen, the cursor position, and lazarobox's own state from `Responder`: DECSCUSR shape (`CSI n SP q`), `CSI ? 1004 h` when focus reports are on, and the OSC 7 folder as a field (not replayed).

**Restore (the new process):** a new `Pane` with the same size and scrollback (src/core/pane.rs:186-199) is fed the snapshot; the replies `feed` returns are discarded (src/core/pane.rs:202-205), so no answer reaches the child.

**Bounds.** One snapshot is at most 8 MiB; when the history would exceed it, the oldest rows are dropped and the first restored line says `[older scrollback not kept by the reload]`. The whole reload state is at most `STATE_CAP`.

**Redraw nudge for full-screen programs.** A pane on the alternate screen, or with a mouse mode on, gets a size change after adoption: `TIOCSWINSZ` to one row less, then back. Linux sends SIGWINCH only when the size changes (P, probe 5: a same-size `TIOCSWINSZ` sent none), so the nudge is two real changes; nvim, htop and less redraw on SIGWINCH (I). That repaints what the snapshot cannot carry (the scroll region, their own state).

**Known losses (I, from the API above):** scroll region and origin mode of a shell that set them (rare), the primary's saved cursor (DECSC), the window title (not used by lazarobox), OSC 8 hyperlinks (not kept by `vt100`), the copy-mode selection and the view's scroll offset (reset to the bottom).

The alternative, a replay of a bounded raw output log, is compared in Open question 2.

### D6. Freezing and adopting a pane (scope 2)

**Freeze (old process).** Today the reader blocks in `read` on a clone of the master (`pump`, src/core/pty/portable.rs:189-203, the reader thread :247-256); bytes it has read but the app has not drawn would be lost at `exec`. The reader becomes a `poll` loop on the master and a per-pane stop pipe. On reload:

1. the runtime writes the stop byte to every reader; each reader finishes delivering its current chunk, acknowledges and returns **without** sending `Exited`;
2. the runtime drains the PTY channel into the app (every `Output` is fed to its `Pane`), so the screens are complete;
3. it waits up to 500 ms for every writer queue to be empty (a byte counter next to the channel, src/core/pty/portable.rs:258-270); a pane that does not read its input refuses the reload: `pane <n> is not reading its input`, and the readers restart;
4. from now on, what a child writes stays in the kernel's PTY buffer; the new process reads it.

The waiter thread is left blocked in `wait()`; it vanishes at `exec`. If it already reaped the child (the `reaped` channel, :92, :272-276), the pane is written as `ended`.

**Adopt (new process).** An `AdoptedHandle` implements `PtyHandle` (src/core/pty/mod.rs:97-131) over the raw master: the same `poll` reader and writer threads (started only after the point of no return, D10), `resize` by `TIOCSWINSZ`, `shell_in_foreground` and `probe` by `tcgetpgrp` as today (src/core/pty/portable.rs:115-129), `kill` with the same SIGHUP / grace / SIGKILL escalation (:131-187), and a waiter that calls `waitpid(pid, 0)`. The children are still children of this process (P, probe 1: `waitpid` on a child forked before `exec` returns its exit status after `exec`). Before adopting, the new process checks `proc::start_time(pid) == start`; `ECHILD` or a different start makes the pane `ended`.

New panes after the reload keep using `spawn_portable` with the stoppable reader.

### D7. What is busy

The reload is refused, nothing is changed and the reason is shown, while any of these holds. The first applicable reason is shown with the pane it concerns; the reasons are those the app already gives for a folder change where they exist (src/core/ai/chat/folder.rs:287-309, src/app/folder.rs:31-43):

| Reason shown | Check |
|---|---|
| `chat <n>: an approval is pending` | `approvals.pending()` |
| `chat <n>: a command is running` | a `PtyPane.run` slot of that chat (src/app/folder.rs:33-43) |
| `chat <n>: the model is answering` | phase not idle, or compaction running |
| `chat <n>: writes are staged or proposed` | batch badge or orphan batch |
| `chat <n>: a note is still being saved` | pending save ids |
| `chat <n>: a note or summary is open` | note editor or summary draft |
| `chat <n>: a message is waiting to be sent` | a parked send |
| `chat <n>: not saved yet` | messages shown but `conversation()` is `None` (the store's create is in flight) |
| `chat <n>: an orchestrator run is live` | `orch_run()` is `Some` for the chat (the run lasts from its first event until its `Ended`: coordinator turn, worker turns, and the time between turns), or the runtime's `RunManager` still has a live run for the pane (`is_live`, src/runtime/orchestra.rs:891) |
| `chat <n>: an orchestrator run is still saving its handoffs to engram` | `run_store_waiting()` (src/app/handoff.rs:185): a run store still waits for the project of its pane or for a save's answer (`MEMORY_WAIT`, 5 s, src/core/ai/chat/memory.rs:27) |
| `chat <n>: the run store is still being read` | the `RunsRead::Recovering { pending }` state of the interrupted-runs read (src/app/run_store.rs:46-48): the store was asked and a key pressed meanwhile waits for its answer (`pending`) |
| `chat <n>: a run is about to start or resume` | pending `RunKeep` work (src/app/run_store.rs:72-100): `goals`, `resuming`, `awaiting`, `resume_of` or `resume_goal` held for a pane (a goal before its first worker, a resume not yet closed, a run the user's next reply will close) |
| `pane <n>: a command is waiting for its shell` | `App.typing` (src/app.rs:1152): a connector line waiting for its login or install shell to speak (`FALLBACK` 2 s, src/app/connector.rs:19) |
| `save or discard the menu changes first` | the open menu has an unsaved draft (below) |
| `a download is running` / `llama-server is being installed` / `a model is being deleted` / `feedback is being sent` | the hub, install, delete and issue slots |
| `llama-server is answering another lazarobox` | the quit's shared check (modal-input spec :799-815) |
| `pane <n> is not reading its input` | D6 step 3 (scope 2 only) |

**Orchestrator runs (V).** A run lives only in this process: `RunManager.runs` holds a `Live` per orchestrating pane with its worker turns as tokio tasks (`tasks: HashMap<WorkerId, JoinHandle<()>>`, src/runtime/orchestra.rs:522) that abort when the `Live` drops (:546-553); a worker turn is an engine turn through the chat `Runner`, so its process group is armed with `GroupKill` and listed on the watcher (§1.3; src/runtime/chat/cli.rs). A run can outlive its orchestrator turn (`turn_over`, `waiting`, :532-535). `exec` ends the tokio runtime and, through D8, would have the watcher kill the workers' groups: the run would be lost, with no state to adopt (its rows are in the store but `Live` is not, and a restart only marks it interrupted, src/app/run_resume.rs). Quitting does the same on purpose (`RunManager::shutdown`, src/runtime/orchestra.rs:1086-1101). So a live run is not restartable like an MCP server or a probe, and the reload takes the simplest safe rule: **any live run refuses it**, with the reason above and the chat's number; the user finishes or cancels the run first. No adoption of runs is attempted. The orchestrator's engram saves are an app-side queue (`run_stores: HashMap<RunId, handoff::RunStore>`, src/app.rs:1069) with answers that would arrive at a process that no longer holds the token: a store with an answer outstanding (or waiting for its project) also refuses, and clears by itself within `MEMORY_WAIT`. Before `exec` the runtime also asks the `RunManager` for a live run (a small accessor next to `is_live`), as a second check for a run that started between the app's check and the teardown, and refuses with the same reason.

**Run bookkeeping and connector lines are not carried (V).** `RunKeep` and `Typing` live only in the app's memory and hold pane ids, goals and a pending key that the new process cannot recreate faithfully (a connector line typed into a shell that is about to be replaced would run in the wrong place, or twice). The reload therefore refuses while any of them is held and **carries none of them in the state**: the `Recovering` read and a typed line clear on their own within seconds, `goals`, `resuming`, `awaiting` and `resume_of` clear when the run starts, ends or is discarded. A stored interrupted run (`RunKeep.interrupted`) is not busy: it is a stored row, offered again after the reload.

**An unsaved menu draft.** The reload closes a clean menu as Esc does (`close_menu(Outcome::Reverted)`, src/app.rs:2076 (comment :2073-2075)), which for an update tab discards the draft by design. A reload must not do that silently: when the menu's draft differs from the config it opened with (`original`, src/core/menu.rs:1212; the same comparison the save path uses for its edits, :2985-3032; a small `is_dirty` accessor next to `original()`, :3092), the reload is refused with `save or discard the menu changes first` and the menu stays open. A menu with no changes is closed as Esc closes it.

Not busy: an ended run whose saves are all answered or given up, an interrupted run offered for resume (it is a stored row; the choice is made after the reload), an idle chat with history (it is reopened from the store, Open question 3), a shell running anything (scope 2 keeps it; scope 1 warns like quit), an open menu with no unsaved changes (the reload closes it as Esc, like the update start, src/app.rs:2076 (comment :2073-2075)), AI detection in flight (the new process detects again at start, `startup_effects`, src/runtime.rs:200).

### D8. The watcher during the reload

1. After the busy check, the runtime stops every listed group with the quit's polite stop (`watch::stop_listed(QUIT_GRACE)`, src/runtime/watch.rs:82-86): MCP servers, the managed llama-server (restarted by the next llamacpp turn), detection probes. After D7 nothing listed is user work: the worker groups of an orchestrator run are listed too, which is why D7 refuses a live run before this step.
2. It closes the MCP hubs, which removes their sockets (src/runtime/mcp/session.rs:276-281). This matters: the sockets are named after the PID (:225-230) and the sweep judges liveness by `comm` (:176-193), which `exec` into the same program keeps, so stale sockets would survive the new process's sweep (I).
3. It checks the watcher's list is empty; if a group was armed meanwhile, it stops again once, then refuses with `something lazarobox started is still running`.
4. `exec` closes the watcher's stdin (P, probe 4); the old watcher finds an empty list and exits without signalling anyone (src/runtime/watch.rs:159-176). Pane shells were never on it (§1.3).
5. The new process starts a new watcher as any start does (src/runtime.rs:245).

Rejected: keeping the old watcher by clearing `FD_CLOEXEC` on its pipe. It would let the managed llama-server live across the reload, but the new process would have to rebuild the `Listed` set and the ownership of every armed group; the cost is one llama-server restart.

### D9. Probing the new binary before `exec`

Before anything is stopped:

1. `InstallEnv.exe` is canonicalized again and must be the path recorded at start-up and pass the same trust rule as the `cargo` the update ran (not under `/tmp`, not writable by others; README.md:456-457).
2. `<exe> __adopt --formats` runs as a child (bounded 5 s, through `GroupKill` like other short probes). It prints the reload state format versions it reads, one per line, and exits 0. The writer's format must be listed.
3. A binary from before this change answers `unknown command '__adopt'` with exit 2 (src/core/cli.rs:39-52, src/main.rs:47-50): the reload is refused with `X.Y.Z cannot reload in place: quit and start it again`. Nothing changed.

### D10. Failure handling

| When | What happens | Panes |
|---|---|---|
| Busy, probe fails, reload state write or `FD_CLOEXEC` fails | refused with the reason; readers unfrozen; flags restored | untouched |
| `execve` returns an error | old process continues; flags restored, readers unfrozen, the MCP servers start on demand again; notice `reload failed: <error>; still running X.Y.Z` | untouched |
| New process: reload state unreadable (magic, version, pid, age, size, owner, parse) | Linux: `execveat(<old exe fd>, "", AT_EMPTY_PATH)` with `--fallback` and the same reload state; the old binary reads its own format (P, probe 3: a deleted binary still runs through its descriptor). Elsewhere: start fresh, close the inherited masters, notice `reload failed: the panes could not be kept (<reason>)` and print the same to stderr after the next quit | Linux: kept; elsewhere: lost, never silently |
| New process: adoption fails after it took ownership of a master | see "Adoption is all-or-nothing" below | all treated alike |
| New process: one pane's entry cannot be adopted (descriptor not a PTY master, pid gone, start differs) | pass 1 only records it; after the point of no return that pane becomes `ended` with its snapshot and a last line `[the program in this pane ended during the reload]`; if its descriptor is open and the child lives, it is killed like a closed pane. State-level failures (below) are not this case | the others kept |
| Linux fallback: the `execveat` of the old binary itself fails (provisional) | the new process does not exit: it keeps every master open (no child gets a hangup), starts no reader or writer, and shows on its first screen `reload failed and the previous lazarobox could not be restarted: <cause>; your shells are still running but not shown; quit to end them`; after quit it prints the same to stderr, and quit ends the children as a normal quit does | shells alive, not shown; ended by quit |
| New process: a panic while adopting | caught (`catch_unwind`); handled as "reload state unreadable", with the state descriptor rewound (below) | as above |
| Scope 1: `execve` fails after the teardown | the old process rebuilds the layout from its own reload state (the same function the new process uses) with new shells; notice says so | restarted, never vanished |

**Adoption is all-or-nothing, in two passes, with one point of no return.** Pass 1 validates the whole reload state without touching a master. State-level checks (magic, version, pid, age, size, owner, parse, and every snapshot decoding) fail the whole adoption: fallback. Entry-level checks (the descriptor is open and is a PTY master by `isatty`/`ptsname`, the pid has the recorded start time) never fail the adoption: they only mark that entry `ended` in memory. Pass 1 closes nothing and signals nothing, not even for a marked entry. Pass 2 builds the `AdoptedHandle`s (owning their descriptors, no thread) and restores every screen into memory, starting no thread. It does not read from, write to or resize any master. It ends at the **point of no return**: every handle built, every screen restored, the state descriptor closed (last). If anything in pass 2 fails or panics before that point, the new process has done nothing visible to a child or the old binary: it leaks the handles on purpose (never dropped, so no master closes and no child is signalled), rewinds the state descriptor with `lseek(fd, 0, SEEK_SET)` and runs the fallback below. Only after the point of no return does the new process, in this order: set `FD_CLOEXEC` on the adopted masters (D4), close and kill the entries marked `ended` as a closed pane is, start every reader and writer thread, and send the nudge and the redraw. Bytes a child wrote during the attempt therefore stay in the kernel's PTY buffer for whoever adopts next. On macOS, where there is no fallback, the masters are closed as the table says; the panes are lost and the user sees the notice `reload failed: the panes could not be kept (<reason>)` on the new process's first screen and on stderr after the next quit, never silence.

The state descriptor stays open in the new process until adoption has fully succeeded; only then is it closed. The fallback (`execveat` of the old binary with `--fallback`) therefore reads the same bytes again from offset 0. Because no thread, read, write, resize, nudge or redraw happens before the point of no return, nothing a failed attempt did is visible to the old binary's second adoption.

The old-binary descriptor (Linux only) is `open("/proc/self/exe", O_RDONLY)` without `O_CLOEXEC`, taken just before `exec`; the new process closes it once adoption succeeded. Using it after a successful adoption is never needed.

### D11. The host terminal across `exec`

- Raw mode, the alternate screen, mouse capture, bracketed paste and focus reports are state of the host terminal and its line discipline; `exec` does not touch them (I). The old process does **not** call `restore()` (src/runtime.rs:2749-2760): no flash of the main screen, and with raw mode still on, a Ctrl+C typed in the gap cannot raise SIGINT (I: `ISIG` is off in raw mode).
- crossterm's saved "original" mode lives in the old process's memory (§1.4). The old process reads the host termios with `tcgetattr(0)` once before `ratatui::init()` and writes it in the reload state. The new process sets that mode back with `tcsetattr` and immediately calls `ratatui::init()`, so crossterm captures the user's real mode and the final quit restores it. The window with `ISIG` on is the few microseconds between the two calls (I).
- The new process re-enters the modes (`enter_modes`, mouse sync) and clears and redraws the whole screen. A second `CSI ? 1049 h` while already on the alternate screen: U per terminal (xterm saves the cursor and clears; others may ignore it); the full redraw makes either harmless.
- Keys typed in the gap stay in the tty buffer and reach the new process, except what crossterm had already read (I, small window).
- Signal dispositions reset at `exec` to the default except ignored ones (I, POSIX); the new process installs its handlers at start as today. A SIGHUP in the gap (the terminal closing) ends the process, as it would end lazarobox anyway.

### D12. Security

- The reload state holds screen text and scrollback, which may show tokens or passwords echoed by a program, and chat drafts. It is protected by: no name on Linux; elsewhere a name inside a 0700 folder for microseconds, `O_EXCL|O_NOFOLLOW`, 0600; the descriptor reaches only the new image of the same process; it is closed as soon as it is read; it is never written to the data folder, the store or logs.
- The fallback descriptor of the old binary is read-only.
- No descriptor reaches a pane child: portable-pty still closes everything above 2 in the child (§1.2); `AdoptedHandle` is never used to spawn.
- The new binary must pass D9's trust rule before it is run with the panes' descriptors.
- The chat tools refuse `/proc/<pid>/fd` like any path outside the read roots (I: the read policy is root-based, src/core/ai/tools/paths.rs).
- The WSL drive write grant (README.md:693) is **not** carried: the next write asks again (conservative; the binary that will hold the grant changed).

### D13. Testing

Unit tests (pure, RED first):

- reload state encode/decode round trip for every pane kind; refused: wrong magic, unknown format version, other pid, older than 30 s, over the cap, truncated blob, header that is not JSON, non-UTF-8 path round trip;
- layout export/import: split trees, weights, focus, zoom, `last_shell`, `next_pane_id`;
- screen snapshot round trip on a `Pane` fed known output: colours (indexed and RGB), bold/italic/underline/inverse, wide and combining characters, wrapped rows, cursor position and visibility, history length and content, a 10 000-row history trimmed to the bound with the marker line, application keypad and cursor, bracketed paste, mouse modes, focus reports, cursor shape, OSC 7 folder; the alternate screen with the primary underneath (nvim-like: prompt, `CSI ? 1049 h`, full-screen draw), with the switch in the middle of a chunk; replies from the restore are discarded;
- busy check: one test per reason in D7, and an idle chat with history is not busy;
- `__adopt --formats` output; the cli `KNOWN` list.

Integration (`tests/reload_exec.rs`, unix): the test spawns its own test binary (`std::env::current_exe()`, `--exact <stage test> --test-threads=1 --nocapture`) as a child with `LAZAROBOX_RELOAD_STAGE=1`, so the `exec` never replaces the harness running the other tests. Stage 1 starts `/bin/sh` through `spawn_portable`, prints a marker, freezes, writes a reload state, clears `FD_CLOEXEC` and execs the same test binary with `LAZAROBOX_RELOAD_STAGE=2`. Stage 2 adopts through the library (no TUI), asserts: same PID, the shell answers through the adopted master, the restored screen equals the snapshot, `waitpid` reports the shell's `exit 7`, and on Linux `/proc/self/fd` holds only 0, 1, 2 and the kept descriptors. The parent asserts stage 2's exit code and output. A second case: a close-on-exec master is not inherited and its shell ends with SIGHUP.

Watcher: with an empty list, the old watcher exits on EOF without signalling; a listed group is stopped before `exec`.

Manual (Linux, Kitty and WSL Windows Terminal): nvim in a pane, htop, `sleep 300 &`, an ssh session, a chat with history, a failed update tab; reload; screens and scrollback match, nvim redraws, `jobs` still lists `sleep`, `echo $LAZAROBOX` is unchanged, quitting afterwards restores cooked mode (`stty -a` matches the value before lazarobox started).

## 3. Open questions

### Q1. The key, and where it appears

- **A. Enter on the Updates row once an update was installed in this run** (the row reads `X.Y.Z installed: Enter reloads`; today Enter does nothing there). Discoverable where the user already is, no new key. Only after an update from the row.
- **B. A also, plus a prefix key always available** (for example `Ctrl+Space R`). Also reloads after a `cargo install` typed by hand or a local build; needs a free key and the D9 probe tells "same version" from "newer".
- **C. A, plus offering it when the binary on disk reports a different version** (`<exe> --version` read when the menu opens). Covers a manual install without a new global key; one short probe per menu open.

Recommendation: **A** now; C as a follow-up if manual installs matter. The notice after a successful install says `Updated to X.Y.Z. Enter on Updates reloads it.`

### Q2. Screen state method

- **A. Serialize the `vt100` state at reload** (D5). No cost until a reload; exact for text, attributes, wrap, cursor, input modes and history; loses scroll region, saved cursor and title; needs the primary-screen capture at the alternate-screen switch (one `Screen` clone per switch, memory about the size of the history while an application runs, I).
- **B. Replay a bounded raw output log** (for example the last 2 MiB per pane, fed to a new parser). Simple and keeps every mode the parser knows, but costs memory in every pane all the time, holds secrets in memory all the time, loses history older than the window, and is wrong when the window starts after a mode change (an nvim started before the window comes back on the primary screen).
- **C. A, plus B's log only for panes on the alternate screen.** Better fidelity for full-screen apps, two mechanisms to maintain.

Recommendation: **A with the redraw nudge** (D5). Full-screen programs repaint themselves on SIGWINCH, so the fidelity gap only matters for shells, which rarely set what is lost.

### Q3. What counts as busy, and an idle chat with history

- **A. The D7 list; an idle chat with history reloads without a prompt** (it reopens the same stored conversation, its draft is carried).
- **B. The D7 list, and a `y/n` prompt whenever any chat is open.** Safer-feeling, but asks every time for nothing that is lost.
- **C. Refuse only on in-flight turns and approvals; let staged writes, notes and runs be dropped with a warning.** Fewer refusals, but loses work the user has not seen.

Recommendation: **A**. Scope 2 asks nothing when nothing is busy; scope 1 asks like quit (`Restart? still running: … (y/n)`), because its pane programs do stop.

### Q4. Platforms for scope 2

- **A. Linux, WSL2 included; macOS and WSL1 get scope 1.** Matches 1.0 (README.md:178) and the probes.
- **B. Linux and macOS.** Same POSIX rules, but unverified on a Mac and without the D10 old-binary fallback there.
- **C. Linux only, WSL excluded until a Windows Terminal check.**

Recommendation: **A**, with macOS added after a manual test on a real Mac (as the quit-names-pane-jobs change did, its task 1.5).

### Q5. Does scope 1 ship first?

- **A. Yes: S1 and S2 first.** Useful on its own (no more lost layout after an update), builds the `__adopt` start, the probe, the reload state and the terminal handling that scope 2 reuses, and is scope 2's fallback.
- **B. Serialization core first (S3), then scope 2, scope 1 last.** Reaches the goal sooner on paper, but ships nothing usable until the riskiest slice lands and has no fallback meanwhile.

Recommendation: **A**.

## 4. Residual risks

These stay after the design above; none changes a scope decision.

1. **The old-binary fallback is Linux-only (D10).** It needs `/proc/self/exe` and `execveat` on a descriptor. On macOS (no `/proc`; never probed, U) a new process that cannot read the reload state starts with one new shell and closes the inherited masters, so **every pane's program ends** (SIGHUP when the master closes). The loss is shown (a notice, then printed after the next quit) but it is destructive, not recoverable. Under Open question 4 recommendation A macOS runs scope 1, where the shells are torn down by design and the prompt says so, so the exposure is limited to the case where Q4 is decided B.
2. **A probe that passes does not prove the adoption.** `__adopt --formats` (D9) shows that the new binary starts, parses its arguments and lists the writer's format. It does not read a real reload state. A new binary whose probe passes but whose adoption later fails (a bug, a panic outside `catch_unwind`, a descriptor it mishandles) is saved only by the Linux fallback, and only when that new process still gets as far as running it: one that is killed or crashes before it (a missing library, SIGSEGV, the OOM killer) leaves the panes without an owner, and their masters close with it. Elsewhere there is no fallback at all (risk 1).
3. **Unprobed (U).** A SIGHUP or a closing terminal in the gap between `exec` and the new process's handlers ends the process (it would end lazarobox anyway, D11), and what happens to the panes then follows from the masters closing; I did not probe that sequence mid-reload. A second `CSI ? 1049 h` while the terminal is already on the alternate screen is handled differently per terminal (xterm clears; others may ignore it), and only the full redraw makes it harmless (D11); it was not probed on Kitty or Windows Terminal.
4. **macOS was never probed.** Every P in this document ran on Linux 6.18 under WSL2. `execve`, `FD_CLOEXEC` and `waitpid` of a child forked before `exec` follow POSIX, but the memfd-less file path (D3), `ps`-based liveness and the terminal behaviour are unverified there; WSL1 is unverified too.
5. **Live orchestrator runs are not carried (D7).** The reload is refused while one is live or while its engram saves are outstanding. The cost is a refusal the user must resolve; the benefit is that no run is lost. If a later change adopts runs, D7 and D8 change with it.

## 5. Probes run (P)

Throwaway Python 3 scripts in a fresh `mktemp -d` (`/tmp/tmp.gfy7J2bYeY`), run with `python3 -I`, removed afterwards. No build of this crate.

1. A shell on a PTY (`pty.fork`), three extra threads, `FD_CLOEXEC` cleared on the master, `execv` of Python: same PID after `exec`; the master is valid; the shell answers `ALIVE-42` through it; `waitpid` on the shell returns its `exit 7`.
2. The same with `FD_CLOEXEC` set, for `/bin/sh` (dash) and `/bin/bash`: the master is gone after `exec` and the shell ends by signal 1 (SIGHUP). Without the flag, both shells are alive 3 s later.
3. A copy of `/bin/sleep`, started then deleted: `/proc/<pid>/exe` reads `… (deleted)`, opening it gives the ELF header, and running `/proc/self/fd/<n>` of that descriptor works (`sleep (GNU coreutils) 9.4`).
4. A watcher-like child reading a close-on-exec pipe: it sees EOF about 0.01 s after the writer's `exec`, while the exec'd program still runs.
5. `TIOCSWINSZ` with the size the PTY already has sends no SIGWINCH (one real change counted 1, the repeated same size added none).
