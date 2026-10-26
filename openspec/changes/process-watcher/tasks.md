# Tasks: Process watcher

- [x] 1.1 RED/GREEN: `start_time` tells a process apart and ends with it.
- [x] 1.2 RED/GREEN: the watcher stops a listed group and its children when its input ends (SIGHUP first), SIGKILLs one that ignores SIGHUP and SIGTERM after the grace, leaves a reaped group and a reused pid alone, ignores malformed lines, never targets groups 0, 1 or its own.
- [x] 1.3 [auto] `lazarobox-shell __watch` stops the listed group once its stdin closes, and exits 0.
- [x] 1.4 [manual] Live, Linux: a hanging `codex --version` probe survived `kill -9` of lazarobox before (main), and was stopped after, 3/3, with the watcher in its own session and gone afterwards; a closed terminal left nothing behind.
- [ ] 1.5 [manual] macOS: the same `kill -9` check on a real Mac.
- [x] 1.6 Docs (README «Al cerrar lazarobox», CHANGELOG) and gate.
