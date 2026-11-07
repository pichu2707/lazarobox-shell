# Delta for Chat Pane

## ADDED Requirements

### Requirement: read_pane approval preview
A pending `read_pane` approval MUST be drawn as a scrollable preview of the EXACT text that would be sent: after `redact(clean())`, with the truncation marker if any, and naming the pane (`read pane N`). The preview MUST scroll with the keys the prompt shows, and the same frozen value MUST be what is drawn and what `y` sends. The preview reuses the batch-review view: `y` stays locked until every line has been seen (scrolled to), then arms after the arming delay, as in the existing batch review. Keys `n`, Esc and Ctrl+C deny; the arming delay of the Approval prompt applies. The composer text and cursor MUST be kept. The model's text is untrusted: control and escape sequences MUST NOT reach the terminal.

#### Scenario: Exact preview, scrollable [auto, TestBackend + insta]
- GIVEN a frozen text of 60 lines with a secret assignment
- WHEN the prompt is rendered and scrolled down
- THEN later lines appear, the secret is redacted, and the drawn text equals the text `y` sends

#### Scenario: y locked until every line is seen [auto]
- GIVEN a frozen text longer than the preview rows
- WHEN `y` is pressed before the last line has been seen
- THEN nothing is sent; after scrolling to the end and the arming delay, `y` sends the frozen text

#### Scenario: Deny returns nothing [auto]
- GIVEN a pending `read_pane` prompt
- WHEN the user presses `n`
- THEN the tool result is a denial, no screen text is returned, and the composer still holds its draft

#### Scenario: Escape sequences neutralised [auto]
- GIVEN a screen holding `\x1b[2J` and a bidi override
- WHEN the preview is drawn
- THEN neither reaches the terminal

### Requirement: read_pane notices
The chat MUST show a visible notice (a system line, never silence) for each of these outcomes of `read_pane`: denied by the user; pane gone (closed before the prompt opened, so there is nothing to read; no prompt); empty screen (denied, no prompt); text truncated by the cap; scrollback requested in a pane running an alternate-screen (full-screen) app, where only the visible grid is read. The model MUST also receive a result that states the outcome.

#### Scenario: Denied [auto]
- GIVEN a pending `read_pane` prompt
- WHEN the user denies it, or it times out
- THEN a notice says the read was denied and the model gets a denied result

#### Scenario: Gone pane [auto]
- GIVEN `read_pane` on a pane that closed before the prompt opened
- WHEN the call is served
- THEN no prompt opens, a notice says the pane is gone, and the result is an error

#### Scenario: Empty screen [auto]
- GIVEN a pane with a blank screen
- WHEN the call is served
- THEN the call is denied without a prompt, a notice says the screen is empty, and the result tells the model the screen is empty

#### Scenario: Truncated [auto]
- GIVEN a read that exceeds the 8 KiB cap
- WHEN the prompt opens
- THEN the preview and a notice state the text was cut and the tail kept

#### Scenario: Scrollback in a full-screen app [auto]
- GIVEN a pane running an alternate-screen app and `scrollback: 50`
- WHEN the call is served
- THEN the visible grid is read and a notice says scrollback is unavailable there
