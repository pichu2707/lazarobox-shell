# Design: Run auto-apply for the chat (chat-write-grant, #432)

## Technical Approach

The user's `a` in a pane's own batch review gives the pane **auto-apply**. Later reviewable batches of that pane, in `run` mode, are answered by the app with a new host decision, `Decision::Standing`, instead of opening the review. Nothing is relaxed in staging or apply. The host accepts `Standing` only for a pane's own reviewable batch. The app learns the result from the call's `ToolResult`.

Verified on `origin/main` 5755d7e3, after #845 S1-S3:
- `ApprovalMode` is `Ask | Run` only (`src/core/ai/config.rs:185`). `auto` migrates to `ask` (`migrate_auto`, `src/runtime/config_write.rs:81`).
- `y` runs the model's command in both modes.
- The read tiers and the git gate live in `src/runtime/run_check.rs`.

None of this touches the batch path. The approval toggle is `Prefix a` (`toggle_approval`, `src/app/tool.rs:492`). It is a two-key sequence, so a bare `a` inside the review does not clash with it.

Invariants:
- Only a user key gives auto-apply.
- The runtime cannot name auto-apply and cannot construct `Standing`.
- Every apply still goes through `Stages::apply` (`src/runtime/tools/stage.rs:197`).

## Architecture Decisions

| # | Decision | Choice | Rejected | Rationale |
|---|---|---|---|---|
| D1 | Host answer | New `Decision::Standing` (`src/core/ai/chat/engine.rs:199`). `files::answered` (`src/runtime/tools/host/files.rs:854`) accepts it only when all of these hold: `scope.read_scope.is_none()` (pane), `sealed.reviewable`, and the root is not a granted drive folder (`roots.on_drive`, checked inside the apply task). A worker gets `NOT_APPROVED`, an unreviewable batch gets `TOO_LARGE`. Every other runtime `match` on `Decision` denies it. | Reuse `Sent` or `Unprompted` | `Sent` would make a forged auto-apply indistinguishable from `y`. `Unprompted` already means "the mode let it go" and is refused for batches, which must stay true. A distinct variant can be audited. |
| D2 | State | On `ChatState` (`src/core/ai/chat/mod.rs`), see "State machine" below. Two fields: `grant: AutoApply` (`Off`, `Given{turn,call}`, `On`) and `inflight: Option<Inflight{turn, call, review: Arc<BatchReview>}>`. Memory only. | One combined enum | An apply already sent must still be reported (line, `/applied`) after an end hook turns the grant off. |
| D3 | Below HOME | No app-side HOME check. `resolve_write` (`src/core/ai/tools/paths.rs:779`) already refuses a root that is HOME, outside HOME or not a real folder. The only exception is the granted drive folder, which is excluded by `drive_grant()` (`mod.rs:1222`) and by D1. A test pins this. | Store a `below_home` flag on the chat | No batch exists outside these roots, so there is one source of truth. |
| D4 | Offer of `a` | A pure `ChatState::offers_auto_apply()` returns true only when all of these hold: mode `Run`, `run_hint` (tools on), `drive_grant()` is `None`, `block_why()` is `None`, no `orch_run()`, and the pending ask is `ApprovalAsk::Batch` with `worker()` `None` and no `orphan()`. Both the key handler and the footer call it. | Cache the flag in `ReviewView` at open | `Prefix a` can change the mode while a review is open, so a cached flag goes stale. |
| D5 | `a` and Enter | In `on_review_key` (`src/app/review.rs:184`), `a` and Enter join `answers`. `approve_batch(id, size, give)` (`:263`): `a` sets `Given(turn,call)` and answers `Decision::Sent`. Enter is the same as `y`, but only for `ApprovalAsk::Batch` (pane, worker, orphan). A commit or screen review sharing the view keeps Enter swallowed. A held key (`repeat`) never approves. A refused gate shows the `y_pressed` hint. An `a` that is not offered gives a visible notice: `a works only for this chat's own batch in run mode`. | Enter for every review view | Decision 1 speaks of batches. A commit is another approval. |
| D6 | Route | In `on_tool_ask` (`src/app/tool.rs:189`), right after the `answers_itself` block (`:238-253`): `ApprovalAsk::Batch(r)` goes to `try_auto_apply`. It needs all of these: `grant == On`, mode `Run`, `r.worker()` is `None`, no pending approval, no `inflight`, no drive grant, no block, no orchestration run, `doc.reviewable()`, no build/env-template/coarse row, and no `review_always` match (PR3). If any fails, fall through to `open_prompt` (`:377`). A pending approval is then refused by `open` (`src/core/ai/chat/approval.rs:1327`, `ANOTHER_PENDING`), never queued, because only worker asks queue. | A separate route function in core | The pending-approval refusal comes for free and matches the comment on `auto_route` (`tool.rs:453-457`). |
| D7 | Result | `ToolEvent::Over` (`tool.rs:66`, built `:77`) gains the result `summary` (the first line, already built by `answered`). The handler at `:146` does this: <br>- `Given` with an `Ok` result on the same (turn, call) sets `On` and adds the line `Auto-apply is on for this conversation`. Any other status sets `Off` with no line. <br>- `inflight` with an `Ok` result adds the applied line and keeps the document (PR3). Any other status ends the grant with `Auto-apply is off because the batch did not apply: <summary>`. | Trust `Sent` | Only the host sees `ApplyResult`. |
| D8 | Fail closed | `ChatState::auto_apply_turn_over(turn)` runs from the `finished` branch of `on_tool_event` (`tool.rs:129`, after the event match) and from `ChatState::cancel()` (called by `chat_cancel_key`, `src/app/chat.rs:741`). If a `Given` or `inflight` call of that turn has no result, the grant becomes `Off` with `Auto-apply is off because the turn ended before the batch's result`. The `inflight` document is kept as "result unknown". Pane close and quit drop `ChatState`. | Wait for a late result | The app cannot know what applied. |
| D9 | Guard | `audit.rs`: a substring rule next to `MODE_NAMES` (`src/runtime/tools/audit.rs:436`) flags any runtime identifier that contains `auto_apply` or `AutoApply`. `the_runtime_never_names_the_approval_mode` (`:921`) covers it. | Exact names only | Covers every accessor name (`end_auto_apply`, `offers_auto_apply`...) without listing each one. |
| D10 | Construction audit | New `standing_violations(src)`. In the non-test code of `src/runtime.rs` and `src/runtime/`, the tokens `Decision :: Standing` may appear only in pattern position (followed by `=>`, `|` or `if`). A bare `Standing`, `Self::Standing`, `Decision::*`, `Decision::{` or `as` on `Decision` is reported. `src/core` may name it only in `engine.rs`. | A type-level capability token | A lexical rule is what the repository already uses (`non_test_tokens`), and it catches a rename or a glob import. |
| D11 | `review_always` | `[ai.tools] review_always: Vec<String>`, compiled once with `core::ai::tools::glob::Glob`, plus a new case-insensitive constructor (as `is_build_file`). The pattern matches the row path relative to the chat folder. A bad pattern becomes a config problem `ai.tools.review_always: "<p>": <GlobError::message>` and is skipped. | A new glob syntax | Same syntax as the model's `glob` tool, so one parser. |
| D12 | Badge | `draw_mode_badge` (`src/ui/components/chat.rs:300-313`) order: ` RUN OFF ` (blocked) wins, then ` RUN+A ` red (short `R`, below 7 columns), then ` RUN `. | ` RUN+A ` over ` RUN OFF ` | A blocked chat never auto-applies (D6). |
| D13 | `/applied` | `Command::Applied` (parser in `src/core/ai/chat/folder.rs:96-105`). It opens a list of `ChatState.applied: Vec<Applied{line, review}>`, newest first. Enter opens a read-only `ReviewView`: no gate, and `y`, `a` and Enter only give a notice. Esc goes back. Dropped with the conversation. | Store in `chat.db` | Decision 3: memory only. |

### State machine (`grant`)

```
Off --a (offered, gate open) --> Given(t,c)        answer Sent
Given(t,c) --Over(t,c,Ok)--> On                    line "on"
Given --Over(non-Ok) | turn over | end hook--> Off
On --eligible batch--> On + inflight(t,c,review)   answer Standing
inflight --Over(Ok)--> line + keep doc
inflight --Over(non-Ok)--> grant Off, line with summary
inflight --turn over/cancel--> grant Off, doc kept "result unknown"
any --end hook--> Off   line "Auto-apply is off because <why>" (only if not Off)
```

**End hooks.** One function, `App::end_auto_apply(id, why)`. It is never called from `end_grants` (`src/app/write.rs:99`). The table lists each event and where it is caught.

| Event | Where it is caught |
|---|---|
| New conversation | `ChatState::set_approval_mode` (`mod.rs:1140`) clears the grant itself, through `history.rs:160` |
| Resumed conversation | same, through `history.rs:575` |
| Pane opens | same, through `history.rs:40` (the grant is `Off` there anyway) |
| Mode toggle | same, through `tool.rs:505` |
| Folder change (`/cd`, restore) | `change_folder` (`src/app/folder.rs:83`) |
| Drive grant set | `folder.rs:454` |
| Tools off | `sync_tools` (`src/app/run.rs:61`) |
| Saved Write toggle changed | the toggle loop at `src/app.rs:2659-2663` (Write only) |
| `/write` grant ends | Has no site of its own: `set_tool_granted(_, false)` is reached only through `end_grants`, and its callers are folder change, tools off, the saved toggle and resume, all covered above |

**Applied line.** `applied without review: src/lib.rs (modify +3 -1), docs/a.md (create +10 -0)`.

## Data Flow

```
propose_batch ─▶ host seals ─▶ ToolCall{Batch} ─▶ on_tool_ask
   answers_itself? ─▶ try_auto_apply ─ eligible ─▶ ToolDecision(Standing) ─▶ answered (pane+reviewable+!drive) ─▶ apply
                                   └ not ─▶ open_prompt ─▶ review ─ y/Enter ─▶ Sent ; a ─▶ Given + Sent
ToolResult{status,summary} ─▶ Over ─▶ Given→On | inflight line/doc | grant Off
```

## File Changes and Review Workload (production lines; tests, fixtures, snapshots and docs excluded)

| PR | Files | ~Lines | 400 budget |
|---|---|---|---|
| PR1 | `engine.rs` (variant); `runtime/tools/host/files.rs` (D1); Standing arms in `runtime/tools/host.rs` and `core/ai/chat/approval.rs`; `core/ai/chat/review.rs` (accessors in `build_with` `:109-144`: `has_build_row`, `has_env_template_row` via `is_env_template` `paths.rs:123`, `has_coarse_row`, `entries()` returning path, kind, +, -); `core/ai/chat/mod.rs` (D2, D8, `set_approval_mode`); `app/tool.rs` (D6, D7); `app/{folder,run}.rs`, `app.rs`, new `app/auto_apply.rs` (end hook, lines); `runtime/tools/audit.rs` (D9, D10) | 320 | OK (tasks.md said ~230) |
| PR2 | `app/review.rs` (D5); `core/ai/chat/mod.rs` (D4); `ui/components/review.rs` (footer `:28-33` with `a`, Enter, Esc and Ctrl+C); `ui/components/chat.rs` (D12) | 110 | OK |
| PR3 | `core/ai/chat/folder.rs` (`/applied`); `core/ai/chat/mod.rs` (history); new `app/applied.rs` and `ui/components/applied.rs` (list, read-only view); `core/ai/config.rs` (`review_always`, problem); `core/ai/tools/glob.rs` (case-insensitive); `app/tool.rs` (match) | 240 | OK |

Delivery: `auto-chain`, `stacked-to-main`. PR1 is safe alone, because nothing gives the grant before PR2. Docs: `README.md` and `README.es.md` together (AGENTS.md), CHANGELOG, GIF.

## Testing Strategy (Strict TDD)

| Layer | What | How |
|---|---|---|
| Unit | `answered` decision table; accessors per build class, env template, coarse (`build_with`), plain file; state machine and every end hook; `offers_auto_apply` table; audit samples (D9, D10); glob config problems | Next to each module |
| App | Routing table of tasks 1.6 (rows only for `ask` and `run`); `a`, Enter, gate and delay (fake clock); Over Ok, Error and Denied; cancel and turn end; regression scenarios for caps, protected folders and secrets | `app/app_tests/auto_apply.rs` |
| Render | ` RUN+A `, ` RUN OFF ` precedence, 6 columns; footer variants; `/applied` | TestBackend + insta |

## Migration / Rollout

No data migration. `review_always` is optional. Reverting PR2 leaves the grant unreachable.

## Stale spec and task lines (post-#845)

- tasks 1.6: "`ask`/`auto` modes still open the review". Now `ask` only.
- tasks 2.1: "not offered in `ask`, `auto`, ...". Drop `auto`. Refs `tool.rs:71,140` are now `:77,:146`.
- tasks 1.4: refs that moved:
  - `app.rs:2573` → `:2659-2663`
  - `mod.rs:1093` → `:1140`
  - `tool.rs:528` → `:505`
  - "`write.rs:99` where a Write grant ends": see the end-hooks table above.
- tasks 1.7: `tool.rs:183` → `:189`; `:238` → `:238-253`.
- tasks 1.8: `tool.rs:140` → `:146`.
- tasks 1.9 (`paths.rs`):
  - `WRITE_DENIED_DIRS :669` → `:689`; it now also holds `.lzr`, so add a `.lzr` row.
  - `resolve_write :759` → `:779`
  - `judge_write :476` → `:495`
  - `judge_real :463` → `:482`
  - `check_path :200` → `:219`
  - `judge_rest :187` → `:206`
- tasks 2.3: README only. It must say `README.md` and `README.es.md`.
- tasks 2.2: the badge must respect ` RUN OFF ` (`chat.rs:304-305`).
- proposal refs that moved:
  - "Out of Scope: `ask` and `auto` modes"
  - `approval.rs:1220` → `:1327`
  - `tool.rs:445-447` → `:453-457`
  - `mod.rs:1156` → `:1222`
  - `paths.rs:721-740` → `:741-760`
- specs/agent-tools: the base (99106097) and the changed text still have `ask`, `auto`, `run`, the toggle cycle with `auto`, exception (1) "in `auto` it is typed", ` AUTO `, and the `auto` scenarios. Main's spec is not yet synced with #845 either (`openspec/specs/agent-tools/spec.md:236,339`). Rebase on main after chat-runs-commands archives (task 3.5).
- specs/chat-pane: the base cites `:824-850` (now `:836`). It still has ` AUTO ` and the old `run` system line, and lacks ` RUN OFF `. Rebase.
- specs/code-write: scenario "a is not offered elsewhere" says "one in `auto` mode". Drop it.
- specs/agent-tools, "Closing the pane or quitting": "nothing is written after the close" cannot hold for a `Standing` already handed to the host (the same is true of `y` today). Reword to "no further batch is applied".

## Open Questions

- [x] Should the applied line cap its paths for a batch of up to 50 entries? Answered 2026-11-28: yes. The line names the first 10 paths, then `+N more`; `/applied` keeps the full list.
- [x] Does Enter approve only batch reviews (pane, worker, orphan) and not the `git_commit` or screen-read reviews that share the view? Answered 2026-11-28: yes, batch reviews only. `git_commit` and screen reads still need `y`.
