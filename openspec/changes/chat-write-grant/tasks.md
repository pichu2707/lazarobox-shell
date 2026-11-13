# Tasks: chat-write-grant

Refs: #432. Test runner: `cargo test --locked`; gates `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`. Strict TDD: observe RED before each rule. Risk high (security, item 2): independent verifier and native review per PR.

## Before PR1
- [ ] 0.1 Owner answers open questions 1 (Enter) and 2 (build files) of the proposal.

## PR1: host decision, grant flag and auto-apply (`a` not yet shown) - ~180 lines
- [ ] 1.1 RED: `files::answered` applies a pane batch on the grant decision, refuses it for a worker batch, and still refuses `Unprompted`, `Typed` and `Busy`.
- [ ] 1.2 Add the `Decision` variant and its host check.
- [ ] 1.3 RED: `writes_granted` ends on a new conversation, a resumed one, the mode leaving `run`, a folder change, tools or Write off.
- [ ] 1.4 Add the flag on `ChatState` and clear it at each end point.
- [ ] 1.5 RED: with the flag set in `run`, a plain-chat batch applies without a review and adds its chat line; a build-file batch, a worker batch and `ask`/`auto` modes still open the review.
- [ ] 1.6 Route `ApprovalAsk::Batch` in `on_tool_ask`.

## PR2: key, marker, docs - ~120 lines + docs
- [ ] 2.1 RED: `a` applies and grants under the gate and arm delay; `a` does nothing in other reviews.
- [ ] 2.2 `a` in `on_review_key`, footer text, ` RUN+W ` marker.
- [ ] 2.3 README `#### Modo run`, `[ai.tools] approval` help, CHANGELOG `[Unreleased]`.
- [ ] 2.4 GIF `docs/assets/run-write-grant.gif` (tape in `docs/assets/tapes/`).
- [ ] 2.5 Sync `openspec/specs/agent-tools` and `code-write`, archive the change.
