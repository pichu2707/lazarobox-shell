# Delta for Agent Orchestrator

## Base of this delta

This delta targets `openspec/specs/agent-orchestrator/spec.md` (read on the `docs/acceptance-judges-proposal` branch, from `main` at b97cceef). Each MODIFIED block is a full copy of the requirement text as it stands, with edits marked "(Previously: ...)". Where a requirement has many scenarios, the existing scenarios stay as they are and only the added or changed ones are listed. The judge behaviour itself is in `acceptance-judges`. Slice tags: [S2] runtime hook; [S3] `judge` task; [S4] tests evidence before close.

## MODIFIED Requirements

### Requirement: Receipt-driven review [S7b, W7, S2]
A `high` change, or any change the user asks to review, MUST ask the user for consent for that change, inline, before any lens review starts, except a change by a cheap author, which is reviewed with no consent prompt (Mandatory review for a cheap author). A declined change MUST go on to the batch review unreviewed, marked `not reviewed`. On consent (or for a cheap author):
- The orchestrator MUST pick only the lenses the change calls for, from `risk`, `resilience`, `readability` and `reliability`, with a one-line reason.
- Each lens MUST run as a read-only `review` worker over the staged change, wrapped as untrusted data.
- Only a severe finding the change caused MUST block. A finding that already held before the change is a follow-up.
- Inferential blockers MUST go to one refuter worker before they count.
- At most one bounded correction MUST run, by a `code` worker, scoped to the blockers that survived; its result is validated against those blockers only.
- The corrections MUST be bounded across a worker's proposals: they share at most three corrections until one of that worker's batches is applied (a rejection does not reset the count). Past that the review still runs and reports, no correction starts, the transcript and the receipt (`correction: limit reached (3 rounds)`) say the correction limit was reached and how many rounds ran, and the batch goes to the user's batch review with its blockers open.
- The review MUST end in a receipt handoff (tier, lenses, findings, correction, outcome), shown in the batch review next to the diff.
[S2] The review MUST also have a judge mode (see `acceptance-judges`): for any change by a writer whose goal is not read-only, one or two read-only judge workers MUST rule the staged change against the agreed standard, inside this same review, reusing its diff, correction worker, guard, correction bound, receipt and reviewer notices. The gate of the review is therefore any writer that is not on a read-only goal, not only a `high` change or a cheap author; the lens review keeps its own gate and consent above. On a `high` change the lenses and the judges MUST both run, and their blockers and failed criteria MUST share one correction round (one corrector, counted once toward the three). Judge consent is asked once per run (see `acceptance-judges`), not per change.
A receipt MUST NOT approve the batch, and the orchestrator MUST NOT consent or approve anything.
(Previously: consent was asked for every reviewed change, whoever the author; since W7 a cheap author's change is reviewed with no consent prompt. S2 adds the judge mode, the widened gate and the shared correction round; before it, a change that was neither `high`, cheap nor asked for was never reviewed.)

Existing scenarios stay unchanged. Added scenarios:

#### Scenario: A low-tier change by a strong author gets a judge [S2, auto]
- GIVEN a `code` worker on `claude>sonnet` that staged a one-file `small` change at a low tier, with judge consent granted
- WHEN its batch is due for review and no lens review is required
- THEN one judge runs and no lens review runs, and the batch review shows the receipt with the judge section

#### Scenario: High change runs lenses and judges with one correction [S2, auto]
- GIVEN a consented `high` change with a lens blocker and a failed criterion
- WHEN the review ends its findings
- THEN one correction worker runs for both, the round counts once, and the receipt lists lens findings and failed criteria

#### Scenario: Declining the lens consent does not cancel the judges [S2, auto]
- GIVEN a `high` change whose per-change lens consent is declined and a run whose judge consent was granted
- WHEN the batch review opens
- THEN no lens ran (`not reviewed` for the lenses) and the judges ran, and the batch is not shown as fully reviewed

### Requirement: Mandatory review for a cheap author, optional for a strong one [W7, S2]
A change staged by a cheap author MUST be reviewed by a model before it reaches the user's batch review, with no consent prompt, using the RDD pipeline (no new agent type). A change staged by any other author MUST be reviewed by lenses only when Receipt-driven review requires it (a `high` change, with consent) or the user asks; [S2] it MUST be ruled by the acceptance judges whenever its goal is not read-only and the run's judge consent was granted. The reviewer MUST be the model the user assigned to the Review row (effective assignment, so an unassigned row resolves to `general`, shown visibly); the judges' models follow `acceptance-judges` (Which model judges). lazarobox MUST NOT pick another reviewer. When a review runs and its engine and model equal the author's, a visible warning MUST say so. When the reviewer is a local engine, a visible notice MUST state the limits of local models as reviewers. When the author is local and the reviewer is a cloud engine, a visible notice MUST say that the staged diff is sent to that engine and model. If the review cannot run (reviewer unavailable), it ends as a `refused` worker and the run stops as any worker failure; the change MUST NOT be silently shown as reviewed. The receipt MUST be shown in the batch review, and a receipt never approves.
(Previously: a change by any other author was reviewed only for a `high` change or when the user asks, with no judge; S2 adds the judges for every non-read-only goal.)

Existing scenarios stay unchanged except "Strong author, optional", which is replaced by:

#### Scenario: Strong author, lenses optional, judge on [W7, S2, auto]
- GIVEN a `code` worker on `claude>sonnet` that staged a `passive` change, the user did not ask for a lens review, and judge consent was granted
- WHEN its batch is due for review
- THEN no lens worker runs and one judge worker runs; with judge consent declined no review worker runs at all and the batch is marked `not judged`

#### Scenario: Cheap author, judge needs the run consent [S2, auto]
- GIVEN a `simple` worker on a local model that staged a change, and a declined judge consent
- WHEN its batch is due for review
- THEN the lens review still runs with no per-change prompt, and no judge runs

### Requirement: Worker contract [S1, W8, S2, S3]
A worker MUST be started from a `WorkerSpec` holding: kind, prompt, the engine and model resolved for that kind at spawn time (`general`, `code`, `chat`, `review`, `summarize`, `simple`, `judge`; the run's override for the kind if one exists, else the menu), the toolset for the kind as limited by the menu ceiling, and a deadline (default 10 minutes, key `worker_timeout_secs`; time spent waiting for the user does not count). It MUST end with a `WorkerResult` holding: status (`done`, `failed`, `cancelled`, `timed_out`, `refused`), the final text (capped and wrapped as untrusted data), elapsed time, and the provider error class and message when the status is `failed`. A worker MUST be one stateless process per turn with a private working folder and the same jail as a chat turn. The engine and model used MUST be those in effect at spawn time (run override, else menu), even if the menu changes later during the run. [S2, S3] A `judge` worker is read-only and is spawned only by the review, never by the orchestrator's `spawn_agent`; its engine is the effective `judge` assignment, except the second judge of a pair, which follows `acceptance-judges` (Which model judges). Until S3 a judge runs on the effective `review` assignment.
(Previously: six kinds without `simple`, a fixed deadline that counted every second, a toolset not limited by the menu; S2 and S3 add the review-only kind `judge`.)

Existing scenarios stay unchanged. Added scenarios:

#### Scenario: Judge workers run on the judge-a and judge-b assignments [S3, auto]
- GIVEN `judge-a` is `claude` / `opus`, `judge-b` is unassigned and `review` is `codex` / `default`
- WHEN the review spawns the first judge
- THEN the fake runner receives a request for claude with model `opus`, and the second judge of a pair receives codex with model `default`

#### Scenario: The orchestrator cannot spawn a judge [S2, auto]
- GIVEN an orchestrator turn
- WHEN `spawn_agent` is called with kind `judge`
- THEN it is refused like any unknown kind

### Requirement: A run ends by itself [S1, S4]
A run MUST end by itself, as a cancel does but with nothing left to stop, once its orchestrator turn has ended and none of its workers is queued or running (user decision, 2026-10-29): the pane then has no active run and Ctrl+O works again with no key pressed. A run halted on a failure MUST NOT end by itself: the alert still needs an answer, and after a retry the run ends once the relaunched worker is done. Ctrl+C, the alert's cancel keys and a new run of the pane end it as before. [S4] A run that applied a change and has no passing test command recorded after its last apply MUST NOT end by itself either: it waits for the user's answer to the tests-evidence prompt (see `acceptance-judges`, Tests evidence before the run closes), as a halted run waits for its alert.
(Previously: a run ended by itself whenever its turn and workers were done, whatever the tests evidence.)

Existing scenarios stay unchanged. Added scenarios:

#### Scenario: Applied change with no tests holds the run [S4, auto]
- GIVEN an orchestrator turn and workers done, one batch applied and no test command recorded after it
- WHEN the run would end
- THEN it does not end, shows `no tests ran · Enter to finish anyway · Esc to keep working`, and Ctrl+O is not yet available

#### Scenario: Docs-only change does not hold the run [S4, auto]
- GIVEN one applied batch whose files are all documentation and no test command recorded
- WHEN the run would end
- THEN it ends by itself with `tests: no tests needed (docs only)` and no prompt

#### Scenario: Enter ends the held run [S4, auto]
- GIVEN the held run
- WHEN Enter is pressed
- THEN the run ends once and Ctrl+O toggles the mode again

#### Scenario: Esc keeps the held run open [S4, auto]
- GIVEN the held run
- WHEN Esc is pressed
- THEN the run stays and the orchestrator can go on
