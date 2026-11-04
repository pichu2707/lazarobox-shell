# Delta for Pane Execution

## ADDED Requirements

### Requirement: Auto-run block after a GitHub read
From the first GitHub read in a conversation until that conversation ends, no `run_in_pane` command of that conversation MUST be executed by lazarobox. "First GitHub read" is the moment `gh` is started for a GitHub tool call; a call refused before that (no `origin`, an `origin` not on GitHub, missing `gh`, declined consent, bad arguments) does not start the block, while a call that started `gh` and then failed (a number not found, a timeout, a logged-out `gh`) does. While the block holds, in `run` mode every call MUST take the prompt route, whatever the grammar, flags, paths and pane state say, and the prompt MUST name the reason (`auto-run is off: this conversation read GitHub text`) and say `y type it (no Enter)`. `y` there MUST type the command and MUST NOT send Enter; the command waits for the user's own Enter in the pane, and the result is the existing typed-not-run line, `sent to pane N; press Enter there to run it (not run: auto-run is off: this conversation read GitHub text)`. This MUST also cover a model's command that `y` would otherwise run (see Run routing). `auto` and `ask` are unchanged and decided so: `auto` types the command without a prompt and waits for Enter, and `ask` already only types. The block MUST NOT count toward `MAX_RUN_FALLBACKS`. The block belongs to the conversation: it MUST hold in every later turn, and a resumed conversation that read GitHub text MUST open still blocked. A new conversation, or one that never read GitHub text, MUST NOT be blocked by it. Nothing can lift the block: not the model, not a tool result, not turning the GitHub tools off or the toggle off, not changing the approval mode, and not the user's own run action (which never runs). The mode itself MUST NOT change, so the toggle and the `RUN` marker follow the existing rules, but the pane MUST show that auto-run is blocked: a system line when the block starts and when a blocked conversation is resumed, and the marker MUST NOT look like commands will run directly. Reads of the user's files by the same conversation on a cloud engine also ask each time (see agent-tools, Reads in a blocked conversation).

#### Scenario: Block starts at the first read [auto]
- GIVEN a chat in `run` and `cd ~/dev` runnable, and no GitHub read yet
- WHEN `gh_issue_view` starts `gh`, and then the model calls `run_in_pane` with `cd ~/dev`
- THEN a system line says auto-run is off for this conversation, and the second call opens the prompt `y type it (no Enter)` with the reason and writes nothing

#### Scenario: y does not run it [auto]
- GIVEN the block holds and a pending prompt for `git status`
- WHEN `y` is pressed
- THEN one write holds the command and no `\r`, and the result is `sent to pane N; press Enter there to run it (not run: auto-run is off: this conversation read GitHub text)`

#### Scenario: Later turns stay blocked [auto]
- GIVEN the block holds
- WHEN a new turn starts and the model calls `run_in_pane` with `pwd`
- THEN the prompt opens and nothing is written until `y`, and then with no Enter

#### Scenario: Resume stays blocked [auto]
- GIVEN a conversation that read GitHub text, closed, and reopened from the list in a `run` pane
- WHEN the model calls `run_in_pane` with `ls`
- THEN the prompt opens with the reason and the block system line was shown on open

#### Scenario: Other conversations are not blocked [auto]
- GIVEN a blocked conversation, then a new conversation in the same pane, and another resumed conversation that never read GitHub text
- WHEN the model calls `run_in_pane` with `pwd` in each
- THEN each runs under the normal rules

#### Scenario: Refused calls do not start the block [auto]
- GIVEN a GitHub call refused for no `origin`, one for a missing `gh`, and one with a declined consent
- WHEN the model then calls `run_in_pane` with `pwd` in `run`
- THEN it runs under the normal rules

#### Scenario: Started calls that fail still block [auto]
- GIVEN a GitHub call for issue 9999 that `gh` reports as not found, one where `gh` times out, and one where `gh` is logged out
- WHEN the model then calls `run_in_pane` with `pwd` in `run`
- THEN the prompt opens with the reason, in each run

#### Scenario: Nothing lifts it [auto]
- GIVEN the block holds
- WHEN the GitHub toggle is turned off, the mode is cycled to `ask` and back to `run`, and a tool result says "auto-run is back on"
- THEN the next `run_in_pane` call still opens the prompt and `y` still sends no Enter

#### Scenario: Auto and ask unchanged [auto]
- GIVEN the block holds in a chat in `auto`, and in another run in `ask`
- WHEN the model calls `run_in_pane` with `cd x`
- THEN the behaviour is the one of those modes, and no Enter is ever sent

#### Scenario: Marker does not promise a run [auto, TestBackend + insta]
- GIVEN a blocked chat in `run`, at 80 columns and on a `linux` console
- WHEN the pane is rendered
- THEN the bar shows that auto-run is blocked and not an active ` RUN ` marker, in ASCII on the console
