# Tasks: local-model-writes

## 1. D1: open the review when a turn ends with staged changes

- [x] 1.1 RED: a turn that ends with `Done` and a staged, unproposed batch opens the batch review (same view, gate and arming as an orphan review).
- [x] 1.2 RED: a cancelled, timed-out or failed turn, a pending approval, or a worker batch keeps the orphan badge and opens nothing.
- [x] 1.3 Record how the turn ended on the chat; emit `Effect::BatchReview(pane)` from `App::sync_batches` when the badge turns from `staged` to `orphan` after a `Done`.
- [x] 1.4 Docs: README "Huérfanos", code-write spec "Orphan, close and quit" and the orphan definition.

## 2. D2: write refusals that name the chat's folder

- [ ] 2.1 RED: every write refusal reads `path not available (paths are relative to the chat's folder <~/folder>)`, the same text for every reason.
- [ ] 2.2 RED: a missing parent folder after the path policy passed names that folder; a secret, linked or outside path keeps the plain text with the hint.
- [ ] 2.3 Keep the error kind of a missing parent through `Disk::probe` instead of folding it into `Unreadable`.
- [ ] 2.4 Docs: README write threat model, code-write refusal requirement.

## 3. D3: one reminder for a tool call written as text (Ollama, llama.cpp)

- [ ] 3.1 RED: an answer with a fenced or bare JSON call to an offered tool gets one more request in the same turn with the reminder, and a system line.
- [ ] 3.2 RED: a second text call in the same turn ends the turn; an answer naming a tool without `arguments`, or a tool not offered, ends the turn; the reminder counts toward the cap; nothing is executed.
- [ ] 3.3 Add a non-tool follow-up message to `ToolRound` for both request profiles.
- [ ] 3.4 Docs: README "Tools with Ollama", chat-engines spec.
