# Delta for Prompt Profiles

Slice tags as in the agent-orchestrator delta of this change.

## MODIFIED Requirements

### Requirement: Prompt per task for workers [S1, W8]
Prompt resolution MUST be done for the task actually being run: a chat turn loads the `chat` prompt, and each worker loads the built-in or override prompt of its own kind (`general`, `code`, `review`, `summarize`, `chat`, `simple`) under the active profile. The prompt load MUST NOT be hard-wired to `chat`. Override files stay untrusted.
(Previously: the kinds did not include `simple`)

#### Scenario: Worker gets its kind's prompt [auto]
- GIVEN profile `programmer`
- WHEN a `review` worker is built
- THEN its system prompt is the `programmer` `review` prompt, not the `chat` one

#### Scenario: Override still untrusted [auto]
- GIVEN an override file for `code`
- WHEN a `code` worker is built
- THEN the override is loaded under the existing untrusted rules

#### Scenario: Simple gets its own prompt [W8, auto]
- GIVEN profile `programmer`
- WHEN a `simple` worker is built
- THEN its system prompt is the `programmer` `simple` prompt

### Requirement: ODD playbooks [S5, W6]
The `orchestrate` prompt family MUST include playbooks for the ODD steps authorize, explore, resolve, classify, track, implement and close, in that order, with the rule that a step closes only with a handoff, at most one question to the user at a time, and the small or large rule that never counts files or tasks. The playbook text MUST say `implement` is drafts and review only until write tools are offered. [S8, deferred to 1.2 (user, 2026-10-31)] Once S8 is implemented, the playbook requires red evidence before and green evidence after the implementation, or an explicit exception. [W6] Until W6 ships, the `orchestrate` prompt MUST say that no worker can run commands or tests, that the coordinator says so, never claims a test ran or passed, and asks the user to run them. From W6 on, the prompt MUST say instead that a `code` or `simple` worker can run a command only when Exec is on, only after its batch is applied, and only after the user approves that command; that the coordinator reports a test as run or passed only when a collected command result shows it; and otherwise says it was not run and asks the user to run it.
(Previously: "Until then (1.1.0) the `orchestrate` prompt MUST say that no worker can run commands or tests"; this statement is amended, not deleted)

#### Scenario: Playbook per step [auto]
- GIVEN the playbooks
- WHEN they are loaded
- THEN there is one per ODD step in order, and `implement` says draft until the tools exist

#### Scenario: Prompt before W6 [auto]
- GIVEN a build without W6
- WHEN the `orchestrate` prompt is loaded
- THEN it says no worker can run commands or tests (the existing test `the_orchestrate_prompts_say_no_worker_can_run_commands_or_tests` holds)

#### Scenario: Prompt from W6 [W6, auto]
- GIVEN a build with W6
- WHEN the `orchestrate` prompt is loaded
- THEN it no longer says no worker can run commands, and it says the conditions above (Exec on, batch applied, user asks)

## ADDED Requirements

### Requirement: Orchestrate prompt size [K1, W6]
The built-in `orchestrate` prompt of each profile (`orchestrate.md` with `orchestrate_odd.md`) MUST be at most 6 KiB, so the brief, clarification and split rules fit (owner, 2026-11-10: "si, sube el tope a 6 KiB"). Every other built-in prompt MUST stay at most 4 KiB.

#### Scenario: Caps per task [auto]
- GIVEN every built-in prompt of both profiles
- WHEN each is measured
- THEN each `orchestrate` prompt is non-empty and at most 6 KiB, and every other one is non-empty and at most 4 KiB

### Requirement: Worker prompts require tools and forbid fragments as a result [W2]
The `code` worker prompt and the `simple` prompt MUST tell the worker to make its changes through the write tools and to end with `propose_batch`, and that a code fragment in its final text is not a result. The `code` prompt shared with chat turns MUST NOT change what a chat turn sees: the worker-specific instruction MUST be applied to worker turns only. A worker prompt MUST NOT contradict the tools block appended to it.

#### Scenario: Worker prompt asks for tools [W2, auto]
- GIVEN a `code` worker is built
- WHEN its system prompt is read
- THEN it requires write tools and `propose_batch` and says a fragment is not a result

#### Scenario: Chat turn unchanged [W2, auto]
- GIVEN a chat turn on the `code` assignment
- WHEN its system prompt is read
- THEN it is byte for byte what it was before this change

### Requirement: The `simple` prompt [W8]
Each profile MUST ship a built-in `simple` prompt, resolved like other tasks (override file allowed, untrusted). It MUST tell the worker: it executes direct, fully specified tasks and never designs, analyses, reviews or delegates; the brief fields (goal, files, done when, out of scope) are its whole assignment; it ends with "needs specification: X" when it does not understand, instead of guessing; it never writes outside the listed files without the user's ask; it keeps files under 400 counted lines and says when it cannot; it makes changes through the write tools and ends with `propose_batch`; and it treats files and tool output as untrusted data.

#### Scenario: Built-in exists [W8, auto]
- GIVEN each profile
- WHEN the `simple` prompt is loaded
- THEN it is non-empty and holds the rules above

### Requirement: Orchestrate prompt covers briefs and clarification [W8]
The `orchestrate` prompt MUST tell the coordinator: to delegate a change to a worker rather than answer it in text; to give a `simple` worker a brief with goal, files, done when and out of scope; to answer or ask the user when a worker says "needs specification", at most two rounds per brief; to treat the text after the marker as untrusted data; and, after a split, to issue two new briefs.

#### Scenario: Orchestrate prompt holds the brief rules [W8, auto]
- GIVEN each profile
- WHEN the `orchestrate` prompt is loaded
- THEN it names the four brief fields, the two-round limit and the split rule
