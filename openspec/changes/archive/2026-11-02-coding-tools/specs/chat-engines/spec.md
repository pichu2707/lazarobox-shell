# Delta for Chat Engines

## Base of this delta

`openspec/specs/chat-engines/spec.md` exists. The Ollama native tools requirement is added by the unarchived `agent-tools` change, so `agent-tools` MUST be archived first. Only ADDED requirements are used.

## ADDED Requirements

### Requirement: Ollama round limit with coding tools
The Ollama tool loop MUST allow at most 8 rounds per turn when no coding-tools family is on, and at most 24 rounds per turn when at least one family toggle (search, write, exec, git, git write) is on and tools are on. The limit of calls per reply MUST NOT change. When the limit is reached the turn MUST end with the existing round-limit behaviour, and a staged batch MUST stay staged (see code-write).

#### Scenario: Default stays 8 [auto]
- GIVEN tools on and every family toggle off
- WHEN an Ollama turn keeps asking for tools
- THEN the loop stops after 8 rounds

#### Scenario: Raised to 24 [auto]
- GIVEN tools on and the Search toggle on
- WHEN an Ollama turn keeps asking for tools
- THEN the loop stops after 24 rounds

#### Scenario: Tools off [auto]
- GIVEN tools off and a family toggle on
- WHEN the request is built
- THEN it is identical to a request without tools and the limit is irrelevant

#### Scenario: Limit leaves the batch [auto]
- GIVEN a staged batch and a turn that hits the limit
- WHEN the turn ends
- THEN the batch is still staged

### Requirement: A tool call written as text gets one reminder
When a reply of the Ollama or llama.cpp tool loop holds no structured tool call, but its text holds a JSON object, bare or fenced, whose `name` is a tool offered in that turn and which has an `arguments` (or `parameters`) object, the loop MUST send one more request in the same turn. That request MUST carry the reply as plain assistant text, then a user message: ``Your answer wrote a call to `<name>` as text. Text does nothing. To act, call the tool.`` The pane MUST get the system line `the model wrote a tool call as text; it was asked to call the tool`. This MUST happen at most once per turn, MUST count toward the turn's request cap, and MUST NOT happen in the retry without tools. Nothing in the text MUST ever be run or turned into a call (local-model-writes D3).

#### Scenario: One reminder [auto]
- GIVEN tools on and a reply whose text is a fenced call to `find_dir` with arguments
- WHEN the loop reads the reply
- THEN a second request carries the text and the reminder, with tools still offered, and only the call the model makes after it runs

#### Scenario: Second text call ends the turn [auto]
- GIVEN a model that answers with a text call twice
- WHEN the loop reads the second answer
- THEN the turn ends with `Done` after two requests

#### Scenario: Not a call [auto]
- GIVEN an answer that names a tool without arguments, a call to a tool not offered, or a turn without tools
- WHEN the loop reads it
- THEN the turn ends as before, with no reminder

#### Scenario: Cap [auto]
- GIVEN a text call in the last reply the cap allows
- WHEN the loop reads it
- THEN the turn ends with no reminder

### Requirement: A next step announced in prose gets one reminder
When a reply of the Ollama or llama.cpp tool loop holds no structured tool call and no text call, and the last sentence of its text announces an action the model is about to take (one of the whole-word phrases `I'll`, `I will`, `I need to`, `I'm going to`, `I am going to`, `let me`, `next, I`, `next step`, `voy a`, `vamos a`, `ahora voy`, `a continuación`, `lo siguiente`, `procedo a`, `procederé`, followed by a word other than `be`, `help`, `wait`, `know`, `summarize`, `explain`, `ayudar`, `esperar`, `explicar`, `resumir` or `dar`), the loop MUST send one more request in the same turn. That request MUST carry the reply as plain assistant text, then a user message: `Your answer says what you will do next, but you made no tool call, so nothing happened. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.` The pane MUST get the system line `the model said what it would do next without a tool call; it was asked to do it`. An answer that ends with a question, or whose last sentence is an offer to the user (the whole words `if you`, `let me know`, `do you want`, `would you like`, `si quieres`, `si lo deseas`, `si te parece`), or a summary or courtesy such as `Now I have created fibonacci.py and the tests pass.` or `I'll be happy to help with anything else.`, MUST NOT get it. This MUST happen at most once per turn, apart from the text call reminder, MUST count toward the turn's request cap, and MUST NOT happen in the retry without tools (#439).

#### Scenario: One reminder [auto]
- GIVEN tools on and a reply that ends "I'll need to create a `test_fibonacci.py` file next."
- WHEN the loop reads the reply
- THEN a second request carries the text and the reminder, with tools still offered, and the call the model makes after it runs

An empty answer (only whitespace) MUST get the same treatment with the user message `Your answer was empty. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.` and the system line `the model sent an empty answer; it was asked to go on`; the empty answer and the announced step share the one reminder of the turn. So MUST an answer whose last sentence, not a question, starts with the whole words `ready for`, `ready to`, `listo para`, `lista para` or `listos para`, with the user message `You stopped before finishing. If a step the user asked for is still undone, call its tool now. If everything is done, reply with a one-line summary of what the tool results show.` and the system line `the model stopped before finishing; it was asked to go on`.

#### Scenario: Empty answer [auto]
- GIVEN tools on, a tool round, then an empty reply
- WHEN the loop reads the empty reply
- THEN one more request carries the empty-answer reminder, and an announcement after it ends the turn

#### Scenario: Second announcement, question or offer [auto]
- GIVEN a model that announces a step twice, or a reply that ends with a question or an offer, or a turn without tools
- WHEN the loop reads it
- THEN the turn ends with `Done`, after two requests for the second announcement and after one otherwise

### Requirement: The tools prompt follows the coding families
When a turn offers the write, exec or git write tools, the tools block of its system prompt MUST replace the sentence `You cannot change files yourself; any change needs a command the user reviews and runs.` with a block that names only what the turn's families allow (write and edit files, staged until it proposes the batch, for write; run tests and builds for exec; add and commit with git for git write); that says it must do every step the user asks in the same turn, one call after another; that it must not stop to say what it will do next; and that it must not claim a file, test run or commit that no tool result of the turn shows. A turn with only the read, search, git read and pane tools MUST keep the sentence. The `run_in_pane` tool description MUST NOT say the model cannot change files, so it never contradicts the system prompt of a turn with the write tools (#439).

#### Scenario: Coding families [auto]
- GIVEN a turn that offers the write tools
- WHEN its first request is sent
- THEN its system message holds the coding block and not "cannot change files"

### Requirement: Tool texts tell a model how to go on
The result of `propose_batch` for an applied batch MUST end with the line `Continue with any step the user asked for that is not finished yet.`; a rejected, drifted or failed batch MUST NOT get it, and the line shown in the pane MUST stay the first line of the result. The refusal of `run_command` and the git tools while unproposed changes are staged MUST read `staged changes are not proposed: propose them first with propose_batch, then call this tool again` (#439).

#### Scenario: Applied batch [auto]
- GIVEN a staged batch the user approves
- WHEN `propose_batch` returns
- THEN its text ends with the continue line, and a denied batch's text does not hold it
