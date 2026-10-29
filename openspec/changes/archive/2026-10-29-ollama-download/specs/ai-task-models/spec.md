# AI Task Models Specification (delta: Ollama downloads)

## MODIFIED Requirements

### Requirement: Curated Ollama catalog
The code MUST ship a static catalog of well-known Ollama model tags, between 8 and 12 entries, with unique non-empty names. Only Ollama has a curated catalog (lmstudio, llamacpp and vllm have none). The catalog MUST be informational: it is never fetched and never refreshed from any network, and the source and the user documentation MUST say it may be stale. It is also the fallback list of the Ollama download flow when the library page cannot be read. lazarobox MUST NOT pull or install any model through a server, except the one `POST /api/pull` of ollama-download, sent after the user confirms it. No chat request MUST ever ask a server to download a model. The only requests lazarobox sends are:
- the localhost detection requests;
- the loopback chat requests of chat-engines, sent only when the user sends a chat message;
- the update check of the updates feature;
- the Hugging Face requests of model-download, sent only while the user is in its download flow;
- the ollama.com and registry.ollama.ai requests and the loopback pull of ollama-download, sent only while the user is in its download flow.

A model is downloaded only by those two flows, after the user confirms it.

#### Scenario: Catalog integrity [auto]
- GIVEN the curated Ollama catalog
- WHEN it is validated
- THEN it has between 8 and 12 entries, all unique and non-empty

#### Scenario: No download path outside the flows [auto]
- GIVEN a download-required model row
- WHEN the cursor is on it and every key is pressed
- THEN no request, command or effect is produced and the assignment is unchanged

#### Scenario: Chat never pulls a model [auto]
- GIVEN `chat` assigned to a server model that is not downloaded
- WHEN a message is sent
- THEN no request is sent to any server and the chat shows the `download required` guidance

#### Scenario: Docs say stale [manual]
- GIVEN the README
- WHEN it is read
- THEN it states that the Ollama catalog is informational and may be out of date, and that a model is downloaded only from the Hugging Face or the Ollama flow, after the user confirms it
