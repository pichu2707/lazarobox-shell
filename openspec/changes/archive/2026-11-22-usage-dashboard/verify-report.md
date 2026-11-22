# Verify report: usage-dashboard

Date: 2026-11-22. Base: main 6c3788ce (S1a-S6 merged: PRs #827-#829, #831, #832, #834, #835, #837). Mode: hybrid, Strict TDD (`cargo test`).
Verdict: PASS WITH WARNINGS (0 CRITICAL, 2 WARNING, 2 SUGGESTION).

## Execution evidence
Run from a new detached worktree at origin/main (outside /tmp). Its `target` and the worktree were removed afterwards.

| Check | Result |
|-------|--------|
| `cargo test` | lib 12113 passed, 0 failed, 2 ignored; other targets all ok, 0 failed |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |

## Tasks
All items in S1a, S1b, S1c and S2-S6 are `[x]` with result lines. `chart_lines` is gone from `src`.

## Spec compliance
| Requirement group | Status |
|---|---|
| Number formatting (compact, tok, grouped, missing is not zero) | OK |
| Range selector (default 7d, switching, no colour) | OK |
| KPI cards (tasks, deleted chat, deltas, n/a, `~`) | OK |
| Tokens by day | OK |
| Heatmap (3-hour buckets under 60 columns) | OK |
| Models table, provider bar, local glyph | OK |
| Agents table, peak | OK (see W2) |
| Provider identity (one-column glyphs) | OK |
| Width tiers and fallback (no-panic loop 0..=120 x 0..=40) | OK |
| Empty, loading, unavailable | OK |
| Privacy, accessibility | OK |
| ai-usage: timing columns, text formats, thresholds, worker usage, Charts removed | OK |
| statusline: compact `today: N tok` | OK |
| README EN/ES state the facts, same headings in the same order | OK |

Documented deviations C1-C8 and R1-R7 in tasks.md are consistent with the code. The manual scenarios were not run live.

## Issues
WARNING:
- W1: the SQL `SUM` over the token columns in `usage_report` can overflow, and then the whole `/cost` read fails. Follow-up issue.
- W2: S5 overlap uses ceil(wall_ms/1000), so back-to-back workers can read one too many in `up to N at once`. Follow-up issue.

SUGGESTION:
- S1: the context-window warnings in chat/mod.rs still print raw token counts.
- S2: the design.md open question about dropping `chart_lines` is still unchecked, though it is implemented per the spec.
