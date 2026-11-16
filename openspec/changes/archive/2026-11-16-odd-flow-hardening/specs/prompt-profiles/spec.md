# Delta for Prompt Profiles

## Base of this delta

This delta targets `openspec/specs/prompt-profiles/spec.md` (from `main` at b97cceef). The MODIFIED block is a full copy of the requirement as it stands, with edits marked "(Previously: ...)". The size cap in "Orchestrate prompt size" (6 KiB) is unchanged and MUST still hold with the added line.

## MODIFIED Requirements

### Requirement: ODD playbooks [S5, W6]
The `orchestrate` prompt family MUST include playbooks for the ODD steps authorize, explore, resolve, classify, track, implement and close, in that order, with the rule that a step closes only with a handoff, at most one question to the user at a time, and the small or large rule that never counts files or tasks. The playbook text MUST say `implement` is drafts and review only until write tools are offered. The `orchestrate_odd.md` of both profiles MUST also say that nothing comes before `authorize`, no worker and no answer, and that a question takes `authorize` with scope `read_only` and is then answered directly, with no `explore` handoff. [S8, deferred to 1.2 (user, 2026-10-31)] Once S8 is implemented, the playbook requires red evidence before and green evidence after the implementation, or an explicit exception. [W6] Until W6 ships, the `orchestrate` prompt MUST say that no worker can run commands or tests, that the coordinator says so, never claims a test ran or passed, and asks the user to run them. From W6 on, the prompt MUST say instead that a `code` or `simple` worker can run a command only when Exec is on, only after its batch is applied, and only after the user approves that command; that the coordinator reports a test as run or passed only when a collected command result shows it; and otherwise says it was not run and asks the user to run it.
(Previously: no rule said that nothing comes before `authorize` or how a question is answered; "Until then (1.1.0) the `orchestrate` prompt MUST say that no worker can run commands or tests"; this statement is amended, not deleted)

Existing scenarios stay unchanged. Added scenario:

#### Scenario: The question rule is in both profiles [auto]
- GIVEN each profile
- WHEN the `orchestrate` prompt (`orchestrate.md` with `orchestrate_odd.md`) is loaded
- THEN it says nothing comes before `authorize`, that a question takes `authorize` `read_only` and is then answered directly, and it is at most 6 KiB
