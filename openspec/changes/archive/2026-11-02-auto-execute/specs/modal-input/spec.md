# Delta for Modal Input

Applies on top of the unarchived `agent-tools` delta for modal-input. The Chat pane input table is unchanged. Tags as in the main spec.

## MODIFIED Requirements

### Requirement: Approval mode binding
The root of the prefix table MUST gain a leaf on the letter `a` that cycles the approval mode of the focused chat pane, `ask`, `auto`, `run`, back to `ask` (see agent-tools, Chat approval mode), with a non-empty description, left out of the root key hint like `e`. No direct Ctrl+letter key is added and `run` has no key of its own. Entering `run` needs no extra confirmation. On a focused chat pane in any chat view it MUST advance the mode and add a system line naming the new mode and what it does (see chat-pane), also while an approval is pending (which stays pending). On a shell pane, and with tools off, it MUST only show a notice and change nothing. It MUST NEVER write to a PTY.
(Previously: the leaf toggled between `ask` and `auto`)

#### Scenario: Leaf in the table [auto]
- GIVEN the prefix table
- WHEN it is validated
- THEN `a` exists only at the root with a non-empty description, no duplicate key exists and the root hint is unchanged and at most 80 columns

#### Scenario: Cycles the focused chat [auto]
- GIVEN PREFIX on a chat pane in `ask`
- WHEN `a` is pressed, then PREFIX and `a` again, then PREFIX and `a` a third time
- THEN the mode is `auto`, then `run`, then `ask`, mode is TERMINAL after each and no effect writes to a PTY

#### Scenario: No confirmation to enter run [auto]
- GIVEN a chat in `auto`
- WHEN PREFIX and `a` are pressed
- THEN the mode is `run` at once, with a system line, and no prompt or modal opens

#### Scenario: Pending approval survives the cycle [auto]
- GIVEN a chat with a pending `run_in_pane` confirm
- WHEN `a` is pressed until the mode is `run`
- THEN the confirm is still pending and nothing is written

#### Scenario: Shell pane and tools off [auto]
- GIVEN PREFIX on a shell pane, and in another run tools off on a chat pane
- WHEN `a` is pressed
- THEN only a notice appears and no mode changes

#### Scenario: Inside a group [auto]
- GIVEN group `w` pending, or COPY, or RESIZE
- WHEN `a` is pressed
- THEN no mode changes
