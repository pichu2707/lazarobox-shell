# Archive Report: chat-notify

**Date**: 2026-10-14
**Artifact store**: hybrid (openspec files + engram `sdd/chat-notify/archive-report`)
**Branch**: `chore/openspec-archive` (from origin/main `fd8e2dc`)
**Status**: Archived, after ai-chat, engram-memory and pane-scroll. No formal verify report exists for this change (its delta is small, additive and tested); the apply progress is engram #3092 (`sdd/chat-notify/apply-progress`). No open tasks remain.

## Change Summary

When a live chat turn (or a summary run) ends with `Done` or any error, and the user is not looking at the pane (the pane is not the active tab's focused pane, or the terminal window lost focus), lazarobox emits one BEL to the host terminal. A user cancel never rings. Opt-in with `[ai] notify` (default off) and a static `Notify` row in the AI menu after `Memory`. Terminal focus reporting (`?1004`) is enabled after raw mode and disabled in `restore_to`, which also runs in the panic hook. The bell is staged as a host byte written after the frame, together with OSC 52 (OSC 52 first, one BEL per tick).

## What Shipped

- PR #163 `feat/chat-notify`, in release 0.10.0 (#164).

## Specs Synced

| Capability | Action | Requirements (before -> after) | Scenarios (before -> after) |
|------------|--------|--------------------------------|-----------------------------|
| `chat-notify` | Created (new). The delta was already a full spec without a Purpose; a Purpose paragraph was written from the proposal and the delta's tag legend was kept in it. | 0 -> 6 | 0 -> 19 |
| `config-menu` | 1 ADDED | 21 -> 22 | 112 -> 116 |
| `config-persistence` | 1 ADDED | 10 -> 11 | 73 -> 76 |
| `configuration` | 1 ADDED | 5 -> 6 | 42 -> 45 |

### Reconciliation at archive time

The `config-menu` text merged earlier (pane-scroll reconciliation) still named `Memory` as the last AI row. After this merge `Notify` is the last row (confirmed in `src/core/menu.rs`: `Profile`, `Context`, `Memory`, `Notify` follow the six task rows). Updated in the main spec: "Sections and items are data" and its descriptor and settings-rows scenarios, "Navigation" (order, "Wrap" now ends on `Notify`, "Task rows in order" now needs ten `j` presses to wrap), "AI task list" and its rendered-row scenarios, "Tiny terminals" and "Picker viewport" (fixed 62x15 and 14 rows replaced by sizes derived from `AI_BODY`: 15 body rows, 62x19 popup, 24x18 minimum today), and `config-persistence` "Owned keys only" (now lists `ai.context`, `ai.notify` and the `ai.memory.*` keys by cross-reference).

## Tasks

11 tasks, all checked. Task 6.2 (`[manual]` Kitty bell check) was ticked at archive time `(user-checked 2026-10-14)`: the user reported on 2026-10-14 that they ran the 0.10.0 manual checks in Kitty, and the bell was among them. No open tasks.

## Relevant Files

- `openspec/specs/{chat-notify,config-menu,config-persistence,configuration}/spec.md`
- `openspec/changes/archive/2026-10-14-chat-notify/` (this folder)
