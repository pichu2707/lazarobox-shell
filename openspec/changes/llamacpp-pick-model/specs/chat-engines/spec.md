# Chat Engines Specification (delta: llamacpp pick model)

Tags as in `chat-engines`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: A managed llama-server turned on from the menu
A save that turns the managed llama-server on, or changes its model, MUST start it in the background at once, without a restart: the statusline reads `starting llama-server with <file>…`, then `llama-server is ready` (and engines are detected again) or the reason it did not start. From then on turns and detection use its port. The server it replaces MUST stop and never start again; a turn that still holds it fails with `llama-server was switched to another model; send again`. Quitting stops the current one as before.

#### Scenario: The next turn uses it [auto]
- GIVEN lazarobox started with llamacpp unmanaged
- WHEN the menu turns it on and a llamacpp turn is sent
- THEN the turn starts that server and talks to its port, and quitting stops it
