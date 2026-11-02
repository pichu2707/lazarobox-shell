# Delta for Pane Commands

Applies on top of the unarchived `agent-tools` change spec of pane-commands. The run action of the user and the prompt route never execute; only a `run_in_pane` call in `run` mode that passes pane-execution does. Tags as in the main spec.

## MODIFIED Requirements

### Requirement: Approval types, never executes
Only an unmodified lowercase `y` press MUST accept a pending confirm. Accepting MUST write the validated command into the target pane and nothing else: no Enter, no newline after the command, no carriage return, in every approval mode, except a model's command in `run` mode, which `y` runs (see pane-execution). Only `n` and Esc MUST decline (nothing is written). `Y`, Enter, a modified `y` and every other key MUST NOT answer: they are swallowed and the confirm stays pending (see modal-input, Approval keys). Repeat and Release events MUST NOT answer. One accept MUST produce exactly one write; a second `y` after the confirm closed MUST do nothing. There MUST be no setting, key or flag that skips the confirm of the run action, and no "always allow". Two paths write without this confirm, both for a model `run_in_pane` call and both running the same validation and rechecks: in a chat in `auto` mode the command is typed and never followed by Enter (see agent-tools, Chat approval mode); in a chat in `run` mode a command that passes the pane-execution grammar, flag allow-list and path policy is typed and followed by Enter in one write, or a model's command the user approves with `y` on this confirm is run the same way; those are the only ways lazarobox ever sends Enter on behalf of a model. In `ask` and `auto`, and for a command `y` cannot run, `y` types it with no Enter.
(Previously: the only path that types without the confirm was `auto`, and lazarobox never sent Enter)

#### Scenario: y types without Enter [auto]
- GIVEN a pending confirm for `cd ~/dev/curso-rust` on pane 2
- WHEN `y` is pressed
- THEN exactly one WritePty addressed to pane 2 is emitted, its bytes end with the command text (or the end of the bracketed paste) and contain no `\r` or trailing `\n` outside the paste

#### Scenario: y never presses Enter in run mode [auto]
- GIVEN a chat in `run` mode with a pending confirm for the off-list command `git status`
- WHEN `y` is pressed
- THEN the write holds the command and no `\r`

#### Scenario: Decline writes nothing [auto]
- GIVEN a pending confirm
- WHEN `n` and Esc are pressed in separate runs
- THEN no WritePty is emitted and the confirm is closed each time

#### Scenario: Other keys are swallowed [auto]
- GIVEN a pending confirm
- WHEN `Y`, Enter, Space and Ctrl+y are pressed
- THEN no WritePty is emitted and the confirm is still pending

#### Scenario: Repeat does not answer [auto]
- GIVEN a pending confirm
- WHEN a Repeat event for `y` arrives
- THEN the confirm is unchanged and nothing is written

#### Scenario: One accept, one write [auto]
- GIVEN a confirm accepted with `y`
- WHEN `y` is pressed again
- THEN no further WritePty is emitted

#### Scenario: User run action never runs [auto]
- GIVEN a chat in `run` mode and the run action used on the bash block `ls`
- WHEN the confirm opens and `y` is pressed
- THEN the command is typed with no `\r`, although `ls` is on the safe list

#### Scenario: Shell stays unexecuted [manual, Kitty]
- GIVEN a bash block `echo hi` and an idle shell pane
- WHEN the confirm is accepted
- THEN `echo hi` appears on the shell's input line, is not executed, and runs only after the user presses Enter in that pane

### Requirement: No output capture
Outside `run` mode the system MUST NOT read back what the command printed, and MUST NOT send any pane output to the model. The outcome of a run action, and of a command typed with `y` or in `auto`, is a system line only (`sent to pane N; press Enter there to run it` or `cancelled`), which is never stored and never sent to the model. The only capture is that of a command run in `run` mode under pane-execution: its bounded, redacted, wrapped output goes to the model as the tool result (see pane-execution) and is never stored beyond the transcript row rules of chat-pane. The run action of the user never captures.
(Previously: no capture in any mode)

#### Scenario: Outcome line only [auto]
- GIVEN an accepted confirm
- WHEN the pane produces output
- THEN the transcript holds only the outcome system line and the next request's history holds no pane output

#### Scenario: Auto captures nothing [auto]
- GIVEN a command typed in `auto`, then run by the user pressing Enter, with output
- WHEN the next request is built
- THEN the history holds no pane output

#### Scenario: Run mode captures [auto]
- GIVEN a chat in `run` and `ls` run through pane-execution
- WHEN the run settles
- THEN the bounded wrapped output is the tool result and nothing else of the pane is read
