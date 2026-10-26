# Tasks: Managed llama-server options

- [x] 1.1 RED/GREEN: a missing model file starts nothing and says so; the next turn starts it once the file is back; a missing program still reads "is it on the PATH?".
- [x] 1.2 Docs (README, CHANGELOG) for the model check.
- [x] 2.1 RED/GREEN: `args` read as a list of strings (also with `managed = false`), clashes and wrong types refused whole with a problem; passed after lazarobox's own, one argv element each (`my model` stays one).
- [x] 2.2 Docs (README, CHANGELOG) for `args`.
- [x] 3.1 RED/GREEN: `idle_minutes` read (default 15, 0 never, wrong values fall back with a problem); the menu steps 5/15/30/never and keeps an in-between value.
- [x] 3.2 RED/GREEN: a lease keeps a turn's server; `stop_if_idle` stops it only past the idle time after the last turn; the live idle time decides, none never stops; the next turn starts it again.
- [x] 3.3 RED/GREEN: a menu save that changes the idle time emits `ManagedIdle`, which reaches the runtime; the row's edit is written as a TOML integer and reads back.
- [x] 3.4 Docs (README, CHANGELOG).
- [x] 3.5 [manual] Live: a real llama-server stops after the idle time and the next turn starts it.
- [x] 4.1 RED/GREEN: the same model on a port of this user is used and never stopped; another model starts ours on the next free port; the same model is found past busy ports; every port taken refuses with the message; a used server that went away is replaced by ours; a start that lost the port to the same model uses that server; the turn and detection use the port it runs on; listeners are read with their owner.
- [x] 4.2 RED/GREEN: managed, the picker lists the local models and choosing one manages it while another llama-server answers.
- [x] 4.3 Docs (README, CHANGELOG).
- [x] 4.4 [manual] Live: two lazarobox, same model shared, then another model on the next port.
- [ ] 4.5 [manual] macOS: `lsof` finds the user's own listener.
