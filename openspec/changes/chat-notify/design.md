# Design: Chat Notify

## Approach

Functional core, imperative shell, as in the rest of the app.

- **Core (`core::ai::chat::notify`)**: `Ended { Done, Error, Cancelled }` and the pure `should_bell(focused_pane, window_focused, pane, ended, enabled)`.
- **Config (`core::ai::config`, `core::config`, `core::ai::menu`, `core::menu`)**: `AiConfig.notify` (default false) beside `context`, read by `read_notify`; `ConfigEdit::is_flag` gains `ai.notify` (the existing `KeyIsTable` collision rule needs no change); `ItemKind::Notify` row after `Memory`; the edit chains after the memory edits.
- **App**: field `window_focused` (starts `true`), `AppEvent::WindowFocus(bool)`, and `Effect::Bell`. `on_chat` records whether the turn was live before applying the event and whether the pane is idle after; a live turn that ended asks `should_bell` with the active tab's focus. `Done` maps to `Ended::Done`; every other final event (errors, the answer cap) to `Ended::Error`. A user cancel never reaches `on_chat` as a live turn, so `Cancelled` exists only in the pure rule to document and test it.
- **Runtime**: `Effect::Bell` sets a flag in `Panes`. In the tick, after the draw, `stage_bell` appends one `0x07` to `host_out` and `flush_host` writes it with the same `stdout` writer as the frame (same path as OSC 52). Flag-then-stage gives the per-tick coalescing. Focus reporting: `EnableFocusChange` next to bracketed paste at startup, `DisableFocusChange` in `restore_to` (also used by the panic hook); `FocusGained/FocusLost` map to `WindowFocus`.

## Decisions

| # | Decision | Rationale |
|---|----------|-----------|
| N-1 | Bell on Done and Error, not on cancel | The user waits for both; a cancel is the user's own action. |
| N-2 | Summary runs bell too | They end with a draft the user must review. |
| N-3 | Flag + stage instead of pushing BEL in `apply` | One BEL per tick for panes finishing together, and the byte is placed after the draw. |
| N-4 | Focus reporting always on | Enabling at toggle time would need a terminal write from the menu path; the mode costs nothing when the toggle is off. |
| N-5 | `window_focused` starts `true` | A terminal without focus reporting must behave as "pane rule only". |
| N-6 | Keep `Ended::Cancelled` in the pure fn | The rule is stated and tested in one place even though the app never produces it. |

## Rollout and rollback

Default off, so nothing changes for existing users. Rollback is a revert of the commits; the stray `notify` key in a user's file is ignored by older builds like any unknown key.
