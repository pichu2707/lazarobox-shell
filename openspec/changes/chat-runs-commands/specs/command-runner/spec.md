# Delta for Command Runner (chat-runs-commands)

Note: `command-runner` is the `run_command` tool (fixed program allow-list, own process, own `y`). It is NOT the `run_in_pane` path; the pane run is specified in agent-tools and pane-execution. This change leaves `run_command` behaviour as is and only removes the mode that no longer exists.

## MODIFIED Requirements

### Requirement: Approval on every command [W5]
Every `run_command` call MUST need an explicit user `y`, every time, in every approval mode, and no read-only classification, no unprompted run and no `y` of a `run_in_pane` command (see agent-tools) MUST ever cover it. The prompt MUST show the program, the arguments, the working directory and a visible notice that network is on and that a test can exfiltrate the project's `.env`. [W5] When the call comes from a worker, the prompt MUST also show the label `worker n/m <kind> <engine>><model>`, MUST join the worker approval queue, and its answer MUST reach the session of that worker only. A declined, timed-out or cancelled prompt ends as `denied` and nothing starts. The call MUST NOT be covered by any auto-approval. Time spent on the prompt does not count against the worker's deadline.
(Previously: scenario `Every mode asks` named the `ask` and `auto` modes; `auto` is removed.)

#### Scenario: Prompt content [auto]
- GIVEN a valid `cargo test` call
- WHEN the prompt is built
- THEN it shows `cargo test`, the cwd and the network notice

#### Scenario: Every mode asks [auto]
- GIVEN chats in `ask` and `run` mode
- WHEN `run_command` is called in each
- THEN each opens a prompt and nothing starts before `y`

#### Scenario: Read-only classification does not apply [auto]
- GIVEN a chat in `run` and a `run_command` call whose program and arguments would classify read-only as a pane command
- WHEN it is dispatched
- THEN a prompt opens and nothing starts before `y`

#### Scenario: Declined [auto]
- GIVEN a pending prompt
- WHEN `n`, a timeout or a cancel ends it
- THEN the result is `denied` and no process was started

#### Scenario: Worker command carries the label [W5, auto]
- GIVEN a worker 1 of 2 `simple` on `ollama>llama3:8b` calling `cargo test`
- WHEN the prompt is built
- THEN it shows the label, `cargo test`, the cwd and the network notice, in `run` mode too

#### Scenario: Worker answer stays with the worker [W5, auto]
- GIVEN two workers with pending command asks
- WHEN the user answers one
- THEN the other worker's ask is unchanged and its worker is not resumed
