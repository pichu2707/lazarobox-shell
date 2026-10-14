# Proposal: Pane Scroll (mouse wheel + COPY for every pane)

## Intent

Output that scrolls off screen cannot be reached in shell or chat panes without knowing COPY exists, and the chat pane has no COPY at all. Give every pane a discoverable vertical scroll: mouse wheel by default, COPY (`Ctrl+Space [`, j/k) as the keyboard path.

## Scope

### In Scope
- Wheel routed to the pane under the pointer, fixed 3-line step, no focus change:
  - child in mouse mode: forward encoded wheel (SGR or legacy, per child encoding)
  - alt screen, no mouse mode (less, man): 3 arrow keys (app-cursor aware)
  - main screen: scrollback ±3 without entering COPY; view anchored; any key encoded to the PTY or scrolling down to 0 returns to live
  - COPY active: COPY motions; chat: transcript ±3 lines
- Mouse capture: SGR `?1000h` + `?1006h` only (no motion); restored on exit and panic.
- Mouse ON by default via the existing top-level `mouse` key; menu row "Mouse" becomes a real toggle saved by config_edit. Native selection needs Shift (documented).
- COPY on chat panes: j/k lines, Ctrl+u/d half page, gg/G, Esc/q back to follow.
- COPY hint in the statusline: `COPY j/k scroll · v select · y copy · Esc exit` (chat variant omits select/copy).
- `[+N]` badge on any scrolled-back pane; statusline position when the focused pane is scrolled.

### Out of Scope
- Shift+PageUp/Down bindings (Kitty consumes them); no new direct Ctrl+letter keys.
- Click-to-focus, click/drag forwarding, motion tracking.
- Chat COPY selection and yank (line-wise `V` + `y` over rendered text, reasoning only if expanded): follow-up; `Ctrl+Y` still copies the last answer.

## Capabilities

### New Capabilities
- `mouse-input`: capture lifecycle, wheel hit-test and routing, wheel encoding, non-COPY scrollback view.

### Modified Capabilities
- `terminal-session`: remove "No mouse capture"; shutdown restores mouse mode.
- `configuration`: `mouse` default true, effective; drop "not supported yet" notice.
- `config-menu`: Mouse row enabled, cycles, cursor no longer skips it.
- `config-persistence`: `mouse` becomes an owned key.
- `copy-mode-scrollback`: COPY allowed on chat panes (reverses ai-chat refusal); scrolled badge.
- `modal-input`: `[` enters COPY on chat panes; chat COPY key table.
- `statusline`: COPY hint; position for non-COPY scroll.
- `chat-pane` (in `openspec/changes/ai-chat`): line-step scroll, COPY on transcript.

## Approach

Exploration approach B. Slices (size:exception accepted):
1. COPY on chat (`Scroll` line step) + `[+N]` badge + statusline hint/position.
2. Pure wheel routing/encoding over `tiling()` rects and vt100 `mouse_protocol_mode/encoding`.
3. Runtime capture (`enter_modes`, `restore_to`, panic hook, `to_app_event`) + config/menu/persistence + spec deltas.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/app.rs`, `src/app/chat.rs` | Modified | wheel dispatch, chat COPY, live reset |
| `src/core/ai/chat/transcript.rs` | Modified | line-step scroll |
| `src/core/pane.rs`, `src/core/keys.rs` | Modified | mouse modes, wheel encoding |
| `src/runtime.rs` | Modified | capture, restore, mouse events |
| `src/core/{config,config_edit,menu}.rs` | Modified | mouse key, toggle |
| `src/ui/...statusline.rs`, `terminal_view.rs` | Modified | hint, badge |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Mouse left enabled after exit/panic | Med | disable in `restore_to` and panic hook; test sequence |
| Wrong wheel encoding in vim/htop | Med | table tests per mode x encoding; manual check |
| Conflicts with ai-chat PR15 in app.rs/runtime.rs | High | rebase per slice; keep logic in new modules |
| chat-pane spec not archived yet | Med | delta targets ai-chat spec; archive order noted |

## Rollback Plan

Revert slices in reverse order; `mouse = false` disables capture at runtime without code changes.

## Dependencies

- ai-chat chat-pane spec and `Scroll` type (present on main 716e2d1).

## Success Criteria

- [ ] Wheel scrolls shell, less/man, vim/htop and chat panes under the pointer.
- [ ] Terminal is clean after quit and after panic.
- [ ] COPY works on chat panes with j/k, Ctrl+u/d, gg/G, Esc.
- [ ] Scrolled panes show `[+N]`; `mouse` toggle persists.

## Proposal question round

User decisions of 2026-10-14 are final. Assumptions for review:
- Chat COPY selection/yank deferred to a follow-up (confirm, or pull it into slice 1).
- Wheel in an unfocused pane while COPY is active elsewhere scrolls that pane's view only, not COPY.
