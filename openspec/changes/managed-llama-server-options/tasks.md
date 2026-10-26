# Tasks: Managed llama-server options

- [x] 1.1 RED/GREEN: a missing model file starts nothing and says so; the next turn starts it once the file is back; a missing program still reads "is it on the PATH?".
- [x] 1.2 Docs (README, CHANGELOG) for the model check.
- [x] 2.1 RED/GREEN: `args` read as a list of strings (also with `managed = false`), clashes and wrong types refused whole with a problem; passed after lazarobox's own, one argv element each (`my model` stays one).
- [x] 2.2 Docs (README, CHANGELOG) for `args`.
- [x] 3.1 RED/GREEN: `idle_minutes` read (default 15, 0 never, wrong values fall back with a problem); the menu steps 5/15/30/never and keeps an in-between value.
- [x] 3.2 RED/GREEN: a lease keeps a turn's server; `stop_if_idle` stops it only past the idle time after the last turn; the live idle time decides, none never stops; the next turn starts it again.
- [x] 3.3 RED/GREEN: a menu save that changes the idle time emits `ManagedIdle`, which reaches the runtime; the row's edit is written as a TOML integer and reads back.
- [x] 3.4 Docs (README, CHANGELOG).
- [ ] 3.5 [manual] Live: a real llama-server stops after the idle time and the next turn starts it.
