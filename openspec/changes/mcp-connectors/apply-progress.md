# Apply progress: mcp-connectors, S9 (per-server approval level)

Mode: Strict TDD (`cargo test`). Delivery: ask-on-risk, stacked-to-main if split.

## Batch 1: S9a (U1-U4), tasks 9.1-9.12 done

| Unit | Commit | Tasks |
|---|---|---|
| U1 | 0d9f91d7 `feat(mcp): read a per-server approval level from config.toml` | 9.1, 9.2 |
| U2 | 3f2c0e66 `feat(mcp): let a server's approval level answer the ask of its tools` | 9.3-9.7 |
| U3 | b13b995c `feat(mcp): a GitHub-blocked conversation asks for every user tool at any level` | 9.8, 9.9 |
| U4 | 57ab1e39 `feat(mcp): say the approval level in the start line and mark unasked results` | 9.10-9.12 |

### TDD cycle evidence

| Task | Test file | Layer | RED | GREEN | Triangulate | Refactor |
|---|---|---|---|---|---|---|
| 9.1 | core/ai/config.rs tests | Unit | written first (type absent) | 216 config tests pass | 3 levels, 6 bad values, siblings kept | literals churn |
| 9.2 | core/config_edit.rs tests | Characterization | n/a (passes on write) | pass | valid and invalid line | none |
| 9.3 | mcp_catalog/tests.rs `unasked` | Unit table | written first | 4 pass | level x catalog x command x class | `is_catalog_server` extracted in U4 |
| 9.4 | chat/approval.rs tests | Unit | written first | pass | 3 modes x 2 levels x blocked | none |
| 9.5 | runtime/tools/host/external_tests.rs | Integration (fake MCP server) | written first | pass | all, ask, reads r/w, per server, coordinator, bad Unprompted | `routed_external` extracted |
| 9.6 | host/external_tests.rs | Integration | characterization (lock already held by `offers` and `admits`) | pass | worker; google filters at all and reads | none |
| 9.7 | app/app_tests/tool_approval.rs | App | characterization (path already denies first) | pass | pending ask, tools off, finished turn | none |
| 9.8 | app_tests/github_block.rs, host/external_tests.rs | App + Integration | written first | pass | 3 modes x 2 levels; ask level shows no reason | `BlockWhy::tool_ask` |
| 9.9 | host/external_tests.rs | Integration | characterization | pass | reads asks, all runs | none |
| 9.10 | runtime/mcp/external/tests.rs | Integration | written first | pass | ask, all, reads custom, reads catalog | none |
| 9.11 | host/external_tests.rs | Integration | written first | pass | marker at all, none at asked `y`, cap, injected text, run_in_pane | `Unprompted::Level(level)` |
| 9.12 | n/a | Docs | n/a | full suite green | n/a | doc comments |

### Notes and deviations

- D32 prompt text: the block reason of a user tool is a new `BlockWhy::tool_ask` ("calls to your servers need your y"), since `read_ask` talks about files. The reason shows only when the ask carries a level (`unasked: Some`), so the default level's prompt is unchanged.
- The audit `the_runtime_never_names_the_approval_mode` bans the identifiers `ApprovalMode`, `set_approval_mode`, `approval_mode`, `set_approval` in all non-test runtime code, including `*_tests.rs` files: the host test approver restates the rule instead of calling `answers_itself`, and U6 must not name a runtime-called function `set_approval` (use `set_approval_with` / another name).
- Test seam: `fake::declared_with` lets a fake server (command `/bin/sh`) keep the catalog's command, so `reads` can be exercised without a real catalog binary.
- Environment: `runtime::mcp::presence::tests::the_client_id_follows_the_presence_and_never_the_secret` fails under umask 002 (tempdir group-writable); passes under `umask 022`. Not caused by this change.

### Remaining: U5 (9.13-9.15), U6 (9.16-9.19), U7 (9.20-9.22)

## Review fixes (after S9a review)

- Added `an_unasked_call_is_cancelled_with_its_turn` (host/external_tests.rs): aborting the turn task while an unasked call is in flight sends nothing more and answers the row as not Ok. No bug found.
- Moved the `call_external` doc paragraph back above `call_external`; `routed_external` keeps its own doc.
- design.md D29: noted that only `command` is compared, not `args` (accepted).
