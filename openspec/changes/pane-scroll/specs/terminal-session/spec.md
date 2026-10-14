# Delta for Terminal Session

Tags: [auto] = `cargo test`; [manual] = real Kitty. Requirements not listed here are unchanged. Base text: `openspec/specs/terminal-session/spec.md`. The ai-chat delta for this capability (`openspec/changes/ai-chat/specs/terminal-session/spec.md`) does not touch "Shell exit and shutdown" or "No mouse capture"; if it is archived first and modifies the first, apply this block on top of its text.

## MODIFIED Requirements

### Requirement: Shell exit and shutdown
When a pane's child exits, that pane MUST close (sibling expands; if it was focused, focus moves per the pane-layout close rule). When the last pane of a tab closes the tab MUST close; when the last tab closes the app MUST quit with the host terminal restored. On quit the host terminal MUST be restored first, including disabling mouse capture when it was enabled (see mouse-input), so the user gets the prompt back immediately; then all child processes of all panes MUST be torn down in parallel, bounded to 3 seconds, after which no child process of any pane MUST remain. Terminal state, including mouse capture, MUST also be restored on panic.
(Previously: terminal state was restored on quit and panic with no mention of mouse capture)

#### Scenario: One of two shells exits [auto]
- GIVEN two panes with pane 2 focused
- WHEN pane 2's PTY-exited event arrives
- THEN pane 2 is closed, no Quit is emitted, and the other pane is focused

#### Scenario: Last shell exits [auto]
- GIVEN one tab with one pane
- WHEN its PTY-exited event arrives
- THEN a Quit effect is emitted

#### Scenario: Quit leaves no orphan [auto, unix PTY]
- GIVEN several running children across panes
- WHEN the app shuts down
- THEN the terminal is restored before the teardown wait, and none of the child pids exists within 3 seconds

#### Scenario: Terminal restored [manual]
- GIVEN `exit` typed in the last pane, or prefix `q` then `y`
- WHEN the app ends
- THEN the host shell is usable (cooked mode, cursor visible, no alt screen, mouse capture off: moving the wheel prints nothing)

#### Scenario: Mouse disabled during restore [auto]
- GIVEN mouse capture enabled
- WHEN the app quits, and in another run panics
- THEN `\x1b[?1006l\x1b[?1000l` is written as part of the restore in both cases

#### Scenario: No orphan shells after quit [manual, Kitty]
- GIVEN several panes and tabs open
- WHEN the app quits
- THEN `pgrep` / `ps` shows no leftover shells from the session

## REMOVED Requirements

### Requirement: No mouse capture
(Reason: mouse capture is now a supported, default-on feature controlled by the `mouse` config key; its lifecycle, sequences and restore rules live in the new `mouse-input` capability. The scenario "Native selection" is replaced by mouse-input "Native selection needs Shift".)
