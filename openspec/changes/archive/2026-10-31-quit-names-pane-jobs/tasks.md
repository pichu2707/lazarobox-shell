# Tasks: Quit names what still runs in the panes

- [x] 1.1 RED/GREEN: `jobs::running` names the background jobs of a pane's session and not its shell, still names a job once the shell exited, ignores a session whose leader is another process, returns nothing for no session.
- [x] 1.2 RED/GREEN: opening the quit prompt (`q`, last-pane `w q`, only-tab `t c`) emits `ListRunning`; `StillRunning` names the jobs in the prompt (three at most, cut, `+N more`, fitted to the width); a late answer is ignored.
- [x] 1.3 RED/GREEN: the runtime records the session of every spawned pane (closed ones included) and answers `ListRunning`; the report after quitting names each job with its process group and re-reads after the settle.
- [x] 1.4 [manual] Live, Linux (tmux, bash and zsh panes): `sleep 7777 &` is named by the prompt, printed on stderr after `exit` in the last pane and keeps running; after `y` bash and zsh end it on their hangup and nothing is printed; a `nohup` job is printed after `y` and keeps running.
- [x] 1.5 [manual] macOS: the same check on a real Mac. [deferred 2026-10-31: 1.0 is Linux-only; macOS checks wait until macOS is supported]
- [x] 1.6 Docs (README «Al cerrar lazarobox», CHANGELOG).
