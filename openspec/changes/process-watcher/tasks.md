# Tasks: Process watcher

- [x] 1.1 RED/GREEN: `start_time` tells a process apart and ends with it.
- [x] 1.2 RED/GREEN: the watcher stops a listed group and its children when its input ends (SIGHUP first), SIGKILLs one that ignores SIGHUP and SIGTERM after the grace, leaves a reaped group and a reused pid alone, ignores malformed lines, never targets groups 0, 1 or its own.
- [x] 1.3 [auto] `lazarobox-shell __watch` stops the listed group once its stdin closes, and exits 0.
- [x] 1.4 [manual] Live, Linux: a hanging `codex --version` probe survived `kill -9` of lazarobox before (main), and was stopped after, 3/3, with the watcher in its own session and gone afterwards; a closed terminal left nothing behind.
- [ ] 1.5 [manual] macOS: the same `kill -9` check on a real Mac.
- [x] 1.7 RED/GREEN: quitting asks a listed group to end (it exits on its own), kills one that ignores the polite signals after the grace, gives no SIGKILL to a group reaped meanwhile, ends at once with nothing listed; a zombie leader counts as ended; the chats are stopped only after the polite stop.
- [x] 1.8 [manual] Live, Linux: a fake `codex` that traps SIGHUP/SIGTERM was SIGKILLed on quit before (main) and ended politely after, on a closed terminal (2/2), SIGTERM and SIGKILL of lazarobox, with nothing left and lazarobox gone in under 0.1 s.
- [x] 1.9 RED/GREEN (verifier B1, A1): a streaming turn keeps running after the loop is gone until the polite stop is over; the watcher gives what a leader left behind the grace.
- [x] 1.6 Docs (README «Al cerrar lazarobox», CHANGELOG) and gate.
