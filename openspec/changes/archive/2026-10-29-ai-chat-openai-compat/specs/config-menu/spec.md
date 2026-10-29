# Config Menu Specification (delta: OpenAI-compatible adapters)

Tags as in `config-menu`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: Engines without a chat adapter are not selectable
This amends level 1 of the Task picker. An available engine that chat has no adapter for (today only `opencode`; see chat-engines, ChatEngine contract) MUST still be listed in catalog order with its status text `detected · not in chat yet`, but its row MUST be disabled: the cursor reaches it, Enter does nothing (the picker stays at level 1 and the draft is unchanged), and the footer reads `detected · not in chat yet · not selectable`. Every engine with an adapter, `lmstudio`, `llamacpp` and `vllm` included, is selectable as before. An assigned engine that is not detected keeps its `(not detected)` row, selectable.

#### Scenario: A detected engine without an adapter is listed but disabled [auto]
- GIVEN llamacpp available with one model and opencode available
- WHEN level 1 opens
- THEN `llamacpp available · 1 model` is selectable and `opencode detected · not in chat yet` is listed but disabled

#### Scenario: Enter on it does nothing [auto]
- GIVEN the cursor on the opencode row of level 1
- WHEN Enter is pressed
- THEN the picker stays at level 1 on that row and the draft is unchanged

#### Scenario: The footer gives its status as the reason [auto]
- GIVEN the cursor on the opencode row of level 1
- WHEN the footer is drawn
- THEN it reads `detected · not in chat yet · not selectable`
