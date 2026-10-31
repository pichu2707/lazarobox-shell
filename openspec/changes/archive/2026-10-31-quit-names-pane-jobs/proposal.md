# Proposal: Quit names what still runs in the panes

## Intent

A pane behaves like any terminal: a job started there with `&`, `nohup` or `disown` keeps running after lazarobox closes, and nothing tells the user. Keep those terminal semantics, but warn about what is still running when closing.

## Scope

In scope:
- The quit prompt (`Ctrl+Space q`, and `w q` / `t c` on the last pane or the only tab) names the jobs still running in the panes: `Quit? still running: sleep 77, npm run dev (y/n)`. At most three names, each cut to 24 columns, `+N more` for the rest; fewer names, then only the count (`Quit? 3 still running (y/n)`), then the plain prompt when the statusline is narrower.
- A job is a process group of a pane's session other than its shell's own group, whether in the foreground (`vim`) or the background. A closed pane's session still counts.
- After lazarobox closes, however it was asked to (the prompt or `exit` in the last pane), the jobs still running are written to stderr once the terminal is restored, with their process group.

Out of scope:
- Stopping those jobs: lazarobox never signals them.
- A closed terminal tab: there is no terminal left to warn on.
- A daemon that left the pane's session (`setsid`): it is not named.

## Approach

`runtime::jobs` lists the processes of the recorded sessions (`/proc` on Linux, `proc_listallpids` + `proc_pidinfo` on macOS) and groups them by process group. A session counts only while its leader is the recorded shell (same start time) or gone. The app asks with `Effect::ListRunning` when it opens the quit prompt; the runtime answers with `AppEvent::StillRunning`. After the quit, the list is read once, and once more 150 ms later when not empty, so jobs ending on their hangup are not named.

## Risks

- None on processes: the change only reads.
- A slow scan on a machine with many processes: only `/proc/<pid>/stat` is read for processes outside the panes' sessions.
