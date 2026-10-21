# Apply progress: agent-orchestrator

## S1a: task row and keys (PR 1, branch feat/agent-orchestrator-s1a)

Status: 10 of 10 tasks done (1a.1 to 1a.10). S1b to S10 pending.

### TDD cycle evidence

| Task | Test files | RED | GREEN | Notes |
|------|-----------|-----|-------|-------|
| 1a.9 | core/ai/chat/engine/availability_tests.rs (+ insta snapshot) | passes before changes (characterization) | passes after | pins the gate-off chat path vs 0.18.0 |
| 1a.1/1a.2 | core/ai/config.rs, core/ai/picker/assign_tests.rs, core/ai/chat/prompts/tests.rs | compile errors then 2 runtime failures (Set all) | 126 config tests | `Task::SET_ALL` added; prompt table keeps 10 prompts |
| 1a.3/1a.4 | core/config_edit.rs, runtime/config_write.rs | compile error (`orchestrator_flag`) | all pass | removal edit for a cleared orchestrate slot |
| 1a.5/1a.6 | core/menu/orchestrator_tests.rs + updated menu tests, ui/components/menu.rs | compile error then count/order failures | 247 menu + ui tests | popup grows to 18 rows; 26 snapshots reviewed |
| 1a.7/1a.8 | core/ai/chat/engine/availability_tests.rs | compile error (`availability_for`) | 30 pass | chat wrapper kept |
| 1a.10 | all | n/a | fmt, clippy (0 warnings), test (5261 pass, 0 fail) | no refactor needed beyond folded cleanups |

### Deviations and decisions
- Table name: specs say `[ai.orchestrate]`; design.md and tasks.md say `[ai.tasks.orchestrate]`. Implemented `[ai.tasks.orchestrate]` (existing `ai.tasks.<id>` convention, `Task::table()`). Specs fixed to match (user decision 2026-10-21).
- `worker_timeout_secs` out of range falls back to 600 with a startup notice (spec) instead of clamping (task text).
- Static rows: spec lists Profile, Context, Memory, Tools, Orchestrator; 0.18.0 also has Approval. Kept Approval; `Orchestrator` is last. AI tab is 13 rows, MENU_BODY 14, popup 18 rows, minimum height 17.
- `core/ai/chat/availability_tests.rs` in tasks.md is really `core/ai/chat/engine/availability_tests.rs`.
- `builtin(profile, Task::Orchestrate)` is a placeholder (chat prompt) until S1d adds `orchestrate.md`.
