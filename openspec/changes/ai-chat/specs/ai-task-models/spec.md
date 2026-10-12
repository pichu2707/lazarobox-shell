# Delta for AI Task Models

Tags: [auto] = `cargo test`; [manual] = real machine. The `chat` assignment, until now only data, is used at runtime by the chat. The other requirements (fixed task set, fallback to general, set-all summary, engine choices, CLI model catalog, server model list and marks, model marks, custom model validity, assignment actions) are unchanged.

## MODIFIED Requirements

### Requirement: Curated Ollama catalog
The code MUST ship a static catalog of well-known Ollama model tags, between 8 and 12 entries, with unique non-empty names. Only Ollama has a curated catalog (lmstudio, llamacpp and vllm have none). The catalog MUST be informational: it is never fetched, never refreshed from any network, and the source and the user documentation MUST say it may be stale. lazarobox MUST NOT download, pull or install any model, and MUST NOT send any request other than the localhost detection requests and the loopback chat requests of chat-engines (sent only when the user sends a chat message). No chat request MUST ever ask a server to download a model.
(Previously: no request other than the localhost detection requests)

#### Scenario: Catalog integrity [auto]
- GIVEN the curated Ollama catalog
- WHEN it is validated
- THEN it has between 8 and 12 entries, all unique and non-empty

#### Scenario: No download path [auto]
- GIVEN a download-required model
- WHEN the cursor is on it and every key is pressed
- THEN no request, command or effect is produced and the assignment is unchanged

#### Scenario: Chat never pulls a model [auto]
- GIVEN `chat` assigned to a server model that is not downloaded
- WHEN a message is sent
- THEN no request is sent to any server and the chat shows the `download required` guidance

#### Scenario: Docs say stale [manual]
- GIVEN the README
- WHEN it is read
- THEN it states that the Ollama catalog is informational, may be out of date, and that nothing is downloaded by lazarobox

## ADDED Requirements

### Requirement: Runtime use of the chat assignment
The chat MUST use the effective assignment of the `chat` task (own assignment, else `general`; see Fallback to general), read from the current configuration on every send, so a saved change applies from the next message. The model MUST be validated against detection before a send: for a server engine the model MUST be non-empty and among the detected models (exact match, with the Ollama tag-less equals `:latest` rule); otherwise the chat shows the model guidance and sends nothing. For a CLI engine the model is not validated (empty and `default` mean the CLI's own default). The chat MUST NOT change any assignment, MUST NOT pick another engine when the effective one is unusable, and MUST NOT write the config.

#### Scenario: Effective assignment used [auto]
- GIVEN `general` is `claude` / `sonnet` and `chat` is `ollama` / `llama3:8b`, both available
- WHEN a chat message is sent
- THEN the turn goes to ollama with `llama3:8b`

#### Scenario: Fallback to general [auto]
- GIVEN `chat` is unassigned and `general` is `claude` / `sonnet`, claude available
- WHEN a chat message is sent
- THEN the turn goes to claude with `sonnet`

#### Scenario: Server model must be detected [auto]
- GIVEN `chat` is `lmstudio` / `m1` and lmstudio lists only `m2`
- WHEN a chat message is sent
- THEN nothing is sent and the guidance names `m1` and `download required`

#### Scenario: Empty server model [auto]
- GIVEN `chat` is `ollama` with an empty model
- WHEN a chat message is sent
- THEN nothing is sent and the guidance says no model is chosen

#### Scenario: CLI model not validated [auto]
- GIVEN `chat` is `claude` / `custom-x` and claude is available
- WHEN a chat message is sent
- THEN the turn is started with that model name

#### Scenario: Assignment never changed [auto]
- GIVEN `chat` is `ollama` / `llama3:8b` and ollama is `not running`
- WHEN a chat message is attempted
- THEN the assignments are unchanged and no other engine is contacted

#### Scenario: Saved change applies to the next message [auto]
- GIVEN an open chat and the menu saves `chat` as `claude` / `opus`
- WHEN the next message is sent
- THEN the turn goes to claude with `opus`
