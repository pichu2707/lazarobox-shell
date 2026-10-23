# Config Menu Specification (delta: llamacpp pick model)

Tags as in `config-menu`: [auto] = `cargo test`; [manual] = real machine.

## MODIFIED Requirements

### Requirement: Stopped servers in the picker
As in configurable-server-ports, except for llamacpp with something to start: level 2 of a stopped llamacpp MUST list the managed `model` (if any) first, then the detected local models, each once, labeled with the file name and marked `lazarobox starts it`, selectable; a path that is not valid text is left out. After them come the assigned models, selectable only while the server is managed. With no managed model and no local file, the note to start it by hand stays.

#### Scenario: A local model is offered [auto]
- GIVEN llamacpp installed but not answering and `/m/a.gguf` found
- WHEN level 2 of llamacpp opens
- THEN `a.gguf` is listed, marked `lazarobox starts it`, selectable

## ADDED Requirements

### Requirement: Choosing a local model manages llama-server
Choosing one of those models MUST turn the managed llama-server on in the draft with that path. On save the menu MUST write `managed = true` (a boolean) and `model = "<path>"` under `[ai.servers.llamacpp]`, keeping every other key and comment of that table. llamacpp takes port 39281 unless the table has its own `port`. A model of a running llama-server, or choosing the model already managed, writes nothing under `[ai.servers]`.

#### Scenario: Save keeps the port [auto]
- GIVEN `[ai.servers.llamacpp] port = 39080 # mine`
- WHEN `/m/my model.gguf` is chosen and saved
- THEN the file has `port = 39080 # mine`, `managed = true` and that `model`, and reads back managed on port 39080
