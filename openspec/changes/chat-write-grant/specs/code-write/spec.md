# Delta for code-write

Change `chat-write-grant`. Base: `openspec/specs/code-write/spec.md`, Requirements `Batch review and coverage gate` and `Nothing writes without approval`. The grant itself (who gives it, when it ends, the marker) is in agent-tools, Chat approval mode.

## MODIFIED Requirements

### Requirement: Batch review and coverage gate
(Everything in the base requirement stays as written; the paragraph below is added.)

**Write grant.** In the review of a batch staged by a plain chat (not a worker, not an orphan) whose approval mode is `run`, the footer MUST offer `a` beside `y`: `a` approves the whole batch exactly like `y`, under the same coverage gate and arm delay, and gives the chat the write grant only once that batch has applied; if the apply refuses it (drift, rollback, any failure), no grant MUST be held. In any other review `a` MUST do nothing. While the chat holds the grant, its proposed batch MUST be applied without opening the review, after the same checks as a batch the user approves (the caps (50 entries, 1 MiB), the path rules (a root inside HOME, no env, secret, `.git` or `node_modules` name) and the env-template overwrite ask) and through the same apply step, unless the batch is not reviewable or holds a change that carries the build-file warning (provisional: depends on open question 2 of the proposal); such a batch MUST open the review as without the grant (a batch that is not reviewable can then only be rejected, as today, with the too-large reason). The grant MUST apply only to a batch of the pane that holds it (a batch staged under `StageKey::Pane` of that pane, never `StageKey::Worker`). Each batch applied under the grant MUST add one chat line naming every path with its kind and added and removed lines. A worker's batch MUST always go through the approval queue (see Worker batches).

#### Scenario: a applies and grants [auto]
- GIVEN a chat in `run` mode, a batch of 3 rows all shown and the arm delay elapsed (fake clock)
- WHEN `a` is pressed
- THEN the batch is applied as with `y` and the chat holds the write grant

#### Scenario: a obeys the gate [auto]
- GIVEN the same batch with 1 of 3 rows shown
- WHEN `a` is pressed
- THEN nothing is applied, no grant is given and the view says how many rows remain

#### Scenario: A later batch applies without review [auto]
- GIVEN a chat holding the write grant
- WHEN the model proposes a batch editing `src/lib.rs`
- THEN no review opens, the file is written through the apply step, and one chat line names `src/lib.rs` with its line counts

#### Scenario: Drift still refuses under the grant [auto]
- GIVEN a chat holding the write grant and a target changed on disk after staging
- WHEN the batch is proposed
- THEN the whole batch is refused and nothing is written

#### Scenario: A failed first batch gives no grant [auto]
- GIVEN a chat in `run` mode, a reviewable batch and a target changed on disk after staging
- WHEN `a` is pressed and the apply refuses the batch
- THEN nothing is written, no grant is held, and the next batch opens the review

#### Scenario: An unreviewable batch is not applied by the grant [auto]
- GIVEN a chat holding the write grant
- WHEN the model proposes a batch whose diff is truncated (not reviewable)
- THEN the grant does not apply it, the review opens, only `n` or Esc work, and the model is told the batch is too large

#### Scenario: Another pane's batch asks [auto]
- GIVEN two chat panes in `run` mode and the grant held by the first
- WHEN the second pane proposes a batch
- THEN the review opens in the second pane and nothing is written until the user's key

#### Scenario: A build file still opens the review (provisional, open question 2) [auto]
- GIVEN a chat holding the write grant
- WHEN the model proposes a batch that changes `build.rs`
- THEN the review opens with the build-file warning and waits for `y` or `a`

#### Scenario: a is not offered elsewhere [auto]
- GIVEN a review in `ask` mode, one in `auto` mode, a worker's batch and an orphan batch
- WHEN `a` is pressed in each
- THEN nothing is applied and no grant is given

### Requirement: Nothing writes without approval
(The base text stays; the sentence below is added.)

A batch applied under a chat's write grant counts as approved by the user. The host MUST accept the grant's answer only for a pane's own batch and only when the batch is reviewable, never for a worker's batch, and MUST refuse a batch answered in any other way that is not the user's `y`.

#### Scenario: A forged answer writes nothing [auto]
- GIVEN a sealed batch of a pane, and in another run a worker's sealed batch
- WHEN the host receives `Unprompted` for the first, and the grant's answer for the second
- THEN both are refused and nothing is written
