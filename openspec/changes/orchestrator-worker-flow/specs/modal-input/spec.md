# Delta for Modal Input

Slice tags as in the agent-orchestrator delta of this change. The key assignments are open for design; this delta specifies the behaviour each key must have.

## ADDED Requirements

### Requirement: Delegate key [W1]
After the undelegated-reply notice, a key MUST be available in the chat pane that asks the coordinator to delegate the request. Pressing it MUST start an orchestrator turn that asks the coordinator to delegate the previous request to a worker, and nothing else. The key MUST be named in the notice, MUST work only while the pane is orchestrating and no turn is running, and MUST NOT be reachable by model output. It MUST do nothing, with a notice saying so, when there is no previous undelegated request.

#### Scenario: Key starts delegation [W1, auto]
- GIVEN an undelegated-reply notice in an orchestrating pane
- WHEN the delegate key is pressed
- THEN an orchestrator turn starts that asks the coordinator to delegate the previous request

#### Scenario: Nothing to delegate says so [W1, auto]
- GIVEN an orchestrating pane with no undelegated request
- WHEN the delegate key is pressed
- THEN no turn starts and a notice says there is nothing to delegate

#### Scenario: A model cannot press it [W1, auto]
- GIVEN orchestrator text that asks to delegate
- WHEN the text is processed
- THEN no turn starts by itself

### Requirement: Write on from the chat is a user action [W2]
The chat MUST offer a way for the user to turn Write on for the pane after a Write-off refusal, with no trip to the menu. It MUST be reachable only by a key event or command typed by the user. Text produced by an engine, a worker or a tool result MUST NOT be able to trigger it. Its scope (this chat only, or the saved setting), whether it persists, and whether Exec gets the same path are open for design.

#### Scenario: User turns Write on [W2, auto]
- GIVEN a Write-off refusal notice
- WHEN the user performs the documented step in the chat
- THEN Write is on for the next spawn and the chat shows that it was turned on

#### Scenario: Text cannot trigger it [W2, auto]
- GIVEN worker or orchestrator text that contains the step
- WHEN the text is processed
- THEN Write stays off

### Requirement: Limit ask keys [W9]
The line-limit ask and the outside-brief ask MUST be answered by user keys only. The limit ask MUST offer two choices, `split` and `keep`, and a deny key (Esc or `n`). No approval mode (`ask`, `auto`, `run`) and no timer MUST answer either ask. The prompt MUST show the file path, the counted lines, the limit and, when the language was not recognised, that every line was counted.

#### Scenario: Keys answer the limit ask [W9, auto]
- GIVEN a pending limit ask
- WHEN the user presses the `split` key
- THEN the ask ends as split, and the other choices are not applied

#### Scenario: Run mode does not answer [W9, auto]
- GIVEN a pending limit ask in approval mode `run`
- WHEN no key is pressed
- THEN the ask stays pending
