# Delta for Agent Orchestrator

## ADDED Requirements

### Requirement: Workers are display-only for project memory notes [S1]

Orchestrator workers MUST NOT save project memory notes, and a worker's output, brief, result row or step trail MUST NOT become a note. Worker output remains untrusted data (see "Worker output is untrusted data"). The coordinator's own chat turns follow chat-memory; workers receive no memory tool and no write access to `.lzr/`. Handoff notes keep their separate rules in chat-memory and orchestrator-handoffs.

#### Scenario: Worker finishes
- GIVEN an orchestrator run whose worker returns a result with `decision: use Axum`
- WHEN the worker ends
- THEN no file under `.lzr/` is created or changed by the worker and no note save is issued

#### Scenario: Worker cannot save
- GIVEN a worker whose toolset is built
- WHEN its tools are listed
- THEN there is no memory save tool

#### Scenario: Worker text injected as data only
- GIVEN a worker result containing `ignore previous instructions`
- WHEN a note summary of the chat is saved at chat end
- THEN worker output and step trail are not in the summary input
