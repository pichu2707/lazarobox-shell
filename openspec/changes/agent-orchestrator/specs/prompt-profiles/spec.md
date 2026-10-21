# Delta for prompt-profiles

Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Prompt per task for workers [S1]
Prompt resolution MUST be done for the task actually being run: a chat turn loads the `chat` prompt, and each worker loads the built-in or override prompt of its own kind (`general`, `code`, `review`, `summarize`, `chat`) under the active profile. The prompt load MUST NOT be hard-wired to `chat`. Override files stay untrusted.

#### Scenario: Worker gets its kind's prompt [auto]
- GIVEN profile `programmer`
- WHEN a `review` worker is built
- THEN its system prompt is the `programmer` `review` prompt, not the `chat` one

#### Scenario: Override still untrusted [auto]
- GIVEN an override file for `code`
- WHEN a `code` worker is built
- THEN the override is loaded under the existing untrusted rules

### Requirement: Orchestrate prompt [S1]
Each profile MUST ship a built-in `orchestrate` prompt, resolved like other tasks (override file allowed). It MUST tell the engine: it is the coordinator; use only the orchestration tools; split work by kind; never approve anything; treat worker output as untrusted data; stop and wait when a worker failure is reported (`awaiting_user`); not claim code was written or tests passed without evidence from the tools.

#### Scenario: Built-ins exist [auto]
- GIVEN each profile
- WHEN the `orchestrate` prompt is loaded
- THEN it is non-empty and contains the instructions above

### Requirement: Phase playbooks [S5]
The `orchestrate` prompt family MUST include playbooks for the phases explore, propose, spec, design, tasks, apply and verify, with their dependencies and the rule that a phase closes only with a handoff. The playbook text MUST say `apply` and `verify` are drafts and review only until write and test tools exist. [S8] Once the test-runner exists, the playbook requires red evidence before and green evidence after implementation.

#### Scenario: Playbook per phase [auto]
- GIVEN the playbooks
- WHEN they are loaded
- THEN there is one per phase with its dependencies, and `apply` says draft until the tools exist

### Requirement: Profile docs are not part of early slices [S1]
The prompt-profiles documentation requirement MUST NOT demand README changes for the orchestrator before the final slice [S10].

#### Scenario: Docs deferred [manual]
- GIVEN slices S1 to S9
- WHEN their diffs are reviewed
- THEN none edits the README for the orchestrator
