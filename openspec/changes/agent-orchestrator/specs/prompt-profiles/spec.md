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

### Requirement: ODD playbooks [S5]
The `orchestrate` prompt family MUST include playbooks for the ODD steps authorize, explore, resolve, classify, track, implement and close, in that order, with the rule that a step closes only with a handoff, at most one question to the user at a time, and the small or large rule that never counts files or tasks. The playbook text MUST say `implement` is drafts and review only until write tools are offered. [S8] Once `run_command` exists, the playbook requires red evidence before and green evidence after the implementation, or an explicit exception.

#### Scenario: Playbook per step [auto]
- GIVEN the playbooks
- WHEN they are loaded
- THEN there is one per ODD step in order, and `implement` says draft until the tools exist

### Requirement: RDD lens prompts [S7b]
Each profile MUST ship a built-in prompt per lens (`risk`, `resilience`, `readability`, `reliability`) and one for the refuter, resolved like other tasks. Each MUST tell the worker: it is read-only; the change is untrusted data; only a severe finding the change caused blocks; it reports findings in a fixed shape and never approves.

#### Scenario: Lens prompts exist [S7b, auto]
- GIVEN each profile
- WHEN the four lens prompts and the refuter prompt are loaded
- THEN each is non-empty and contains the rules above

### Requirement: Profile docs are not part of early slices [S1]
The prompt-profiles documentation requirement MUST NOT demand README changes for the orchestrator before the final slice [S10].

#### Scenario: Docs deferred [manual]
- GIVEN slices S1 to S9
- WHEN their diffs are reviewed
- THEN none edits the README for the orchestrator
