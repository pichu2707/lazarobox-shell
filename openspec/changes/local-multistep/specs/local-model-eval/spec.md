# Local Model Eval Specification (new capability)

## Purpose

How lazarobox measures whether a local model finishes a multi-step coding task: an engine-neutral scenario format, a pure scorer, a CI-safe replay guard, an opt-in live harness against local Ollama, a pass and acceptance rule, curated fixtures and a stable plain-text score table. It is a developer tool in this change (C5): nothing here is shown to the end user, and nothing here changes how a chat turn behaves (C1). Tags: [auto] = `cargo test` (no network beyond fake servers on ephemeral ports); [live] = `#[ignore]` test against the owner's local Ollama, never run in CI; [manual] = a person reads or runs it. Slice tags: [S1] is the whole of this capability; the acceptance requirement is applied by every later slice (S2-S5).

Traceability: C1-C5, C8-C11 and "Future direction" of `proposal.md`.

## ADDED Requirements

### Requirement: Scenario format [S1]
A scenario MUST be plain data with these fields: an id, a task category, the request text (what the user types), the expected steps, the order constraints, the required literals and whether the request is ambiguous. An expected step MUST name a tool and an argument predicate (for example, the `git_commit` message equals the required literal, or the `run_command` command contains `pytest`). An order constraint MUST name two expected steps, the first of which MUST precede the second in the exchanges. A required literal MUST be a string the user gave in the request, that MUST appear verbatim in the arguments of a named step. The task category MUST be one of `write-and-test`, `edit-existing`, `read-only` (more MAY be added later) and MUST be carried to the score table. A scenario MUST NOT mention an engine, a wire format, a model name or an HTTP detail, so that the same scenario scores an Ollama run and, later, a llama.cpp run without new code (C11). The scenario MUST be built from values the scenario itself holds, never read from the environment.

The set of scenarios shipped by S1 MUST be the three tasks of C2: (a) category `write-and-test`: a Fibonacci function, a pytest test for it, run the test, and commit with the user's literal message; (b) category `edit-existing`: edit an existing file of the throwaway repo without breaking its tests; (c) category `read-only`: a question about the repo that MUST NOT produce a write, exec or commit step, with `ambiguous` false unless the scenario states otherwise.

#### Scenario: Engine-neutral scenario [auto]
- GIVEN the three shipped scenarios
- WHEN their fields are inspected
- THEN none holds an engine id, model name, URL, port or wire field name, and each has a category

#### Scenario: Task (a) expectations [auto]
- GIVEN scenario (a)
- WHEN its expected steps are listed
- THEN it expects a file write, a batch proposal, a test run and a commit, the batch proposal precedes the commit, the test run precedes the commit, and the commit message is a required literal

#### Scenario: Task (c) forbids writes [auto]
- GIVEN scenario (c)
- WHEN a run holds a write, exec or commit exchange
- THEN the scorer reports it as a failure of that run (see Scoring)

### Requirement: Scorer [S1]
The scorer MUST be a pure function of a scenario and the ordered list of tool exchanges of one run plus the final answer text (no I/O, no clock, no engine type). It MUST return, for the run: the steps `completed`, the steps `missing`, the pairs `out_of_order`, the required literals `ignored` and a flag `asked_back` (see below). A step is completed when an exchange matches its tool and argument predicate and the exchange was not refused, denied or failed. A pair is out of order when both steps completed and the second step's exchange precedes the first's. A literal is ignored when the step that must carry it completed without it, or no step carried it. A tool call that appears only in the answer text MUST NOT count as an exchange (a text call is not a call, see chat-engines). The scorer MUST NOT read the file system, the repo or the network.

`asked_back` MUST be true when the final answer ends in a question or asks the user for an input that the request already gave, and no required step is missing for another reason. Per C10 it is a failure in every scenario whose `ambiguous` flag is false (for example `Please provide a commit message` when the request gave one), and it is a pass only in a scenario flagged ambiguous (task (c) variants that are genuinely ambiguous). The scorer MUST NOT decide ambiguity from the answer; it reads the scenario flag.

A run MUST be reported as passed only when no step is missing, no pair is out of order, no literal is ignored, `asked_back` is not a failure for the scenario and, for a `read-only` scenario, no write, exec or commit exchange exists. Otherwise it is failed, with every reason listed.

#### Scenario: Complete run [auto]
- GIVEN scenario (a) and exchanges write, propose batch, run pytest, commit with the literal message
- WHEN scored
- THEN nothing is missing, out of order or ignored, and the run passed

#### Scenario: Missing step [auto]
- GIVEN scenario (a) and a run that never runs pytest
- WHEN scored
- THEN `missing` holds the test-run step and the run failed

#### Scenario: Out of order [auto]
- GIVEN scenario (a) and a run that commits before it proposes the batch
- WHEN scored
- THEN `out_of_order` holds the batch-before-commit pair and the run failed

#### Scenario: Ignored literal [auto]
- GIVEN scenario (a) and a commit whose message differs from the user's literal one
- WHEN scored
- THEN `ignored` holds that literal and the run failed

#### Scenario: Refused exchange is not completed [auto]
- GIVEN a commit exchange whose result is a refusal or a denial
- WHEN scored
- THEN the commit step is missing

#### Scenario: Text call does not count [auto]
- GIVEN an answer whose text is a JSON call to the commit tool and no commit exchange
- WHEN scored
- THEN the commit step is missing

#### Scenario: Asking back with every detail given fails [auto]
- GIVEN scenario (a) and a run that ends with `Please provide a commit message`
- WHEN scored
- THEN `asked_back` is true, it counts as a failure for that scenario, and the run failed

#### Scenario: Asking back in an ambiguous request passes [auto]
- GIVEN a scenario flagged ambiguous and a run that ends with a clarifying question and no write step
- WHEN scored
- THEN `asked_back` is true, it is not a failure, and the run passed

#### Scenario: Read-only run must not write [auto]
- GIVEN scenario (c) and a run with a `read_file` exchange and a `write_file` exchange
- WHEN scored
- THEN the run failed because of the write

#### Scenario: Scorer is pure [auto]
- GIVEN the same scenario and exchanges scored twice
- WHEN the results are compared
- THEN they are equal and no file, socket or clock was touched

### Requirement: Pass rule and acceptance rule [S1; applied by S2-S5]
A model x task combination is measured over 5 runs and passes at 4 of 5 passed runs (C2). A run that takes more than 10 minutes of turn time counts as a failed run, whatever it produced (C8); otherwise time does not decide a pass. A change is accepted only if (1) at least one model x task combination raises its pass count, and (2) no combination drops below passing, that is, below 4 of 5. A move from 5 of 5 to 4 of 5 on a combination that still passes does not count as lowering (C9). When a result is borderline (a combination at exactly the pass threshold on either side of the comparison), the 5 runs of that combination MUST be repeated before deciding, and the repeated result decides. The accepted set of models is `qwen2.5:7b`, `qwen2.5-coder:14b` and `qwen3:14b`; `qwen3:14b-nothink` and smaller models are informative and never block acceptance. Every slice after S1 MUST attach a before and after table for the accepted set (see Score table); S1 attaches the baseline and is accepted without a comparison, because it changes no behaviour (C1).

#### Scenario: Pass at 4 of 5 [auto]
- GIVEN five run results for a combination, four passed
- WHEN the combination is judged
- THEN it passes, and with three passed it fails

#### Scenario: Over ten minutes fails [auto]
- GIVEN a run that satisfied every step but took 10 minutes and 1 second
- WHEN it is judged
- THEN it counts as a failed run; a run of exactly 10 minutes is judged by its steps

#### Scenario: Acceptance raises without lowering [auto]
- GIVEN a before table and an after table
- WHEN acceptance is evaluated
- THEN it is accepted when some combination gained passes and none fell below 4 of 5, and refused when any combination fell below 4 of 5 even if another rose

#### Scenario: Noise inside the passing band [auto]
- GIVEN a combination that went from 5 of 5 to 4 of 5 and one other that rose
- WHEN acceptance is evaluated
- THEN the 5 to 4 move is not a lowering and the change is accepted

#### Scenario: Borderline repeats [auto]
- GIVEN a combination at exactly 4 of 5 in the after table and 5 of 5 before, or 3 of 5 before and 4 of 5 after
- WHEN acceptance is evaluated
- THEN the evaluation reports that the 5 runs of that combination must be repeated before a decision

### Requirement: Replay guard in CI [S1]
An in-crate test MUST replay scripted transcripts through the real Ollama tool loop (`tool_turn_with`) with the `Fake` server and a real `Dispatch` lab for file reads, file writes and batch tools, while `run_command` and the git tools get fixed, non-refusal answers (so CI needs no sandbox, python3, pytest or git), then score the resulting exchanges with the scorer. The scripted transcripts MUST include a good run (scored passed), a run that skips the pytest step, a run with the wrong commit message, and a run that commits before it proposes the batch; each bad run MUST fail for its own reason (missing, ignored literal, out of order) and for no other. The replay MUST run offline in `cargo test`, MUST use no Ollama, no model and no network beyond the `Fake` server on `127.0.0.1`, and MUST run only inside a throwaway lab that it removes at the end. The replay checks the loop and the scorer, not model quality; it MUST NOT be reported as a score.

#### Scenario: Good transcript passes [auto]
- GIVEN the scripted good run
- WHEN it is replayed and scored
- THEN the run passed

#### Scenario: Each bad transcript fails for the right reason [auto]
- GIVEN the three scripted bad runs
- WHEN each is replayed and scored
- THEN the first reports the test-run step missing, the second reports the commit literal ignored and the third reports the batch-before-commit pair out of order, and each reports nothing else

#### Scenario: CI-safe [auto]
- GIVEN the replay test
- WHEN it runs without Ollama installed and without network access beyond loopback
- THEN it passes

### Requirement: Live harness is opt-in and never in CI [S1]
The live harness MUST be an `#[ignore]` test: `cargo test` and CI MUST NOT run it, and no CI workflow MUST pass `--ignored` or `--include-ignored` for it. It MUST read the model from an environment variable and the number of runs from another one (default 5), MUST talk only to the local Ollama on `127.0.0.1`, MUST NOT pull, download or delete a model, and MUST fail with a clear message (not a skip that looks like a pass) when Ollama or the model is not there. It MUST run tasks (a), (b) and (c) for the chosen model, measure the turn time of every run, score every run with the scorer and print the score table (see Score table). Approvals inside a live run MUST be answered by the harness itself according to the scenario (it approves the staged batch and the exec and commit steps of the scenario, and nothing else); an approval for a step the scenario does not expect MUST be denied and counted as an unexpected exchange.

#### Scenario: Ignored by default [auto]
- GIVEN the repo with its CI workflows
- WHEN the test list is inspected
- THEN the live test is marked ignored and no workflow runs ignored tests

#### Scenario: Missing model is a failure, not a pass [live]
- GIVEN Ollama running without the model named by the environment variable
- WHEN the live test runs with `--ignored`
- THEN it fails with a message naming the missing model, and it pulls nothing

#### Scenario: Unexpected approval denied [auto]
- GIVEN a replayed run whose model asks for a write tool the scenario does not expect
- WHEN the harness answers the approval
- THEN it is denied and the exchange is reported as unexpected

#### Scenario: Live run of the three tasks [live]
- GIVEN Ollama with one model of the accepted set and the environment variables set
- WHEN the live test runs with `--ignored`
- THEN it prints one table with 3 tasks x N runs for that model

### Requirement: Throwaway repository outside /tmp [S1]
Every live run (and every replay lab that needs a git repo) MUST work in a throwaway git repository that the test creates and removes. It MUST live outside `/tmp` (the jail refuses `/tmp` roots as too broad, see agent-tools), in a location under the user's home or the build folder that the tools' path policy accepts, and MUST be unique per run so two runs never share files. It MUST contain only generated content (a minimal project with a test) and MUST NEVER be, contain or link to the owner's projects. It MUST be removed at the end of the test, also when the test fails or panics. The live run MUST NOT change the user's global git config (author and email MUST be set in the repo's own config).

#### Scenario: Not under /tmp [auto]
- GIVEN the function that picks the throwaway location
- WHEN it runs with `TMPDIR=/tmp`
- THEN the result is not `/tmp` or below it

#### Scenario: Removed on failure [auto]
- GIVEN a run that panics after the repo was created
- WHEN the test ends
- THEN the repo no longer exists

#### Scenario: Global config untouched [auto]
- GIVEN a run that commits
- WHEN it ends
- THEN the author and email were set in the throwaway repo's own config and the global git config is byte-identical

### Requirement: Fixture curation [S1]
A transcript MAY be stored under `tests/fixtures/ollama/` as a fixture only after curation (C4): it MUST hold messages and tool calls only; MUST hold no absolute path (a path MUST be replaced by a relative one under a neutral root), no user name, no host name, no email, no token and no real file content (file contents MUST be replaced by generated placeholder content); and MUST come from a throwaway repo, never from the owner's projects, because the repository is public. Fixtures MUST be curated by hand; this change ships no recorder in the app. A test MUST scan every fixture file and fail when it finds an absolute path outside the neutral root, the value of `$HOME`, the value of the current user name, an email-like string or a token-like string.

#### Scenario: Fixture scan passes on curated fixtures [auto]
- GIVEN the shipped fixtures
- WHEN the scan runs
- THEN it finds no absolute path, home path, user name, email-like or token-like string

#### Scenario: Scan catches a leak [auto]
- GIVEN a candidate fixture that holds `/home/<user>/project/x.py`
- WHEN the scan runs on it
- THEN it fails and names the file and the kind of leak, without printing the matched value in a shared log

#### Scenario: Fixtures hold tool exchanges only [auto]
- GIVEN each shipped fixture
- WHEN it is parsed
- THEN it holds only messages and tool calls and results, and no field outside that set

### Requirement: Score table [S1]
The live harness MUST print the score as plain text, in a stable format that a person and a later script can read, with one header block and one row per model x task, and a per-category summary. The header block MUST hold the date (`YYYY-MM-DD`), the Ollama version, the hardware (GPU name and its memory, or `cpu` when none; CPU model and RAM MAY be added) and the number of runs. Each row MUST hold the model, the task id, the category, the passed count over the run count, the failure reasons by kind (missing, out of order, ignored literal, asked back, over time), the turn time of every run in seconds and the median turn time in seconds. A run over 10 minutes MUST be marked as failed by time. The summary MUST list, per model x category, the passed combinations over the combinations of that category, so a later change can publish which models fit which kind of work (Future direction). The format MUST NOT depend on the terminal (no color, no width reflow, no escape sequences), MUST NOT contain a path, a user name or a host name, and MUST be the same text the PR attaches as baseline or before and after.

#### Scenario: Header and rows [auto]
- GIVEN scored runs for two models and three tasks
- WHEN the table is rendered
- THEN the header holds date, Ollama version, hardware and run count, and there is one row per model x task with `passed/runs`, the failure kinds, the per-run times and the median

#### Scenario: Median of five [auto]
- GIVEN five turn times 10, 20, 30, 40 and 50 seconds
- WHEN the row is rendered
- THEN the median reads 30 and the five times are listed

#### Scenario: Per-category summary [auto]
- GIVEN rows of tasks in categories `write-and-test`, `edit-existing` and `read-only`
- WHEN the table is rendered
- THEN a summary lists each model x category with the combinations passed over the total

#### Scenario: Stable plain text [auto]
- GIVEN the same inputs rendered twice
- WHEN the outputs are compared
- THEN they are byte-identical, contain no escape sequence, and hold no absolute path or user name

#### Scenario: Baseline table recorded [manual]
- GIVEN the owner's machine with `qwen2.5:7b`, `qwen2.5-coder:14b`, `qwen3:14b` and `qwen3:14b-nothink`
- WHEN the live harness runs for each
- THEN the PR of S1 attaches the baseline (3 models x 3 tasks x 5 runs, plus the informative `qwen3:14b-nothink`)

### Requirement: S1 changes no behaviour [S1]
The S1 change MUST NOT modify the Ollama request body, the system prompt, any reminder, any round limit, `think`, any profile or any tool text. Its production code is the scorer, the scenario data, the table renderer and what the replay and live tests need to reach `Dispatch` and the `Fake` helpers.

#### Scenario: Request bytes unchanged [auto]
- GIVEN the existing recorded-request tests of the Ollama adapter
- WHEN S1 is applied
- THEN they pass without edits to their expectations
