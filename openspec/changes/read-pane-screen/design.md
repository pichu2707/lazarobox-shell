# Design: Read Pane Screen (`read_pane` for the chat)

Evidence: **[R]** read in code for this design; **[I]** inferred. Proposal decisions 1-7 and the owner settlements of 2026-11-07 (proposal, "Settled assumptions") are binding.

## Technical Approach

The model's call raises an ask with no text. The App, which owns the pane emulators on the UI loop [R `app/run.rs:428` reads `pty.emu`], reads the pane when the ask arrives, builds the exact text with a pure core builder, freezes it, and opens the existing batch review view (scroll and coverage gate) over it. `y` sends the frozen text back to the waiting call as a new `Answer::Screen`, the way `Answer::Ran` carries a run report [R `engine.rs:226`, `approver.rs:164`]. The host wraps it with `label::wrap`. No roster snapshot, no persistence.

## Key Decisions

| # | Topic | Choice | Rejected, and why |
|---|---|---|---|
| D1 | Who reads | App, at `on_tool_ask` [R `app/tool.rs:139`]. A chat's own ask never queues (only worker asks do [R `approval.rs:271`]), so arrival is prompt open | Host or roster snapshot: secrets in shared memory, stale text |
| D2 | Target | `pane` is the stable `PaneId::number()` that `list_panes` prints [R `runtime.rs:1490`, `layout.rs:13`], never reused. Resolved among App panes with `pty()`; a chat pane has no PTY, so the chat's own pane is never readable | Tab position: changes with layout, could hit another pane |
| D3 | Ask on every engine | `ask_for(ReadPane, _)` returns `ApprovalAsk::Screen` for every locality; host sets it unconditionally. `answers_itself` returns `None` for it (falls through the Read/Search match [R `approval.rs:837-847`]); `route` only routes `Run` [R `:755`]. Pinned by tests over every mode | A grant or toggle: owner decision 3 |
| D4 | Preview | Reuse the review view, following `CommitReview` [R `review/commit.rs`]: `ScreenReview { text, review: Arc<BatchReview> }`; doc = Summary line, Warning lines (cut, no scrollback), Context lines of the text. `BatchReview` gains an optional `summary` that `counts()` returns, so the header reads `pane 3 screen, 24 lines` instead of `1 file, +0 -0`. `y` arms only after every line was seen and `ARM_DELAY` | Enlarging the 6-row prompt [R `approval_rows.rs:15`]: `fits` refuses `y` when text is hidden, an 8 KiB screen never fits. New view: duplicate of a tested gate |
| D5 | Exact preview | Rows come from `escape(line)`; after `clean` there is nothing left to escape (tabs never reach a vt100 grid). A test pins `escape(l) == l` for builder output, and `wrap(nonce, text)` contains `text` verbatim (`clean` is idempotent, 8 KiB < `OUTPUT_CAP`) | Rendering the wrapped text: tags would differ from what the user reads |
| D6 | Pipeline | `read_lines` (visible span, plus scrollback) -> `copy::join` (re-joins soft-wrapped rows so a split assignment is redacted) -> `clean` -> `memory::redact::redact` -> tail cap | Cap before redact: a cut could split a secret pattern |
| D7 | Caps | `scrollback` arg `ArgKind::Int { min: 1, max: 200 }` (parse refuses more, like `read_file` lines). Total text <= 8 KiB always, visible screen included; keep the tail on a line boundary; first line `[earlier lines cut: kept the last N lines]`, counted inside the cap | Head kept: errors are at the bottom. Silent clamp of 500: hides the limit |
| D8 | Answer channel | `Answer::Screen(ScreenText)`; `ScreenText` is a newtype whose `Debug` prints bytes only. New `Effect::ToolScreen { pane, turn, call, text }`; runtime claims the wait (`Waits::take`) and sends it | Shared slot inside the ask: hidden mutable state across threads |
| D9 | Host answers | `(Answer::Screen(t), ReadPane)` -> `ended(Ok, wrap(nonce, t), "N lines from pane P")`. `Answer::Screen` for any other call, and `Decided(Sent | Unprompted | Typed)` for `ReadPane`, are denied `NOT_APPROVED`: only the frozen text can answer | Treating `Sent` as approval: a mode answer could release a read |
| D10 | After freeze | Pane closes or changes: the approved frozen text is still sent; nothing is re-read | Re-read on `y`: preview != sent |
| D11 | Workers | `Family::Pane` is absent from every worker toolset and `Toolset::Orchestrator`; `scope.offers` refuses [R `host.rs:602`]. `files::gate` line for `ListPanes` extends to `ReadPane` [R `host/files.rs:132`]. Tests assert both | - |
| D12 | Future `page` | `Call::ReadPane { pane, scrollback: Option<u32> }`; a later optional `page` adds one field, builder already returns line counts | - |

## Data Flow

    model -> host.execute(ReadPane) -> ApprovalAsk::Screen{pane,scrollback,frozen:None}
          -> App.on_tool_ask: resolve pane -> read_lines -> screen::build
               gone / empty -> deny + notice (no prompt)
               else freeze ScreenReview -> request_call + open_review
          -> y (all seen) -> Effect::ToolScreen(text) -> Waits -> Answer::Screen
          -> host: label::wrap(nonce, text) -> model
          -> n / Esc -> Denied(DECLINED) + system line "screen read denied: pane N"

## Notices (never silent)

| Case | User sees | Model gets |
|---|---|---|
| Denied | system line | `denied: ...` |
| Pane gone / not listed | notice `pane N is not open` | `denied: the pane is not open` |
| Empty screen | notice | `denied: the pane's screen is empty` |
| Truncated | Warning line in preview | marker line in text |
| Scrollback on alternate screen | Warning line `full-screen app: visible screen only` | same note as first line |

## File Changes

| File | PR | Action |
|---|---|---|
| `src/core/ai/tools/screen.rs` | 1 | Create: `build(rows, alt, scrollback_asked) -> Option<Screen>`, caps, markers |
| `src/core/pane.rs` | 1 | Add `screen_span(scrollback) -> LineSpan` |
| `src/core/ai/chat/review/screen.rs`, `review.rs` | 1 | `ScreenReview`; `BatchReview.summary` |
| `src/core/ai/tools/mod.rs` | 2 | `Call::ReadPane`, ToolDef, parse |
| `src/core/ai/chat/approval.rs`, `engine.rs` | 2 | `ApprovalAsk::Screen`, `ask_for`, `arm_delay`; `Answer::Screen`, `ScreenText` |
| `src/runtime/tools/host.rs`, `fs.rs`, `host/files.rs` | 2 | Ask, answer arms, `read_pane` |
| `src/app.rs`, `app/tool.rs`, `app/review.rs`, `app/run.rs`, `runtime.rs`, `runtime/chat.rs`, `host/review.rs` | 2 | Read/freeze, notices, approve/reject branch, effect delivery, exhaustive arms |
| `golden/tools_list.json`, `README.md` | 2 | Fixture; screen-content section |

## Testing Strategy (strict TDD, `cargo test`)

| Layer | What |
|---|---|
| Unit `screen.rs` | visible only; scrollback 1..200; 8 KiB tail with marker inside cap; wrapped assignment redacted; empty -> `None`; alt + scrollback note; `escape(l)==l` |
| Unit approval | `ask_for` asks on Local and Cloud; `answers_itself` `None` in ask/auto/run, blocked or not |
| Unit registry | `read_pane` in chat list, absent for every worker and Orchestrator |
| Host | Screen answer wrapped; `Sent`/`Unprompted` denied; Screen on other calls denied |
| App | freeze equals sent after pane writes and after pane close; gone/empty notices; `y` locked until seen |
| Golden | fixture regenerated |

## Delivery (stacked-to-main, 400 production lines, tests excluded)

| PR | Content | Estimate |
|---|---|---|
| 1 | builder + span + `ScreenReview` + summary; not offered, tool list byte-identical | ~190 |
| 2 | everything else | ~320 (README ~25) |

Revised from ~250: the App and runtime wiring is ~120. Fallback if tasks forecast PR 2 over 380: README to PR 3.

## Migration / Rollout

No migration. One revert per PR.

## Open Questions

None blocking.
