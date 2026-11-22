# Delta for Chat Pane (chat-runs-commands)

## MODIFIED Requirements

### Requirement: Approval mode badge and system lines
A chat pane with tools on in `run` mode MUST show the badge ` RUN ` in red, high contrast, in every chat view, focused or not; `ask` shows none; there is no ` AUTO ` badge. When the pane is narrower than the badge (below 5 columns for ` RUN `) the badge MUST shrink to `R`; the badge text is bold on the base background colour. The badge is system-drawn: never stored, yanked or sent to the model. Changing the mode (see modal-input) MUST add a system line naming the new mode and saying what it does. The `ask` line reads `approval: ask (every command asks for y; y runs it and the chat reads the output)`. The `run` line reads `approval: run (read-only commands run at once; every other command asks for y; y runs it and the chat reads the output)`. System lines are never stored or sent to the model. After a `cd` ran under pane-execution the transcript MUST show the system line `pane N: <exact command> → <cwd>`.
(Previously: three modes with an ` AUTO ` badge in amber; the `ask` line said it asks for each command; the `run` line named `cd, pwd, ls, tree, mkdir`.)

#### Scenario: Badges [auto, TestBackend + insta]
- GIVEN chats in `run` and `ask` with tools on
- WHEN rendered
- THEN ` RUN ` is red and `ask` has no badge

#### Scenario: Narrow pane [auto, TestBackend + insta]
- GIVEN a chat in `run` rendered at 4 columns, and another at 5 columns
- WHEN rendered
- THEN the badges read `R` and ` RUN `, and nothing panics

#### Scenario: System line on change to run [auto]
- GIVEN a chat in `ask`
- WHEN the mode changes to `run`
- THEN the system line reads `approval: run (read-only commands run at once; every other command asks for y; y runs it and the chat reads the output)`; it is not in the store or the next request

#### Scenario: System line on change to ask [auto]
- GIVEN a chat in `run`
- WHEN the mode changes to `ask`
- THEN the system line reads `approval: ask (every command asks for y; y runs it and the chat reads the output)`

#### Scenario: Cd line shows the exact command [auto]
- GIVEN `cd ~/dev/curso-rust` run and settled
- WHEN rendered
- THEN a system line reads `pane N: cd ~/dev/curso-rust → <cwd>`

#### Scenario: Tools off shows nothing [auto, TestBackend + insta]
- GIVEN tools off and `approval = "run"`
- WHEN rendered
- THEN no badge appears and the snapshot equals the one without this change

## ADDED Requirements

### Requirement: Run prompt keys, notices and resume
The prompt of a model's `run_in_pane` call MUST show the keys `RUN_KEYS` exactly as `y run it · n cancel · p next pane`. When `y` can only type the command (see agent-tools, A `y` runs the model's command) the prompt MUST show `TYPE_KEYS` exactly as `y type it (no Enter) · n cancel · p next pane` and the reason on its own line before `y` is pressed; a prompt the user opened with the run action (`Prefix e`) MUST show `TYPE_KEYS`. The former constant `RUN_NOW_KEYS` is merged into `RUN_KEYS`. The arming delay of 400 ms for prompts the model opened is unchanged. Every refusal and every fallback MUST have a visible line, from this list and no other text for the same case:
- `sent to pane N; press Enter there to run it (not run: <reason>)` for any typed-only result, with `<reason>` one of: `auto-run is off: this conversation read GitHub text`, `the pane has a half-typed line`, `pane N is running a program`, `the command has more than one line`, `the command has non-ASCII text and the locale is not UTF-8`, `commands are not run in this chat (policy)`, `the pane refused the write`, `the pane's directory cannot be read on this system` (macOS: `y` only types there);
- the prompt reason `5 unprompted runs already made in this turn`;
- the prompt reason `the pane's directory cannot be read on this system`;
- the system line `pane N: waiting for your input in the pane; type it there (it is never sent to the chat)`;
- the system line `pane N: <exact command> is still running after 30 s; the chat goes on with the output so far`;
- the system line `pane N: no input after 5 min; the chat goes on (the command still waits in the pane)`;
- the continuation note `lazarobox: the command you approved in pane N finished.` when a cut command ends and the chat resumes again with its final output.
A run MUST resume the chat by itself: when the run ends, settles or is cut, the tool result continues the same turn and the model's next request is built with no user message; no `continue` message, `output is ready` message or key is asked of the user. The `RUN_HINT` text is the UI hint the user sees (`src/core/ai/chat/transcript.rs`) and MUST read exactly `Ctrl+Space e  type in pane` (see agent-tools, Model guidance); no UI hint or hint line of the chat pane may tell the user to run a command the model proposed by hand or to tell the chat when its output is ready.

#### Scenario: Run keys [auto, TestBackend + insta]
- GIVEN a model prompt for `brew install jq` in `ask`
- WHEN rendered
- THEN the keys line reads `y run it · n cancel · p next pane`

#### Scenario: Type keys with reason [auto, TestBackend + insta]
- GIVEN a model prompt in a GitHub-blocked conversation, and in another run one for a pane with a half-typed line
- WHEN rendered
- THEN each shows `y type it (no Enter) · n cancel · p next pane` and its reason line before `y`

#### Scenario: Prefix e shows type keys [auto, TestBackend + insta]
- GIVEN the run action on a bash block
- WHEN rendered
- THEN the keys line reads `y type it (no Enter) · n cancel · p next pane`

#### Scenario: Chat resumes with no user message [auto]
- GIVEN an approved run that settles
- WHEN it ends
- THEN the next request holds the tool result and no new user message, and the transcript has no row asking the user to report the output

#### Scenario: Every refusal has a line [auto]
- GIVEN each typed-only reason, the cap, the unknown cwd, the waiting state and the 30 s cut
- WHEN each case happens
- THEN exactly the line listed above is shown for it, and none is silent

#### Scenario: Waiting state is visible [auto]
- GIVEN a `sudo` run that reaches a password prompt
- WHEN the wait starts
- THEN the transcript shows `pane N: waiting for your input in the pane; type it there (it is never sent to the chat)`
