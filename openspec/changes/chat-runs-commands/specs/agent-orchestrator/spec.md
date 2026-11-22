# Delta for Agent Orchestrator (chat-runs-commands)

## ADDED Requirements

### Requirement: Workers do not inherit the chat's run flow
Orchestrator workers MUST behave as before this change. No worker is offered `run_in_pane`, so no worker can run a pane command, unprompted or after `y`, and the `y` semantics of the chat (see agent-tools, A `y` runs the model's command) MUST NOT apply to any worker ask: a worker's `run_command` still needs its own `y` with the worker label and queue (see command-runner), a worker's batch is still reviewed as a whole, and a worker's read outside its scope still follows the approval mode as the chat's read does. The policy hook `may_run_on_y` MUST return not-allowed for a call of worker origin (see agent-tools, Policy hook for running on y). The classifier, the 30 s cut-off, the waiting-for-input state and the model guidance texts of this change MUST NOT be given to a worker. Where other requirements of this capability list the modes `ask`, `auto` and `run`, the mode set is now `ask` and `run`; the behaviour named for each remaining mode is unchanged.

#### Scenario: Worker toolset unchanged [auto]
- GIVEN a `code` worker and a `simple` worker with Exec on, in `ask` and in `run`
- WHEN their tool lists are built
- THEN neither holds `run_in_pane`, and `run_command` is held only as before

#### Scenario: Worker command asks and y does not use the chat flow [auto]
- GIVEN a worker calling `cargo test` in `run`
- WHEN the prompt opens and `y` is pressed
- THEN the prompt shows the worker label, the call runs as `run_command` does (own process, no pane write), and no write reaches any pane

#### Scenario: Hook refuses worker origin [auto]
- GIVEN `may_run_on_y` called with worker origin in `ask` and in `run`
- WHEN it decides
- THEN it returns not-allowed

#### Scenario: Worker read outside scope follows the mode [auto]
- GIVEN a worker read outside the run's scope in `ask` and in `run`
- WHEN `read_file` is called
- THEN the approval mode answers it exactly as it answers the chat's read, as before this change
