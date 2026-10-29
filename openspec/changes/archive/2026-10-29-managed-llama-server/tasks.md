# Tasks: Managed llama-server

- [x] 1.1 RED/GREEN: `managed` and `model` of `[ai.servers.llamacpp]`, the managed port, problems, and the keys ignored under other servers.
- [x] 1.2 RED/GREEN: the first llamacpp turn starts the server and later turns reuse it; quitting stops it; a port that already answers is refused; a failed start says why; the chat gate lets an installed managed llamacpp through.
- [x] 1.3 RED/GREEN: on Linux the server dies with lazarobox (parent-death signal). Closing the terminal no longer aborts lazarobox before its quit cleanup (checked by hand: 3/3 left the server running before the fix, 0/3 after).
- [x] 1.4 Docs (README, CHANGELOG) and gate.
- [x] 1.5 [manual] With a real `llama-server` b11815: starting lazarobox runs nothing; the first turn starts the server on 127.0.0.1:39281 in its own process group; a second turn reuses it; a clean quit, a closed terminal and SIGKILL all leave no server behind.
