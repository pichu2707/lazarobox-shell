# Delta for Chat Notify (agents overview)

## Base of this delta

This delta targets the owning spec `openspec/specs/chat-notify/spec.md` as it stands on 2f20e9df. Today the only trigger is the end of a chat turn ("A finished live turn asks for one bell", openspec/specs/chat-notify/spec.md:37; the single `Effect::Notify` site, src/app.rs:3220-3235). The decision `should_notify` (src/core/ai/chat/notify.rs:20-30), the delivery table and the fixed bodies (notify.rs:140-148) stay. This delta adds a second trigger under the same `[ai] notify` mode (src/core/ai/config.rs:107-121), so it is pending Q3 of the proposal (option A, reuse `[ai] notify`). CLI agents in panes are left out of it until Q3's opt-in for them is decided with S5.

Slice tag: [S3].

## ADDED Requirements

### Requirement: A wait asks for one notification [S3] (pending Q3)
When a chat or worker row (see agents-overview, States) enters `waiting for approval` or `waiting for you`, the app MUST emit one notification, by the same mode and delivery as a finished turn, exactly when `[ai] notify` is not `off` and the row's pane is not watched (another pane is focused or the window lost the focus). The body MUST be fixed text naming the agent and the pane, never content: `Worker <n> waits for approval in pane <p>`, `Chat waits for approval in pane <p>`, `Worker <n> failed in pane <p>; the run waits for you`, `Chat waits for you in pane <p>`. A row that stays waiting MUST NOT notify again; a row that leaves the waiting state and enters it again within 30 s (assumption) MUST NOT notify again; several rows that start waiting in the same app update MUST give one notification naming the first and `+N more`. A wait that the user caused while watching (the pane is focused) MUST NOT notify. Rows of CLI agents MUST NOT notify under this requirement.

#### Scenario: Worker ask in an unwatched pane [auto]
- GIVEN notify `desktop`, the chat in pane 3 not focused, and worker 2 of its run
- WHEN an ask of worker 2 queues
- THEN exactly one `Effect::Notify` is returned with the body `Worker 2 waits for approval in pane 3`

#### Scenario: Off stays silent [auto]
- GIVEN notify `off`
- WHEN any row starts waiting
- THEN no effect is returned

#### Scenario: Watched pane is silent [auto]
- GIVEN the chat is the focused pane of a focused window
- WHEN its own approval opens
- THEN no effect is returned

#### Scenario: No repeat within the hold [auto]
- GIVEN a fake clock and a worker whose ask was answered at t=0 after a notification
- WHEN the same worker asks again at t=10 s and at t=45 s
- THEN no notification is returned at t=10 s and one at t=45 s

#### Scenario: A finished turn still notifies as before [auto]
- GIVEN notify `bell` and an unwatched streaming chat
- WHEN `Done` arrives
- THEN exactly one `Effect::Bell` is returned, as without this change
