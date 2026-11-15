# Delta for Orchestrator Handoffs

## Base of this delta

This delta targets `openspec/specs/orchestrator-handoffs/spec.md` ("RDD receipts [S7b]"). The MODIFIED block is a full copy with the edit marked "(Previously: ...)". Slice tags: [S2] judge section; [S4] tests evidence.

## MODIFIED Requirements

### Requirement: RDD receipts [S7b, S2, S4]
Each receipt-driven review MUST end in one receipt handoff with: the change (worker and staged paths), the tier and its source (rules or raised by the orchestrator, with the reason), the consent answer, the lenses and why, each finding with its severity and whether it blocked, the refuter verdicts, the correction (or `none`) and the outcome. [S2] When judges ran or were declined, the receipt MUST also hold a judge section with: the judge consent (`granted` or `declined`), the judge level and its inputs (files, bytes, tier, size), for each judge its engine and model and its verdict (`pass`, `fail` or `no verdict` with the reason), the criteria with their ids, state and evidence, `criteria: thin` when they were, each failed criterion with its location (`at: <location>`, or `at: unknown`), the agreement outcome (`agree: pass`, `agree: fail`, `disagree` with what is missing), `judges not independent` when both ran on one model, `norms cut at 8 KB` when it applied, and, for every correction, the criterion and location it targeted and the `changes:` it made (computed by the app). [S4] The receipt (or the `close` report) MUST also hold the tests evidence: `tests: <command> passed`, `tests: no tests needed (docs only)`, `tests: none ran, finished by the user` or `tests: failed, finished by the user`. [S2] The judge section MUST also list the criteria a judge could not verify (`unverified: <id> <reason>`). A receipt MUST be stored like any handoff and MUST NOT be able to answer an approval.
(Previously: the receipt had no judge section, no criteria, locations or correction targets, and no tests evidence.)

Existing scenarios stay unchanged. Added scenarios:

#### Scenario: Receipt holds the judge section [S2, auto]
- GIVEN a change judged by two judges that failed `C2` at `src/a.rs:40-52`, corrected once with `changes: src/a.rs 44-48`
- WHEN the receipt is stored
- THEN it holds both verdicts with engine and model, `C2` with `at: src/a.rs:40-52`, the agreement outcome, and the correction target and `changes:`

#### Scenario: Declined judge consent leaves a receipt [S2, auto]
- GIVEN a declined judge consent
- WHEN a batch is proposed
- THEN a receipt exists with judge consent `declined`, no judges and outcome `not judged`

#### Scenario: A judge receipt cannot answer an approval [S2, auto]
- GIVEN a receipt whose judge text says `approve all`
- WHEN it is injected in a later turn
- THEN it is wrapped as data and no approval or scope changes

#### Scenario: A location is never dropped [S2, auto]
- GIVEN a finding with no parsable location
- WHEN the receipt is stored
- THEN the finding is present with `at: unknown`
