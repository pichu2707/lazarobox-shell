# Delta for Pane Layout

Tags: [auto] = `cargo test`; [manual] = real Kitty. Existing requirements are unchanged for shell panes; the additions below give panes a kind and define how chat panes take part in the layout (the content of a chat pane is in chat-pane).

## ADDED Requirements

### Requirement: Pane kinds
Every pane MUST have a kind, shell (PTY) or chat. Both kinds MUST share one `PaneId` sequence (ids never reused), the same tree, rect tiling, separators, minimum size (2 rows x 10 columns), geometric focus, resize, zoom and close rules. The kind MUST NOT change the computed rects: a layout with the same shape gives the same rects whatever the kinds. Only shell panes have a PTY; a chat pane MUST NOT cause SpawnPane, WritePty or ResizePty effects.

#### Scenario: Kinds do not change geometry [auto]
- GIVEN two layouts of the same shape, one with two shell panes and one with a shell and a chat pane
- WHEN rects are computed
- THEN the rects and separators are identical

#### Scenario: Ids shared and never reused [auto]
- GIVEN a shell pane 1 and a chat pane 2
- WHEN pane 2 is closed and a shell pane is split off
- THEN the new pane has an id different from 1 and 2

#### Scenario: Chat pane emits no PTY effects [auto]
- GIVEN a chat pane and a border move that changes its size
- WHEN the effects are computed
- THEN no ResizePty is emitted for the chat pane, and ResizePty is still emitted for shell panes whose size changed

#### Scenario: Existing PTY behaviour unchanged [auto]
- GIVEN the layouts, snapshots and effects of tabs with only shell panes
- WHEN the seam is in place
- THEN every existing test and snapshot passes unchanged

### Requirement: Chat panes in split, focus, zoom and close
Splitting from a focused chat pane (`w v`, `w h`) MUST create a shell pane that starts in the chat's origin cwd (see terminal-session). `Ctrl+Space c` opens a chat pane as a right split of the focused pane (see chat-pane). Focus movement, resize and zoom MUST work for chat panes like for shell panes. Closing a chat pane MUST remove it like any pane, cancel its running stream, and let its sibling expand. A tab whose last pane is a chat pane closes when that pane closes. The app quits only when the last pane of the last tab closes, so a chat pane alone keeps the app alive after the last shell of a tab exits.

#### Scenario: Split from a chat pane [auto]
- GIVEN a focused chat pane whose origin cwd is `/tmp/p`
- WHEN `w v` is pressed
- THEN a shell pane is created, a SpawnPane effect names `/tmp/p`, and the new pane is focused

#### Scenario: Close a chat pane [auto]
- GIVEN two panes (shell and chat) with the chat focused and streaming
- WHEN `w q` then `y` are pressed
- THEN the chat pane is removed, a ChatCancel effect is emitted for it, the shell fills the area and is focused

#### Scenario: Chat pane survives the last shell [auto]
- GIVEN a tab with one shell pane and one chat pane, and the app has one tab
- WHEN the shell exits
- THEN the chat pane fills the area and is focused, and no Quit is emitted

#### Scenario: Last pane is a chat pane [auto]
- GIVEN one tab with only a chat pane and a second tab
- WHEN `w q` then `y` are pressed
- THEN the prompt is `Close pane? (y/n)` and the tab closes

#### Scenario: Only chat pane in the only tab [auto]
- GIVEN one tab with only a chat pane
- WHEN `w q` is pressed
- THEN the prompt is `Quit? (y/n)`

#### Scenario: Zoom a chat pane [auto, TestBackend]
- GIVEN a chat pane next to a shell pane
- WHEN the chat is zoomed
- THEN the chat fills the pane area, no separator is drawn and its stream keeps running

#### Scenario: Resize with a chat neighbour [auto]
- GIVEN a shell pane left of a chat pane, shell focused
- WHEN resize right is pressed in RESIZE
- THEN the border moves one cell, ResizePty is emitted for the shell only

#### Scenario: Focus across kinds [auto]
- GIVEN a shell pane left of a chat pane, shell focused
- WHEN focus right is requested
- THEN the chat pane is focused and keys now go to its composer
