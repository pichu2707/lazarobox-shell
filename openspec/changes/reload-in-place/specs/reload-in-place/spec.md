# Reload In Place Specification

## Purpose

Replace the running lazarobox with the newly installed binary in the same process, after an update, without stopping what runs in the panes (issue #598; owner: "y el dos si es posible hacerlo creo que además nos beneficia más que no tener que parar todo lo que se está haciendo para la actualización"). Scope 2 keeps each pane's process and screen; scope 1 (the stepping stone and the fallback) keeps the tabs, layout and folders and starts new shells. Slice tags: [S1] handoff and layout, [S2] scope 1, [S3] screen snapshot, [S4] PTY freeze and adoption, [S5] scope 2 wiring, [S6] fallbacks. Scenario tags: [auto] = `cargo test` (pure, fake PTY, or a unix PTY with a real `exec` of the test binary); [manual] = real terminal. Items marked "assumption" are proposals pending the open questions in design.md.

Terms: "old process" = lazarobox before `exec`; "new process" = the same process after `exec` of the new binary; "handoff" = the state the old process passes to the new one (design D3); "live pane" = a PTY pane whose child has not been reaped.

## Requirements

### Requirement: Reload starts only from a probed, trusted new binary [S2]
A reload MUST run the binary at the path lazarobox was started from (the install path recorded at start-up), and only when, at the moment of the reload, that path still resolves to the same place, passes the same trust rule as the `cargo` the update runs, and the binary answers `__adopt --formats` within 5 seconds with a list that contains the handoff format version of the running lazarobox. Otherwise nothing MUST be stopped or changed, and the reason MUST be shown: `X.Y.Z cannot reload in place: quit and start it again` for a binary without `__adopt`, else `reload refused: <reason>`.

#### Scenario: A binary from before this change [auto]
- GIVEN the installed binary answers `unknown command '__adopt'` with exit 2
- WHEN the reload is asked
- THEN the footer reads `0.9.0 cannot reload in place: quit and start it again`, no process is signalled and every pane keeps running

#### Scenario: Format not supported [auto]
- GIVEN the new binary lists only format `2` and the running lazarobox writes `1`
- WHEN the reload is asked
- THEN it is refused and nothing changes

### Requirement: Busy refuses the reload with a visible reason [S5]
The reload MUST be refused, with nothing changed, while any chat has an approval pending, a command running for it, the model answering or a compaction running, writes staged or proposed, a note being saved, a note or summary open, a message waiting to be sent, messages not yet stored, or a live orchestrator run; while a download, an install, a model delete or a feedback send is in flight; while the managed llama-server is answering another lazarobox; and, in scope 2, while a pane does not take its queued input within 500 ms. The first reason MUST be shown as `reload refused: <reason>` naming the pane for a chat (`chat 3: the model is answering`). An idle chat with history MUST NOT refuse the reload (assumption, Open question 3). An open menu MUST be closed as Esc closes it.

#### Scenario: A chat is answering [auto]
- GIVEN chat pane 3 streaming an answer
- WHEN the reload is asked
- THEN the footer reads `reload refused: chat 3: the model is answering` and the turn goes on

#### Scenario: An idle chat with history [auto]
- GIVEN an idle chat pane with 12 stored messages and the draft `and then`
- WHEN the reload is asked
- THEN it is not refused, and after it the pane shows the same conversation with the draft `and then`

#### Scenario: A live orchestrator run [auto]
- GIVEN an ORCH pane whose run has a worker running
- WHEN the reload is asked
- THEN it is refused with `chat <n>: an orchestrator run is live`

### Requirement: Handoff file [S1]
The handoff MUST be readable only through a descriptor that survives `exec`: on Linux an anonymous memory file with no name; elsewhere a file created with `O_EXCL|O_NOFOLLOW` and mode 0600 in lazarobox's 0700 private run folder and unlinked before `exec`. It MUST start with the magic `LAZAROBOX-HANDOFF`, a format version and the writer's version, and hold the process id, the creation time, the host terminal's mode and size, the tabs (layout tree, weights, focus, zoom, last shell), the next pane id, and each pane: kind (`shell`, `job`, `ended`, `chat`), size, folder, and for scope 2 the master descriptor number, child pid and start time and a screen snapshot; for a chat its conversation id, folder, draft and ORCH flag; and the pane sessions the quit prompt names. It MUST NOT hold the environment, tokens, secrets, chat messages or approvals. The new process MUST refuse a handoff with another magic, a format version it does not list, another process id, older than 30 seconds, larger than 64 MiB, not owned by its user or with group or other permission bits; it MUST close the descriptor once read and never write it.

#### Scenario: Round trip [auto]
- GIVEN two tabs, one with a vertical split of a shell and a chat, the other zoomed on an ended pane
- WHEN the handoff is written and read back
- THEN tabs, weights, focus, zoom, last shell, next pane id, kinds, folders, conversation id and draft are equal

#### Scenario: Refused handoffs [auto]
- GIVEN a handoff with the wrong magic, then format `9`, then another pid, then 31 seconds old, then a truncated blob
- WHEN each is read
- THEN each is refused with its reason and nothing in it is used

#### Scenario: Never named on Linux [auto, linux]
- GIVEN a reload in progress
- WHEN the private run folder is listed just before `exec`
- THEN it holds no handoff file

#### Scenario: A folder that is not text [auto]
- GIVEN a pane whose folder name holds the byte `0xff`
- WHEN the handoff is written and read back
- THEN the folder is the same bytes

### Requirement: Only the kept descriptors cross exec [S4]
Before `exec`, close-on-exec MUST be cleared only on the master of each live pane, the handoff descriptor and, on Linux, a read-only descriptor of the running binary; every other descriptor MUST stay close-on-exec. If clearing fails for one of them, or `exec` fails, the flags MUST be set back and the reload abandoned. No kept descriptor MUST reach a pane child.

#### Scenario: Descriptor list after exec [auto, linux]
- GIVEN a test process with one shell pane that execs itself through the reload path
- WHEN its descriptors are listed after `exec`
- THEN they are 0, 1, 2, the master, the handoff and the old-binary descriptor, and nothing else

### Requirement: Scope 1 restarts keeping the layout [S2]
In scope 1 the old process MUST ask first, as quit does, naming the jobs still running in the panes (`Restart? still running: sleep 77 (y/n)`); on `y` it MUST save the chats and stop the panes and every child it started as quitting does, without restoring the host terminal, then `exec` the new binary with the handoff. The new process MUST rebuild the same tabs, layout, focus and zoom, start a new shell in each shell pane's last folder (falling back as a new pane does when the folder is gone), reopen each chat pane's conversation in its folder with its draft, and show `restarted with X.Y.Z: the shells started again`. The jobs left running MUST be named in the new process's first notice instead of on stderr.

#### Scenario: Layout and folders come back [auto]
- GIVEN two tabs: shells in `/w/a` and `/w/b` split side by side, and a chat on conversation 812
- WHEN scope 1 runs through the handoff
- THEN the new app has the same tabs and split, the shells are spawned in `/w/a` and `/w/b`, and the chat shows conversation 812

#### Scenario: Jobs are named [auto]
- GIVEN a pane that started `sleep 77 &`
- WHEN scope 1 is asked
- THEN the statusline reads `Restart? still running: sleep 77 (y/n)`, and after `y` the new process's notice names `sleep 77`

### Requirement: Scope 2 keeps the panes running [S5]
In scope 2 every live pane's child MUST keep running across the reload with the same pid, and its output written during the reload MUST NOT be lost: the old process stops its readers at a freeze point, draws what they read, and leaves the rest in the PTY. The new process MUST adopt each live pane whose pid still has the recorded start time, report its exit as before (with the exit status for a `job` pane), and show `reloaded to X.Y.Z`. Children that are not panes (chat engines, MCP servers, the managed llama-server, probes) MUST be stopped before `exec` as quitting stops them, and start again on demand.

#### Scenario: A shell survives [auto, unix PTY, real exec]
- GIVEN a shell pane that printed `BEFORE`
- WHEN the reload runs and the new process writes `echo AFTER-$((40+2))` to the adopted pane
- THEN the pid is unchanged, the screen shows `BEFORE` and then `AFTER-42`, and `exit 7` is later reported as the pane's exit

#### Scenario: Output during the reload [auto, unix PTY, real exec]
- GIVEN a pane running `sh -c 'sleep 0.2; echo LATE'`
- WHEN the reload freezes its reader before `LATE` is written
- THEN the new process shows `LATE` once

#### Scenario: nvim keeps running [manual]
- GIVEN nvim open on a file in one pane and `sleep 300 &` in another
- WHEN the reload runs
- THEN nvim redraws the same file at the same place, `jobs` in the second pane still lists `sleep 300`, and `echo $LAZAROBOX` prints the same pid as before

### Requirement: The watcher stops nothing during a reload [S5]
Before `exec`, every process group on the watcher's list MUST have been stopped and taken off it, and the MCP session sockets removed; if a group is still listed after one more stop, the reload MUST be refused with `something lazarobox started is still running`. The old watcher MUST end on `exec` without signalling any group, and the new process MUST start its own watcher. Pane shells are never on the list.

#### Scenario: Empty list at exec [auto]
- GIVEN a running MCP server listed with the watcher
- WHEN the reload reaches `exec`
- THEN the server was stopped and unlisted first, and the watcher exits on its stdin's end having signalled nothing

### Requirement: The host terminal stays usable [S2]
The old process MUST NOT restore the host terminal before `exec`: raw mode and the alternate screen stay on. It MUST pass the host terminal's mode from before raw mode; the new process MUST set it back immediately before entering raw mode again, re-enter its terminal modes and redraw the whole screen. After a reload, quitting MUST leave the host terminal as it was before lazarobox started.

#### Scenario: Cooked mode after quitting [manual]
- GIVEN `stty -a` noted before starting lazarobox
- WHEN lazarobox is reloaded, then quit
- THEN `stty -a` prints the same

### Requirement: Failures never lose panes silently [S6]
A failure before `exec` MUST change nothing. A failed `exec` MUST keep the old process running with every pane, and show `reload failed: <error>; still running X.Y.Z`. When the new process cannot read the handoff or panics while adopting, on Linux it MUST run the old binary through its descriptor with the same handoff; elsewhere it MUST start with a single new shell and show and, after its next quit, print `reload failed: the panes could not be kept (<reason>)`. A pane that cannot be adopted MUST stay shown with its last screen and the line `[the program in this pane ended during the reload]`. When scope 1's `exec` fails after the teardown, the old process MUST rebuild the layout from its own handoff with new shells.

#### Scenario: exec fails [auto]
- GIVEN an `exec` that returns `ENOENT`
- WHEN the reload runs
- THEN the old process goes on, every pane is live, its descriptors are close-on-exec again, and the notice reads `reload failed: … ; still running 1.8.0`

#### Scenario: New binary cannot read the handoff [auto, linux, real exec]
- GIVEN a new binary that refuses the handoff
- WHEN it starts
- THEN the old binary runs again through its descriptor and adopts the panes

#### Scenario: One pane cannot be adopted [auto]
- GIVEN a handoff whose pane 2 pid now has another start time
- WHEN the new process adopts
- THEN pane 2 shows its last screen and `[the program in this pane ended during the reload]`, and the other panes are adopted

### Requirement: Platforms [S5]
Scope 2 MUST be offered on Linux, WSL2 included (assumption, Open question 4). On other platforms the reload MUST run scope 1.

#### Scenario: macOS runs scope 1 [auto, macos]
- GIVEN lazarobox on macOS after an update
- WHEN the reload is asked
- THEN the scope 1 prompt opens
