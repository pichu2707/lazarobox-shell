# Archive Report: panes-tabs

- Change: `panes-tabs` (Slice 2: panes and tabs), tracker `feat/panes-tabs`, verified in PR #50 (commit 22f4f68 at archive start)
- Archived: 2026-10-11, to `openspec/changes/archive/2026-10-11-panes-tabs/`
- Artifact store: hybrid (openspec + engram)
- Archive branch: `feat/panes-tabs-archive`

## Task completion gate

- `tasks.md`: 103 of 103 tasks ticked, 0 unchecked (`grep -c '^- \[ \]'` = 0). No stale-checkbox reconciliation was needed.
- `verify-report.md`: present. Verdict PASS WITH WARNINGS, 0 CRITICAL.

## Gate override (explicit user approval, 2026-10-11)

- `gentle-ai sdd-status panes-tabs` reported `blockedReasons: ["verify-report.md is not clearly passing."]`.
- Cause: the verify-report parser in gentle-ai 1.36.5 blocks on keywords (CRITICAL, PENDING, TODO, FAIL) found anywhere in the report. Here they appear as "0 CRITICAL", the mandatory `### CRITICAL` heading ("None.") and test names containing "pending". The report verdict is PASS WITH WARNINGS.
- Upstream issue: Gentleman-Programming/gentle-ai#848, closed as fixed in a later version.
- The user explicitly approved archiving despite this block on 2026-10-11.
- The override covers ONLY this verify-report block. It does not cover unchecked tasks (none exist). `verify-report.md` was not edited.

## Engram traceability

| Artifact | Observation ID |
|---|---|
| proposal | #2725 |
| spec | #2726 |
| design | #2727 |
| tasks | #2730 |
| verify-report | #2891 |
| apply-progress | #2733 (stale per W-4; superseded by tasks.md, which is authoritative) |

## Specs merged

Main spec sources of truth updated under `openspec/specs/`:

| Capability | Action | Changes |
|---|---|---|
| copy-mode-scrollback | Updated | 3 ADDED (Per-pane COPY state, Position indicator, Focus change exits COPY) |
| modal-input | Updated | 5 ADDED, 4 MODIFIED (TERMINAL mode, Prefix key, Quit confirmation, Paste and resize in any mode) |
| statusline | Updated | 4 ADDED, 1 RENAMED, 4 MODIFIED |
| terminal-emulation | Updated | 2 ADDED, 2 MODIFIED |
| terminal-session | Updated | 5 ADDED, 4 MODIFIED |
| pane-layout | Created (new capability) | Full spec copied from the delta |
| configuration | Created (new capability) | Full spec copied from the delta |
| tabs | Created (new capability) | Full spec copied from the delta |
| key-encoding | Untouched | No delta targets it |

REMOVED sections: none in any delta (all "(None.)").

## Requirement and scenario counts (requirements / scenarios)

| Capability | Before | After |
|---|---|---|
| copy-mode-scrollback | 6 / 13 | 9 / 25 |
| modal-input | 5 / 9 | 10 / 38 |
| statusline | 4 / 8 | 8 / 25 |
| terminal-emulation | 4 / 18 | 6 / 34 |
| terminal-session | 5 / 10 | 10 / 29 |
| pane-layout | new | 8 / 29 |
| configuration | new | 3 / 11 |
| tabs | new | 6 / 27 |
| key-encoding | 3 / 7 | 3 / 7 (unchanged) |

Verified per requirement: each MODIFIED and ADDED block matches its delta's scenario count, no requirement name is duplicated, and the scenario totals reconcile (for example modal-input: 1 kept + 12 modified + 25 added = 38).

## Merge decisions (non-mechanical)

- statusline RENAMED "Quit confirmation visible" to "Confirmation prompts visible". The old block had 1 scenario and was replaced by the MODIFIED body (3 scenarios), as the delta specifies.
- Requirements replaced by MODIFIED blocks lose their previous scenarios, which the delta supersedes: modal-input "TERMINAL mode" (1 to 4), "Paste and resize in any mode" (1 to 2), statusline "Live cwd" (2 to 4), terminal-emulation "Terminal view rendering" (3 to 5), terminal-session "Resize" (3 to 7) and "Shell exit and shutdown" (3 to 5).
- The engram spec summary (#2726) lists tabs as 6/25 and terminal-session as 9/26. The delta files count 27 and 28 scenarios. The file counts were used here.

## Deferred follow-ups (from design.md and verify-report.md)

Out of scope (verify-report section 8):
- The `?` command viewer. The `?` entry is reserved and `PREFIX_TREE` is its data source.
- Keymap configuration. The prefix tree stays a static table.
- Per-pane dirty tracking. A single `dirty` flag redraws everything.
- FQDN vs short hostname matching (ADR 21 follow-up). Currently rejected as remote.
- Clearing a sticky OSC 7 value after ssh or exec (ADR 21 limitation).
- The `config-menu` change, stacked on this tracker as a separate, still-open change.

Known and accepted limitations (verify-report section 6): sticky OSC 7; FQDN vs short hostname mismatch; a `chmod 000` directory passes `is_dir` and fails through `SpawnFailed`.

## Open warnings and suggestions at archive time

- W-3 (open): TDD evidence tables exist only from S9 onward. Process finding, no code impact.
- S-3 (open): test `a_binding_in_a_group_returns_to_terminal_with_no_effect_yet` keeps a stage-specific name. Rename in a later cleanup.
- S-4 (open): `drop_kills_background_jobs` is timing sensitive. Consider a deadline-based wait.
- S-5 (open): the ADR 21 FQDN follow-up could state the user-visible symptom.
- Resolved in PR #50: W-1, W-2, W-4, S-1, S-2.

## Next steps

- Review and merge the archive PR into `feat/panes-tabs`.
- `config-menu` (`feat/config-menu`) remains open and stacked on this tracker.
