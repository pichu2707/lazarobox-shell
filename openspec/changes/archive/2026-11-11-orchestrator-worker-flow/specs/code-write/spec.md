# Delta for Code Write

Slice tags: [W9] limit ask before a batch; [W10] one batch per write-capable worker.

## MODIFIED Requirements

### Requirement: Worker batches [W10]
A batch staged by an orchestrator worker MUST be reviewed and applied by the user through the approval queue of the orchestrator (S3). A worker MUST NOT apply a batch itself. [W10] Each write-capable worker MUST have its own batch: one worker's `propose_batch` MUST NOT seal or close the files another worker staged, and each batch is reviewed on its own, labelled with its worker. Until W10 ships, write-capable workers share the pane's batch and run one at a time. The interaction details are pinned by design.
(Previously: the batch was the pane's, so a second worker's proposal would seal the first worker's files)

#### Scenario: Worker cannot apply [auto]
- GIVEN a worker with a staged batch
- WHEN the worker finishes
- THEN nothing is written until the user approves through the queue

#### Scenario: One proposal does not seal another's files [W10, auto]
- GIVEN workers 1 and 2 each with staged files in their own batch
- WHEN worker 1 calls `propose_batch`
- THEN only worker 1's files are proposed, worker 2 can still stage, and approving worker 1's batch applies only worker 1's files

## ADDED Requirements

### Requirement: Limit and outside-brief asks come before the batch is proposed [W9]
For a `simple` worker, the line-limit ask and the outside-brief ask (see agent-orchestrator) MUST be answered before the batch containing the affected file is proposed, so the batch review shows what the user already accepted. A file whose ask is pending is not part of a proposed batch. The asks MUST NOT be answered by any approval mode. These asks do not write anything: a batch is still written only through the batch review.

#### Scenario: Ask first, review after [W9, auto]
- GIVEN a `simple` worker that staged a 401-line file and then calls `propose_batch`
- WHEN the limit ask is still pending
- THEN the file is not proposed until the user answers, and the later batch review shows the file with the kept lines

#### Scenario: A denied ask leaves the extra lines unwritten [W9, auto]
- GIVEN the limit ask denied
- WHEN the batch is proposed
- THEN the batch holds no file over the limit

#### Scenario: Split discards the staged batch [W9, auto]
- GIVEN the limit ask answered `split`
- WHEN the answer is applied
- THEN the worker's staged batch is discarded, nothing is proposed or written, and the worker's result and a notice show how many staged files were discarded

## Open points
- Whether staged content of an over-limit file is kept or discarded after `split`. Resolved by the owner on 2026-11-10: the whole staged batch of that worker is discarded, with the count shown.
- Where the line count is taken (at each write tool call or at `propose_batch`); the visible result is the same: no batch reaches review with an unanswered ask. Resolved by the owner on 2026-11-10 (design D14): at `propose_batch`, before the review and the seal.
