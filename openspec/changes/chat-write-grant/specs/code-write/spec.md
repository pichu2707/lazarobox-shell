# Delta for code-write

Change `chat-write-grant` (the concept is called run auto-apply; the folder keeps its name). Base: `openspec/specs/code-write/spec.md` on origin/main (99106097), Requirements `Batch review and coverage gate` and `Nothing writes without approval`. Each requirement below is given as its full current text (collapsed), then its full changed text, which is the one that replaces it. Who gives auto-apply, when it ends and the marker are in agent-tools, Chat approval mode.

`Deny rules` (`openspec/specs/code-write/spec.md:53-54`) is **not modified**: an environment template still reaches the disk only through the user's approval of the batch review, in every approval mode (#583). The rules of this change keep it true under auto-apply.

## MODIFIED Requirements

What changes in `Batch review and coverage gate`: the "Auto-apply" and "Environment templates" paragraphs are added after the base paragraph, and the scenarios below the base scenarios.

<details>
<summary>Current text on origin/main, <code>openspec/specs/code-write/spec.md</code> lines 189-225</summary>

> ### Requirement: Batch review and coverage gate
> A proposed batch MUST open a full-pane review view in the chat pane. It shows every change: a create shows the new text, an overwrite or edit shows a diff, a `create_dir` shows its path. The approving key MUST stay disabled until every row of the whole batch has been drawn on screen (the coverage gate; a row in a hidden pane does not count), and it MUST stay disabled for the arm delay after the view is first drawn (longer than the delay of other approvals, value pinned by design). The review is all-or-nothing: the user approves the whole batch or rejects the whole batch. A change to a build file (for example `Makefile`, `build.rs`, `package.json`) MUST carry a visible warning. A rejected or timed-out review MUST end as `denied`, MUST write nothing, and MUST discard the batch. A review cancelled with Ctrl+C MUST also end as `denied` and write nothing, but MUST leave the batch as an orphan (see Orphan, close and quit); a propose call dropped after the batch was sealed and before its ask shows MUST NOT discard the batch either. A pending approval of another kind MUST make the propose tool return `denied` (`another approval is pending`).
>
> #### Scenario: Gate blocks early approval [auto]
> - GIVEN a batch of 40 diff rows of which 12 have been shown
> - WHEN `y` is pressed
> - THEN nothing is applied and the view says how many rows remain
>
> #### Scenario: Gate opens after full coverage [auto]
> - GIVEN the same batch, every row shown and the arm delay elapsed (fake clock)
> - WHEN `y` is pressed
> - THEN the apply step starts
>
> #### Scenario: Reject discards [auto]
> - GIVEN a proposed batch
> - WHEN `n` or Esc is pressed
> - THEN the disk is unchanged, the batch is gone and the propose call returns `denied`
>
> #### Scenario: Ctrl+C on review keeps an orphan [auto]
> - GIVEN a proposed batch under review
> - WHEN Ctrl+C is pressed
> - THEN the propose call returns `denied`, nothing is written and the batch is kept as an orphan, while `n` still discards it
>
> #### Scenario: A propose dropped before its ask shows keeps the batch [auto]
> - GIVEN a batch sealed by a propose call whose ask has not been announced yet
> - WHEN the call is dropped (the turn is cancelled)
> - THEN the batch is kept and can be reviewed as an orphan
>
> #### Scenario: Build file warning [auto]
> - GIVEN a batch that stages `build.rs`
> - WHEN the review renders
> - THEN the row for `build.rs` carries the warning [TestBackend + insta]
>
> #### Scenario: CLI timeout while reviewing [auto]
> - GIVEN a CLI call timeout shorter than the review
> - WHEN it expires
> - THEN the call returns `denied`, the review is withdrawn, the batch is kept as an orphan and a later `y` writes nothing

</details>

### Requirement: Batch review and coverage gate
A proposed batch MUST open a full-pane review view in the chat pane. It shows every change: a create shows the new text, an overwrite or edit shows a diff, a `create_dir` shows its path. The approving key MUST stay disabled until every row of the whole batch has been drawn on screen (the coverage gate; a row in a hidden pane does not count), and it MUST stay disabled for the arm delay after the view is first drawn (longer than the delay of other approvals, value pinned by design). The review is all-or-nothing: the user approves the whole batch or rejects the whole batch. A change to a build file (for example `Makefile`, `build.rs`, `package.json`) MUST carry a visible warning. A rejected or timed-out review MUST end as `denied`, MUST write nothing, and MUST discard the batch. A review cancelled with Ctrl+C MUST also end as `denied` and write nothing, but MUST leave the batch as an orphan (see Orphan, close and quit); a propose call dropped after the batch was sealed and before its ask shows MUST NOT discard the batch either. A pending approval of another kind MUST make the propose tool return `denied` (`another approval is pending`).

**Auto-apply.** In the review of a batch staged by a plain chat (not a worker, not an orphan) whose approval mode is `run`, with tools on, on a folder below HOME that is not a granted Windows drive folder, the footer MUST offer `a` beside `y`: `a` approves the whole batch exactly like `y`, under the same coverage gate and arm delay, and gives the chat auto-apply (see agent-tools, Chat approval mode) only once the host reports that batch applied in full; if the apply refuses it or ends partial (drift, rollback, any failure), no auto-apply MUST be held. In any other review `a` MUST do nothing and MUST NOT be offered.

While the chat holds auto-apply, a batch it proposes MUST be applied without opening the review only when all of these hold: it is the chat pane's own batch (staged under `StageKey::Pane` of that pane, never `StageKey::Worker`); the mode is `run` and tools are on; no other approval is pending; the batch is reviewable; no row is an environment template; no row carries the build-file warning (provisional: classes and rule depend on open question 2 of the proposal); no row has a coarse diff and the conversation has not read GitHub text (both provisional, open question 5); and the chat folder is not a granted Windows drive folder. A batch that fails any of them MUST open the review as without auto-apply, and auto-apply MUST stay held (a batch that is not reviewable can then only be rejected, as today, with the too-large reason). A batch that arrives while another approval is pending MUST be denied with `another approval is pending`, not queued and not applied.

An applied batch goes through the same checks as one the user approves: the caps (50 entries, 1 MiB) and the path rules (a root inside HOME, no env, secret, `.git` or `node_modules` name, and the secret-looking names of the Deny rules) at staging, and the same apply step (drift check, atomic writes, rollback). Under auto-apply none of them MAY be relaxed. The app MUST learn that a batch applied from the call's tool result (status `Ok` only for an applied batch); a batch that does not apply in full under auto-apply MUST end auto-apply, with one chat line giving the first line of the result. Each batch applied under auto-apply MUST add one chat line naming every path with its kind and added and removed lines. A worker's batch MUST always go through the approval queue (see Worker batches).

**Environment templates.** A batch with an environment template row (an env name ending in `.example`, `.sample`, `.template` or `.dist`) MUST always open the batch review, under auto-apply too, and auto-apply MUST stay held. The Deny rules requirement is unchanged: a template reaches the disk only through the user's approval of the batch review, in every approval mode. The ask that `write_file` makes before staging over a template that already exists is not that review and MUST NOT stand in for it.

#### Scenario: Gate blocks early approval [auto]
- GIVEN a batch of 40 diff rows of which 12 have been shown
- WHEN `y` is pressed
- THEN nothing is applied and the view says how many rows remain

#### Scenario: Gate opens after full coverage [auto]
- GIVEN the same batch, every row shown and the arm delay elapsed (fake clock)
- WHEN `y` is pressed
- THEN the apply step starts

#### Scenario: Reject discards [auto]
- GIVEN a proposed batch
- WHEN `n` or Esc is pressed
- THEN the disk is unchanged, the batch is gone and the propose call returns `denied`

#### Scenario: Ctrl+C on review keeps an orphan [auto]
- GIVEN a proposed batch under review
- WHEN Ctrl+C is pressed
- THEN the propose call returns `denied`, nothing is written and the batch is kept as an orphan, while `n` still discards it

#### Scenario: A propose dropped before its ask shows keeps the batch [auto]
- GIVEN a batch sealed by a propose call whose ask has not been announced yet
- WHEN the call is dropped (the turn is cancelled)
- THEN the batch is kept and can be reviewed as an orphan

#### Scenario: Build file warning [auto]
- GIVEN a batch that stages `build.rs`
- WHEN the review renders
- THEN the row for `build.rs` carries the warning [TestBackend + insta]

#### Scenario: CLI timeout while reviewing [auto]
- GIVEN a CLI call timeout shorter than the review
- WHEN it expires
- THEN the call returns `denied`, the review is withdrawn, the batch is kept as an orphan and a later `y` writes nothing

#### Scenario: a applies and gives auto-apply [auto]
- GIVEN a chat in `run` mode, a batch of 3 rows all shown and the arm delay elapsed (fake clock)
- WHEN `a` is pressed and the batch's tool result arrives with status `Ok`
- THEN the batch was applied as with `y` and the chat holds auto-apply, and not before that result

#### Scenario: a obeys the gate [auto]
- GIVEN the same batch with 1 of 3 rows shown
- WHEN `a` is pressed
- THEN nothing is applied, no auto-apply is given and the view says how many rows remain

#### Scenario: A later batch applies without review [auto]
- GIVEN a chat holding auto-apply
- WHEN the model proposes a batch editing `src/lib.rs`
- THEN no review opens, the file is written through the apply step, and one chat line names `src/lib.rs` with its line counts

#### Scenario: Drift refuses and ends auto-apply [auto]
- GIVEN a chat holding auto-apply and a target changed on disk after staging
- WHEN the batch is proposed
- THEN the whole batch is refused, nothing is written, the chat line gives the refusal, auto-apply is off, and the next batch opens the review

#### Scenario: A failed first batch gives no auto-apply [auto]
- GIVEN a chat in `run` mode, a reviewable batch and a target changed on disk after staging
- WHEN `a` is pressed and the apply refuses the batch (tool result status `Error`)
- THEN nothing is written, no auto-apply is held, and the next batch opens the review

#### Scenario: A partial first batch gives no auto-apply [auto]
- GIVEN a chat in `run` mode, a reviewable batch, a step that fails and an undo that fails too (`ApplyResult::Partial`)
- WHEN `a` is pressed
- THEN the tool result is `Error`, no auto-apply is held, and the next batch opens the review

#### Scenario: A sent answer alone gives nothing [auto]
- GIVEN a chat in `run` mode and a batch answered with `a`
- WHEN the pane's mode is changed, or the conversation ends, before the tool result arrives, and then a result `Ok` arrives
- THEN no auto-apply is held

#### Scenario: An unreviewable batch is not applied by auto-apply [auto]
- GIVEN a chat holding auto-apply
- WHEN the model proposes a batch whose diff is truncated (not reviewable)
- THEN auto-apply does not apply it, the review opens, only `n` or Esc work, the model is told the batch is too large, and auto-apply stays held

#### Scenario: Another pane's batch asks [auto]
- GIVEN two chat panes in `run` mode and auto-apply held by the first
- WHEN the second pane proposes a batch
- THEN the review opens in the second pane and nothing is written until the user's key

#### Scenario: A worker's batch asks [auto]
- GIVEN a chat holding auto-apply and an orchestrator worker that proposes a batch
- WHEN the ask arrives
- THEN it goes through the approval queue and nothing is written until the user's `y`

#### Scenario: An environment template opens the review [auto]
- GIVEN a chat holding auto-apply
- WHEN the model proposes a batch with `.env.example` among its rows
- THEN the review opens and waits for `y` or `a`, nothing is written before it, and auto-apply is still held; the next batch without such a row applies without a review

#### Scenario: The template ask does not replace the review [auto]
- GIVEN a chat holding auto-apply, a `.env.example` that exists, and the ask of `write_file` over it answered with `y`
- WHEN the batch is proposed
- THEN the review still opens and nothing is written until the user's key on it

#### Scenario: A build file still opens the review (provisional, open question 2) [auto]
- GIVEN a chat holding auto-apply
- WHEN the model proposes a batch that changes one file of each class in turn: `build.rs`, `scripts/run.sh`, `.github/workflows/ci.yml`, `.husky/pre-commit`, and a file of a project below a dot folder of HOME
- THEN for each the review opens with the build-file warning and waits for `y` or `a`, and auto-apply stays held

#### Scenario: A coarse diff opens the review (provisional, open question 5) [auto]
- GIVEN a chat holding auto-apply
- WHEN the model proposes a batch with a row whose diff is coarse
- THEN the review opens with the coarse-diff warning

#### Scenario: A conversation that read GitHub text asks (provisional, open question 5) [auto]
- GIVEN a chat holding auto-apply whose conversation read GitHub text
- WHEN the model proposes a batch
- THEN the review opens

#### Scenario: A Windows drive folder opens the review [auto]
- GIVEN a chat whose folder is a granted Windows drive folder, in `run` mode
- WHEN the review of a batch opens, and when a batch is proposed in a chat that somehow holds auto-apply
- THEN `a` is not offered, `a` does nothing, and the batch opens the review

#### Scenario: a is not offered elsewhere [auto]
- GIVEN a review in `ask` mode, one in `auto` mode, a worker's batch and an orphan batch
- WHEN `a` is pressed in each
- THEN nothing is applied and no auto-apply is given

#### Scenario: The caps still refuse under auto-apply [auto]
- GIVEN a chat holding auto-apply
- WHEN the model stages a 51st entry, and in another batch a file that takes it over 1 MiB
- THEN each is refused at staging with "batch is over 50 entries or 1 MiB: split it into smaller batches", as without auto-apply, and nothing is written

#### Scenario: Protected folders still refuse under auto-apply [auto]
- GIVEN a chat holding auto-apply
- WHEN `write_file` is called with `.git/hooks/pre-commit` and with `node_modules/x/index.js`
- THEN each is refused and nothing is staged

#### Scenario: A root outside HOME still refuses under auto-apply [auto]
- GIVEN a chat holding auto-apply whose folder is HOME itself, one whose folder is outside HOME, and a path that climbs out with `..`
- WHEN `write_file` is called in each
- THEN each is refused and nothing is staged

#### Scenario: Existing secret names still refuse under auto-apply [auto]
- GIVEN a chat holding auto-apply
- WHEN `write_file` is called with `.env`, `.envrc`, `.npmrc`, `.netrc` and `.git-credentials`
- THEN each is refused and nothing is staged

#### Scenario: Secret-looking file names still refuse under auto-apply (provisional, BLOCKING, open question 4) [auto]
- GIVEN a chat holding auto-apply and the owner's decision of open question 4
- WHEN `write_file` is called with `token.json`, `server.pem`, `server.key`, `client_secret.json` and `service-account.json`
- THEN each is refused (decision a) or opens the review (decision b), and nothing is written unseen

What changes in `Nothing writes without approval`: the exception for an auto-applied batch, the host sentence, and two scenarios.

<details>
<summary>Current text on origin/main, <code>openspec/specs/code-write/spec.md</code> lines 303-309</summary>

> ### Requirement: Nothing writes without approval
> No tool MUST change a file, directory or permission on disk except the apply step of an approved batch. Staging, searching and proposing MUST NOT touch the disk.
>
> #### Scenario: Recording filesystem [auto]
> - GIVEN every write tool called against a recording filesystem and no approval given
> - WHEN the calls complete
> - THEN no file or directory was created, changed or removed

</details>

### Requirement: Nothing writes without approval
No tool MUST change a file, directory or permission on disk except the apply step of an approved batch, or of a batch applied under auto-apply, which counts as approved by the user because the user gave auto-apply with a key (see Batch review and coverage gate). Staging, searching and proposing MUST NOT touch the disk. The host MUST accept the auto-apply answer only for a pane's own batch and only when the batch is reviewable, never for a worker's batch, and MUST refuse a batch answered in any other way that is not the user's `y`.

#### Scenario: Recording filesystem [auto]
- GIVEN every write tool called against a recording filesystem and no approval given
- WHEN the calls complete
- THEN no file or directory was created, changed or removed

#### Scenario: A forged answer writes nothing [auto]
- GIVEN a sealed batch of a pane, and in another run a worker's sealed batch
- WHEN the host receives `Unprompted` for the first, and the auto-apply answer for the second
- THEN both are refused and nothing is written

#### Scenario: An unreviewable batch is refused the auto-apply answer [auto]
- GIVEN a sealed pane batch that is not reviewable
- WHEN the host receives the auto-apply answer
- THEN it is refused with the too-large reason and nothing is written
