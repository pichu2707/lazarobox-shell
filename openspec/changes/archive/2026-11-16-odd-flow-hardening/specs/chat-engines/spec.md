# Delta for Chat Engines

## Base of this delta

This delta targets `openspec/specs/chat-engines/spec.md` (from `main` at b97cceef). The MODIFIED block is a full copy of the requirement as it stands, with edits marked "(Previously: ...)". The run-level re-ask that follows a spent reminder is in `agent-orchestrator` (Gates on step state are visible and bounded).

## MODIFIED Requirements

### Requirement: A tool call written as text gets one reminder
When a reply of the Ollama or llama.cpp tool loop holds no structured tool call, but its text holds a JSON object, bare or fenced, whose `name` or `tool` is a tool offered in that turn and which has an `arguments` (or `parameters`) object, the loop MUST send one more request in the same turn. That request MUST carry the reply as plain assistant text, then a user message: ``Your answer wrote a call to `<name>` as text. Text does nothing. To act, call the tool.`` The pane MUST get the system line `the model wrote a tool call as text; it was asked to call the tool`. This MUST happen at most once per turn, MUST count toward the turn's request cap, and MUST NOT happen in the retry without tools. Nothing in the text MUST ever be run or turned into a call (local-model-writes D3). When the final reply of a turn, after the reminder was spent or not allowed by the cap, still is such a written call, the turn MUST end marked as ending on an unanswered written call, so an orchestrating run can re-ask (see `agent-orchestrator`); the mark changes no output of the turn and runs nothing.
(Previously: only the `name` key was accepted, though the orchestrator's own text-call check already accepted `tool`; the turn end carried no mark.)

Existing scenarios stay unchanged. Added scenarios:

#### Scenario: The `tool` key gets the reminder [auto]
- GIVEN tools on and a reply whose text is a fenced `{"tool": "find_dir", "arguments": {...}}` with `find_dir` offered
- WHEN the loop reads the reply
- THEN a second request carries the text and the reminder naming `find_dir`, and only the call the model makes after it runs

#### Scenario: A `tool` key naming a tool not offered [auto]
- GIVEN a reply whose text is `{"tool": "nope", "arguments": {}}` with `nope` not offered
- WHEN the loop reads it
- THEN the turn ends as before, with no reminder and no mark

#### Scenario: A spent reminder marks the turn end [auto]
- GIVEN a model that answers with a written call twice (either key)
- WHEN the loop reads the second answer
- THEN the turn ends with `Done` after two requests, marked as ending on an unanswered written call, and nothing was run

#### Scenario: A plain answer is not marked [auto]
- GIVEN a reply of plain text, or text that only names a tool
- WHEN the turn ends
- THEN the turn end carries no mark
