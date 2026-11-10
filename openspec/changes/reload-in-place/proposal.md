# Proposal: Reload in place after an update (the panes keep running)

## Intent

Issue #598, "Goal: reload after an update without stopping the panes". The owner's direction (2026-11-10, verbatim):

> "lo que quiere hacer es un reload una vez que se ha actualizado, ocmo si lo hiciéramos aquí"

and, on the scope:

> "y el dos si es posible hacerlo creo que además nos beneficia más que no tener que parar todo lo que se está haciendo para la actualización"

Today, after the update tab finishes, the Updates row reads `X.Y.Z installed: restart` (src/core/update.rs:380-383) and the footer reads `Updated to X.Y.Z · restart lazarobox to use it` (src/core/menu.rs:3033). The user has to quit and start lazarobox again (README.md:460: "el proceso en marcha no se reemplaza"). Quitting stops every pane (src/runtime.rs:771-811) and lazarobox saves no session: tabs, panes and folders are lost; only the chat history is kept.

This change replaces the running lazarobox with the newly installed binary **in the same process** (`exec`, same PID), so:

- **Scope 2, the goal (owner's choice):** the panes' processes keep running and each pane's screen and scrollback look the same after the reload.
- **Scope 1, the stepping stone and the fallback:** the same key restarts with the new version and reopens the same tabs and panes in their folders; the shells start again and the programs that ran in them do not.

## Scope

In scope:

1. A reload action on the Updates row once an update was installed in this run (see Open question 1 for whether it is offered at other times).
2. A **handoff**: before `exec`, the old process writes its state (tabs, layout, focus, zoom, each pane, each chat's conversation) to a private, unnamed 0600 file whose descriptor survives `exec`; the new process reads it, then closes it.
3. **Scope 1**: the new process rebuilds the tabs and layout and starts a new shell in each pane's last folder; chat panes reopen their stored conversation.
4. **Scope 2**: the PTY master descriptors survive `exec`; the new process adopts each pane's live child and rebuilds its screen from a snapshot taken by the old process.
5. A **busy check** that refuses the reload, with a visible reason, while something would be lost: a chat answering, a pending approval, writes staged or proposed, an orchestrator run, a download or install, and the rest listed in design D7.
6. **Failure handling**: a failure before `exec` changes nothing; `exec` failing keeps the old process; a new binary that cannot adopt falls back to scope 1 or to the old binary; panes are never lost silently.
7. Docs: README «Avisos de actualización» and «Al cerrar lazarobox», CHANGELOG.

Out of scope:

- Saving a session to disk to reopen after a real quit or a reboot (the handoff lives only across one `exec`).
- Reloading a different install (a distro package, a local build) than the one the Updates row installed, unless Open question 1 decides otherwise.
- Keeping in-flight chat turns, approvals, orchestrator runs or downloads alive across the reload: they refuse it instead.
- Native Windows: lazarobox has no native Windows build (README.md:95); WSL is Linux here.

## Approach (short; the detail is in design.md)

- `exec` keeps the PID (probed on this machine, design D1), so the pane shells stay children of lazarobox and `LAZAROBOX=<pid>` (src/core/pty/mod.rs:50-53) stays true.
- portable-pty marks every PTY master close-on-exec (portable-pty 0.9.0 `src/unix.rs:64`) and its reader and writer clones too (`filedescriptor` `F_DUPFD_CLOEXEC`, `src/unix.rs:149`). The reload clears `FD_CLOEXEC` only on the descriptors it keeps: one master per live pane, the handoff descriptor and, on Linux, a descriptor of the old binary for the fallback. Probed: a close-on-exec master closes at `exec` and the shell dies of SIGHUP; a master without it survives and the shell answers through it after `exec`.
- The pane readers stop at a freeze point first, so no output byte is read by the old process and lost: what the children write afterwards stays in the kernel PTY buffer for the new process.
- The `__watch` watcher never lists pane shells (src/runtime/proc.rs:65-72 registers only `GroupKill` users; src/runtime.rs:1743-1755 spawns panes without one; README.md:170). It does list chat engines, MCP servers, the managed llama-server and short probes, and it stops them when its stdin pipe closes, which `exec` does (probed: EOF 10 ms after `exec`). The reload therefore stops those restartable groups first, exactly as quitting does, and refuses when anything listed is user work.
- The screen is serialized from the `vt100` parser through its public API, with the primary screen captured when an application enters the alternate screen; full-screen applications get a resize nudge so they redraw themselves (design D5, Open question 2).

## Slices (advisory, about 400 production lines each; tests and docs may exceed)

| Slice | What | Production lines |
|---|---|---|
| S1 | Handoff container and layout export/import (pure core) | 250-330 |
| S2 | Scope 1: the reload key, prompt, teardown, `exec`, `__adopt`, new-binary probe | 300-380 |
| S3 | Screen snapshot core (pure: encoder, alternate-screen capture, restore) | 250-350 |
| S4 | PTY freeze and adoption (reader with a stop pipe, adopted handle, `FD_CLOEXEC`) | 280-380 |
| S5 | Scope 2 wiring: busy check, watcher, terminal state, adoption in the new process | 300-400 |
| S6 | Fallbacks: per-pane fallback, old-binary fallback on Linux, notices, docs | 150-250 |

S1 and S2 ship scope 1 on their own (Open question 5). Each later slice keeps scope 1 working.

## Risks

- **`exec` is one-way.** Once it succeeds the old program is gone; a new binary that dies before adopting takes the panes with it. Mitigated by probing the new binary before `exec` (D9), by the Linux old-binary fallback (D10) and by catching panics during adoption.
- **Screen fidelity.** `vt100` 0.16.2 does not expose the scroll region, origin mode, the saved cursor or the inactive grid (design D5). Shells are unaffected in practice (inferred); full-screen programs are asked to redraw.
- **Secrets.** The handoff holds screen text and scrollback. It never has a name on disk on Linux (memfd), is unlinked at once elsewhere, is 0600, and is closed as soon as it is read (D3).
- **Platforms.** Only Linux (WSL2 included) was probed. macOS follows the same POSIX rules but is unverified and not supported in 1.0 (README.md:178).

## Open questions

See design.md, "Open questions": the key and where it appears, the screen state method, what counts as busy (and whether an idle chat reloads without a prompt), the platforms for scope 2, and whether scope 1 ships first. Each has options, tradeoffs and a recommendation.
