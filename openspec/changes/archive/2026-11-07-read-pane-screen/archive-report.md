# Archive Report: read-pane-screen

**Date**: 2026-11-07
**Artifact store**: hybrid (openspec files + engram `sdd/read-pane-screen/archive-report`)
**Branch**: `docs/openspec-closeouts-2` (no git operation run by this phase; nothing committed)
**Status**: Archived. Clean close, no gate override. Verdict PASS WITH WARNINGS, 0 CRITICAL.

## Change Summary

The chat can call `read_pane` (the `Pane` family, chat only) to read the text of another pane: the visible screen by default, or up to 200 lines and 8 KiB of scrollback, keeping the tail. Every call asks, on every engine and in every approval mode. The App reads the pane when the ask opens, freezes the exact text, shows it in a scrollable review that `y` unlocks only after every line was seen, and sends that same frozen text, redacted and wrapped as untrusted data. Denied, gone, empty and truncated outcomes each show a notice. Workers and the coordinator never receive the tool. Per the verify report, the change is already merged and released in v1.14.0 (PR1 merged as #622; the tasks file records PR2 as complete).

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Explore | `sdd/read-pane-screen/explore` | #3740 (referenced by the proposal; not retrieved by this phase) |
| Proposal | `sdd/read-pane-screen/proposal` | #3747 |
| Spec (delta set) | `sdd/read-pane-screen/spec` | #3748 |
| Design | `sdd/read-pane-screen/design` | #3749 |
| Tasks | `sdd/read-pane-screen/tasks` | #3750 |
| Apply progress | `sdd/read-pane-screen/apply-progress` | none saved |
| Verify report | `sdd/read-pane-screen/verify-report` | #3768 |
| Archive report | `sdd/read-pane-screen/archive-report` | saved by this phase (ID assigned by engram) |

The file copies in this folder are the canonical content for the files-side trail. The engram tasks observation (#3750) is an older snapshot: it says the manual PR2 checks remain for the owner, while the file records them as recorded on 2026-11-07 and all 19 tasks as `[x]`. The file was used, per the hybrid rule.

## Specs Synced

| Capability | Action | Details |
|------------|--------|---------|
| `agent-tools` | Updated | ADDED `read_pane tool` (9 scenarios), appended at the end. MODIFIED `Tool registry` (the default-list sentence now names `list_panes` and `read_pane`; scenarios unchanged). MODIFIED `Off equals 0.18.0` (names `read_pane` and the golden `tools_list.json`; scenario updated). |
| `lazarobox-mcp-server` | Updated | MODIFIED `Tools served` (`read_pane` listed in the served set, asked in every approval mode, and the two scenarios `read_pane served and asked` and `read_pane not served to a worker` added at the end). |
| `chat-pane` | Updated | ADDED `read_pane approval preview` (4 scenarios) and ADDED `read_pane notices` (5 scenarios), appended at the end. |

### Merge details

- The delta paths were read from `openspec/changes/read-pane-screen/specs/` and merged against the current `openspec/specs/` content. All three target capabilities existed, so none was created.
- No `Rebase needed` header exists in any of the three deltas. Nothing to remove.
- The `(Previously: ...)` annotations are delta-only notes. They were not carried into the main specs. They are kept verbatim in the archived copies of the deltas.
- Each MODIFIED block in the delta is the complete requirement (every main scenario is repeated), so each was applied as its exact replacement. No scenario was dropped and none was turned into an extension.
- The `chat-pane` ADDED `read_pane approval preview` text is the amended text from the delta: `y` is locked until every line is seen, then arms after the delay. The engram spec (#3748) carries the same amendment.

## Tasks

`openspec/changes/archive/2026-11-07-read-pane-screen/tasks.md`: all 19 tasks (1.1 to 1.8 and 2.1 to 2.11) are ticked `[x]`. Zero unchecked. No checkbox was changed at archive time, so no stale-checkbox reconciliation was needed.

## Verification

`verify-report.md` (#3768): "PASS WITH WARNINGS (0 CRITICAL, 2 WARNING, 1 SUGGESTION)". `cargo test` lib 9886 passed, 0 failed, 1 ignored; `tests/jail.rs` 23 passed and 2 environmental failures ("jail not enforced"); clippy and fmt clean. The verify report maps every `[auto]` scenario in the three deltas to a passing test. Manual PR1 and PR2 checks were recorded on 2026-11-07 on Ollama `qwen3:14b-nothink`.

WARNINGS (none blocks archive):
1. The cloud-engine manual variant was not run (no authenticated CLI in the isolated HOME). The tool path is engine-independent and covered by unit tests.
2. No apply-progress artifact, so Strict TDD RED/GREEN order cannot be audited retroactively. No insta snapshot test is identifiable by name for the exact-preview scenario.

SUGGESTION: re-run `tests/jail.rs` outside the sandbox for a fully green record.

## Archive Gate

- No CRITICAL issues. No override requested or used.
- All tasks complete.
- Delta specs merged into `openspec/specs/{agent-tools,lazarobox-mcp-server,chat-pane}/spec.md`, as described above.

## Deferred Follow-ups

- The cloud-engine manual check (verify WARNING 1) is open for the owner.
- Optional: re-run `tests/jail.rs` outside the sandbox (verify SUGGESTION).

## Git Operations Left To The User

No git operation was run by this phase, and this session had no shell or delete tool. The change folder was copied by writing each file to the archive path. The active folder was NOT removed. Nothing has been committed. Note that `tasks.md` and the `chat-pane` delta in the active folder were modified in the working tree and are not yet committed. The archived copies hold the same content as the files on disk at archive time.

To finish the move, the user should run after reviewing the archive:
- `git rm openspec/changes/read-pane-screen/{proposal.md,design.md,tasks.md,specs/agent-tools/spec.md,specs/chat-pane/spec.md,specs/lazarobox-mcp-server/spec.md}` (tracked files; check with `git ls-files`)
- `rm -r openspec/changes/read-pane-screen` (untracked: verify-report.md)
- `git add openspec/changes/archive/2026-11-07-read-pane-screen openspec/specs`

## Relevant Files

- `openspec/specs/agent-tools/spec.md` (merged)
- `openspec/specs/lazarobox-mcp-server/spec.md` (merged)
- `openspec/specs/chat-pane/spec.md` (merged)
- `openspec/changes/archive/2026-11-07-read-pane-screen/` (copied change folder: proposal.md, design.md, tasks.md, verify-report.md, specs/{agent-tools,chat-pane,lazarobox-mcp-server}/spec.md, and this archive-report.md)
- `openspec/changes/read-pane-screen/` (original folder, to be removed by the user, see above)
</content>
