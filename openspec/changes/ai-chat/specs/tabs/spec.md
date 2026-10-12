# Delta for Tabs

Tags: [auto] = `cargo test`; [manual] = real Kitty. Existing requirements are unchanged; chat panes follow the same rules as shell panes, with the additions below.

## ADDED Requirements

### Requirement: Chat state is per-tab state
A tab's chat panes, with their composer, transcript, scroll position and running streams, MUST be part of that tab's state. Switching tabs MUST preserve them. A stream MUST keep running while its tab is not active, deltas MUST keep accumulating, and the exchange MUST be persisted when it finishes, like background output of shell panes. Closing a tab MUST cancel the streams of all its chat panes.

#### Scenario: State preserved across tabs [auto]
- GIVEN tab 1 with a chat pane holding composer text `draft` and a transcript, and tab 2 created
- WHEN the user returns to tab 1
- THEN the composer text and the transcript are as left

#### Scenario: Stream continues in a background tab [auto]
- GIVEN a stream running in tab 1 and tab 2 active
- WHEN deltas and the final event arrive
- THEN returning to tab 1 shows the complete answer and the pane is idle

#### Scenario: Closing a tab cancels streams [auto]
- GIVEN tab 2 with two chat panes each streaming
- WHEN tab 2 is closed
- THEN a ChatCancel effect is emitted for each of them

#### Scenario: Events of a closed tab are dropped [auto]
- GIVEN a closed tab that had a streaming chat pane
- WHEN a late delta for that pane arrives
- THEN it is ignored with no panic

### Requirement: Tab label with a chat pane focused
When the focused pane of a tab is a chat pane, the tab label MUST use the basename of that chat's origin cwd, with the same rules as a shell pane's cwd (`/` for the root, just `N` when unknown).

#### Scenario: Label from the origin cwd [auto]
- GIVEN a tab whose focused pane is a chat opened from `/home/u/proj`
- WHEN the tab labels are computed
- THEN the label is `1 proj`

#### Scenario: Label without origin cwd [auto]
- GIVEN a focused chat pane with no known origin cwd
- WHEN the label is computed
- THEN the label is its number only
