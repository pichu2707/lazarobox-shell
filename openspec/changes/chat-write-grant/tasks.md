# Tasks: chat-write-grant

Refs: #432. Test runner: `cargo test --locked`; gates `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`. Strict TDD: observe RED before each rule. Risk high (security, item 2): independent verifier and native review per PR.

## Before PR1
- [ ] 0.1 Owner answers open questions 1 (Enter) and 2 (build files) of the proposal. PR1 does not start before both are answered; the build-file rule of the specs is provisional until question 2. Open question 3 (audit of auto-applied batches) is decided before PR2 and may add a task there.

## PR1: host decision, grant flag and auto-apply (`a` not yet shown) - ~180 lines
- [ ] 1.1 RED: `files::answered` applies a pane batch on the grant decision, refuses it for a worker batch, and still refuses `Unprompted`, `Typed` and `Busy`, and refuses the grant decision on a batch that is not reviewable (`TOO_LARGE`).
- [ ] 1.2 Add the `Decision` variant and its host check (pane batch only, reviewable only).
- [ ] 1.3 RED: `writes_granted` ends on a new conversation, a resumed one, the mode leaving `run`, a folder change, tools or Write off.
- [ ] 1.4 Add the flag on `ChatState` and clear it at each end point.
- [ ] 1.5 RED: with the flag set in `run`, a plain-chat batch applies without a review and adds its chat line; a build-file batch, a not reviewable batch, a worker batch, a batch of another pane and `ask`/`auto` modes still open the review.
- [ ] 1.6 Route `ApprovalAsk::Batch` in `on_tool_ask`, checking the flag of the pane that asked, reviewable and no build-file row.

## PR2: key, marker, docs - ~120 lines + docs
- [ ] 2.1 RED: `a` applies and grants under the gate and arm delay; `a` does nothing in other reviews; when the apply of the first `a` batch is refused (drift, rollback), no grant is held.
- [ ] 2.2 `a` in `on_review_key`, footer text, red ` RUN+W ` marker; set the flag only after the apply reports the batch applied.
- [ ] 2.3 README `#### Modo run`, `[ai.tools] approval` help, CHANGELOG `[Unreleased]`.
- [ ] 2.4 GIF `docs/assets/run-write-grant.gif` (tape in `docs/assets/tapes/`).
- [ ] 2.5 Sync `openspec/specs/agent-tools` and `code-write`, archive the change.
