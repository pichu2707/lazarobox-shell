# Delta for Command Runner

Slice tags: [W5] labelled worker command ask; [W6] `run_command` for workers.

## MODIFIED Requirements

### Requirement: Approval on every command [W5]
Every `run_command` call MUST need an explicit user `y`, every time, in every approval mode. The prompt MUST show the program, the arguments, the working directory and a visible notice that network is on and that a test can exfiltrate the project's `.env`. [W5] When the call comes from a worker, the prompt MUST also show the label `worker n/m <kind> <engine>><model>`, MUST join the worker approval queue, and its answer MUST reach the session of that worker only. A declined, timed-out or cancelled prompt ends as `denied` and nothing starts. The call MUST NOT be covered by any auto-approval. Time spent on the prompt does not count against the worker's deadline.
(Previously: no worker could call `run_command`, so the prompt carried no worker label and no queue)

#### Scenario: Prompt content [auto]
- GIVEN a valid `cargo test` call
- WHEN the prompt is built
- THEN it shows `cargo test`, the cwd and the network notice

#### Scenario: Every mode asks [auto]
- GIVEN chats in `ask` and `auto` mode
- WHEN `run_command` is called in each
- THEN each opens a prompt and nothing starts before `y`

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

## ADDED Requirements

### Requirement: Worker commands run after the worker's batch is applied [W6]
A command called by a worker MUST be refused with the existing `propose them first` message while that worker's own batch is staged or in review. A command MUST NOT run against staged content (no overlay). Until W10 ships, the existing per-pane refusals (a run already active in the chat, two runs across the app, the busy gate `CHAT_BUSY`) apply to workers as they do to a chat, and each refusal MUST be shown to the user as well as returned to the worker. Whether a worker's refusal is scoped to its own batch once workers have their own batches, and whether more than one command per pane may run, is open for design.

#### Scenario: Refused while the batch is staged [W6, auto]
- GIVEN a worker whose batch is staged or in review
- WHEN it calls `run_command`
- THEN the call returns `propose them first`, no prompt opens, and a notice shows the refusal

#### Scenario: Runs after the batch is applied [W6, auto]
- GIVEN a worker whose batch was approved and applied, and the user approving the command
- WHEN it calls `cargo test`
- THEN the command runs against the disk

#### Scenario: Busy gate is disclosed [W6, auto]
- GIVEN a run already active in the pane
- WHEN a worker calls `run_command`
- THEN it is refused before any prompt and the user sees a notice with the cause
