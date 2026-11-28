# Proposal: Run auto-apply for the chat

Status: proposal, owner decisions recorded (2026-11-27), not started. Refs: #432. Finishes the write part (S4) of `chat-auto-permissions`. That feature is tracked in the owner's `odd/tasks/chat-auto-permissions.md`, not in git (`mcp-connectors` cites it the same way), and its other parts (search, in-root reads, pane commands) are merged (#386, #387).

Name: the folder keeps its name, `chat-write-grant`, but the concept is called **auto-apply** everywhere in this change, so it is not mistaken for the `/write` grant (`/write` and `/exec` turn a tool family on for a conversation, `src/app/write.rs`; they are unrelated and unchanged).

## Intent

In `run` mode the chat already searches, reads inside its folder and runs safe pane commands on its own, but every write still opens the batch review and waits for `y`. The owner asked:

> "los de escritura con permiso y si se lo dan pueda escribir directamente no tener que ir haciendo cambios, o bien si no le dan el permiso total que lo deje listo para que den al intro"

So a write asks; when the user accepts it with the auto-apply key, later writes of the same conversation apply directly; without it, the write stays ready for the user to confirm (today's review). Decided by the owner: auto-apply lasts for the current conversation only (a new chat or a resumed saved chat asks again), and it is given in the review of the first batch, not by a separate command.

**For:** users who let the chat work on a project in `run` mode and do not want to approve every batch.

## Scope

### In Scope
- **Key in the review:** in a plain chat in `run` mode, on a folder under HOME, the batch review offers a second approving key, `a`: apply this batch and auto-apply the next ones of this conversation. `y` keeps applying only this batch. `a` obeys the same coverage gate and arm delay as `y`. Auto-apply is held only once that first batch has applied in full (see "How the app learns that a batch applied"): a refused or partial apply (drift, rollback, any failure) gives none.
- **Applying under auto-apply:** while it is held, a `propose_batch` of that chat pane applies without opening the review, when every condition of code-write, Batch review and coverage gate holds (below). The checks that run at staging and at apply stay as they are: the caps (50 entries, 1 MiB; `MAX_ENTRIES`, `MAX_BYTES`, `src/core/ai/tools/stage.rs:19-21`), the path rules of `resolve_write` (a root inside HOME, no env, secret (`SECRET_FILES` and `SECRET_PATTERNS`, so `token.json`, `*.pem`, `*.key`, `client_secret*.json` and `service-account*.json` too), `.git` or `node_modules` name; the chain is `judge_write` `:495`, `judge_real` `:482`, `check_path` `:219`, `judge_rest` `:206`, `is_excluded_name` `:145`; `src/core/ai/tools/paths.rs:779`, `WRITE_DENIED_DIRS` at `:689`, which also holds `.lzr`) and the apply step (drift check, atomic writes, rollback; `Stages::apply`, `src/runtime/tools/stage.rs:197`). The chat shows one line per applied batch with its first 10 paths and line counts, then `+N more` (owner answer 2026-11-28); `/applied` keeps the full list.
- **Batches that still open the review under auto-apply** (auto-apply stays held; it is not ended):
  - a batch with an **environment template** row (`.env.example`, `.env.local.sample`...): see decision A. Not provisional.
  - a batch with a **build-file row** (the classes of decision 2), or a row that matches a pattern the user added in `[ai.tools] review_always`.
  - a batch that is **not reviewable** (a truncated diff, shown as too large today; `ReviewDoc::reviewable`, `src/core/ai/chat/review.rs:186`): it can only be rejected there.
  - a batch with a **coarse diff** row (decision 4).
  - any batch of a chat whose folder is a **granted Windows drive folder** (`ChatState::drive_grant()` is `Some`; `src/core/ai/chat/mod.rs:1222`, `src/app/review.rs:273-277`): there `a` is not offered either.
  - a batch of a conversation that read GitHub text (`ChatState::block_why()` is `Some`; decision 4).
  - a batch that arrives while another approval is pending: it is denied with `another approval is pending`, as every pane ask is today (`open`, `src/core/ai/chat/approval.rs:1327`; and the comment of `auto_route`, `src/app/tool.rs:453-457`: a pending approval is not the mode's to answer). It is not queued and not applied.
- **One pane:** auto-apply belongs to the chat pane that gave it. Another chat pane never inherits it and asks as usual.
- **End of auto-apply:** a new conversation, a resumed saved conversation, any change of the pane's approval mode (leaving `run` included), a change of the chat's folder (`/cd`), tools turned off, the Write family no longer offered to the chat (the saved `write` toggle changed, or its `/write` grant ended), the pane closing and quitting the app. Also a batch applied under it that does not apply in full (drift, refusal, failure, partial): the next batch asks again. It fails closed: a turn that is cancelled or ends, or a pane that closes, while an auto-applied batch has no result (`Over(call, status)`) ends it, because the app cannot know whether that batch applied. It is never stored, never sent to an engine, and only a user key can give it. It has its own end hook; it does not use `end_grants` (see Approach 1).
- **Marker:** while it is held, the chat's ` RUN ` marker reads ` RUN+A ` in red.
- **Host check:** the host applies a batch only on the user's `y`, or on the new auto-apply decision that it accepts for a pane's own batch and never for a worker's batch. A forged `Unprompted` still writes nothing.
- **Guard:** no runtime code may name the auto-apply flag, as for the approval mode (`MODE_NAMES`, `src/runtime/tools/audit.rs:436`, design D19).
- **Enter in the review:** Enter approves a batch like `y`, with the same gate and delay, in every batch review (pane, worker, orphan; decision 1). A `git_commit` or screen-read review that shares the view still needs `y` (owner answer 2026-11-28).
- **Applied batches stay readable:** each auto-applied batch keeps its review document for the conversation, in memory; `/applied` lists them and opens one read-only (decision 3).
- **Docs:** README `#### Modo run`, the review footer text, the `[ai.tools] approval` help, CHANGELOG, and a GIF of auto-apply.

### Out of Scope
- `ask` mode: the review is unchanged and `a` is not offered (the `auto` mode no longer exists after #845; a saved `auto` migrates to `ask`).
- Orchestrator worker batches (code-write W10): they keep the approval queue; auto-apply never covers them.
- Orphan batches reviewed later through the badge: they keep `y` only.
- `run_command`, `git_add`, `git_commit`, `gh_issue_draft` and user MCP tools: each still needs its own `y`.
- The Windows-drive grant (`wsl-drive-access`): unchanged and separate; its batches always open the review.
- Approval levels per MCP server (`mcp-connectors` S9, open question 10).
- The code-write "Deny rules" requirement: **not modified**. See decision A.

## Owner decision A: environment templates never auto-apply

The code-write "Deny rules" requirement (`openspec/specs/code-write/spec.md:53-54`, #583) says an environment template reaches the disk "only through the user's approval of the batch review, in every approval mode". That rule stays as it is. So under auto-apply a batch with an env-template row always opens the batch review, and auto-apply stays held (the next batch without such a row applies directly again). The ask that `write_file` makes before staging over a template that already exists (`overwrites_template`, `src/runtime/tools/host/files.rs:711`) is not the batch review and does not replace it: it only decides whether the model may learn what the file holds. No wording of this change says otherwise.

## How the app learns that a batch applied

The app answers `Decision::Sent` (`src/core/ai/chat/engine.rs:199`) and only the host sees the apply result. The result reaches the app as the call's `StreamEvent::ToolResult { id, status, .. }` (`src/core/ai/chat/engine.rs:370`), which the app already reads (`ToolEvent::Over`, `src/app/tool.rs:77`, handled at `:146`). The host gives status `Ok` only when `ApplyResult::applied()` is true, that is `ApplyResult::Applied`; every other result (`Drifted`, `Invalid`, `Failed`, `Partial`, `Denied`, `VolumeUnsupported`, `Unsupported`) ends as `Error` (`answered`, `src/runtime/tools/host/files.rs:854-908`; `ApplyResult`, `src/runtime/tools/apply.rs:107-147`). So the app marks the call when `a` answers it (state "pending": turn, call) and sets auto-apply only when `Over(call, Ok)` arrives for that call and the pane is still in the same state; any other status drops the mark. `Sent` alone proves nothing.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `agent-tools`: Approval model (its exceptions list gains one; the "no per-session trust" sentence follows); Chat approval mode (auto-apply, its end points and marker).
- `chat-pane`: the badge requirement (`openspec/specs/chat-pane/spec.md:825`) gains ` RUN+A `. Its delta is `specs/chat-pane/spec.md` (the full current requirement and the changed one); the code change is in PR2.
- `code-write`: Batch review and coverage gate (the `a` key and the batches auto-apply covers); Nothing writes without approval (an auto-applied batch counts as approved).

## Approach

1. **State:** an `auto_apply` field on `ChatState` (`src/core/ai/chat/mod.rs`): off, pending `(turn, call)` or on. Memory only. Its own end hook, because `end_grants(id, toggles, why)` (`src/app/write.rs:99`) is keyed by `ToolToggle` and returns early when no toggle is held (`:117-119`), so it would never clear this flag. The hook is `App::end_auto_apply(id, why)`, called next to `end_write_for_folder` in `change_folder` (`src/app/folder.rs:83`), next to `end_all_write` in `sync_tools` (`src/app/run.rs:61`), and where the saved Write toggle changes (the toggle loop, `src/app.rs:2659-2663`). A `/write` grant ending has no site of its own: it is reached only through `end_grants`, and its callers are covered by the other sites (design, End hooks). `ChatState::set_approval_mode` (`src/core/ai/chat/mod.rs:1140`) clears it itself: that setter is the one call of a new conversation (`src/app/history.rs:160`), a resume (`:575`), the toggle (`src/app/tool.rs:505`) and the pane opening (`src/app/history.rs:40`).
2. **Key:** `a` in `on_review_key` (`src/app/review.rs:184`) beside `y`, shown in the footer only for a plain-chat batch in `run` mode on a folder under HOME; it runs `approve_batch` (`:263`), marks the call as pending and answers `Decision::Sent`. Auto-apply is set when `Over(call, Ok)` arrives (section above).
3. **Auto-apply route:** in `on_tool_ask` (`src/app/tool.rs:189`), after the `answers_itself` block (`:238-253`), an `ApprovalAsk::Batch` with no worker, mode `run`, tools on, auto-apply on for that pane, no approval pending, no drive grant, and a review that is reviewable with no build, env-template or coarse row, is answered with the new decision instead of opening the review. The app keeps the review's path list and counts until `Over(call, status)`; on `Ok` it adds the chat line, on any other status it adds a line with the first line of the result and ends auto-apply.
4. **Review accessors:** `ReviewDoc` (`src/core/ai/chat/review.rs:74`) keeps only truncation (`reviewable()`); the build warning is a `LineKind::Warning` line pushed while building (`:132`) and `lines()` is `pub(crate)` for the renderer only. Add accessors computed in `build_with`: a build row, an env-template row, a coarse row, and the list of entries (path, kind, added, removed).
5. **Host:** a new `Decision` variant (named in the first PR; it must not be one of the guarded names) that `files::answered` (`src/runtime/tools/host/files.rs:854`) accepts like `Sent` for a pane session only, and only on a reviewable batch (the test `Sent` already passes, `:872-873`).
6. **Guard:** add the auto-apply names (`auto_apply`, `set_auto_apply`, `AutoApply`) to `MODE_NAMES` (`src/runtime/tools/audit.rs:436`); the test `the_runtime_never_names_the_approval_mode` (`:921`) then covers them.
7. **Lines and marker:** the chat line per auto-applied batch; ` RUN+A ` in `src/ui/components/chat.rs` (the ` RUN ` marker is at `:307`).

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/ai/chat/mod.rs`, `engine.rs`, `review.rs` | Auto-apply state; new `Decision` variant; review accessors |
| `src/app/review.rs`, `tool.rs`, `history.rs`, `folder.rs`, `run.rs`, `write.rs`, `app.rs` | `a` key, route, `Over` handling, end hook and its call sites |
| `src/runtime/tools/host/files.rs`, `audit.rs` | Accept the decision for pane batches only; guard names |
| `src/ui/components/chat.rs`, `review.rs` | Marker and footer |
| `README.md`, `CHANGELOG.md`, `docs/assets/` | Docs and GIF |
| `openspec/specs/agent-tools`, `code-write` | Spec sync |

**Size:** estimate ~695 production lines over 3 PRs (PR1 ~335, PR2 ~120, PR3 ~240; see `tasks.md` Review Workload Forecast; not measured). **Risk:** high (security: it adds the first approval that lasts beyond one call, and touches the host's apply guard). Independent verifier and native review per PR.

**Slicing (3 PRs):** (1) host decision, state and end points, review accessors, guard and route, with tests, `a` not yet shown; (2) `a` and Enter keys, footer, marker, lines, README, CHANGELOG, GIF; (3) `/applied` and `[ai.tools] review_always`, their docs, and spec sync.

## Residual risk

- The build-file rule (decision 2) is a denylist of names and folders (`is_build_file`, `src/core/ai/tools/paths.rs:741-760`). A file outside it can still change behaviour on the next run, for example `.vscode/settings.json` or `.yarnrc.yml`, and an auto-applied batch can write it without a review unless the user lists it in `[ai.tools] review_always`. `/applied` lets the user read afterwards what was written.
- Prompt injection can come from any file the model reads (a README, a source comment, a dependency, a tool result), not only from GitHub text, so the `block_why()` rule covers one source of many.

## Coordination

`mcp-connectors` (open question 10) also proposes changing the sentence "There MUST be no "always allow", no per-session or per-tool trust, no configuration that approves automatically" of agent-tools Approval model. Whichever change lands second rebases its delta on the other's text.

`admin-policy-lock` (open PR #672) lists "the full write grant of run mode" as a future lock key in its `tasks.md`. This change is compatible: a lock would only stop `a` from being offered and from setting auto-apply, and the rest stays as written here.

## Owner decisions (2026-11-27)

The four open questions were answered by the owner; the answers are quoted from `odd/tasks/chat-auto-permissions.md` L59-L62.

1. **Enter in the review.** "si, el enter lo podemos usar para aceptar": Enter approves a batch like `y`, with the same coverage gate and arm delay, in every batch review.
2. **Build files under auto-apply.** "acéptalo así y que pueda ampliarse si es necesario por parte del usuario": the classes listed in the code-write delta keep opening the review under auto-apply, and the user can add patterns with `[ai.tools] review_always` (extend only; the built-in classes cannot be removed). The key name and its glob syntax are this change's proposal.
3. **Audit of auto-applied batches.** "dale, vamos con el diff": an auto-applied batch keeps its review document for the conversation, in memory only, and `/applied` opens it read-only. Nothing caps the number of batches; the per-batch caps stay.
4. **Coarse diff and GitHub text.** "si, leer es menos restrictivo que la escritura": both open the review under auto-apply.

Design questions answered by the owner on 2026-11-28 (recorded in `design.md`, Open Questions; no earlier decision changes):

5. **Length of the applied line.** The chat line of an auto-applied batch names the first 10 paths, then `+N more`; `/applied` keeps the full list.
6. **Enter scope.** Enter approves only batch reviews (pane, worker, orphan); `git_commit` and screen-read reviews still need `y`.
