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

### Requirement: A key opens the grant dialog [W6c]
`Ctrl+Space` then `x` MUST open the same dialog as the `/exec` command and `Ctrl+Space` then `f` the same dialog as `/write` (owner 2026-11-11). A key MUST NEVER grant: only the user's `y` in the dialog does, exactly as for the command. The key MUST share the command's path, so every refusal of the command (another approval pending, tools off, no tools offered, toggle already on, no folder, conversation list open) MUST show the same refusal line, and a key pressed in a pane that is not a chat MUST show a notice naming Exec or Write. Opening by key MUST keep the composer draft. The bindings MUST be named "Exec" and "Write" (never "run", which is an approval mode) and MUST be listed in the prefix table. Text produced by an engine, a worker or a tool result MUST NOT be able to press them.

#### Scenario: A key opens the dialog and never grants [W6c, auto]
- GIVEN a chat with tools on and Exec off
- WHEN the user presses `Ctrl+Space x`
- THEN the Exec dialog is open, Exec is not granted, and `y` grants it while `n` and Esc refuse

#### Scenario: The Write key does the same [W6c, auto]
- GIVEN a chat with tools on and Write off
- WHEN the user presses `Ctrl+Space f`
- THEN the Write dialog is open and only `y` grants Write

#### Scenario: A refusal is visible and equals the command's [W6c, auto]
- GIVEN tools off, or the toggle already on, or another approval pending
- WHEN the user presses the key
- THEN the chat shows the line the command would show and no dialog opens

#### Scenario: Outside a chat the key says so [W6c, auto]
- GIVEN the focused pane is a shell
- WHEN the user presses the key
- THEN a notice says Exec (or Write) is for chat panes

### Requirement: Limit ask keys [W9]
The line-limit ask and the outside-brief ask MUST be answered by user keys only, with one ask per batch listing every file over the limit and every file outside the brief (K2, owner 2026-11-10: "Dos teclas"). `y` keeps the batch as it is. `n` or Esc ends the worker and discards its staged batch, showing the count: when a file is over the limit this is a split, and the coordinator issues two new briefs; when the ask lists only files outside the brief it is a decline, `writes outside the brief declined: N staged files discarded`, which is neither a split nor a clarification round. No approval mode (`ask`, `auto`, `run`) and no timer MUST answer either ask. The prompt MUST show the file path, the counted lines, the limit and, when the language was not recognised, that every line was counted.

#### Scenario: Keys answer the limit ask [W9, auto]
- GIVEN a pending limit ask listing a file over the limit
- WHEN the user presses `y`, and in a second run `n`, and in a third Esc
- THEN `y` keeps the batch; `n` and Esc end the worker as split and discard its staged batch, showing the count

#### Scenario: Declining writes outside the brief [W9, auto]
- GIVEN a pending ask that lists only files outside the brief
- WHEN the user presses `n`
- THEN the batch is discarded with `writes outside the brief declined: N staged files discarded`, and no split and no clarification round is counted

#### Scenario: Run mode does not answer [W9, auto]
- GIVEN a pending limit ask in approval mode `run`
- WHEN no key is pressed
- THEN the ask stays pending
