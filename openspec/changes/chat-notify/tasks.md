# Tasks: Chat Notify

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~700 total (tests and snapshots included) |
| 400-line budget risk | Medium (docs about 250 lines of the total) |
| Chained PRs recommended | No (one branch, work-unit commits) |
| Delivery strategy | single branch, no PR opened by this run |

Decision needed before apply: No
Chained PRs recommended: No
400-line budget risk: Medium

Rules: strict TDD (RED then GREEN). Gate per commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (also with `ENGRAM_PORT=8123`). Each commit is verified alone in a scratch worktree.

## 1. Proposal and specs
- [x] 1.1 Proposal, specs, design and tasks (commit `docs(openspec): propose chat-notify`)

## 2. Config key
- [ ] 2.1 RED/GREEN: `AiConfig.notify` default false, `read_notify` and its problem text
- [ ] 2.2 RED/GREEN: `ConfigEdit::is_flag` covers `ai.notify`; write in place, create, scalar replace, table collision (core and `save_config` message)

## 3. Menu row
- [ ] 3.1 RED/GREEN: `ItemKind::Notify` after Memory, toggle, edit; update the order and wrap tests
- [ ] 3.2 Regenerate affected snapshots and inspect that only the extra row differs

## 4. Decision and app wiring
- [ ] 4.1 RED/GREEN: pure `should_bell` and `Ended`
- [ ] 4.2 RED/GREEN: `window_focused`, `AppEvent::WindowFocus`, `Effect::Bell` from `on_chat` (done, error, summary, cancel, stale, duplicate, off)

## 5. Runtime
- [ ] 5.1 RED/GREEN: `Effect::Bell` flag, `stage_bell` after the draw, coalescing and ordering after OSC 52
- [ ] 5.2 RED/GREEN: focus reporting enabled at startup, `FocusGained/FocusLost` mapped, disabled in `restore_to`

## 6. Verification
- [ ] 6.1 Full gate at HEAD, plain and with `ENGRAM_PORT=8123`
- [ ] 6.2 [manual] In Kitty, with notify on, start a long answer, switch to another pane or window: the bell rings once when it finishes; looking at the chat: no bell
