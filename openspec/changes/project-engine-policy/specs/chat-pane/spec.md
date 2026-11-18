# Chat Pane Specification (delta: project engine policy)

## Base of this delta

This delta targets `openspec/specs/chat-pane/spec.md` on e480992e. It adds one badge beside the existing ones ("Approval mode badge and system lines", "Per-pane ORCH mode and badge"); those requirements are unchanged when no rule requires local. Slice tag: [P2].

## ADDED Requirements

### Requirement: Local badge
A chat pane whose rule requires local engines (see project-engine-policy, Sources and how they combine; any source, the global switch included) MUST show the badge ` LOCAL ` on its top row, in the style family of AUTO, RUN and ORCH, just left of the ORCH badge when that one is drawn, else just left of the mode badge. When only one column is left it MUST be drawn as `L`; with no room it MUST be omitted, never drawn partially, and it MUST NOT displace the mode badge, the ORCH badge or the batch badge. It MUST be absent when the chat's rule does not require local. It follows the chat's rule as it is now: it appears and goes with a folder change and with a menu save of the global switch.

#### Scenario: Badge in a marked project [auto, TestBackend + insta]
- GIVEN a chat in a folder covered by a project entry with `local_only = true`, in ORCH mode, rendered 80x12
- WHEN drawn
- THEN the top row holds ` LOCAL ` left of ` ORCH `, and the mode badge is where it is without this requirement

#### Scenario: No rule, no badge [auto, TestBackend + insta]
- GIVEN a chat in an unmarked folder and local-only mode off
- WHEN drawn
- THEN the render equals the one without this requirement

#### Scenario: Narrow [auto]
- GIVEN a chat whose rule requires local and widths from 0 to 40
- WHEN drawn at each width
- THEN nothing panics, the badge is ` LOCAL `, `L` or absent, never cut, and the other badges keep their places

#### Scenario: Follows the folder [auto]
- GIVEN a chat with the badge in `/w/acme` (marked)
- WHEN `/cd /w/oss` (unmarked) is confirmed
- THEN the badge is gone
