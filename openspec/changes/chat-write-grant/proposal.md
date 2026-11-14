# Proposal: Write grant for the chat in `run` mode

Status: proposal, not started. Refs: #432. Last task (T3, S4) of `chat-auto-permissions`; the other parts (search, in-root reads, pane commands) are merged.

## Intent

In `run` mode the chat already searches, reads inside its folder and runs safe pane commands on its own, but every write still opens the batch review and waits for `y`. The owner asked:

> "los de escritura con permiso y si se lo dan pueda escribir directamente no tener que ir haciendo cambios, o bien si no le dan el permiso total que lo deje listo para que den al intro"

So a write asks; when the user grants it in full, later writes of the same conversation apply directly; without the grant, the write stays ready for the user to confirm (today's review). Decided by the owner: the grant lasts for the current conversation only (a new chat or a resumed saved chat asks again), and it is given in the review of the first batch, not by a separate command.

**For:** users who let the chat work on a project in `run` mode and do not want to approve every batch.

## Scope

### In Scope
- **Grant key in the review:** in a plain chat in `run` mode, the batch review offers a second approving key, `a`: apply this batch and trust the next writes of this conversation. `y` keeps applying only this batch. `a` obeys the same coverage gate and arm delay as `y`. The grant is held only once that first batch has applied: if it is refused (drift, rollback, any apply failure), no grant is held.
- **Applying under the grant:** while the grant is held, a `propose_batch` of that chat applies without opening the review, after the same checks (the caps (50 entries, 1 MiB), the path rules (a root inside HOME, no env, secret, `.git` or `node_modules` name) and the env-template overwrite ask) and through the same apply step (drift check, atomic writes, rollback). The chat shows one line per applied batch with its paths and line counts.
- **Batches that still open the review under the grant:** a batch with a build-file change (the files that already carry the visible warning; provisional, pending open question 2), and a batch that is not reviewable (a truncated diff, shown as too large today): the grant never applies it, so it opens the review as today, where it can only be rejected.
- **One pane:** the grant belongs to the chat pane that gave it. Another chat pane never inherits it and asks as usual.
- **End of the grant:** a new conversation, a resumed saved conversation, the approval mode leaving `run`, a change of the chat's folder (`/cd`), tools or the Write family turned off, the pane closing and quitting the app. It is never stored, never sent to an engine, and only a user key can give it.
- **Marker:** while the grant is held, the chat's ` RUN ` marker reads ` RUN+W ` in red.
- **Host check:** the host applies a batch only on the user's `y`, or on a new grant decision that it accepts for a pane's own batch and never for a worker's batch. A forged `Unprompted` still writes nothing.
- **Docs:** README `#### Modo run`, the review footer text, the `[ai.tools] approval` help, CHANGELOG, and a GIF of the grant.

### Out of Scope
- `ask` and `auto` modes: the review is unchanged and `a` is not offered.
- Orchestrator worker batches (code-write W10): they keep the approval queue; a grant never covers them.
- Orphan batches reviewed later through the badge: they keep `y` only.
- `run_command`, `git_add`, `git_commit`, `gh_issue_draft` and user MCP tools: each still needs its own `y`.
- The Windows-drive grant (`wsl-drive-access`): unchanged and separate.
- Approval levels per MCP server (`mcp-connectors` S9, open question 10).

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `agent-tools`: Approval model (the "no always allow, no per-session trust" sentence gains this one exception); Chat approval mode (the grant belongs to `run` and its marker).
- `code-write`: Batch review and coverage gate (the `a` key); Nothing writes without approval (a granted batch counts as approved).

## Approach

1. **State:** a `writes_granted` flag on `ChatState` (`src/core/ai/chat/mod.rs`), in memory only, cleared at every end point listed above (`ListOutcome::New` and `on_resumed` in `src/app/history.rs`, the mode toggle in `src/app/tool.rs`, and `end_grants` in `src/app/write.rs` for folder change and tools off).
2. **Key:** `a` in `on_review_key` (`src/app/review.rs`) beside `y`, shown in the footer only for a plain-chat batch in `run` mode; it runs `approve_batch` and sets the flag only once the apply reports that the batch was applied, not when the answer is sent.
3. **Auto-apply:** in `on_tool_ask` (`src/app/tool.rs`), an `ApprovalAsk::Batch` with no worker, mode `run`, the flag set of that pane, a reviewable batch and no build-file row is answered with the new decision instead of opening the review.
4. **Host:** a new `Decision` variant (name pinned in design) that `files::answered` (`src/runtime/tools/host/files.rs`) accepts like `Sent` for a pane session only, and only on a reviewable batch (the `reviewable` test that `Sent` already passes, `answered`, `src/runtime/tools/host/files.rs:872-873`).
5. **Lines and marker:** a system line per auto-applied batch; ` RUN+W ` in `src/ui/components/chat.rs`.

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/ai/chat/mod.rs`, `engine.rs` | Grant flag; new `Decision` variant |
| `src/app/review.rs`, `src/app/tool.rs`, `src/app/history.rs`, `src/app/write.rs` | `a` key, auto-apply route, end points |
| `src/runtime/tools/host/files.rs` | Accept the grant decision for pane batches only |
| `src/ui/components/chat.rs`, `review.rs` | Marker and footer |
| `README.md`, `CHANGELOG.md`, `docs/assets/` | Docs and GIF |
| `openspec/specs/agent-tools`, `code-write` | Spec sync |

**Size:** about 250-350 production lines. **Risk:** high (security: it adds the first approval that lasts beyond one call, and touches the host's apply guard). Independent verifier and native review per PR.

**Slicing (2 PRs):** (1) host decision, flag, end points and auto-apply, with tests, `a` not yet shown; (2) `a` key, footer, marker, lines, README, CHANGELOG, GIF and spec sync.

## Coordination

`mcp-connectors` (open question 10) also proposes changing the sentence "There MUST be no "always allow", no per-session or per-tool trust, no configuration that approves automatically" of agent-tools Approval model. Whichever change lands second rebases its delta on the other's text.

`admin-policy-lock` (open PR #672) lists "the full write grant of run mode" as a future lock key in its `tasks.md`. This change is compatible: a lock would only stop `a` from being offered and from setting the flag, and the rest stays as written here.

## Open Questions

1. **Enter in the review.** The owner said "que lo deje listo para que den al intro". Today the review confirms with `y`, not Enter. Should Enter also approve the batch (same gate and delay), or is `y` enough?
2. **Build files under the grant.** Proposed: a batch that changes a build file (`Makefile`, `build.rs`, `package.json`…) still opens the review even with the grant, because a build can run it. Accept, or apply those too?
3. **Audit of auto-applied batches.** Under the grant a batch applies without a review, so its diff is no longer shown. Should an auto-applied batch keep its diff reachable, for example in the history, besides the one chat line with paths and counts?
