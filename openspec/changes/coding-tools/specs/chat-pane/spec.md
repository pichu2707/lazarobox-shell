# Delta for Chat Pane

## Base of this delta

`openspec/specs/chat-pane/spec.md` exists. The approval prompt and tool rows are added by the unarchived `agent-tools` change, so `agent-tools` MUST be archived first. Only ADDED requirements are used, so the existing `Closing a chat pane` requirement is not replaced: the requirement below adds the batch rule on top of it. Key bindings are provisional and pinned by design.

## ADDED Requirements

### Requirement: Batch review view
A proposed write batch, and a `git_commit` review, MUST open as a full-pane view inside the chat pane, in the same family as the list, memory and tag views. The view MUST render every row of the batch with a fixed row model, and the renderer and the approving key MUST share one coverage count. The view MUST show how many rows remain unseen and keep the approving key disabled until all are shown and the arm delay has passed. The view MUST support scrolling, and MUST work in zoom and after a resize. It MUST show the total number of files, the added and removed lines, a warning on build files, and for a commit the message and the signing notice. `n` and Esc MUST reject the whole batch.

#### Scenario: Remaining count [auto, TestBackend + insta]
- GIVEN a batch of 30 rows with 10 shown
- WHEN the view renders
- THEN it shows 20 rows remaining and the approve key reads as disabled

#### Scenario: Scroll covers rows [auto]
- GIVEN a batch taller than the pane
- WHEN the user scrolls to the end
- THEN the coverage count reaches the total and the approve key is enabled after the arm delay

#### Scenario: Resize keeps coverage [auto]
- GIVEN rows already covered
- WHEN the pane is resized
- THEN the covered rows stay covered and no row is lost

#### Scenario: Reject [auto]
- GIVEN an open review
- WHEN `n` is pressed
- THEN the view closes, the batch is discarded and the transcript gets a system line

#### Scenario: Reject waits like approve, except when it is the only exit [auto]
- GIVEN a reviewable batch whose review was first drawn at t
- WHEN `n` or Esc is pressed before t + 800 ms
- THEN nothing happens, and at t + 800 ms (the same delay as `y`) they reject the batch
- AND on a batch that cannot be reviewed (too large or empty) `n` and Esc reject at once

#### Scenario: Only drawn rows count [auto]
- GIVEN a review opened in a background tab, a hidden pane or under another chat view
- WHEN the pane is resized or scrolled without being drawn
- THEN no row counts as seen and the arm delay has not started, until a frame draws the review

#### Scenario: The countdown ends on screen [auto]
- GIVEN a drawn review counting down with no stream running
- WHEN the arm delay elapses
- THEN the screen is redrawn once more and the header reads `y ready`

#### Scenario: The review owns the pane [auto, TestBackend]
- GIVEN an open review in a pane with the AUTO badge or a scrolled transcript
- WHEN the pane renders and the wheel turns over it
- THEN the badges do not cover the counter row, the wheel scrolls the review and never the transcript, and other mouse events do nothing
- AND the keys row cuts from the right with `n reject` first, then `y`, then the scroll hints

### Requirement: Staged batch indicator
While the chat holds a staged batch the pane MUST show an indicator with the number of staged entries. An orphan batch (after a cancelled turn) MUST show a distinct indicator and MUST offer the keys to review it or discard it. The indicator MUST be visible in every chat view, focused or not, and MUST disappear when the batch is applied or discarded.

#### Scenario: Indicator count [auto, TestBackend + insta]
- GIVEN 3 staged entries
- WHEN the pane renders
- THEN the indicator shows 3

#### Scenario: Orphan indicator and keys [auto]
- GIVEN a turn cancelled with a staged batch
- WHEN the pane renders and the review key is pressed
- THEN the orphan indicator is visible and the review view opens

#### Scenario: Gone after discard [auto]
- GIVEN an orphan batch
- WHEN it is discarded
- THEN the indicator disappears

### Requirement: Closing discards a batch
Closing a chat pane that holds a staged or orphan batch MUST discard the batch and MUST show a notice that names the number of discarded entries. Nothing is written to disk by the close. Quitting the app MUST do the same for every chat.

#### Scenario: Close notice [auto]
- GIVEN a chat with 2 staged entries
- WHEN the pane is closed
- THEN the batch is gone, the disk is untouched and a notice names 2 entries

#### Scenario: Quit [auto]
- GIVEN two chats with staged batches
- WHEN the app quits
- THEN both batches are discarded and a notice is shown
