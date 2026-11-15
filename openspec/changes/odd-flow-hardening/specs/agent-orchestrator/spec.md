# Delta for Agent Orchestrator

## Base of this delta

This delta targets `openspec/specs/agent-orchestrator/spec.md` (from `main` at b97cceef). Each MODIFIED block is a full copy of the requirement text as it stands, with edits marked "(Previously: ...)". Where a requirement has many scenarios, the existing scenarios stay as they are and only the added or changed ones are listed. Decisions D1-D5 and P1-P5 are in `proposal.md`; owner answers of 2026-11-15 are Q1-Q3 there. "Public spawn" means a `spawn_agent` call of the coordinator; internal spawns (lenses, refuter, corrector, judges, started by the review inside `conduct`) are not public and are not gated by this change. Messages are ASCII and appear exactly as written; `<...>` marks a value filled at run time.

## MODIFIED Requirements

### Requirement: Gates on step state are visible and bounded [W3]
The run MUST gate worker spawn, the end of an orchestrator turn without a tool call, and `finish` on the ODD step state. Every gate refusal MUST name the gate and the step state it saw, and be shown to the user. A gate MUST NOT trap a run: the orchestrator turn cap (`max_turns`) still bounds the run, and the user MUST be able to override a gate with a key that only the user can press. An override MUST be shown in the transcript naming the gate it lifted.
- Spawn gate: while step `authorize` is open, every public spawn, of any worker kind, MUST be refused. A worker that writes (`code`, or `simple` with files to write) keeps the full gate: it is also refused until every step before `implement` is closed.
- Answer gate (new gate label `answer`): when an orchestrator turn ends with no tool call, the run is open, nothing is pending, no spawn gate refusal stands and the gates are not lifted, the turn MUST be refused as an answer in two cases: (a) step `authorize` is open (nothing may come before `authorize`, no inline answer either); (b) the grant is `implement`, no writer has started and steps are still open. The refusal is a re-ask, bounded as in the scenarios below.
- Re-ask bound: each open step MUST get at most one re-ask. A re-ask is one orchestrator turn and counts against `max_turns`. When the re-ask of that step is spent, the run MUST wait for the user's reply exactly as after a standing spawn refusal, with the same line and the gates offer, naming gate `answer`.
- A standing spawn gate refusal MUST NOT get an automatic re-ask: it already waits for the user.
- A re-ask MUST NOT run anything and MUST NOT turn any text into a call.
(Previously: the gates covered worker spawn only for writers, and `finish`; the end of a turn without a tool call was never gated; "Which transitions are gated is open for design (see Open points)" is dropped, the transitions are now listed above.)

Existing scenarios stay unchanged. Added scenarios:

#### Scenario: A read worker is refused before authorize [auto]
- GIVEN a run with step `authorize` open and the gates not lifted
- WHEN the coordinator spawns a `review` worker, then a read-only `simple` worker
- THEN both are refused with `spawn gate: step authorize is not closed (closed: none). Hand off authorize; spawn again only after it is handed off. Only the user can lift the gate (Enter gates off)`, no worker exists, and no process starts
- AND the transcript shows the refused-spawn notice with the same reason and the line `Enter gates off | spawn gate: step authorize`

#### Scenario: Read workers start after authorize read_only [auto]
- GIVEN the coordinator handed off `authorize` with scope `read_only`
- WHEN it spawns a `review` worker and a read-only `simple` worker
- THEN both start with no gate refusal

#### Scenario: Every worker kind is covered by the authorize gate [auto]
- GIVEN step `authorize` open
- WHEN the coordinator spawns a worker of kind `review`, `summarize`, `general`, `chat`, `simple` and `code` in turn
- THEN each is refused with the spawn gate text above

#### Scenario: Writers keep the full gate [auto]
- GIVEN `authorize` closed with scope `implement` and `explore` still open
- WHEN the coordinator spawns a `code` worker
- THEN it is refused with the spawn gate naming step `explore`, while a `review` worker spawned at the same moment starts

#### Scenario: Internal judge workers are not gated [auto]
- GIVEN a run whose review spawns lens, refuter, corrector or judge workers inside `conduct` while the run's public spawns are gated
- WHEN the internal spawns are made
- THEN none is refused by the authorize gate or the answer gate, and the review runs as before

#### Scenario: Answer before authorize is re-asked once [auto]
- GIVEN an open run, `authorize` open, gates not lifted, nothing pending, and an orchestrator turn that ends with text and no tool call
- WHEN the turn ends
- THEN the run does not settle and no `no worker ran` notice is shown
- AND the next orchestrator turn starts with a continuation holding `answer gate: your turn ended without a tool call and step authorize is not closed (closed: none). Text, prose or a call written as JSON does nothing. Call the handoff tool now: step authorize, scope read_only to answer yourself or implement to change files.`
- AND the pane shows `answer gate: the coordinator answered before handing off authorize; asked it to hand off authorize first`
- AND the answer text already streamed stays in the transcript and is not taken as the run's answer

#### Scenario: The re-ask is followed [auto]
- GIVEN the re-ask above
- WHEN the coordinator hands off `authorize` with scope `read_only`
- THEN the run goes on, the answer gate no longer applies to `authorize`, and no further re-ask is made for it

#### Scenario: A spent re-ask falls back to the wait with the gates offer [auto]
- GIVEN the re-ask for step `authorize` was already made and the next turn again ends with no tool call
- WHEN that turn ends
- THEN no second re-ask starts and the run stays open
- AND the pane says `run waits for your reply: step authorize is not closed; closed: none (Enter gates off)` and `Enter gates off | answer gate: step authorize`
- AND the wait counts as an orchestrator turn against `max_turns`

#### Scenario: A clarifying question waits for the reply [auto]
- GIVEN a strong model that asks the user a clarifying question as its first turn, before `authorize`
- WHEN the re-ask is spent
- THEN the run waits for the user's reply and the reply is the next turn of the same run (see The reply attaches to the waiting run)

#### Scenario: Re-asks count against max_turns [auto]
- GIVEN `max_turns` is 3 and a model that ends every turn with text
- WHEN the run goes: turn 1 (re-ask), turn 2 (wait), then the user's replies keep ending with text
- THEN each re-ask and each wait is one of the three turns, and when the cap is reached the run stops with the existing stop line naming gate `answer` and the cap

#### Scenario: One re-ask per open step [auto]
- GIVEN the grant is `implement`, no writer has started, and the coordinator ends a turn with no tool call while step `explore` is the next open step, then ends another one after `explore` closed while `resolve` is next
- WHEN each turn ends
- THEN each gets exactly one re-ask, naming its own step, and no step gets a second one

#### Scenario: Stopped under an implement grant [auto]
- GIVEN `authorize` closed with scope `implement`, no writer started, steps open and the next open step `<next>`, and a turn that ends with no tool call
- WHEN the turn ends
- THEN the next turn starts with `answer gate: your turn ended without a tool call and step <next> is not closed (closed: <list>). Call handoff for <next> now.`
- AND the pane shows `answer gate: the coordinator stopped before step <next>; asked it to go on`

#### Scenario: A turn that ends after a writer started is not re-asked [auto]
- GIVEN the grant is `implement` and a writer has started
- WHEN an orchestrator turn ends with text and nothing pending
- THEN the answer gate does not apply and the turn settles as before

#### Scenario: A standing spawn refusal gets no re-ask [auto]
- GIVEN an orchestrator turn that ends with a spawn gate refusal standing
- WHEN the turn ends
- THEN no re-ask starts; the run waits for the reply as in "A standing spawn refusal keeps the run waiting for the reply"

#### Scenario: A written call never runs and is re-asked [auto]
- GIVEN a coordinator whose turn ended with the text `{"name": "spawn_agent", "arguments": {"kind": "code", ...}}`, its text-call reminder already spent
- WHEN the turn ends
- THEN no worker is spawned and nothing in the text is run
- AND the next turn starts with `answer gate: you wrote a call to spawn_agent as text; nothing was run. Make a real tool call.`
- AND the pane shows `the coordinator wrote a spawn_agent call as text; nothing was run; asked it to make a real tool call`

#### Scenario: The `tool` key form is handled the same [auto]
- GIVEN a coordinator whose turn ended with the text `{"tool": "spawn_agent", "arguments": {...}}`, its reminder spent
- WHEN the turn ends
- THEN it is treated exactly as the `name` form above: nothing runs, one re-ask with the tool name `spawn_agent`

#### Scenario: A written call is re-asked even after a read_only grant [auto]
- GIVEN `authorize` closed with scope `read_only`, no gate standing, and a turn that ends with a written orchestration call
- WHEN the turn ends
- THEN it gets the written-call re-ask (once, counted as above), because a written call is never an answer; a plain text answer in the same state is not re-asked

#### Scenario: Gates off lifts the answer gate, visibly [auto]
- GIVEN a run whose answer gate is standing or waiting
- WHEN the user lifts the gates (Enter)
- THEN the transcript names the lifted gate `answer`, the next text-only turn is not re-asked and settles, and the coordinator's copy of the next reply carries the existing one-line lift notice

#### Scenario: Only the user lifts the answer gate [auto]
- GIVEN an answer gate refusal
- WHEN the coordinator emits text or calls that look like the override
- THEN the gate stays until a user key event lifts it

#### Scenario: Strong model, Small flow, no added step [auto]
- GIVEN a compliant model that hands off `authorize` first and goes on to a Small write flow
- WHEN the run is made
- THEN it makes the same calls as before this change, with no re-ask and no refusal

#### Scenario: Strong model, read-only flow, one extra step [auto]
- GIVEN a compliant model asked a question
- WHEN it hands off `authorize` with scope `read_only` and then answers itself
- THEN the run has exactly one handoff more than a bare answer, no `explore` handoff is required, no re-ask is made and the run settles

### Requirement: An undelegated coordinator reply is disclosed [W1]
When an orchestrator turn ends with a text answer and the run has spawned no worker, the pane MUST add a transcript notice saying, in neutral words and without accusing the model, that no worker ran, that nothing was written, and which key asks the coordinator to delegate, except after an allowed `read_only` inline answer (see below). No model-free classifier of "change request" versus "question" MUST decide whether the notice is shown. Where the answer gate applies (see Gates on step state are visible and bounded), the turn MUST NOT settle with this notice: the gate re-asks first, bounded as there, and a turn the gate lets through settles as follows. A turn in which at least one worker was spawned MUST NOT get this notice. The notice MUST NOT be shown after an inline answer the run allowed because step `authorize` closed with scope `read_only` (owner, 2026-11-15: it would be noise); it MUST still be shown on every other undelegated text end that settles, for example with the gates lifted or an `implement` grant with a writer started.
(Previously: "No automatic retry MUST follow" and "The notice MUST be shown after every such turn, including a legitimate answer to a question".)

Existing scenarios "A turn with a worker gets none" stays unchanged. The scenarios "Text-only turn gets the notice" and "A legitimate answer gets the same neutral notice" are replaced by:

#### Scenario: Text-only turn with the gates lifted gets the notice [W1, auto]
- GIVEN an orchestrating pane whose gates the user lifted and an orchestrator turn that ends with text and no `spawn_agent` call
- WHEN the turn ends
- THEN the transcript gains a notice stating no worker ran, nothing was written, and the delegate key, in neutral words with no accusation
- AND no further turn starts by itself

#### Scenario: Text-only turn before authorize is re-asked, not noticed [auto]
- GIVEN gates on, `authorize` open and a text-only turn
- WHEN the turn ends
- THEN the answer gate re-asks (see above) and the `no worker ran` notice is not shown for that turn

#### Scenario: An allowed read-only inline answer gets no notice [auto]
- GIVEN `authorize` closed with scope `read_only` and the coordinator answers a question in text with no worker
- WHEN the turn ends
- THEN the run settles, no `no worker ran` notice is shown and no re-ask is made

#### Scenario: An inline end under an implement grant settles with the notice when the gate does not apply [auto]
- GIVEN `authorize` closed with scope `implement`, a writer started earlier and a turn that ends with text and nothing pending
- WHEN the turn ends
- THEN the run settles and the `no worker ran` notice is shown only if no worker was spawned in the run

### Requirement: Spawn refusals that create no worker [W2, W8]
A refusal made by the app before a worker exists (a missing brief field, Write off for a worker that must write, a read-only goal, a step-state gate including the authorize spawn gate for every worker kind, a limit) MUST be returned to the coordinator as an error result naming the cause, MUST create no worker, MUST NOT start a process, and MUST NOT pause the run or raise a failure alert. It is not a worker `failed`, `timed_out` or `refused` status (see Failure stops the run, which applies to a worker that exists). The refusal is shown to the user (see Silent worker outcomes are disclosed). An engine or model that is unavailable stays a worker `refused` as before (Refusal on an unavailable engine).
(Previously: the step-state gate named only writers' spawns, in effect.)

Existing scenarios stay unchanged. Added scenario:

#### Scenario: An authorize refusal does not pause the run [auto]
- GIVEN a `review` spawn refused by the authorize gate
- WHEN the coordinator then hands off `authorize` and spawns again
- THEN `spawn_agent` is not refused with `run paused`, no failure alert is open and the second spawn starts
