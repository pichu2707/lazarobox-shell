# Tasks: Read Pane Screen (`read_pane`)

Inputs: proposal.md, design.md (D1-D12), delta specs. Store `hybrid`; Strict TDD (`cargo test`); order inside a PR is RED -> GREEN -> REFACTOR. Budget counts production only (prompt and README prose count). Each PR passes `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | PR1 ~190, PR2 ~320 (incl. README ~25) |
| 400-line budget risk | Medium (PR2; fallback below) |
| Chained PRs recommended | Yes |
| Suggested split | PR 1 (inert core) -> PR 2 (wiring) |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main |

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Medium

Fallback: if PR2 forecasts above 380 at apply time, move README to PR3.

### Suggested Work Units

| Unit | Goal | Likely PR | Notes |
|------|------|-----------|-------|
| 1 | Builder, `Pane::screen_span`, `ScreenReview`, review summary; tool NOT offered | PR 1 | base `main`; tool list byte-identical |
| 2 | Tool registered, ask/answer, App freeze + notices, `Effect::ToolScreen`, README | PR 2 | base `main` after PR 1 merges |

## PR 1: inert core

- [x] 1.1 RED `src/core/pane.rs`: `screen_span` returns visible rows, visible plus N scrollback, and visible only on the alternate screen (flag set).
- [x] 1.2 GREEN `Pane::screen_span` (D6, alt note).
- [x] 1.3 RED `src/core/ai/tools/screen.rs`: builder tests: visible; scrollback tail; 8 KiB tail with "[earlier lines cut: kept the last N lines]" inside cap (D7); wrapped `KEY=secret` split across soft wraps redacted; blank screen -> `None`; alt-screen note line; `escape(l)==l` and `wrap(nonce,text)` contains text verbatim (D5).
- [x] 1.4 GREEN new `screen.rs`: `read_lines -> copy::join -> clean -> redact -> tail cap` (cap after redact).
- [x] 1.5 RED `chat/review/screen.rs`: `ScreenReview { text, review }` doc = Summary + Warning (cut / no scrollback) + Context; header "pane 3 screen, 24 lines"; `y` locked until all lines seen, then ARM_DELAY; rows equal `escape(line)`.
- [x] 1.6 GREEN `ScreenReview` (CommitReview pattern) plus optional `summary` in `BatchReview::counts()` (`review.rs`).
- [x] 1.7 GUARD: golden `tools_list.json` and default tool list unchanged.
- [x] 1.8 REFACTOR; module wiring only.
- Manual PR1: none visible; confirm app behaves exactly as before.
  - Recorded 2026-11-07: nothing visible changed (PR2 checks below ran on top of it).

## PR 2: wiring

- [x] 2.1 RED `tools/mod.rs`: `Call::ReadPane { pane, scrollback: Option<u32> }` parse; refuses missing `pane`, `pane:"2"`, `scrollback:"x"`, extra key, `scrollback` >200 (D7, D12). GREEN `ToolDef` + parse; Pane family, in default list.
- [x] 2.2 RED `approval.rs`: `ask_for(ReadPane)` -> `ApprovalAsk::Screen` on Local and Cloud in `ask`/`auto`/`run`; `answers_itself` None in all modes (D3). GREEN.
- [x] 2.3 RED `engine.rs`: `Answer::Screen(ScreenText)`, bytes-only Debug. GREEN, plus `approver.rs` arm.
- [x] 2.4 RED host: `(Screen, ReadPane)` -> ok `wrap(nonce,t)` "N lines from pane P"; Screen on other calls and `Sent/Unprompted/Typed` on ReadPane -> `NOT_APPROVED` (D9). GREEN `runtime/tools/host.rs`, `fs.rs`.
- [x] 2.5 RED registry: `read_pane` absent for Worker/Orchestrator/`Toolset::Code`; forged call refused unasked; `files::gate` ListPanes line extends to ReadPane (D11). GREEN `host/files.rs`.
- [x] 2.6 RED App `app/tool.rs` `on_tool_ask`: reads at ask, freezes, opens `ScreenReview`; frozen == sent after pane writes and after close (D10); own pane (no PTY) and unknown pane -> denied "the pane is not open", no prompt (D2). GREEN.
- [x] 2.7 RED notices: denied/timeout "screen read denied: pane N"; empty -> notice, no prompt, result "the pane's screen is empty"; truncated Warning; alt-screen Warning "full-screen app: visible screen only"; `y` locked until seen; `n`/Esc/Ctrl+C deny, composer kept. GREEN `app/review.rs`, `app/run.rs`.
- [x] 2.8 RED/GREEN `Effect::ToolScreen { pane, turn, call, text }`: runtime claims wait (`Waits::take`) and sends; add exhaustive arms in `runtime.rs`, `runtime/chat.rs`, `host/review.rs`.
- [x] 2.9 Regenerate golden `tools_list.json` (holds `read_pane`); update the "Off equals 0.18.0" pin.
- [x] 2.10 README: `read_pane` section (~25 lines). Fallback: PR 3.
- [x] 2.11 REFACTOR; run `cargo test`, clippy, fmt.
- Manual PR2: Ollama and one cloud engine, `ask`/`auto`/`run`: model calls `read_pane` on another pane; review opens; `y` locked until scrolled to end; secrets redacted; `n` denies with a notice; closed pane, own pane, blank pane and `vim` with scrollback each show the notice; worker/orchestrator never see the tool.
  - Recorded 2026-11-07 on Ollama `qwen3:14b-nothink`, v1.14.0 debug build, isolated HOME: all seven checks PASS in ask, auto and run (review opens, `y` locked until `seen 150/150`; `ghp_` token split by a wrap redacted; `n`/Esc/Ctrl+C deny with `screen read denied: pane 1` and keep the draft; closed pane, chat's own number and blank pane give a line and no review; `vim` scrollback shows `full-screen app: visible screen only`; frozen text reached the model (TICK2 while the pane showed TICK10); a worker reported no `read_pane`). Cloud-engine variant not run (no authenticated CLI in the isolated HOME); the tool path is engine-independent and covered by unit tests.
