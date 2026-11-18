# Agents Overview Specification

## Purpose

One local view of every AI agent at work in lazarobox: orchestrator workers, chat panes, and CLI agents (claude, codex, gemini, opencode) running in shell panes. For each it shows who it is, its state, where it works, how long it has been in that state and, when the engine reported it, its tokens and cost; a key jumps to it, and an alert says when one starts waiting for the user (see chat-notify). Everything is built from state lazarobox already holds or already probes; nothing about the rows is sent to a model. Slice tags: [S1] chats and workers, view and jump; [S2] worker transcript jump, branch, per-worker approval wait; [S4] CLI agents in panes; [S5] bell and OSC signals. Scenario tags: [auto] = `cargo test` (TestBackend + insta, fake PTYs, fake probes); [manual] = real machine. Requirements marked `(pending Qn)` follow the recommended option of open question Qn of the proposal and change if the owner picks another. Texts and keys are provisional and pinned by design.

Terms: "row" = one agent as the view shows it. "Watched" = the pane is the focused pane of the active tab and the terminal window has the focus, as in chat-notify. "CLI agent" = a program in a shell pane's foreground whose name is in the agent table.

## Requirements

### Requirement: Agent rows hold no content [S1]
A row MUST hold only: the kind (`worker`, `chat`, `cli`), the agent's identity (for a worker its role id, worker number and run id; for a chat its pane; for a CLI agent its table id), the engine and model ids when known, the state, the instant the state began, the tab and pane numbers, the folder, the branch (S2), and the engine-reported usage. It MUST NOT hold any message, prompt, answer, reasoning, tool argument, command line argument, error text or screen text. A failed worker's row shows `failed` and its error class id, never the error text.

#### Scenario: Fields are the allowed set [auto]
- GIVEN the row type
- WHEN a test lists its fields against the allowed set above
- THEN no field can carry free text other than the engine, model, role and agent table ids, the folder and the branch

#### Scenario: A failed worker shows its class only [auto]
- GIVEN a worker that failed with class `rate_limit` and the error text `429: slow down`
- WHEN its row is built
- THEN the row reads `failed rate_limit` and no cell of the rendered view holds `429`

### Requirement: Who [S1, S4]
A worker row MUST show its role id, `engine>model` (as the worker row in the chat does, transcript.rs:150-166) and `worker <n>`; a worker whose engine was unavailable shows its role and `no engine`. A chat row MUST show `chat` and the engine and model of its last or current request, or of the menu assignment before its first request. [S4] A CLI agent row MUST show the agent table id (`claude`, `codex`, `gemini`, `opencode`) and no engine or model.

#### Scenario: A worker and a chat [auto]
- GIVEN a chat in pane 2 on `claude>sonnet` whose run has worker 1, `review codex>gpt-5`
- WHEN the rows are built
- THEN one row reads `chat claude>sonnet` in pane 2 and one reads `review codex>gpt-5 worker 1` in pane 2

### Requirement: States [S1, S2, S4]
The state of a row MUST be derived as follows, first match wins:
- worker: `waiting for approval` while an ask of that worker is the chat's pending approval or queued behind it [S2]; `waiting for you` while the run is halted on that worker; `queued`, `working` (status `running`), `done`, `failed`, `cancelled`, `timed out` or `refused` from its status;
- chat: `waiting for approval` while the chat has a pending approval of its own; `waiting for you` while its run is halted or a gate offer waits for an answer; `working` while its phase is `Waiting` or `Streaming`; `idle` otherwise;
- CLI agent [S4]: `running` while it is the pane's foreground program; with Q2's quiet time, `quiet` after 30 s (assumption) without pane output; [S5] `waiting for you (bell)` after a bell or an OSC 9 / OSC 777 notification from the pane, until the pane is next focused or produces output.
A CLI agent state other than `running` MUST be drawn as a guess (a trailing `?`, assumption) (pending Q2).

#### Scenario: Worker ask queued [S2, auto]
- GIVEN a chat with its own pending approval and an ask of worker 2 queued behind it
- WHEN the rows are built
- THEN the chat row and the worker 2 row both read `waiting for approval`

#### Scenario: Halted run [auto]
- GIVEN a run halted on worker 1's failure
- WHEN the rows are built
- THEN worker 1 reads `waiting for you` and the chat row reads `waiting for you`

#### Scenario: Streaming chat [auto]
- GIVEN a chat whose phase is `Streaming`
- WHEN its row is built
- THEN it reads `working`

### Requirement: How long [S1]
Each row MUST show how long it has been in its current state, taken from the instant the app saw the state begin (the run manager does not send a running worker's start, event.rs:61-62); an ended worker shows the elapsed time its row carries. The time is drawn as `12s`, `4m`, `1h05m`. A row whose state did not change keeps its instant across rebuilds.

#### Scenario: Since the state began [auto]
- GIVEN a fake clock and a worker seen `running` at t=0
- WHEN the view is drawn at t=125 s
- THEN its row reads `working 2m`

### Requirement: Where [S1, S2]
Each row MUST show the tab number and pane number of its pane, and the folder with the home directory as `~` (a worker's folder is its chat folder). [S2] When the folder is inside a git repository the row MUST also show the branch, `detached <7 hex>`, and `(worktree)` for a linked worktree, read from the `HEAD` file without running `git` (runtime::workspace), off the UI loop and cached per folder.

#### Scenario: Another tab [auto]
- GIVEN a chat in pane 4 of tab 2 opened in `~/code/app`
- WHEN its row is drawn
- THEN it reads `tab 2 pane 4 ~/code/app`

#### Scenario: Branch [S2, auto]
- GIVEN that folder is a repository whose `HEAD` holds `ref: refs/heads/feat/x`
- WHEN the branch is read
- THEN the row adds `feat/x`

### Requirement: Usage [S1]
A worker or chat row MUST show the engine-reported usage with the existing label (`<in> in <out> out`, and `$x.xxxx` only when the engine reported a cost), `(partial)` when the sum misses a report, and `no data` when nothing was reported. It MUST NEVER estimate a value or show a missing value as 0 (see ai-usage). A CLI agent row MUST show no usage.

#### Scenario: No data is not zero [auto]
- GIVEN a done worker whose engine reported nothing
- WHEN its row is drawn
- THEN it reads `no data`

### Requirement: The overview view [S1] (pending Q1)
`Ctrl+Space A` (see modal-input, Agents overview key) MUST open a list of every row of every tab, waiting rows first (`waiting for approval`, then `waiting for you`), then `working`, `queued`, `running`, `quiet`, then `idle`, then ended rows, and inside each group by tab and pane number. The view MUST show `No agents at work` when there are no rows. It MUST work with `mouse` off. While it is open its "how long" column refreshes at most once a second.

#### Scenario: Order [auto, TestBackend + insta]
- GIVEN an idle chat in tab 1, a streaming chat in tab 2, and a worker of the tab 2 chat waiting for approval
- WHEN the view is opened
- THEN the worker row comes first, then the streaming chat, then the idle chat

#### Scenario: Empty [auto]
- GIVEN only shell panes with no CLI agent
- WHEN the view is opened
- THEN it shows `No agents at work`

### Requirement: Jump [S1, S2]
Enter on the selected row, or the row's key (`1` to `9`, then letters, as LINKS), MUST close the view, switch to the row's tab and focus its pane. [S2] For a worker that is queued or running, it MUST also select that worker and open its transcript (see modal-input, Worker transcript key); for an ended worker it focuses the chat pane and the notice says `Worker <n> has ended` (assumption; design may extend the transcript view to ended workers whose trace is kept). A row whose pane no longer exists MUST NOT jump; the notice says `That pane is closed`.

#### Scenario: Jump to another tab [auto]
- GIVEN tab 1 active and a chat row in tab 2 pane 4
- WHEN its row key is pressed
- THEN tab 2 is active, pane 4 is focused and the view is closed

#### Scenario: Jump to a running worker [S2, auto]
- GIVEN worker 2 running in the chat of pane 3
- WHEN Enter is pressed on its row
- THEN pane 3 is focused and worker 2's transcript is open

### Requirement: CLI agent table [S4] (pending Q2)
A pane MUST be listed as a CLI agent only when the file name of its foreground program, or for `node`, `bun` and `deno` the file name of its first argument without the `.js`, `.mjs`, `.cjs` or `.ts` extension, equals an id of a fixed table: `claude`, `codex`, `gemini`, `opencode`. Any other program MUST NOT give a row and its name MUST NOT be shown. The name MUST come from the probe that already refreshes the `list_panes` roster (no new poll, no new `/proc` read beyond the foreground `cmdline` and `cwd`), and the environment of the program MUST NOT be read. The `list_panes` text MUST stay byte-identical. A chat pane is never a CLI agent row.

#### Scenario: A claude session [S4, auto]
- GIVEN a shell pane whose foreground argv is `claude --resume`
- WHEN the roster is refreshed
- THEN one row reads `claude running` in that pane, and no argument is held in the row

#### Scenario: Through node [S4, auto]
- GIVEN a foreground argv `node /usr/lib/node_modules/@google/gemini-cli/dist/index.js`
- WHEN it is matched
- THEN no row is made (the file name is `index`), a known miss documented in the README

#### Scenario: Unknown program [S4, auto]
- GIVEN a foreground program `vim`
- WHEN the roster is refreshed
- THEN no row is made

#### Scenario: list_panes unchanged [S4, auto]
- GIVEN the stored `list_panes` fixtures
- WHEN the roster gains the program field
- THEN the `list_panes` text equals the fixtures byte for byte

### Requirement: Attention signals from a pane [S5] (pending Q2, Q3)
While a CLI agent is in a pane's foreground, a BEL, or an OSC 9 or OSC 777 notification written by that pane, MUST set the row's state to `waiting for you (bell)`. The payload of an OSC notification MUST NOT be shown, kept or logged. A bell from a pane without a CLI agent MUST NOT make a row. Output or focus on the pane clears the state.

#### Scenario: Bell from claude [S5, auto]
- GIVEN a pane running `claude`
- WHEN it writes `\x07`
- THEN its row reads `waiting for you (bell)` until the pane writes again

### Requirement: Nothing reaches a model or a log [S1]
No row, and no text of the view, MUST be part of a prompt, a tool result, `list_panes`, `read_pane` or any engine request. A log line about the view MAY carry kinds, ids and state names only.

#### Scenario: Tool texts unchanged [auto]
- GIVEN a run with workers and a CLI agent in a pane
- WHEN `list_panes` is called by a chat
- THEN its text is the same as without this change

### Requirement: Cost of the view [S1, S4]
Rows for chats and workers MUST be rebuilt from app state on the events that change them or while the view is open; no timer MUST be added for them. CLI agent detection MUST reuse the existing 1 s roster refresh on the blocking pool. A per-pane last-output instant, if kept (Q2), MUST be one store per output event.

#### Scenario: No new probe [S4, auto]
- GIVEN a fake PTY that counts its probe calls
- WHEN ten roster ticks pass with the view open
- THEN the probe was called ten times, as without the view
