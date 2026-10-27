# Delta for modal-input

Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Notice and scope keys [S1]
In a chat pane the provider notice MUST accept with Enter or `y` and dismiss with Esc; the run-start scope prompt MUST approve with `y` and deny with `n` or Esc. These keys apply only while the view is shown and MUST NOT reach any PTY.

#### Scenario: Notice keys [auto]
- GIVEN the notice shown
- WHEN Enter is pressed (or Esc)
- THEN it is accepted (or dismissed without accepting)

#### Scenario: Scope keys [auto]
- GIVEN the scope prompt shown
- WHEN `y` (or `n`) is pressed
- THEN the scope is approved (or denied, leaving an empty scope)

### Requirement: ORCH toggle key [S1]
In a chat pane, Ctrl+O MUST toggle the pane's ORCH mode when the global orchestrator gate is on, as a one-shot key (a held key does not toggle again, like the other debounced chat keys). Ctrl+O is not bound in the chat key table today (bound: Enter, Esc, Ctrl+C, Ctrl+Y, Ctrl+B, Ctrl+R, Ctrl+J, Alt+Enter, Ctrl+U, PageUp, PageDown) and MUST NOT shadow any of them. It MUST be handled before the key can reach a PTY and be shown in the chat footer hint while the gate is on. With the gate off it does nothing.

#### Scenario: Ctrl+O toggles [auto]
- GIVEN the gate on and a chat pane with ORCH off
- WHEN Ctrl+O is pressed, then pressed again
- THEN the mode is on, then off

#### Scenario: Held key toggles once [auto]
- GIVEN the gate on and ORCH off
- WHEN Ctrl+O arrives as a Repeat
- THEN the mode does not change

#### Scenario: Existing keys unchanged [auto]
- GIVEN the chat key table
- WHEN Ctrl+C, Ctrl+Y, Ctrl+B, Ctrl+R, Ctrl+J and Ctrl+U are pressed
- THEN each behaves as before this change

### Requirement: Failure alert keys [S1, S2]
While a failure alert is shown: `r` MUST retry, `c` or Esc MUST cancel the run; [S2] `m` MUST open the picker at model level (change model) and `p` at engine level (change provider). Other keys MUST NOT answer the alert. An approval prompt, if any, keeps its own keys and is answered by user key events only.

#### Scenario: Retry key [auto]
- GIVEN a failure alert
- WHEN `r` is pressed
- THEN the worker is relaunched from scratch, with the previous error appended to its prompt

#### Scenario: Cancel keys [auto]
- GIVEN a failure alert
- WHEN `c` is pressed (or Esc)
- THEN the run is cancelled

#### Scenario: Change keys [S2, auto]
- GIVEN a failure alert
- WHEN `m` is pressed, then Esc, then `p`
- THEN the model picker opens, the alert returns, then the engine picker opens

### Requirement: Cancel keys for workers [S1, S3]
Ctrl+C MUST cancel the orchestrator turn and all workers of the run (existing chat cancel). [S3] A chat key MUST cancel the selected worker, and another the whole run; each is shown in the footer hint while a run is active.

#### Scenario: Ctrl+C [auto]
- GIVEN a run with two workers
- WHEN Ctrl+C is pressed
- THEN the turn and both workers are cancelled

#### Scenario: Cancel the selected worker [S3, auto]
- GIVEN a run with two workers and worker 1 selected
- WHEN the cancel-worker key is pressed
- THEN worker 1 is cancelled and worker 2 keeps running
