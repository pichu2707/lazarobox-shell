# Delta for Chat Pane

Slice tags as in the agent-orchestrator delta of this change.

## ADDED Requirements

### Requirement: Worker-flow notices are transcript notices [W1, W2, W7, W8]
Every notice this change introduces (undelegated reply, silent worker outcomes, spawn refusals, the Write-off refusal, gate refusals and overrides, the clarification limit, the same-model warning, the local-reviewer limits, the cloud-diff notice, the queue-for-one-writer notice) MUST be a system line in the pane's transcript. Each MUST be ASCII, short, neutral in tone, never sent to a model as history, never saved as a chat message, and included in yank and COPY like other system lines. A notice MUST be shown each time its condition occurs. The Write-off refusal notice MUST name the `Write` toggle and the step to turn it on from the chat.

#### Scenario: Notice is a system line, not history [W1, auto]
- GIVEN a notice raised during a run
- WHEN the next orchestrator turn is built
- THEN the history does not hold it, and the transcript still shows it

#### Scenario: Notice repeats [W1, auto]
- GIVEN the undelegated-reply condition happens in two turns
- WHEN each turn ends
- THEN the notice is shown after each

### Requirement: Worker asks are labelled and queued [W5, W6, W9]
A worker's command ask, line-limit ask and outside-brief ask MUST show the label `worker n/m <kind> <engine>><model>` and join the worker approval queue (one prompt visible at a time, FIFO, the count waiting shown), like a worker's read ask. Each answer applies to its own prompt, reaches the session of the worker that asked, and reaches no other worker. Cancelling a worker denies its pending and queued asks. A timer never answers any of them.

#### Scenario: Command ask reaches its own worker [W5, auto]
- GIVEN workers 1 and 2 each calling `run_command`
- WHEN the user answers the first prompt `y` and the second `n`
- THEN worker 1's command runs, worker 2's ends `denied`, and neither answer reaches the other worker

#### Scenario: Ask shows the label [W5, auto]
- GIVEN worker 2 of 3, kind `simple` on `ollama>llama3:8b`, asking to run `cargo test`
- WHEN the prompt is drawn
- THEN it shows `worker 2/3 simple ollama>llama3:8b` with the program, args and cwd

#### Scenario: Cancel denies the ask [W9, auto]
- GIVEN a worker with a pending line-limit ask
- WHEN the worker is cancelled
- THEN the ask ends as denied and no extra lines are written

### Requirement: A limit and a clarification stop are visible in the run row [W8, W9]
A worker row whose worker ended "needs specification", or ended with a split chosen, MUST show that state in its status text, and the run state MUST show the clarification round of the brief (`round n of 2`) while a brief is being clarified.

#### Scenario: Row shows the round [W8, auto, TestBackend + insta]
- GIVEN a `simple` worker that ended "needs specification" in round 2
- WHEN the row and run state are rendered
- THEN the row says it needs specification and the run state shows `round 2 of 2`
