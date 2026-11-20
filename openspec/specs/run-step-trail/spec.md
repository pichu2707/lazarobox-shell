# Run Step Trail Specification

## Purpose

Tool calls are not stored in `chat.db`, so a run that stopped at the turn cap, hit the run timeout, was cancelled or hit an engine error could not be diagnosed after the fact or after a restart. The run step trail is a per-run, capped, cleaned and redacted list of one-line steps, stored with the run and shown to the user. Issue #771. Tags: [auto] = `cargo test`; [manual] = real machine.

Terms: "Trail" = the ordered list of step lines of one run, plus a count of dropped steps ("cut"). "Step" = one line: a coordinator tool call, a handoff, a worker tool call, or the resume separator. "Coordinator step" = a tool call of the coordinator's turn or a handoff. "Worker step" = one tool call of a worker. "Interrupted run" = a run still open when the app stopped. "Resumed run" = a run started from an interrupted one (`Prefix r r`); its "predecessor" is that interrupted run. "Generated text" = text lazarobox builds itself: tool name, argument summary, outcome class and a reason from a fixed lazarobox set.

## Requirements

### Requirement: Step lines (shipped, Slices 1-3)
Each tool call of the coordinator's turn MUST become one trail line holding the tool name, a one-line argument summary and the outcome: `ok`, `refused: <reason>` or `error: <text>`. Each handoff MUST become one line holding the unit, the verdict and the reason. A long call MUST keep how it ended: the call text is cut, the outcome is not. Judge steps are out of scope; they MAY later use the same `Trail::call` / `Trail::handoff` entry points unchanged.

#### Scenario: Call and outcome [auto]
- GIVEN a coordinator turn that calls a tool and gets a refusal
- WHEN the call result arrives
- THEN the trail holds one line with the tool, the argument summary and `refused: <reason>`

#### Scenario: Long call keeps its outcome [auto]
- GIVEN a call whose argument summary is longer than the line cap
- WHEN it is stored
- THEN the line is within the cap and still ends with its outcome

#### Scenario: Handoff line [auto]
- GIVEN a handoff of a unit with a verdict and a reason
- WHEN the handoff event arrives
- THEN the trail holds one line with unit, verdict and reason

### Requirement: Cap and cut note (shipped)
A trail MUST hold at most a fixed number of steps of at most a fixed number of bytes each (`trail::LINE_BYTES`; the code value, 180 bytes, wins over the 160 in the original proposal). When a step is dropped, the count of dropped steps MUST be kept, and any report of the trail MUST say "N earlier steps not shown" with that count. The cut note MUST be visible, never omitted when N is above zero.

#### Scenario: Over the cap [auto]
- GIVEN a run with more steps than the cap
- WHEN the trail is shown
- THEN only the newest steps are listed and the report says "N earlier steps not shown" with the exact count

### Requirement: Cleaning and redaction (shipped)
Every stored line, including handoff reasons, MUST pass the same helpers as the transcript: `clean`, `scrub` and `redact_tokens`. `scrub` MUST mask, best effort, secret-looking values in JSON members, `name=value`, `--name value`, headers, URL query values, curl `-u`, `sshpass -p` and `mysql -pVALUE`.

#### Scenario: Token in an argument [auto]
- GIVEN a call whose argument summary holds a token and a `--password value` pair
- WHEN it is stored and the trail is shown
- THEN neither the token nor the value appears in the stored text or the report

### Requirement: Storage and survival (shipped)
The trail MUST be stored with its run in one nullable `trail` value of the run row, written as a first line `cut N` followed by one plain line per step. It MUST survive a restart and MUST be read back with the run on recovery. A save carrying no trail MUST leave the stored trail unchanged. No schema version bump is required; older builds MUST ignore the value.

#### Scenario: Round trip [auto]
- GIVEN a trail with a cut count and several steps
- WHEN it is written to text and read back
- THEN the lines and the cut count are equal

#### Scenario: Save without a trail [auto]
- GIVEN a run row with a stored trail
- WHEN a save arrives that carries no trail
- THEN the stored trail is unchanged

#### Scenario: Survives a restart [auto]
- GIVEN a run with steps, saved, and the app restarted
- WHEN the interrupted run is read
- THEN its trail is available with the same lines

### Requirement: Where the trail is shown (shipped)
The trail MUST be shown, as system lines, with the stop report of a turn cap, a run timeout and a cancel; under an engine error of the coordinator turn while the run is still open; with the interrupted-run lines after a restart; and on a clean finish. A trail MUST be listed once per stop: when a stop report and an end both apply, it appears one time. It MUST NOT be listed for a replaced run.

#### Scenario: Stop report [auto]
- GIVEN a run stopped at the turn cap
- WHEN the stop report is shown
- THEN the newest steps are listed after it as system lines

#### Scenario: Engine error [auto]
- GIVEN a coordinator turn that ends with an engine error and the run still open
- WHEN the error is shown
- THEN the trail is listed under the error

#### Scenario: Listed once [auto]
- GIVEN a run that stops and then ends
- WHEN both events are handled
- THEN the trail appears one time

#### Scenario: Clean finish [auto]
- GIVEN a run that finishes cleanly
- WHEN it ends
- THEN its trail is listed, and a replaced run lists none

### Requirement: Never silent on a failed save (shipped)
When a run save fails, the app MUST say in a visible notice that the step trail may be missing. Steps of a run that never started MUST NOT be dropped silently: a notice MUST say so when they are discarded.

#### Scenario: Save fails [auto]
- GIVEN a run save that fails
- WHEN the failure is handled
- THEN a visible notice says the step trail may be missing

### Requirement: Display only, never sent to a model
The trail MUST be shown only through system lines. It MUST NOT be stored as a chat message, MUST NOT be sent to any model, MUST NOT enter the resume seed (`resume_message`), and MUST NOT be read by compaction or summary. This holds for the carried trail and the worker steps below.

#### Scenario: Resume seed holds no trail [auto]
- GIVEN an interrupted run with a stored trail, including carried and worker lines
- WHEN the resume seed is built
- THEN no trail line appears in it

#### Scenario: No model request holds a trail [auto]
- GIVEN a pane with a trail
- WHEN the next model request is built
- THEN no trail line appears in it

### Requirement: Resumed run carries the predecessor's trail
When a run is started from an interrupted one, the new run's trail MUST start with the predecessor's stored trail, followed by one separator line `resumed from run <id>` naming the predecessor, and then the new run's own steps. Only the direct predecessor's stored trail MUST be read; a resume of a resumed run carries its predecessor's trail, which already holds the earlier one. The carried cut count MUST add to the new one, so the report still says "N earlier steps not shown" with the sum. The carried and own steps MUST respect the same cap rules; how the cap is split between carried and own steps, and how steps are evicted when the cap is reached, is a design decision. The predecessor's row MUST keep its own trail unchanged. The carried trail MUST survive another restart. When the carried trail is first shown (at resume time or only on the next stop report) is a design decision; whichever the design picks MUST be consistent and MUST NOT list the trail twice for one stop. A predecessor with no stored trail MUST start the new run as an ordinary empty trail with no separator.

#### Scenario: Resume carries steps [auto]
- GIVEN an interrupted run with a stored trail of several steps
- WHEN the user resumes it and the new run later stops
- THEN the stop report lists the predecessor's steps, then `resumed from run <id>`, then the new run's own steps

#### Scenario: Cut counts add [auto]
- GIVEN a predecessor trail with cut count 5 and a new run that drops 3 steps
- WHEN the trail is shown
- THEN the report says "8 earlier steps not shown"

#### Scenario: Survives another restart [auto]
- GIVEN a resumed run whose trail was carried and saved
- WHEN the app restarts and the interrupted run is read
- THEN its trail holds the carried lines, the separator and its own steps

#### Scenario: Predecessor row unchanged [auto]
- GIVEN a resumed run
- WHEN the new run adds steps and is saved
- THEN the predecessor's stored trail is unchanged

#### Scenario: No predecessor trail [auto]
- GIVEN an interrupted run with no stored trail
- WHEN it is resumed
- THEN the new run's trail has no carried lines and no separator

### Requirement: Worker tool calls enter the trail
Each worker tool call MUST be stored as one line of the form `worker <id>: <tool> <arg summary> -> ok | refused: <fixed reason> | error`, built from the worker's `OrchestraEvent::Trace` `Call` and `Result` items. The argument summary MUST go through `clean`, `scrub`, `redact_tokens` and the same byte caps as coordinator calls. An error outcome MUST be stored as `error` with no detail.

#### Scenario: Failed worker call after a restart [auto]
- GIVEN a worker whose tool call returned an error, and a restart
- WHEN the interrupted run is read
- THEN the trail has one line naming the worker, the tool and `error`

#### Scenario: Refused worker call [auto]
- GIVEN a worker call refused for a reason in the fixed set
- WHEN it is stored
- THEN the line ends with `refused: <that reason>`

### Requirement: Only generated text is stored
The stored trail MUST hold only generated text. It MUST NOT hold a worker's raw tool output, a result `summary` (it can carry file, issue or stderr text), or raw `gh` stderr. A refusal reason MUST be taken from a fixed set of lazarobox strings; an unknown reason MUST be stored as plain `refused`. This applies to the coordinator, worker and carried lines alike.

#### Scenario: Hostile result text [auto]
- GIVEN a worker tool result whose text carries a token and a fake instruction
- WHEN the call is stored
- THEN neither the token nor the instruction text appears in the stored trail

#### Scenario: Unknown refusal reason [auto]
- GIVEN a refusal whose reason is not in the fixed set
- WHEN it is stored
- THEN the line ends with plain `refused`

### Requirement: Worker lines never evict coordinator steps
Worker steps MUST have their own share of the cap, so that worker lines never push coordinator steps or handoffs out of the trail, however many calls workers make. The size of each share and the eviction order inside a share are design decisions. Steps dropped from the worker share MUST still be counted and shown through the "N earlier steps not shown" note, which MUST include them.

#### Scenario: Worker flood [auto]
- GIVEN workers that make far more calls than the cap, and a few coordinator steps and handoffs
- WHEN the trail is shown
- THEN every coordinator step and handoff is still listed

#### Scenario: Worker drops are counted [auto]
- GIVEN workers whose calls exceed their share
- WHEN the trail is shown
- THEN the cut note includes the dropped worker steps

### Requirement: Reports keep the last coordinator steps visible
A report of a trail MUST NOT let worker lines hide the newest coordinator steps. It MUST list the last 12 coordinator-share lines (coordinator calls, handoffs, separators) and, separately, the last 4 worker lines, so at most 16 lines are listed, in chronological order. The "N earlier steps not shown" note MUST count the steps of both shares that are stored or dropped but not listed.

#### Scenario: Busy workers [auto]
- GIVEN a trail with 30 coordinator-share lines and 30 worker lines
- WHEN the trail is shown
- THEN the report lists the last 12 coordinator-share lines and the last 4 worker lines, at most 16, in order

#### Scenario: Cut note counts both shares [auto]
- GIVEN a trail whose coordinator share and worker share both have lines not listed
- WHEN the trail is shown
- THEN the report says "N earlier steps not shown" with N the sum of both shares' unlisted and dropped steps

### Requirement: Stored format stays readable by older builds
The text of the stored trail MUST stay `cut N` on the first line followed by one plain line per step, with no new column or schema change, so rows written with carried or worker lines still read in older builds.

#### Scenario: Older reader [auto]
- GIVEN a trail text with a separator and worker lines
- WHEN it is read with the original `cut N` then plain lines parser
- THEN every line is read as a step and none fails to parse
