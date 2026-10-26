# Proposal: Process watcher

## Intent

A clean quit already stops what lazarobox started, but a crash, a SIGKILL or any exit that skips the quit cleanup leaves every CLI turn, MCP server and managed llama-server running. On macOS, which has no parent-death signal, the orphaned llama-server then holds its port and every later start fails with `llama-server not started: another program answers on 127.0.0.1:<port>`. Make sure nothing lazarobox started outlives it, however it ends.

## Scope

In scope:
- A watcher process (`lazarobox-shell __watch`) started first in `run()`, in its own session, before any pane or child exists.
- Every `GroupKill` lists its group with the watcher (pgid and leader start time) and takes it off when disarmed (reaped).
- When the watcher's stdin ends (lazarobox ended), it sends SIGHUP and SIGTERM to the groups still listed, SIGKILL after 3 s, and exits.
- A group is signalled only while its leader is the listed process (same start time) or gone; never 0, 1 or the watcher's own group.

Out of scope:
- `engram serve`: the user's shared memory server, left running on purpose.
- Pane shells: they get SIGHUP from their terminal when lazarobox's PTY closes.
- Making quit return without waiting for the children (next task of this change).

## Approach

`runtime::proc::start_time` reads a process's start time (`/proc/<pid>/stat` on Linux, `proc_pidinfo` on macOS). `runtime::watch` holds the watcher: `start()` spawns it and keeps the non-blocking write end of its stdin in a `OnceLock`; `register`/`unregister` write one line each; `watch()` reads the list until EOF and stops what is left.

## Risks

- Killing the wrong process: guarded by the leader start-time check and the fact that a pid is not reused while a group with that id has members.
- A full pipe never blocks lazarobox: the write is non-blocking and the line is dropped.
- The watcher must not keep a terminal or a PTY open: it starts before any pane and in its own session.
