# Verify report: mcp-connectors (#436)
Date: 2026-11-22 | Mode: hybrid | Executor: sdd-verify (read-only, no cargo run)

## Verdict
ARCHIVABLE. 0 CRITICAL, 4 WARNING, 2 SUGGESTION.

## Completeness
- tasks.md: 92 done, 10 open.
- All 10 open tasks are non-code: 8 [manual] live checks with real accounts or a real install,
  1 reader walkthrough (6d.5), 1 upstream issue (6b1.7).
- Real missing code or docs: none.
- The open tasks are tracked as a checklist on #436:
  https://github.com/pichu2707/lazarobox-shell/issues/436#issuecomment-7083224927

## Spec coverage (8 delta specs, 42 requirements)
| Spec | Reqs | Code on main | Tests |
|---|---|---|---|
| mcp-catalog | 11 | core/ai/mcp_catalog/{entries,gsc,checks}.rs | mcp_catalog/tests.rs, gsc/tests.rs, workspace_tests.rs |
| mcp-connect | 9 | mcp_catalog/{connect,link}.rs, runtime/mcp/{link,google_link,probe}.rs | connect/*tests.rs, link/tests.rs, probe/tests.rs |
| mcp-secrets | 7 | core/ai/mcp_secrets/{save,client,lazarobox_client,text}.rs, runtime/mcp_secrets.rs | mcp_secrets/*tests.rs |
| mcp-servers | 4 | runtime/mcp/{session,external,client,shim}.rs | runtime/mcp/*tests.rs |
| agent-tools | 4 | core/ai/tools/mcp.rs, runtime/tools/host | host/external_tests.rs |
| config-menu | 5 | config menu rows and approval level | app tests |
| configuration | 1 | core/ai/config.rs | config tests |
| config-persistence | 1 | core/config_edit.rs | config_edit tests |

Check method: rg by keyword and by module. Requirements were not traced one by one.

## Tests
Not run in this verify. apply-progress reports the full suite green, with RED/GREEN/triangulation tables per slice.

## Open items (not blocking)
- Manual-live: 4.6, 5.3, 6a2.10, 6c.9, 6b1.6, 6b1.9, 6b2.7, 9.22b
- Walkthrough: 6d.5
- Upstream: 6b1.7 (AminForou/mcp-gsc login command issue)

## WARNING
1. The behaviour of the real binaries is confirmed only by the open live checks (U items in 2.5 and 5.1).
2. Open questions 11-15 were resolved by the orchestrator. The owner's confirmation is not recorded.
3. The presence test fails under umask 002. This is the environment, not this change.
4. The test suite was not re-run here.

## SUGGESTION
- Open the 6b1.7 upstream issue once the owner agrees.
- Track the live checks on #436. Done.
