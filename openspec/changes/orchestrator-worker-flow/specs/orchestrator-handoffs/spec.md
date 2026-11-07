# Delta for Orchestrator Handoffs

Slice tag: [W4] app-checked handoffs. Tags: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: App check of a handoff against the ledger [W4]
At every handoff of a `code` or `simple` worker, lazarobox MUST compare the files the handoff claims to have changed or written against the worker's change ledger (`files changed`), without any model. A file named in the handoff that the ledger does not show MUST be flagged `claimed, not in ledger` with its path. The flag MUST be shown in the acknowledgement of the `handoff` call, in every `collect` made after the handoff exists (a line of lazarobox's own before the untrusted-data wrapper, never inside it, so the worker's text cannot forge it), in the `close` report of the run, and in the stored note of the handoff. The first `collect` of a worker happens before its handoff exists and so carries no check. The flag is app evidence: the orchestrator's text MUST NOT remove it, and the orchestrator's verdict (`on_course`, `off_course`, `blocked`) MUST stay the orchestrator's, shown beside the flag. When the check cannot run (a handoff with no recognisable path, or no ledger), the result MUST say `not checked` and why, never `ok`. Whether an `off_course` verdict is forced by a flag, and whether the reverse (a ledger file the handoff does not mention) is flagged, are open for design.

#### Scenario: Claimed file not in the ledger [W4, auto]
- GIVEN a handoff whose `done` says `wrote src/a.rs and src/b.rs` and a ledger that holds only `src/a.rs`
- WHEN the handoff is checked
- THEN `src/b.rs` is flagged `claimed, not in ledger`, in the handoff acknowledgement, in a later `collect` result, in the `close` report and in the stored note, and no model was called

#### Scenario: Claim matches the ledger [W4, auto]
- GIVEN a handoff that names only files the ledger holds
- WHEN it is checked
- THEN no flag is raised and the result says the claims match the ledger

#### Scenario: The orchestrator cannot clear a flag [W4, auto]
- GIVEN a flagged handoff and an orchestrator verdict `on_course` with a reason saying the flag is wrong
- WHEN the handoff is saved
- THEN the flag is stored and shown, and the verdict is shown beside it

#### Scenario: Nothing to check says so [W4, auto]
- GIVEN a handoff of a worker with no ledger
- WHEN it is checked
- THEN the result says `not checked` with the reason

#### Scenario: A code worker with an empty ledger [W1, W4, auto]
- GIVEN a `code` worker that ended `done` with an empty ledger and a handoff claiming a written file
- WHEN the handoff is checked
- THEN the file is flagged and the empty ledger is disclosed to the user (see agent-orchestrator, Silent worker outcomes are disclosed)

### Requirement: Disclosure fields in a handoff [W4, W8]
A handoff of a `simple` worker MUST record whether the worker asked for a clarification (the text after "needs specification"), the number of clarification rounds of its brief so far, extra brief fields that were flagged, and whether a split was chosen. The recorded text is inert data like any handoff (Handoffs are inert data).

#### Scenario: Round count recorded [W8, auto]
- GIVEN a `simple` worker that ended with "needs specification: which file?" on its second round
- WHEN its handoff is built
- THEN the handoff holds the question, the round number 2 and the brief id, and the text is wrapped as untrusted data when read back
