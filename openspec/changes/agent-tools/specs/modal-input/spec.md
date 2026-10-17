# Delta for Modal Input

All existing requirements stay as they are. The Chat pane input table is unchanged: the new chat keys live behind the prefix (user rule 2026-10-13). Tags as in the main spec; [design-confirm] = the letter is pinned by design.

## ADDED Requirements

### Requirement: Run-in-pane binding
The root of the prefix table MUST gain a leaf for the run-in-pane action of pane-commands, with a non-empty description, on the letter `e`, which collides with no existing root key or group. It MUST be left out of the root key hint if the hint would exceed 80 columns, as `o`, `s` and `T` are. On a focused chat pane showing the conversation view it MUST open the pending confirm of pane-commands; on a shell pane, on a chat list or editor view, and with tools off it MUST only show a notice and emit nothing. It MUST NEVER write to a PTY by itself.

#### Scenario: Leaf in the table [auto]
- GIVEN the prefix table
- WHEN it is validated
- THEN the leaf exists at the root with a non-empty description, no duplicate key exists and the root hint is at most 80 columns

#### Scenario: Opens the confirm [auto]
- GIVEN PREFIX on a chat pane whose last answer has a bash block
- WHEN the binding is pressed
- THEN a pending confirm exists, mode is TERMINAL and no WritePty is emitted

#### Scenario: Shell pane and other views [auto]
- GIVEN PREFIX on a shell pane, and on a chat pane showing the conversation list
- WHEN the binding is pressed
- THEN only a notice appears and no effect is emitted

#### Scenario: Inside a group does nothing special [auto]
- GIVEN group `w` pending, or COPY, or RESIZE
- WHEN the binding letter is pressed
- THEN no confirm opens

### Requirement: Approval mode binding
The root of the prefix table MUST gain a leaf on the letter `a` that toggles the approval mode (`ask` or `auto`) of the focused chat pane (see agent-tools, Chat approval mode), with a non-empty description, left out of the root key hint like `e`. No direct Ctrl+letter key is added. On a focused chat pane in any chat view it MUST flip the mode and add a system line naming the new mode, also while an approval is pending (which stays pending). On a shell pane, and with tools off, it MUST only show a notice and change nothing. It MUST NEVER write to a PTY.

#### Scenario: Leaf in the table [auto]
- GIVEN the prefix table
- WHEN it is validated
- THEN `a` exists only at the root with a non-empty description, no duplicate key exists and the root hint is unchanged and at most 80 columns

#### Scenario: Toggles the focused chat [auto]
- GIVEN PREFIX on a chat pane in `ask`
- WHEN `a` is pressed, then PREFIX and `a` again
- THEN the mode is `auto`, then `ask`, mode is TERMINAL and no effect writes to a PTY

#### Scenario: Shell pane and tools off [auto]
- GIVEN PREFIX on a shell pane, and in another run tools off on a chat pane
- WHEN `a` is pressed
- THEN only a notice appears and no mode changes

#### Scenario: Inside a group [auto]
- GIVEN group `w` pending, or COPY, or RESIZE
- WHEN `a` is pressed
- THEN no mode changes

### Requirement: Approval keys
While a chat pane has a pending approval (pane command or tool call) and is focused in TERMINAL, keys MUST act as follows and every other key MUST be swallowed (no composer edit, no WritePty, no Quit):

| Key | Effect |
|---|---|
| `y` (unmodified, lowercase, Press) | approve |
| `n`, Esc | decline |
| `p` (unmodified, pane command only) | move the target pane to the next shell pane |
| Ctrl+C | cancel the stream and deny the approval; never Quit |
| Ctrl+Space | enter PREFIX as in any pane |

Any other key, `Y`, and `y` with a modifier MUST NOT approve; they are swallowed (the prompt stays). Only `n` and Esc decline. The approval is per-chat state routed before the chat key table; it is not an `InputMode` variant, so it survives focus and tab changes. Repeat and Release MUST NOT answer. A paste MUST NOT answer and MUST NOT be inserted into the composer while the approval is pending. While MENU is open no key reaches the approval. In the other modes (PREFIX, COPY, RESIZE) the approval is not answered and stays pending.

#### Scenario: y approves, n and Esc decline [auto]
- GIVEN a pending pane-command approval
- WHEN `y`, and in other runs `n` and Esc, are pressed
- THEN `y` emits one WritePty to the target and the others emit none; the approval is closed each time

#### Scenario: Not approving keys [auto]
- GIVEN a pending approval
- WHEN `Y`, Enter, Space, Ctrl+y and Alt+y are pressed
- THEN no WritePty is emitted and the approval is still pending

#### Scenario: Repeat and Release [auto]
- GIVEN a pending approval
- WHEN Repeat and Release events for `y` arrive
- THEN nothing is answered

#### Scenario: Paste does not answer [auto]
- GIVEN a pending approval and the composer holding `draft`
- WHEN a paste of `y` arrives
- THEN the approval is pending and the composer still holds `draft`

#### Scenario: Cycle changes only the target [auto]
- GIVEN a pending approval with shell panes 1 and 2
- WHEN the cycle key is pressed
- THEN the target changes and no WritePty is emitted

#### Scenario: Ctrl+C denies, never quits [auto]
- GIVEN a streaming chat with a pending approval
- WHEN Ctrl+C is pressed
- THEN a ChatCancel effect is emitted, the approval is denied, and no Quit or WritePty is emitted

#### Scenario: Keys swallowed [auto]
- GIVEN a pending approval
- WHEN `a`, Tab and F5 are pressed
- THEN nothing changes, the composer is untouched and no WritePty is emitted

#### Scenario: Prefix still works [auto]
- GIVEN a pending approval
- WHEN `Ctrl+Space` then `w` then `v` are pressed
- THEN a split is made and the approval is still pending

#### Scenario: MENU shields the approval [auto]
- GIVEN a pending approval and MENU opened with `Ctrl+Space m`
- WHEN `y` is pressed
- THEN the menu handles `y` as an unlisted key and the approval is unanswered

#### Scenario: Real keys in Kitty [manual, Kitty]
- GIVEN a chat with a pending confirm in Kitty
- WHEN the user presses `p`, `y`, and then (in another run) `n`
- THEN each key behaves as listed and none is intercepted by the terminal

### Requirement: Pane removal and the pending approval
A pending approval is not cancelled by the removal of a pane other than its chat pane and its target pane. If the target pane is removed, `y` MUST be refused with a notice and the cycle key stays usable (see pane-commands); if the chat pane is removed the approval ends denied. This is the exception to "Any pane removal while a confirmation is pending MUST cancel it" of Close confirmations, which keeps applying to the close-pane, close-tab and quit confirmations.

#### Scenario: Other pane removed [auto]
- GIVEN a pending approval on pane 2 and a third pane is closed
- WHEN `y` is pressed
- THEN the write goes to pane 2

#### Scenario: Target removed [auto]
- GIVEN a pending approval on pane 2 and pane 2 exits
- WHEN `y` is pressed
- THEN no WritePty is emitted and a notice says the pane is gone

#### Scenario: Close prompts still cancel on removal [auto]
- GIVEN "Close pane? (y/n)" pending and a pane removal
- WHEN `y` is pressed afterwards
- THEN no pane is closed, as before
