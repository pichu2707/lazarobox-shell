# Tasks: Chat Notify

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,290 measured (1,254 added, 38 removed): 287 docs, the rest code, tests and 17 snapshot lines |
| 400-line budget risk | Medium (no commit above ~350 lines; the menu commit is mostly updated tests) |
| Chained PRs recommended | No (one branch, work-unit commits) |
| Delivery strategy | single branch, no PR opened by this run |

Decision needed before apply: No
Chained PRs recommended: No
400-line budget risk: Medium

Rules: strict TDD (RED then GREEN). Gate per commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (also with `ENGRAM_PORT=8123`). Each commit is verified alone in a scratch worktree.

## 1. Proposal and specs
- [x] 1.1 Proposal, specs, design and tasks (commit `docs(openspec): propose chat-notify`)

## 2. Config key
- [x] 2.1 RED/GREEN: `AiConfig.notify` default false, `read_notify` and its problem text
- [x] 2.2 RED/GREEN: `ConfigEdit::is_flag` covers `ai.notify`; write in place, create, scalar replace, table collision (core and `save_config` message)

## 3. Menu row
- [x] 3.1 RED/GREEN: `ItemKind::Notify` after Memory, toggle, edit; update the order and wrap tests
- [x] 3.2 Regenerate affected snapshots and inspect that only the extra row differs

## 4. Decision and app wiring
- [x] 4.1 RED/GREEN: pure `should_bell` and `Ended`
- [x] 4.2 RED/GREEN: `window_focused`, `AppEvent::WindowFocus`, `Effect::Bell` from `on_chat` (done, error, summary, cancel, stale, duplicate, off)

## 5. Runtime
- [x] 5.1 RED/GREEN: `Effect::Bell` flag, `stage_bell` after the draw, coalescing and ordering after OSC 52
- [x] 5.2 RED/GREEN: focus reporting enabled at startup, `FocusGained/FocusLost` mapped, disabled in `restore_to`

## 6. Verification
- [x] 6.1 Full gate at HEAD, plain and with `ENGRAM_PORT=8123`
- [x] 6.2 [manual] In Kitty, with notify on, start a long answer, switch to another pane or window: the bell rings once when it finishes; looking at the chat: no bell (user-checked 2026-10-14)
