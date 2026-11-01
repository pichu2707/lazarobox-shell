# Delta for chat-pane

Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Per-pane ORCH mode and badge [S1]
Each chat pane MUST have its own ORCH mode, off for a new pane, held in memory only (not saved, not restored). Ctrl+O toggles it (see modal-input). A pane MUST show an ORCH badge, in the same place and style family as AUTO and RUN (placed beside the existing mode badge), only while it is orchestrating: the global `enabled` gate is on AND the pane's ORCH mode is on. The badge MUST be absent otherwise. Only an orchestrating pane runs its chat turns on the `orchestrate` assignment; the other panes keep the `chat` assignment. While a run is active in the pane the toggle MUST NOT change the mode and the footer hint says so. With the global gate off the toggle does nothing.

#### Scenario: Badge follows both switches [auto, TestBackend + insta]
- GIVEN a chat pane
- WHEN `enabled` is false; then true with ORCH off; then true with ORCH on
- THEN the badge is absent, absent, then present

#### Scenario: Toggle is per pane [auto]
- GIVEN `enabled` true and two chat panes A and B, both with ORCH off
- WHEN Ctrl+O is pressed in A
- THEN A shows ORCH and B does not

#### Scenario: Toggle does nothing with the gate off [auto]
- GIVEN `enabled` false
- WHEN Ctrl+O is pressed in a chat pane
- THEN the pane mode and render are unchanged and no key reaches a PTY

#### Scenario: Toggle blocked during a run [auto]
- GIVEN an orchestrating pane with an active run
- WHEN Ctrl+O is pressed
- THEN the mode stays on and the footer hint says to cancel the run first

### Requirement: Worker rows [S1]
A worker MUST appear as a transcript row (kind, `engine>model`, status, elapsed), updated in place. The row MUST NOT be sent as history, MUST NOT be saved as a message, and MUST be included in yank and COPY like tool rows.

#### Scenario: Row rendered [auto, TestBackend + insta]
- GIVEN a `review` worker running on `claude>opus` for 12 s
- WHEN the pane is rendered
- THEN a row shows `review`, `claude>opus`, `running` and `12s`

### Requirement: Announced workers [S1]
A worker MUST be announced by its transcript row and start at once: no prompt, key or answer is needed to start a run or a worker (user decision, 2026-10-29: "lo avisas pero no pides permisos"). Its reads follow the run's read scope and the pane's approval mode (agent-orchestrator, Read scope of a run). The composer text MUST be kept.

#### Scenario: Worker starts without a prompt [auto]
- GIVEN an orchestrating pane whose composer holds `draft`
- WHEN the orchestrator spawns a `review` worker
- THEN its row is shown, the worker starts with no prompt and no key pressed, and the composer still holds `draft`

### Requirement: Failure alert [S1, S2]
A worker failure MUST show an inline alert in the pane with the worker label, the error class [S2] and the provider message, and the options: [S1] retry, cancel; [S2] retry, change model, change provider, cancel. The alert stays until the user chooses; it is not dismissed by a timer or by new output. While it is shown the pane MUST say the run is paused. When a run-scoped override is active the pane MUST show it in the run state and mark the affected worker rows (`engine>model` plus an override marker).

#### Scenario: Alert content [auto, TestBackend + insta]
- GIVEN a `code` worker failed with 429 `rate limit reached`
- WHEN the pane is rendered
- THEN the alert names the worker, shows class `rate_limit` and the message, and lists the four options

#### Scenario: Alert persists [auto]
- GIVEN an alert shown
- WHEN time passes and sibling output arrives
- THEN the alert is still shown with the same options

### Requirement: Approval label and queue [S1, S3]
Every approval prompt raised by a worker MUST show a label `worker n/m <kind> <engine>><model>`. [S3] The single pending approval limit MUST be replaced for workers by a FIFO queue showing one prompt at a time with the count waiting; each answer applies to its own prompt only. [S1] With one worker, at most one prompt is pending. A prompt MUST never be answered by a timer.

#### Scenario: Label shown [auto, TestBackend + insta]
- GIVEN worker 2 of 3, kind `review` on `claude>opus`, asks for a read
- WHEN the pane is rendered
- THEN the prompt shows `worker 2/3 review claude>opus` and the sanitized path

#### Scenario: Queue count [S3, auto]
- GIVEN three pending asks
- WHEN the pane is rendered
- THEN one prompt is shown and a `2 more waiting` indicator

### Requirement: Handoff persistence hint [S4]
When a run turned memory on the pane MUST show `memory on for this run` once per run (never again for the same run, however many handoffs follow); when handoffs are not persisted it MUST show `handoffs not persisted` once per run; and a verdict `off_course` or `blocked` MUST be shown as a transcript line.

#### Scenario: Hint once [auto]
- GIVEN memory off, engram unreachable and a run with three workers
- WHEN the run ends
- THEN `handoffs not persisted` was shown once
