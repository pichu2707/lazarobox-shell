# Delta for Chat Engines

## Base of this delta

`openspec/specs/chat-engines/spec.md` exists. The Ollama native tools requirement is added by the unarchived `agent-tools` change, so `agent-tools` MUST be archived first. Only ADDED requirements are used.

## ADDED Requirements

### Requirement: Ollama round limit with coding tools
The Ollama tool loop MUST allow at most 8 rounds per turn when no coding-tools family is on, and at most 24 rounds per turn when at least one family toggle (search, write, exec, git, git write) is on and tools are on. The limit of calls per reply MUST NOT change. When the limit is reached the turn MUST end with the existing round-limit behaviour, and a staged batch MUST stay staged (see code-write).

#### Scenario: Default stays 8 [auto]
- GIVEN tools on and every family toggle off
- WHEN an Ollama turn keeps asking for tools
- THEN the loop stops after 8 rounds

#### Scenario: Raised to 24 [auto]
- GIVEN tools on and the Search toggle on
- WHEN an Ollama turn keeps asking for tools
- THEN the loop stops after 24 rounds

#### Scenario: Tools off [auto]
- GIVEN tools off and a family toggle on
- WHEN the request is built
- THEN it is identical to a request without tools and the limit is irrelevant

#### Scenario: Limit leaves the batch [auto]
- GIVEN a staged batch and a turn that hits the limit
- WHEN the turn ends
- THEN the batch is still staged

### Requirement: A tool call written as text gets one reminder
When a reply of the Ollama or llama.cpp tool loop holds no structured tool call, but its text holds a JSON object, bare or fenced, whose `name` is a tool offered in that turn and which has an `arguments` (or `parameters`) object, the loop MUST send one more request in the same turn. That request MUST carry the reply as plain assistant text, then a user message: ``Your answer wrote a call to `<name>` as text. Text does nothing. To act, call the tool.`` The pane MUST get the system line `the model wrote a tool call as text; it was asked to call the tool`. This MUST happen at most once per turn, MUST count toward the turn's request cap, and MUST NOT happen in the retry without tools. Nothing in the text MUST ever be run or turned into a call (local-model-writes D3).

#### Scenario: One reminder [auto]
- GIVEN tools on and a reply whose text is a fenced call to `find_dir` with arguments
- WHEN the loop reads the reply
- THEN a second request carries the text and the reminder, with tools still offered, and only the call the model makes after it runs

#### Scenario: Second text call ends the turn [auto]
- GIVEN a model that answers with a text call twice
- WHEN the loop reads the second answer
- THEN the turn ends with `Done` after two requests

#### Scenario: Not a call [auto]
- GIVEN an answer that names a tool without arguments, a call to a tool not offered, or a turn without tools
- WHEN the loop reads it
- THEN the turn ends as before, with no reminder

#### Scenario: Cap [auto]
- GIVEN a text call in the last reply the cap allows
- WHEN the loop reads it
- THEN the turn ends with no reminder
