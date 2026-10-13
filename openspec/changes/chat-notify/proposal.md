# Proposal: Chat Notify (ring the terminal bell when an unwatched answer finishes)

## Intent

A chat answer can take a minute. The user switches to another pane or another window while it streams and has no cue when it ends. When an answer finishes and the user is not looking at it, lazarobox emits the terminal bell (BEL, `\x07`) to its own terminal, so the host terminal (Kitty) renders it as the user configured it (sound, visual flash, urgency hint).

The feature and its rules were approved by the user on 2026-10-13. Open details were resolved by the apply phase as documented below.

## Scope

In scope:
- A pure decision `should_bell(focused_pane, window_focused, pane, ended, enabled)`.
- Opt-in config key `[ai] notify = true|false`, default `false`, read and saved like `[ai] context`.
- A static AI menu row `Notify` (`off` / `on`) after `Memory`.
- `Effect::Bell`, written to the host terminal by the runtime after the frame is drawn (same queue as OSC 52).
- Terminal focus reporting (crossterm `EnableFocusChange`) to know whether the window is focused; restored on exit and on panic.

Out of scope: sounds or desktop notifications of our own, per-pane settings, notifying for shell panes, writing anything to a PTY.

## Decisions taken (the user was asleep; documented, easy to revisit)

| Topic | Decision |
|-------|----------|
| What finishes an answer | The final event of a live turn: `Done` bells; an error bells too (`Failed`, `Interrupted`, `Truncated`, answer cap), because the user is waiting either way. A user cancel never bells. |
| Summary runs | A summary run (`Prefix s`) that finishes bells under the same rule: the user waits for the draft to review it. |
| "Not looking" | Notify is on AND (the pane is not the focused pane of the active tab, OR the terminal window is not focused). A pane in another tab is not focused. |
| Focus reporting unsupported | No focus event ever arrives; the window counts as focused (pane rule only). |
| Focus reporting lifecycle | Enabled at startup whatever the toggle says (toggling at runtime would need terminal writes from the menu path); disabled on restore and in the panic hook with the other modes. |
| Rate limit | One bell per pane per answer holds by construction: a turn ends once and later events of it are dropped. Panes that finish within the same runtime frame (16 ms tick) share one BEL; panes finishing in different ticks each ring. |
| Where BEL is written | `Effect::Bell` only flags the runtime; the BEL byte is appended to the pending host bytes after the draw and written by the same writer as the frame. Never mid-frame, never to a PTY. |

## Risks

- Kitty's own `enable_audio_bell`, `visual_bell_duration` and `window_alert_on_bell` decide what the user perceives; lazarobox only sends BEL. A multiplexer in between may swallow it (not our case).
- Focus reporting is a terminal mode that must be undone: covered by the restore and panic-hook tests.
