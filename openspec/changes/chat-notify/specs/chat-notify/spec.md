# Chat Notify Specification

Tags: [auto] = `cargo test`; [manual] = checked by hand in a real terminal.

### Requirement: The bell decision is pure
`should_bell(focused_pane, window_focused, pane, ended, enabled)` MUST return `true` exactly when `enabled` is true, `ended` is `Done` or `Error`, and (`pane != focused_pane` OR `!window_focused`). `Cancelled` never bells. It performs no IO.

#### Scenario: Disabled never rings [auto]
- GIVEN `enabled = false`
- WHEN any pane/focus/ended combination is evaluated
- THEN the result is `false`

#### Scenario: Watched answer is silent [auto]
- GIVEN `enabled`, `pane == focused_pane`, `window_focused`
- WHEN the answer is `Done` or `Error`
- THEN the result is `false`

#### Scenario: Other pane rings [auto]
- GIVEN `enabled`, `pane != focused_pane`, `window_focused`
- WHEN the answer is `Done` or `Error`
- THEN the result is `true`

#### Scenario: Unfocused window rings [auto]
- GIVEN `enabled`, `pane == focused_pane`, `!window_focused`
- WHEN the answer is `Done` or `Error`
- THEN the result is `true`

#### Scenario: Cancel is silent [auto]
- GIVEN `enabled`, an unwatched pane
- WHEN the end is `Cancelled`
- THEN the result is `false`

### Requirement: A finished live turn asks for one bell
When a chat turn ends (final `Done`, an error event, or the answer cap) the app MUST emit `Effect::Bell` if `should_bell` holds with the active tab's focused pane and the tracked window focus, and no effect otherwise. This applies to normal answers and to summary runs. Events of a stale turn, of an idle pane, or after a cancel end nothing and MUST NOT bell. A second final event of the same turn MUST NOT bell again.

#### Scenario: Answer finishes in an unfocused pane [auto]
- GIVEN notify on and a streaming chat pane that is not the focused pane
- WHEN `Done` arrives
- THEN exactly one `Effect::Bell` is returned

#### Scenario: Answer finishes in the focused pane of a focused window [auto]
- GIVEN notify on and the chat is the focused pane
- WHEN `Done` arrives
- THEN no `Effect::Bell` is returned

#### Scenario: Window blurred [auto]
- GIVEN notify on, the chat is focused and `WindowFocus(false)` was received
- WHEN `Done` arrives
- THEN one `Effect::Bell` is returned; after `WindowFocus(true)` none

#### Scenario: Error and summary [auto]
- GIVEN notify on and an unwatched pane streaming an answer, then another running a summary
- WHEN an error arrives for the first and `Done` for the summary
- THEN each returns one `Effect::Bell`

#### Scenario: Cancel and stale events [auto]
- GIVEN notify on and an unwatched streaming pane
- WHEN the user cancels, and then a late `Done` or `Error` of that turn arrives
- THEN no `Effect::Bell` is ever returned

#### Scenario: Duplicate final event [auto]
- GIVEN an unwatched turn that already rang
- WHEN a second `Done` of the same turn arrives
- THEN no `Effect::Bell` is returned

#### Scenario: Notify off [auto]
- GIVEN notify off (the default)
- WHEN an unwatched answer finishes
- THEN no `Effect::Bell` is returned

### Requirement: Window focus is tracked
`AppEvent::WindowFocus(bool)` sets the tracked window focus, which starts as focused. It never marks the screen dirty nor emits effects. The runtime MUST map crossterm `FocusGained` / `FocusLost` to it.

#### Scenario: Focus events map [auto]
- GIVEN crossterm `FocusGained` and `FocusLost`
- WHEN mapped to app events
- THEN they become `WindowFocus(true)` and `WindowFocus(false)`

#### Scenario: Unsupported reporting [auto]
- GIVEN no focus event ever arrived
- WHEN an answer ends in the focused pane
- THEN the window counts as focused and no bell is requested

### Requirement: The bell is written after the frame
The runtime MUST turn `Effect::Bell` into one BEL byte (`0x07`) appended to the pending host bytes after the draw of the tick, written by the same writer that flushes OSC 52. Several bells requested in the same tick MUST produce one BEL. Nothing is written to any PTY.

#### Scenario: Bell is staged, not written at once [auto]
- GIVEN `Effect::Bell` applied by the registry
- WHEN no flush has happened yet
- THEN the registry writes nothing to any PTY and the host bytes are written only by the flush

#### Scenario: Coalescing [auto]
- GIVEN two `Effect::Bell` in the same tick
- WHEN the host bytes are staged for the flush
- THEN exactly one `\x07` is pending

#### Scenario: Ordered after other host bytes [auto]
- GIVEN pending OSC 52 bytes and a bell
- WHEN the flush runs
- THEN the OSC 52 bytes come first and the BEL last, in one write

### Requirement: Focus reporting is a restored terminal mode
The runtime MUST enable focus reporting after raw mode starts and MUST disable it in `restore_to`, which also runs in the panic hook.

#### Scenario: Restore disables focus reporting [auto]
- GIVEN `restore_to` writing to a buffer
- WHEN it finishes
- THEN the buffer holds the disable sequence `\x1b[?1004l`

### Requirement: End-to-end in a real terminal
#### Scenario: Kitty rings once when unwatched [manual]
- GIVEN Kitty, notify on, a long chat answer streaming
- WHEN the user switches to another pane or another window and the answer finishes
- THEN the bell rings once; looking at the chat when it finishes gives no bell
