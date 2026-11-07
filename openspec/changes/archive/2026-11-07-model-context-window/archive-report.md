# Archive Report: model-context-window

**Date**: 2026-11-07
**Artifact store**: hybrid (openspec files + engram `sdd/model-context-window/archive-report`)
**Branch**: `docs/openspec-closeouts-2` (no git operation run by this phase; nothing committed)
**Status**: Archived. Clean close, no gate override. Verdict PASS WITH WARNINGS, 0 CRITICAL.

## Change Summary

Ollama local models get their real context window at detection (`context_length` from the same `POST /api/show` reply detection already sends, kept in RAM and keyed by digest). The request sends a stable `options.num_ctx = min(model max, num_ctx_cap)` (cap default 16384) and `keep_alive` (`"30m"` unset, the released `[ai.servers.ollama] keep_alive` when set), so Ollama does not reload between turns. The history budget subtracts the system prompt and the tool schemas on every local path, including unknown windows (12,000 characters with a 4,000-character history floor). Overflow trims whole user+assistant pairs and shows a visible notice; an unknown Ollama window shows a statusline notice once per model. chat.db is untouched. The code is already on main and released.

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Explore | `sdd/model-context-window/explore` | none saved |
| Proposal | `sdd/model-context-window/proposal` | #3611 |
| Spec (delta set) | `sdd/model-context-window/spec` | #3612 |
| Design | `sdd/model-context-window/design` | #3613 |
| Tasks | `sdd/model-context-window/tasks` | #3614 |
| Apply progress | `sdd/model-context-window/apply-progress` | none saved |
| Verify report | `sdd/model-context-window/verify-report` | #3769 |
| Archive report | `sdd/model-context-window/archive-report` | saved by this phase (ID assigned by engram) |

The file copies in this folder are the canonical content for the files-side trail. The engram copies were read for traceability and differ from the files in revisions (for example, engram tasks #3614 is the pre-review summary of 8 phases and 6 units; the file has 22 tasks, all `[x]`, plus the W1 to W6 review fixes and owner decisions). The file versions were used, per the hybrid rule.

## Specs Synced

| Capability | Action | Details |
|------------|--------|---------|
| `chat-engines` | Updated | MODIFIED `Loopback-only streaming client` (body paragraph extended with Ollama `options.num_ctx` and `keep_alive`; scenarios `Body content` and `Body carries the context block only inside the system prompt` updated; nothing removed). MODIFIED `Context window` (paragraph replaced: three-part budget, known and unknown windows, head subtraction, #556 MCP weight; scenario `Character cap for a server` replaced by `...with an unknown window`; 4 scenarios added: `Known window replaces the fixed cap`, `Head counted even when the window is unknown`, `Unknown window keeps a history floor`, `Over-window notice once per streak`, plus `Unknown Ollama window is visible`, 5 in total). ADDED `Ollama context and keep-alive options` (6 scenarios). ADDED `Overflow trims in blocks with a visible notice` (3 scenarios). |
| `ai-engine-detection` | Updated | ADDED `Ollama context window from the existing show probe` (8 scenarios). |
| `configuration` | Updated | MODIFIED `Prompt-cache options of the local servers` (paragraph: unset keep-alive now sends `"30m"`; scenario `Unset` replaced by `Unset keep-alive sends 30m to Ollama`, plus `Explicit keep-alive replaces the default` and `A whole number of seconds is sent as a duration`; `Each to its server` kept). ADDED `Ollama context cap` (4 scenarios). |
| `chat-pane` | Updated | MODIFIED `Context truncation marker` (paragraph: the marker also fires on the real-window and character-cap trims; scenario `Window trim shows the marker` added; the three existing scenarios kept). |

### Merge details

- The delta paths were read from `openspec/changes/model-context-window/specs/` and merged against the current `openspec/specs/` content. No target folder was missing, so none was created.
- No `Rebase needed` header exists in any of the four deltas. Nothing to remove.
- The `(Previously: ...)` annotations are delta-only notes. They were not carried into the main specs. They are kept verbatim in the archived copies of the deltas.
- `keep_alive_minutes` does not appear in any merged text. The configuration delta says there is no separate keep-alive key, which matches the merge with main 1.4.0 in tasks.md.
- The `Context window` scenario `Character cap for a server` (10 messages, total at most 12,000) would contradict the head subtraction on an unknown window, so it was replaced by its modified version rather than kept beside it. This follows the delta, which states the scenario as modified.
- The `Context window` delta still reads: "Whether a small reserve for the model's output is also subtracted is a design decision." The design resolved it (the existing 75% `PROMPT_SHARE` is the reserve for a known window; none for an unknown one). The text was merged as the delta wrote it. It is left as an open sentence in the main spec, and the owner may want it rewritten as normative.
- W4 (external MCP tool schemas) is already reflected in the merged `Context window` text, through the `(#556)` sentence. Per the owner, it was fixed by #556 (commit 2c5a676d).
- W5 (orchestra worker prompts outside the budget) is an accepted owner decision recorded in tasks.md and verify-report.md. It is not in any spec text, so nothing was merged for it.

## Tasks

`openspec/changes/archive/2026-11-07-model-context-window/tasks.md`: all 22 tasks (0.1, 1.1 to 1.2, 2.1 to 2.5, 3.1 to 3.3, 4.1 to 4.3, 5.1 to 5.4, 6.1 to 6.2, 7.1 to 7.2) are ticked `[x]`. Zero unchecked. No checkbox was changed at archive time, so no stale-checkbox reconciliation was needed.

One inconsistency is inside tasks.md: the "Merge with main" section says task 7.2 "stays open for the owner", while the 7.2 checkbox is `[x]` and records the manual run on 2026-11-07. The checkbox and the verify report agree (7.2 recorded), so the checkbox was taken as the state. The text was left as written.

## Verification

`verify-report.md` (#3769): "PASS WITH WARNINGS (0 CRITICAL, 3 WARNING, 1 SUGGESTION)". `cargo test` lib 9886 passed; `tests/jail.rs` 2 environmental failures ("jail not enforced"); clippy and fmt clean.

WARNINGS (none blocks archive):
1. W4 and W5 accepted gaps (external MCP schemas, orchestra worker prompts). W4 fixed by #556 (commit 2c5a676d). W5 is an accepted owner decision. The verify report asks the coordinator to confirm the follow-up issue exists; this phase could not check that (no GitHub access from this phase).
2. No apply-progress artifact; Strict TDD RED/GREEN order cannot be audited.
3. Manual 7.2 was run on one Ollama model (`qwen3:14b-nothink`) only.

SUGGESTION: an explicit test pinning chat.db `user_version` 3 and mode 0600 across detection plus a chat turn.

## Archive Gate

- No CRITICAL issues. No override requested or used.
- All tasks complete.
- Delta specs merged into `openspec/specs/{chat-engines,ai-engine-detection,configuration,chat-pane}/spec.md`, as described above.

## Deferred Follow-ups

- Owner decision on the open sentence about the output reserve in `chat-engines` `Context window` (see Merge details).
- Confirm that the follow-up issue for W4 and W5 exists (verify-report WARNING 1).
- Manual 7.2 on a second Ollama model is optional; the scope accepted one model (verify WARNING 3).

## Git Operations Left To The User

No git operation was run by this phase, and this session had no shell or delete tool. The change folder was copied by writing each file to the archive path. The active folder was NOT removed. Nothing has been committed. Note that `tasks.md` in the active folder is modified in the working tree and not yet committed. The archived copy holds that modified version.

To finish the move, the user should run after reviewing the archive:
- `git rm openspec/changes/model-context-window/{proposal.md,design.md,tasks.md,specs/chat-engines/spec.md,specs/ai-engine-detection/spec.md,specs/configuration/spec.md,specs/chat-pane/spec.md}` (tracked files; check with `git ls-files`)
- `rm -r openspec/changes/model-context-window` (untracked: verify-report.md)
- `git add openspec/changes/archive/2026-11-07-model-context-window openspec/specs`

## Relevant Files

- `openspec/specs/chat-engines/spec.md` (merged)
- `openspec/specs/ai-engine-detection/spec.md` (merged)
- `openspec/specs/configuration/spec.md` (merged)
- `openspec/specs/chat-pane/spec.md` (merged)
- `openspec/changes/archive/2026-11-07-model-context-window/` (copied change folder: proposal.md, design.md, tasks.md, verify-report.md, specs/{chat-engines,ai-engine-detection,configuration,chat-pane}/spec.md, and this archive-report.md)
- `openspec/changes/model-context-window/` (original folder, to be removed by the user, see above)
</content>
