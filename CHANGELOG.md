# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

0.8.0 is the first version published to crates.io. 0.7.0 is an unpublished
baseline.

## [Unreleased]

## [1.20.0] - 2026-11-11

### Added

- The `Connectors` area lists a `Workspace` group: `google` (Gmail, Docs and
  Sheets through workspace-mcp 2.4.1) and `apps-script` (clasp 3.4.1). For
  `google`, lazarobox registers only 8 Gmail tools (search, read, attachment,
  draft and labels) and never `send_gmail_message` nor `start_google_auth`, so
  replies are drafts you send from Gmail. Search Console (`gsc`) is pinned to
  mcp-search-console 0.4.2 and `reauthenticate` is no longer offered to the
  model; GA4 pins only its 10 read tools. Linking from
  the menu comes in a later release.
- Windows Terminal under WSL has a test record (`docs/terminals/windows-terminal/`)
  and counts as a recorded terminal. `Alt+Enter` is taken by Windows Terminal
  for fullscreen: use `Ctrl+J` for a newline in the chat, or unbind the key.
- WezTerm (Windows build with its WSL domain) has a test record
  (`docs/terminals/wezterm/`) and counts as a recorded terminal. As in Windows
  Terminal, `Alt+Enter` is taken for fullscreen: use `Ctrl+J`, or disable the
  default assignment in `wezterm.lua`.

### Changed

- With ORCH on, the coordinator now delegates by default: a request that
  changes files is done by workers started with `spawn_agent`, even when you
  do not mention it, and the coordinator no longer writes the code in its
  reply (quoting code to answer a question is still fine). It still answers
  questions, plans and summarises directly.

### Fixed

- The orchestrator's model review no longer starts a correction worker for
  every re-proposal. Each proposal had its own "one correction", so a small
  local model that kept finding trivial blockers caused dozens of them. A
  worker's proposals now share at most three corrections until one of its
  batches is applied; after that the review still reports, a line and the
  receipt say the correction limit was reached and how many rounds ran, and
  the batch goes to your review with its blockers open.

- A worker's first `run_command` in a folder it had only staged was answered
  with a bare `path not available`, and the same call ran once its batch was
  applied. It now gets the real reason (`propose them first`) when its own
  batch stages that folder, and a `cwd` that is not a folder is named in the
  refusal, the same text whatever is there. Secret and hidden places keep the
  bare refusal.

- A llama.cpp server that serves its model under another id (for example the
  GGUF path, because `llama-server` runs without `--alias`) no longer reads
  `download required`: llama.cpp cannot download anything. The chat now says
  that the server at `127.0.0.1:<port>` does not serve the configured model,
  to start `llama-server` with `--alias <model>` or set the model to the
  served name, and then lists up to three ids it serves. The task row and the model list read
  `not served`.

## [1.19.0] - 2026-11-11

### Added

- Google Search Console joins the `Connectors` area (`gsc`), connected with a
  service account: `GSC_CREDENTIALS_PATH` (a path, in `secrets.toml`) and
  `GSC_SKIP_OAUTH = "true"`, no login step, and `GSC_ALLOW_DESTRUCTIVE` is
  never declared. See "Conectar un servicio" in the README.

### Fixed

- A review that cannot start while the author keeps re-proposing no longer
  floods the pane: the same notice (and its blocked receipt) is one line with
  a count, `(22 times)`. A different cause is still its own line.
- A command typed into a pane without Enter (`y` on a half-typed line in `run`
  mode, `ask`, `auto`) is now reported to the model as `NOT RUN YET`, with the
  reason, instead of a result that read as done, so it no longer claims a file
  was written when nothing ran. The result row says `typed, not run`.
- When an orchestrator run ends (`run_timeout_mins`, a stop or a cancel), the
  batch reviews and asks of its workers still on screen close with `review
  closed: the run ended; nothing was written`, and a late `y` no longer writes
  a batch the run had already denied.
- A worker whose task row is unassigned (for example `simple`, which runs on
  `general`) now says `(default)` on its row in the orchestra panel, like the
  AI menu row does, instead of showing only `engine>model`.
- An orchestrator turn whose coordinator cannot get its tools (for example the
  jail refuses the tools program because its folder is writable by group) now
  ends before the model is called, with a visible error naming the cause and
  the fix, instead of sending a text-only turn where the coordinator says it
  has no `spawn_agent` tool. The advice fits the cause: permissions only for
  a path others can write, otherwise to assign another engine. Plain tool
  chats and workers still degrade to text with the notice.
- Orchestrator notices name the right thing: the Write-off refusal says "a
  worker that writes files" (it also fires for a `simple` brief with
  `outputs`), and the "needs specification" line of a clarification round
  names the original brief, as the run-stop message does.

## [1.18.0] - 2026-11-11

### Added

- The `Connectors` tab of the menu (key `4`) is visible now. Its one row,
  `Connectors`, opens a list of the catalog services (Microsoft Clarity and
  Google Analytics 4) with their state: `not declared`, `disabled`, `values
  missing: <NAMES>`, `declared` and the last probe of the session. The list
  reads `config.toml` and `secrets.toml` each time it opens (names only, never
  a value) and says `restart lazarobox to use it in chats` when the file has
  servers that the running chats did not load. The area writes nothing.
- Each service of the `Connectors` area has its own screen: the guide and its
  warnings, the exact `[[ai.mcp.servers]]` entry to paste, the `secrets.toml`
  table to fill (names only, `set` or `not set`), the file paths and the
  actions. `i` installs and `l` logs in by typing the command, shown whole
  after a `y` prompt, into a new shell tab and sending Enter; `p` probes the
  declared server after a `y` and Esc stops it. Nothing runs without the `y`.
  See "Conectar un servicio" in the README.
- A `simple` worker is started from a brief (`goal`, `inputs`, `outputs`,
  `done_when`, `out_of_scope`) instead of free text. An incomplete brief or a
  path outside the project creates no worker and says so in the pane and to
  the coordinator; its `outputs` decide whether the worker writes, and a
  `simple` that writes is refused on a read-only goal or with Write off.
- A `simple` whose `outputs` overlap a queued or running worker's is refused,
  naming that worker. A brief's extra fields are flagged as `brief extra
  fields: <names>` in the spawn result, in `collect` and in the handoff. The
  `orchestrate` prompts now tell the coordinator to delegate a change and to
  give a `simple` worker a brief.
- A `simple` worker can call `needs_specification` when its brief is not
  enough. It can then write and run nothing more; its row says `needs
  specification`, the pane names the worker and the brief, the coordinator
  gets the question inside the untrusted-data wrapper (with a lazarobox line
  saying the tool was called) and the handoff stores it as `clarification`.
- A brief has at most two clarification rounds. While a worker's question is
  open, a new `simple` spawn must carry `answers: N` (the worker's id) or it
  is refused naming N, in the pane too; the row says `round n of 2`, and the
  handoff keeps the round and the brief. A third question stops the run with
  a line naming the brief and the limit. The turn cap still ends a run stuck
  waiting, and its line names the open clarification. The `orchestrate`
  prompts teach the coordinator the rule.
- A `simple` worker whose batch leaves a file above 400 counted lines asks
  you before the batch is reviewed or proposed, and before the mandatory
  review: `y` keeps the batch whole, `n` or Esc split the work and discard
  the staged batch, with the count shown (`split chosen: N staged files
  discarded`). The worker ends, and the coordinator is told to issue two new
  briefs; a split is not a clarification round. No approval mode and no
  timer answers it, and the worker's clock stops while it waits. Test
  files have no cap; an unknown language counts every line and says so. After
  a split `collect` marks the discarded files `discarded`; a batch whose lines
  cannot be counted is not proposed.
- The same ask names the files a `simple` worker staged that its brief does
  not list (`outside the brief: ...`), inside the run's scope; a secret path
  or one outside the chat's folder is still refused at staging, never asked.
  When the ask lists only such files, `n` or Esc decline: the batch is
  discarded (`writes outside the brief declined: N staged files discarded`),
  and that is neither a split nor a clarification round. After a split the
  worker's row says `split, staged batch discarded`.
- `Ctrl+Space x` opens the `/exec` dialog and `Ctrl+Space f` the `/write`
  dialog (`w` is the window group). A key never grants: only your `y` in the
  dialog does. A refusal shows the command's line, the composer draft is kept,
  and outside a chat a notice says Exec (or Write) is for chat panes.
- `/exec` turns Exec on for one chat, on the same path as `/write`: type it
  in the composer and a dialog names Exec and the folder and says every
  command still asks. Only a lowercase `y` grants it; `n` and Esc say `Exec
  stays off`. The grant lives in that chat's memory, leaves the saved Exec
  toggle and `config.toml` alone, and reaches the next message and, in an
  orchestrating chat, the next `code` worker (a running worker keeps its
  tools). It ends with a system line when the folder changes, tools turn
  off, the saved Exec toggle changes or a conversation is resumed, and
  ending it does not touch a `/write` grant. A model call, a worker, a
  tool result or pasted text cannot open or answer the dialog.
- With ORCH on, a worker on a local engine (Ollama or llama.cpp with a model
  that stays on the machine; an Ollama `*-cloud` model is not local) always
  has its staged change reviewed before your batch review, at every risk
  tier and with no consent ask, by the model of the Review row (the
  effective assignment). The receipt says `consent: mandatory`. The author is
  judged by the engine it was spawned with, not the menu now. If the Review
  row's engine cannot run, its worker ends `refused`, the run stops with the
  failure alert, and the batch reaches you `not reviewed` with the reason in
  the receipt; no other engine is tried. The review's workers (the lenses, the
  refuter and a correction) are internal: they never count against
  `max_workers`, so a review never lacks room, but a run can start more model
  calls than the visible cap, which can cost money on a cloud Review row.
  Each review start spawns at most 5 internal workers (two lenses, the
  refuter, a correction and its check), and a review starts for each proposal
  of an author, so a re-proposal repeats it; `max_parallel`, the worker
  deadlines, the run's wall clock and `max_turns` still bound it.
  They still show in the worker rows and are never waited on for a handoff.
- A review says one notice at its start, in the transcript: when author and
  reviewer are the same model; when the Review row is a local model (it can
  miss what a larger model catches); and when the staged diff goes to a
  reviewer on another engine than the author's (or a local author's diff to a
  cloud one), naming it.
- A new `simple` task for direct, fully specified work. The Models tab gets a
  `simple` row (`[ai.tasks.simple]`) after `orchestrate`; unassigned it uses
  `general`, `Set all tasks` leaves it alone, and a local server engine on it
  is not marked `not recommended`. With ORCH on, the coordinator can start a
  `simple` worker with `spawn_agent`: it gets the read, write and
  `run_command` tools under the menu's Write and Exec switches, its own
  prompt (override file `simple.md`), and on a read-only goal it starts
  with the read tools only (no write, no `run_command`) instead of being
  refused. An older build ignores
  the new table.

### Changed

- The model review of a worker's own batch, consent included, no longer
  counts against the worker's deadline (`worker_timeout_secs`); review
  workers keep their own deadline. A review that stops before it concludes
  now says why in its receipt and in the batch review line.

### Fixed

- A write refused after a split said the worker had asked for a clearer brief;
  it now says the worker chose to split.
- A worker whose clock was stopped (waiting for you or for its review) was
  ended `timed_out` when another worker of the run timed out, by the wall
  time since it started. Each worker is now timed out by its own clock only.

## [1.17.0] - 2026-11-10

### Added

- With ORCH on, a `code` worker gets `run_command` when Exec is on in the AI
  menu (the menu stays a ceiling: no other worker kind and never the
  coordinator gets it). Its command runs only after its batch was approved
  and applied, against the disk; before that it is refused with `propose
  them first`, and the pane now says so (`worker 2/3 code ollama>llama3:8b:
  run_command refused: ...`), as it does for a busy pane. Every command asks
  you, labelled with the worker, in every mode. What each one gave reaches
  the coordinator through `collect`: how it ended (its exit line, or denied)
  as lazarobox's own record by program name, which the worker's text cannot
  pass for, and its arguments and the cleaned, capped end of its output as
  untrusted data, after the files changed. The coordinator prompt now says a
  test counts as run or passed only when such a result shows it; otherwise it
  says the test was not run and asks you to run it.
- With ORCH on, the coordinator now gets `list_panes` and `read_pane`, with
  exactly the asks a chat gets: `list_panes` asks on a cloud engine and runs
  free on a local one, `read_pane` asks on every call and engine, and no
  approval mode answers either. It still never gets `run_in_pane` (a forged
  call is refused unasked), and workers get no pane tool. Before, a
  coordinator asked about the screen could only say it had no tool to see it.
- claude's model list comes from claude itself (#433): detection runs
  `claude -p /model --output-format json` once (a slash command: no model
  turn, no cost), jailed like a chat turn but never bare, with a 5 s timeout,
  and the picker shows `default (CLI picks)`, then every alias claude lists in
  its order (`sonnet`, `opus`, ..., `sonnet[1m]`, `opusplan`), then `custom…`.
  It is cached for 10 minutes like codex's and `r` refreshes it. Without a
  jail (and on macOS, where claude is never jailed), or when the reply cannot
  be read, the static claude list is shown with the reason.
- MCP server values can live in `~/.config/lazarobox/secrets.toml` (or under
  `$XDG_CONFIG_HOME`), one table per server name: a variable an
  `[[ai.mcp.servers]]` entry names in `env` takes its value from that
  server's own table first, then from lazarobox's environment, so a value
  reaches only its own server. The file must be a regular file with mode
  0600 and at most 64 KiB; it is read at each start of a chat's servers, its
  problems are shown once per start without any value, and lazarobox never
  writes it.

- The chat can draft an issue: the new `gh_issue_draft` tool (kind `idea` or
  `bug`, title, description) asks a `y` at the composer and, at the end of the
  turn, opens the Feedback form on the draft's kind with the title and the
  description in their own fields. You review it and send it with Ctrl+S; the
  model never writes to GitHub. New `[ai.tools] issue_draft` toggle, on by
  default, with its own `Issue draft` row on the Tools tab.
- With the draft tool offered, the tools block of the system prompt tells the
  model to offer a draft (by calling the tool, whose `y`/`n` is the permission)
  when you ask for an issue, when lazarobox lacks what you asked for, or when a
  lazarobox tool or the chat fails; never for errors in your own code. Without
  the tool (an engine without tools, tools off or `issue_draft = false`), a
  failed turn or a tool error shows `If this looks like a lazarobox problem,
  type /feedback to report it`, at most once per turn and never after a
  denied call.
- The model picker deletes a downloaded Ollama model: `d` on its row opens a
  confirmation with the name and the size, and only a plain `y` deletes it,
  through the local server. A model that a task or a running chat turn uses is
  refused and the notice says who uses it, and so is any delete while a
  download runs. The notice says what was freed, as an upper bound, since
  Ollama models share layers.
- The model picker deletes a llama.cpp model file the same way: `d` on its row
  measures it, and the confirmation names the file, its folder and its size
  (all parts of a split model), and warns when the file is in the shared
  Hugging Face cache. In that cache the snapshot link and its blob go
  together, a blob another snapshot still uses stays, and a repository left
  empty goes. lazarobox checks the file again before removing anything: it
  must lie inside the scanned model folders with links followed, and it
  removes only files and folders left empty. The managed llama-server's model
  and a model a task uses are refused.

### Changed

- With ORCH on, the time a worker waits for your answer (its batch review or
  a read ask, even one queued behind another worker's) no longer counts
  against its deadline (`worker_timeout_secs`): its clock stops while you
  decide and goes on when you answer, and its work time still counts. Only
  its own asks stop it, never a sibling's. Its turn's total limit stops the
  same way on the server, Ollama and CLI paths; a chat turn keeps its
  15-minute total as before. The run wall clock (`run_timeout_mins`) still
  counts while you decide. A CLI still cuts one tool call at 900 s by itself:
  that ends the wait (the model gets a tool error and goes on, and the
  worker's clock resumes), and the run wall clock still bounds the worker.
- With ORCH on, a worker's command ask is labelled like its read ask: the
  prompt shows `worker 2/3 code ollama>llama3:8b: run a command in the
  sandbox` above the program, its arguments, the folder and the network and
  `.env` notice. It waits in the worker queue behind the ask shown, your
  answer reaches only the worker that asked, and cancelling that worker
  withdraws it. A chat's own command ask is unchanged.

### Fixed

- A draft issue no longer opens over a menu screen that waits for you: the
  engine and model picker, its delete confirmation, the Hugging Face token
  field and its disconnect question, and the usage view stay as they were, and
  the chat names the draft that was not opened, as with the Memory warning.

### Security

- The read tools also refuse, and leave out of listings, the token locations
  of the MCP connectors: `~/.google_workspace_mcp`, `~/.clasprc.json` and
  `~/.config/mcp-gsc`; writes and the git tools refuse them too. The git
  tools now also exclude a secret root that is itself a file (such as
  `~/.local/share/fish/fish_history` or `.clasprc.json`), not only the
  contents of a secret folder.

## [1.16.0] - 2026-11-09

### Added

- A click on an `http://` or `https://` link shown in a shell pane or in the
  chat opens it in the browser, on the release of the left button. A link that
  wraps opens whole. With no browser the link is copied and a notice says so.
- `Ctrl+Space` then `u` lists the links the focused pane shows: a row's key
  opens it, `y` copies the selected one, `Esc` closes the list.

## [1.15.2] - 2026-11-09

### Changed

- Chat: `run_command` follows `[build] jobs` too, so a build started by the
  model cannot use more compile jobs than a build in a pane. A change in the
  menu applies to the next command.

## [1.15.1] - 2026-11-09

### Fixed

- Orchestrator: the review of an orphaned worker batch that was never proposed
  (the worker was cancelled while staging, or ended without `propose_batch`)
  now opens with the worker label too, as 1.14.4 promised. A batch from plain
  chat stays unlabelled.
- Orchestrator: a worker ending or being cancelled no longer takes away a review
  you opened on an orphan. Reviews a worker asked for are withdrawn as before.
- Batch review: a drifted apply says `1 path changed` instead of `1 paths
  changed`.

## [1.15.0] - 2026-11-09

### Added

- Menu: a `Build jobs` row in Settings (`[build] jobs`). New pane shells get
  `CARGO_BUILD_JOBS` and `CMAKE_BUILD_PARALLEL_LEVEL`. `auto` (the default)
  caps the jobs by the cores and by the memory at 2 GiB a job, so a machine
  with many cores and little RAM no longer runs out of memory on a full
  build; `l` counts up from 1 and `h` from `auto` jumps to all the cores.

## [1.14.4] - 2026-11-08

### Fixed

- Batch review: Ctrl+C on an open review (or on a proposal cancelled before
  its ask showed) no longer discards the staged work; the batch stays as an
  orphan for you to review or discard, as the README promises. Only your own
  `n` or `Esc` discards it.
- Orchestrator: the review of an orphaned worker batch keeps the worker label
  (`n/m kind engine>model`) instead of opening unlabelled.
- Orchestrator: the close report no longer says the claims match the ledger
  while a bare name or an absolute path in the handoff was not compared; it
  says `not checked`. Only project-relative paths with a directory are compared.

## [1.14.3] - 2026-11-08

### Changed

- Folder picker: when you open a chat from a pane that is in `$HOME`, the
  first row is now that folder (`current`, preselected, with a warning that a
  broad folder means no writes, commands or git for the chat's tools) instead of the last used project, so
  one Enter no longer moves you to another project. `/`, system folders and
  folders outside `$HOME` are still never a row, and nothing is chosen
  without a key press.
- Batch review: the first row now starts with the chat's folder (`~` for
  `$HOME`, cut from the front when long), so you see where the batch will be
  written before you press `y`. The folder only takes the room the counter
  leaves and never displaces the gate.

## [1.14.2] - 2026-11-07

### Fixed

- Orchestrator: when a step gate refuses a code worker and the coordinator's
  turn ends, the run now waits for your reply instead of ending. Once the pane
  says the run waits, press Enter to lift the gates and send your reply: it
  continues the same run and the coordinator is told the gates were lifted.
  The pane says when the run waits and when the wait ends. A lift pressed
  while the first turn still runs is not kept if that turn ends without a
  refusal standing.
- Orchestrator: a spawn refused by a step gate no longer tells the coordinator
  never to retry it. The refusal and the coordinator prompts now say to spawn
  again once that step is handed off.

## [1.14.1] - 2026-11-07

### Fixed

- A command whose stdout went past its cap no longer loses the stderr it wrote
  just before: the final read after the kill now drains both pipes.

## [1.14.0] - 2026-11-07

### Added

- The chat can now read the screen of another pane with the new `read_pane`
  tool, so it sees what a command printed (for example over SSH), not only the
  folder. It reads the visible screen, and with `scrollback` (1 to 200) that
  many earlier lines too, keeping the last 8 KiB. Every call asks, on every
  engine and in every approval mode: the review shows the exact text that will
  be sent, already cleaned and redacted, and `y` stays locked until you have
  scrolled to the end. The text is frozen when the review opens, so what you
  approve is what the model gets, even if the pane changes or closes. Only
  panes `list_panes` shows can be read, never the chat's own; ORCH workers and
  the coordinator never get the tool. A denied read, a pane that is not open,
  a blank screen and a read refused because another approval is pending each
  print a chat line; a cut text and scrollback asked of a full-screen app are
  warnings in the review. Redaction is partial: read the text before you
  approve.

## [1.13.0] - 2026-11-07

### Added

- ORCH now checks a worker's handoff against what the worker really staged.
  When the coordinator's handoff names a path (with a directory) that the
  worker's change ledger does not hold, the handoff acknowledgement, every
  later `collect` of that worker and the stored note say `named in handoff,
  not in ledger: <path>`, and so does the close report, under the worker. The
  flag stays when the coordinator saves the handoff again without the path;
  only the worker staging it clears it. A handoff that names no path says `not
  checked` and why, never `ok`, and a code worker judged `on_course` that
  staged nothing gets a neutral second line. lazarobox makes the check itself,
  without any model, and it is conservative: a name only counts when it ends in
  a known source extension, a bare file name, a host-like path or a name with
  odd characters is never flagged, and paths outside the project are never
  shown.
- ORCH now holds a run to its own steps. A `code` worker is spawned only once
  the steps before `implement` are closed, and `finish` is refused when the
  run was granted `implement` and no `code` worker ran. Every refusal is a
  chat line naming the gate and the step, and the coordinator gets the same
  text; nothing pauses and no failure alert opens. The review lenses and the
  correction worker lazarobox starts are not gated. If a gate keeps refusing
  until `max_turns`, the stop line names it. While a refusal stands, the line
  above the composer says `Enter gates off`: Enter on an empty composer lifts
  both gates for the live run and the chat says so; it ends with that run. With
  no live run it lifts nothing, and the chat says that too. Only your key press
  lifts a gate: text, a worker's answer or a tool call never does. The line
  goes when a `code` worker starts or the run ends. Gates are on for every ORCH
  run, so a coordinator that skips `authorize` now stops at the spawn.
- Two `code` workers of one ORCH run can now write at the same time, each
  with a batch of its own. When one proposes, the review lists only its
  files, and it is labelled with the worker; two pending proposals are
  reviewed one at a time, in arrival order. Approving one does not touch the
  other. The review lenses and the correction read the batch of the worker
  they review, a worker that staged and never proposed is named even when a
  sibling did propose, and a closed pane or a finished run leaves one orphan
  badge that counts all of them. A plain chat is not a worker and keeps its
  single batch.
- When two workers of a run start on the same local server (Ollama, llama.cpp
  and the like), the chat says once per run and engine that the server may
  queue their requests and names the key that fixes it, `max_parallel = 1` in
  `[ai.orchestrator]`. `max_parallel` stays your only limit: nothing is
  throttled for you. Cloud engines and Ollama `*-cloud` models are not
  counted.

## [1.12.0] - 2026-11-07

### Added

- Tools with llama.cpp: `llamacpp` joins the engines whose requests carry
  tools, so chat turns and ORCH workers on a `llama-server` run the same tool
  loop as on Ollama. Verified on Qwen2.5-7B-Instruct Q4_K_M with llama-server
  b11935 (`--jinja --parallel 1`); Qwen3.8-27B was not run. It needs a model
  and chat template that support tool calls: Qwen2.5-Coder-7B answers its tool
  calls as text and fails, as Ollama's `qwen2.5-coder:14b` does. `lmstudio` and
  `vllm` stay text-only.

- `/write` in a chat turns Write on for that chat only. It opens a `y` dialog
  that names the Write toggle and the folder; the grant lives in memory, is not
  saved to the config, and ends (with a line) on a folder change, a resume,
  tools turned off or a change of the saved Write toggle. It reaches the next
  worker the coordinator spawns (a worker that is already running keeps its
  tools) and, in a plain chat, the next turn. Only you can open it: a model,
  a worker or a tool result cannot.

### Changed

- ORCH workers now follow the Write toggle of the AI menu. Before, the saved
  toggles were ignored for workers and a `code` worker always got the write
  tools. Now, while Write is off (the default):
  - a spawn of a `code` worker is refused with a notice in the chat, and the
    run goes on; type `/write` in the chat (or turn Write on in the AI menu,
    `Ctrl+Space m`) and spawn it again;
  - a `code` worker that is retried, relaunched or taken off the queue follows
    the toggle at that moment, and if it is refused the run asks again;
  - a review correction is skipped with a notice;
  - a worker that is already running keeps the tools it started with.

  Turning ORCH on says this when Write is off.
- When a turn ends while a batch is waiting for your answer, the model that
  owns the batch can now propose it again and you are asked again. Before,
  `propose_batch` was refused with "waiting for review" and the batch could
  only be reviewed from the pane badge. This applies to a plain chat and to
  workers; a second proposal while your first ask is still open stays refused.

### Fixed

- An ORCH `code` worker that changed no file and whose reply holds tool calls
  written as JSON text (small local models on Ollama or llama.cpp do this
  instead of calling the tools) now gets a notice that says so: `... ended done
  and changed no file; its reply holds tool calls written as text: the model
  may lack tool support`. The coordinator is told the same, so it no longer
  reports files that were never created. A worker whose turn ran without tools
  keeps the plain notice.

## [1.11.0] - 2026-11-06

### Added

- An unknown key inside a table lazarobox owns (`[statusline]`, `[tabbar]`,
  `[updates]`, `[workspace]`, and `[ai]` with every table under it) now gives
  a startup notice with its full path, for example
  `config: unknown key: ai.tols`, so typos no longer pass silently. The file
  still loads, the key is ignored and a menu save keeps it. Keys and tables at
  the root of the file that lazarobox does not read are not reported.

## [1.10.0] - 2026-11-06

### Added

- Under WSL, `run_command` and git work in a Windows drive folder the user
  granted, inside the same jail as in the home folder. The grant prompt now
  reads `Allow writes, commands and git in <folder>? (y/n)`, and without the
  grant every write, command and git verb answers `path not available: the
  chat's folder <folder> is on a Windows drive, where writes, commands and git
  need your permission; ask the user to run /cd <folder>`. A broad drive folder
  stays refused even with a grant, and the jail also refuses, without letter
  case, a drive root, `Users`, any `Users/<name>`, the Windows system folders
  and DOS 8.3 short names as the root of a command. Refs #550.

## [1.9.0] - 2026-11-05

### Added

- Under WSL, the chat can write in a Windows drive folder once the user
  allows it: right after the folder is chosen in the picker, a `/cd` is
  confirmed or a conversation resumes there, the chat asks
  `Allow writes in <folder>? (y/n)`, and only an unmodified lowercase `y`
  grants. The grant covers that folder and its subfolders, lives in memory
  until the app quits, ends on a folder change, is not stored (a resume asks
  again) and is not inherited by a new chat; `/cd` to the current folder asks
  again. Broad drive folders never ask. Without the grant every write answers
  `path not available: the chat's folder <folder> is on a Windows drive, where
  writes need your permission; ask the user to run /cd <folder>`. On drvfs,
  which has no no-replace rename, a new file is published with `linkat`, which
  never replaces a file either. `run_command` and git on a drive are still
  refused. Refs #550.

## [1.8.0] - 2026-11-05

### Added

- Under WSL, the chat's read tools reach the Windows drives (`/mnt/c`,
  `/mnt/d`…, drvfs mounts from `/proc/mounts`), so a project on `C:` can be
  read and chosen as the chat folder. Windows system folders (and what an
  upgrade leaves: `Windows.old`, `$Windows.~BT`…), DOS 8.3 short names such
  as `PROGRA~3`, other users' profiles, and `AppData`, `NTUSER.DAT*` and the usual secret folders and
  files of the own profile and `Public` stay refused, compared without letter
  case. The own profile comes from `%USERPROFILE%`, asked once at start; until
  it answers only `Public` is readable. A drive root, `Users` and a profile
  root are broad folders, accepted only when typed. Writes, `run_command` and
  git on a drive are still refused. Refs #550.

## [1.7.1] - 2026-11-04

### Fixed

- Overwriting an existing environment template no longer tells the model what it
  holds. `write_file` over an existing `.env.example` (or `.sample`,
  `.template`, `.dist`) answered "staged" or "use edit_file" depending on a
  secret-looking run, CRLF or size, with no ask. It now asks first like
  `edit_file` does, once per file per turn, and a denial gives the same answer
  whatever the file holds. A worker, which cannot be asked, gets the usual
  refusal. Creating a new template is unchanged. Refs #583.
- Turning ORCH on (`Ctrl+O`) now says in the chat that, in this mode, the chat
  itself cannot write files, run commands or read GitHub, even with those
  tools on in the AI menu: a `code` worker writes, through the batch review,
  and `Ctrl+O` goes back to a plain chat. Before, the model said it had no
  tools and nothing explained why.

## [1.7.0] - 2026-11-04

### Added

- Feedback looks for open issues like yours before creating one. `Ctrl+S` first
  searches the repository's open issues with the key words of the title. When
  some share at least two of them, the form lists them: `Enter` adds your
  report to the chosen one as a comment instead of opening a duplicate, and
  `Ctrl+S` creates the issue anyway. With nothing similar, or when the search
  fails or takes more than 10 seconds, the issue is created as before.

### Fixed

- Under WSL with `networkingMode=mirrored`, engine detection no longer waits
  800 ms for engines that are not running (LM Studio, vLLM…). There a connect
  to a port nothing listens on is never answered, so detection now gives up a
  connect after 200 ms; a running server answers it in about a millisecond
  (#584).
- Feedback no longer sends you to file a new issue when a comment on an existing
  one finds `gh` missing or not logged in: the form says so and keeps the chosen
  issue, so `Enter` tries it again. The same goes for any failed comment. Long
  titles in the list of similar issues are cut by display columns, so
  double-width characters stay on the row, and the footer says it is looking for
  similar issues during the search (#588).
- A chat can write an environment template such as `.env.example`. It was
  always refused with no prompt, because the write primitives judged the name
  more strictly than the path policy. Now it stops at the batch review like any
  write and reaches the disk only with your `y`, in every approval mode. `.env`
  and `.env.local` stay refused, and so does reading the template (#583).

## [1.6.0] - 2026-11-04

### Added

- GitHub read tools, off by default: `gh_issue_list`, `gh_issue_view` and
  `gh_pr_view` read the issues and pull requests of the repository in the
  chat folder's `origin`, through your own `gh` login. Turn them on with
  `[ai.tools] github = true` or the new `GitHub` row of the Tools tab. They
  never return a diff, never take a repository from the model, and show
  everything they read as untrusted data with secrets masked. A cloud engine
  (and an Ollama `*-cloud` model) asks for your `y` before the first read of a
  repository in each turn.
- `/github` in the chat composer lets one chat use the GitHub read tools
  without turning the toggle on. It reads the folder's `origin` (no network),
  names the repository in a dialog and only an unmodified `y` grants; `n` and
  Esc decline and every other key says why it did nothing. It is a command of
  yours: no message, no request, nothing stored, and the model, a tool result
  or a stored message cannot open or answer it. The grant is for that chat
  only, starts at the next message, never writes the config and ends with a
  resume, a folder change, tools off or the saved toggle going off. A cloud
  engine still asks before the first read of each turn. `//github` sends the
  text, and `/github` followed by words says `/github takes no arguments`.
- A quiet hint above the chat composer suggests `/github` and names the
  repository (`Type /github to let this chat read owner/name's issues and
  PRs`) when tools are on, the GitHub toggle is off and the folder's `origin`
  is on github.com. One line, ASCII, shortened by dropping the name first;
  shown once per pane and repository, never stored or sent to the model.
- After a GitHub read, commands of that conversation wait for your Enter for
  the rest of the conversation, in every approval mode: `run_in_pane` only
  types, `y` types without Enter, and the bar shows `RUN OFF`. On a cloud
  engine `read_file` and `grep` also ask each time. The block survives a
  resume; it is kept as a marker row, so an older build opening that
  conversation shows its "not shown" notice for it and does not enforce the
  block. A conversation with saved messages this build does not know opens
  blocked as well, and every text of that block says so instead of claiming
  GitHub was read.

### Fixed

- When `$HOME` is reached through a symlink, a refused write now names the
  chat's folder as `~/…` instead of its full real path (#438).
- The chat's context budget now counts the schemas of external MCP tools.
  The runtime reports what they weigh when a turn starts: the context fill of
  that turn is corrected, and the next turns reserve them, so many connected
  tools no longer push an Ollama prompt past its window unnoticed. The turn
  that first starts the servers is still budgeted without them (#556).
- An orchestra worker on Ollama whose prompt and setup pass the model's
  window now says so at the top of its transcript, with the estimated tokens
  and the window, instead of Ollama cutting the prompt silently (#557).
- In WSL, text copied with `y` no longer pastes with a `<feff>` in front.
  `clip.exe` kept the byte order mark as a character, so lazarobox now copies
  with `powershell.exe` (`Set-Clipboard`, UTF-8, given 5 s to start) and falls
  back to `clip.exe`, which only gets the mark for non-ASCII text.
- In `run` mode, a `cd` you approve with `y` now adds the
  `pane N: cd … → <dir>` line to the transcript, like a `cd` that runs on its
  own. A run that finds the pane moved now says "the pane's directory changed
  before the command could run" instead of naming a check that a `y` run never
  had (#438).

## [1.5.0] - 2026-11-03

### Added

- Ollama turns now ask for a context window: `options.num_ctx` is the smaller
  of the model's own window (read from the `POST /api/show` reply detection
  already sends, no new request) and `[ai.servers.ollama] num_ctx_cap`
  (default 16384). Every turn of a model sends the same value, so Ollama never
  reloads it because of it. The menu does not write the key; a bad value falls
  back with a startup notice.
- The desktop entry now shows the Lazarobox logo. `install-desktop` writes the
  embedded icons (PNG 256, 128, 48, 32 and 16 px, plus an SVG) to
  `<XDG_DATA_HOME>/icons/hicolor/` and the entry uses `Icon=lazarobox`;
  `--uninstall` removes them. A failed icon write is reported and exits with
  an error, but keeps the installed entry (#268). Existing users must re-run
  `install-desktop` to get the logo.
- `[ai] tool_mark` (`"auto"`, `"emoji"` or `"text"`) to force how the tool mark
  is drawn. `"text"` is for terminals with no emoji font, where `🔧` shows as an
  empty box and cannot be detected. A bad value falls back to `"auto"` with a
  startup notice (#488).

### Changed

- Ollama turns send `keep_alive = "30m"` when `[ai.servers.ollama] keep_alive`
  is unset, instead of Ollama's own five minutes; a value set in the file is
  sent as written. Other engines' requests do not change.
- The history budget of local servers now counts the system prompt, the tools
  block and the tool schemas before any message, also when the window is
  unknown (the 12,000-character cap is reduced by that head, never below 4,000
  characters). Trimming still drops whole user and assistant pairs, oldest
  first, with the usual marker. A message that cannot fit a known window is
  still sent whole, with a notice, and an Ollama model with an unknown window
  shows `window unknown for <model>: 12k chars` once per run. A Modelfile
  `PARAMETER num_ctx` does not override the cap when the window is known; when
  it is unknown no `num_ctx` is sent and the Modelfile value applies.
- The wait line of a chat turn says what it waits for: `waiting for your
  answer…` under an approval prompt, `running <tool>…` while a tool runs, and
  the model otherwise. For a local server engine the long wait adds the
  seconds the request clock has run and its limit (`still waiting for the
  model… 45s (gives up at 120s)`), counted from the clock's own slow event and
  only while the line waits for the model (not under an approval prompt or a
  running tool). Once the server starts to answer, the runtime reports its
  reads (at most one per second, dropped if the channel is full) and the limit
  shown is the 60 s idle limit counted from the last read the app knows, up to
  one second low, and capped at the 15 minute limit of the whole turn (`gives
  up at 73s`). Known gap: in the no-tools retry the first seconds of the new
  request still show the previous limit. The CLI engines show neither seconds
  nor limit. The rolling summary run counts from its own clock, not from the
  last turn (#437).

### Fixed

- A turn that Ollama retries without tools after a 400 now tells the user once
  per pane, without claiming the model lacks tool support, as llama.cpp already
  did (#550).
- Reading a SQLite file now says it is a database that cannot be read as text
  and suggests a `sqlite3` command for the user to run in their pane, instead
  of `the file is not text` (#550).
- The chat prompt now says the file tools only reach the home folder and that
  the model must report a refusal or failed tool instead of guessing contents
  (#550).
- A chat opened in a folder outside the home folder and kept with `Esc` now
  says the file tools cannot read it and that the project must move into the
  home folder (`/cd` refuses folders outside it) (#550).
- A key the approval prompt does not take, a paste, and an answer that comes
  before the prompt is armed no longer vanish silently: the footer says
  `Answer the pending confirm first` or `Prompt just opened: answer again in a
  moment`. The batch review does the same, except that an early `y` keeps its
  own hint in the view and shows no such notice. The answer keys still never
  answer an unarmed prompt (#437).

## [1.4.0] - 2026-11-03

### Added

- Prompt-cache options for the local servers: `[ai.servers.ollama] keep_alive`
  (how long the model stays loaded after a turn, such as `"30m"` or seconds)
  and `[ai.servers.llamacpp] cache_prompt`. Unset, nothing is sent and each
  server keeps its default (#515).

## [1.3.1] - 2026-11-03

### Fixed

- A program that asks for focus reports (mode 1004, as nvim does) now gets
  `ESC[I` and `ESC[O` when the lazarobox window gains or loses focus, while its
  pane has the focus. Other panes get nothing (#483).

## [1.3.0] - 2026-11-03

### Added

- `/feedback <text>` in a chat opens the Feedback form with the text written in
  it: the first line is the title and the rest the description. Nothing reaches
  the model, and the issue is sent only by the form's `Ctrl+S`. `//feedback`
  sends a message with one slash less (#551).

## [1.2.4] - 2026-11-03

### Fixed

- `install-desktop` judges `x-terminal-emulator` by the terminal it resolves to:
  the picker marks it `(tested)` and the note names that terminal when it has a
  record (#487).

## [1.2.3] - 2026-11-03

### Changed

- Deleting a conversation from the chat list now takes `Del`, then `Enter` to
  confirm or `Tab` to also delete its notes. `d`, `y` and `a` no longer delete.
  This is a deliberate exception to the key binding promise of the 1.0
  stability rules, released as a patch because the old keys could lose data
  (#491).

### Fixed

- Words typed right after the chat opens on its conversation list can no longer
  delete a conversation: `day` deleted it, with its notes when they could be
  deleted (#491).

## [1.2.2] - 2026-11-02

### Added

- `/cost` shows the cache share per engine for today, the last 7 days and this
  chat (`cache today: claude 91% · codex 79%`): cache reads over fresh input,
  cache reads and cache writes. Engines that report no cache show no figure.
  Visibility only; no prompt changes (#515).
- `/cost` shows the prefill speed of local server engines that report timings
  (Ollama, llama.cpp, LM Studio, vLLM) (`prefill today: llamacpp
  2.2 ms/1k tok · ollama 1.4 ms/1k tok`), from Ollama's `prompt_eval_duration`
  and llama.cpp's `prompt_ms`; a low figure means the prompt cache was hit.
  Ollama's `load_duration` is stored too. Two additive nullable columns of
  `turn_usage`, no schema version change. Visibility only (#515).

## [1.2.1] - 2026-11-02

### Added

- `list_panes`, a read-only chat tool that tells the model what each pane
  runs: its number, whether it is focused, its folder and the foreground
  program (`nvim prueba.rs`). It is free on local engines; on a cloud engine
  it asks first and the prompt shows the exact text that would be sent, so the
  foreground command lines of your panes reach a provider only after your `y`.
  Arguments are an allowlist, not a denylist: the focused pane shows the
  program name, flags without their `=value` and paths (starting with `/`,
  `./`, `../` or `~/`, or naming an existing entry in the program's folder);
  every other argument is `…`, and `bash -c`, `python -c`, `node -e`, `perl -e`
  and `ruby -e` show the flag only. Other panes show the program name only.
  Each argument is capped at 120 characters and each pane at 300, and paths
  under `$HOME` show as `~/`. The environment of other processes is never
  read, and the shared state only ever holds redacted text. Linux reads
  `/proc/<pgid>/cmdline` and `cwd`; on macOS the program and folder are
  `unknown`. A pane that cannot be read shows `program: unknown`. Existing
  file names are shown as they are (`--password-file creds.txt` shows
  `creds.txt`; contents are never read) and only the first 16 arguments of the
  focused pane are looked up on disk. Every fragment is cleaned like the other
  prompts (no escape sequences, bidi or zero-width characters), redacted and
  capped; a program title with spaces or `:` shows as `…`. The text is bounded
  (5 panes, focused first, then `+N more`) so the prompt stays answerable,
  and the refresh (the `/proc` reads and the disk lookups) runs off the UI
  loop. A pane list older than 10 s says so in the text, because the refresh
  may be stuck. An Ollama `*-cloud` model counts as
  local, so it does not ask.
- Pin change: `list_panes` joins the legacy tool families, so the default
  tool list (the MCP `tools/list` bytes, the Ollama and llama.cpp `tools`
  body and the tools prompt, 865 to 1033 bytes) is no longer the 0.18.0 one.
  The golden and the registry pins were updated on purpose.
- `lazarobox-shell --check-install` prints whether the tools can run from this
  install, outside the TUI: exit 0 when the binary and its folder pass the
  trust check, exit 1 with the same reason and `chmod` fix as the chat notice
  otherwise.

### Changed

- When group write is the only reason `tools are off` (the usual result of
  `cargo install` under `umask 002`), the notice ends with
  `(install with umask 022 to keep it)` before the `chmod` command (which stays
  last, ready to paste), and the README install and update sections explain why and how to avoid it. The trust
  rule is unchanged. (#526)
- The in-app update (Updates row) runs its `cargo install` with `umask 022` for
  that child process only, so the binary it installs is not group-writable even
  when the user's umask is 002. The `cargo install` arguments are passed
  unchanged. (#526)
- `/cost` no longer describes a chat-history-off view: chat history is always
  on, and an unreadable `chat.db` shows `usage history is not available`.

### Fixed

- The tools on the `codex` engine were refused by codex itself (`MCP tool call
  requires approval, but approval policy is never`), so no call reached
  lazarobox: `list_panes`, `read_file` and the rest failed, and no lazarobox ask
  appeared. codex now passes our calls on (`default_tools_approval_mode =
  "approve"` for the `lazarobox` server only) and they reach lazarobox's own
  approval, which is unchanged. Checked with codex 0.162.1; the key needs codex
  0.125 or later (older versions reject it and the turn fails with codex's
  config error).
- `Ctrl+Space r r` and `Ctrl+Space r d` showed nothing when the answer was
  already known, or when the store answered at once (#501, #527): the notice
  (`No interrupted run to resume or discard`) was set but the screen was not
  redrawn, so the `RUN` key hint stayed up. The key now always redraws.
- `/cost` showed `unknown` for a turn that used several models (gemini's router
  plus its main model, claude's sub-agents). Such a turn is now stored and shown
  as `mixed` (`gemini › mixed`). Rows stored before this stay `unknown`.
- Chat with `codex` and `gemini` no longer sends the user's own setup to the
  provider (#519). Neither CLI has a flag that stops it loading its global
  instruction files (`~/.codex/AGENTS.md`, `~/.gemini/GEMINI.md`) or skills
  (`~/.gemini/skills/`), so each turn now runs with a private `CODEX_HOME` or
  `GEMINI_CLI_HOME` inside its wiped temp folder. It holds only symlinks to the
  login files (nothing is copied or moved) and, for gemini, a `settings.json` with the login type
  and `context.memoryBoundaryMarkers`. A turn that cannot prepare it fails with a notice
  instead of using the real folder. `GEMINI_SYSTEM_MD` is also dropped from the
  child environment. A codex login refresh writes `auth.json` in place, so it follows the link to
  the real file. The turn warns before it starts when no login is found, honours
  a validated `GEMINI_CLI_HOME`, rewrites gemini's `.env` with only its login keys (never a link, so `GEMINI_SYSTEM_MD` cannot come back through it), reads `settings.json` with comments,
  and also drops `GEMINI_CLI_SYSTEM_SETTINGS_PATH`, `GEMINI_CLI_SYSTEM_DEFAULTS_PATH`
  and `GEMINI_CLI_TRUSTED_FOLDERS_PATH`. The two CLIs keep no state between turns
  (model cache, installation id), which costs a few extra calls. A gemini login
  refreshed during a turn is not saved back (lazarobox never writes credential
  files), so each turn refreshes the token again, which is a little slower; if
  Google ever rotated the refresh token, a new `gemini` login would be needed.
  The login notices show once per run.
  Neither CLI walks up from its working folder for `AGENTS.md` or `GEMINI.md`
  any more (`-c project_root_markers=[]` for codex, `context.memoryBoundaryMarkers`
  set to `[]` for gemini). A value of the gemini `.env` that cannot be rewritten
  safely is skipped with a notice, never silently; a line that opens a quote it
  does not close, and the lines up to its closing quote, are skipped with a
  notice. In the `.env`, `KEY=abc#def` keeps `#def`: only ` #` starts a comment.
  Empty values do not count as a login.
- `Prefix r r` and `Prefix r d` no longer say `No interrupted run to resume or
  discard` when ORCH was never turned on since the start: the store is asked on
  the spot (off the UI thread, with a `Looking for interrupted runs` notice) and
  the answer completes the key that waited (including `Turn ORCH on in this pane
  first`). A failed or silent (10 s) read is said and retried by the next press,
  never reported as `No interrupted run`. (#501)
- A refused worker (for example one routed to an ollama model that is not
  downloaded) now gives the coordinator the reason in the tool result (one
  redacted line, capped) instead of only `worker N: refused`. (#502)
- `m` on a refused worker opens the model picker of the engine configured for
  its task instead of the engine picker; with no engine to take from, it opens
  the engines and a notice says why. (#502)
- The failure alert shows the error class (`other` is omitted), `(setup)` for a
  refused worker whose engine or model is not available. The refusal reason
  given to the coordinator has absolute paths replaced by `<path>`. (#502)

## [1.2.0] - 2026-11-02

### Added

- The menu's AI tab is split into the tabs Models, Tools, Connectors and Chat
  after Settings, so the popup is 17 rows high instead of 25 and the smallest
  usable terminal is 24x16; the Usage row of Settings takes the 17th row. `Tab`
  and `Shift+Tab` still cycle them and the new jump keys `1` to `5` go straight
  to a tab by its fixed number (`2` Models, `3` Tools, `5` Chat). Connectors
  stays hidden until it has rows, so `4` says there are no connectors yet. The
  bar hint reads `⇥ Tab · 1-5`. A digit or `Tab` that cannot switch tabs now
  shows a notice in the footer (`Tabs are 1-5`, `No other tabs`) instead of
  doing nothing. Nothing changes in `config.toml`.
- `/cost` (alpha) in the chat composer opens a view over the conversation,
  headed `AI usage (alpha)`, with the tokens of today and of the last 7 days
  over every stored chat, the usage per engine and model, this chat and this
  session. `Esc` closes it, the arrows and `PageUp`/`PageDown` scroll it and
  long lines wrap. It is a command of yours: no message, no request, and it
  works during a stream. `/cost` followed by text says `/cost takes no
  arguments`; `//cost` sends the text `/cost`. A value an engine did not report
  reads `no data` (never 0) and dollars appear only when the engine reports
  them, labelled `list-price equivalent` by default. With chat history off it
  shows this session only, from memory, and says so.
- A dim usage line (alpha) under each chat answer (`usage: in 2 · out 4 ·
  $0.0049 list-price equivalent`), on by default. Turn it off with
  `line = false` under `[ai.usage]` or in the Usage menu view. A turn the
  engine reported nothing for reads `usage: no data`; a cut or failed turn gets
  the line only if it has a record, marked `partial`. It is not stored, not
  sent to the engine, not part of a yank and not shown after a resume.
- The status bar of a focused chat shows ` · today: N tok` (alpha; all chats
  and engines, today), on by default. Turn it off with `statusline = false`
  under `[ai.usage]` or in the Usage menu view. Dollars are added only for
  engines in `per-token` mode (`$0.42`, `<$0.01` below half a cent, nothing for
  a zero sum). It is read from the history when the first chat is focused and
  again when the local day changes, never shows yesterday's figure, and a read
  that fails keeps the last figure, is told as `Usage not read: <reason>`,
  shows `today: ?` when no valid figure is left, and is retried after a minute.
  It is the first segment dropped when the bar is narrow.
- `[ai.usage]` (`line`, `statusline`, and the thresholds `turn_tokens`,
  `day_tokens`, `day_usd`) and
  `[ai.usage.billing]` (`claude`, `codex`, `gemini`: `subscription`, the
  default, or `per-token`). A wrong value falls back to its default and is
  reported at startup; a threshold of 0 or less is off.
- AI usage settings in the menu (alpha). A new `Usage (alpha)` row in Settings,
  after `llama-server idle`, reads `line on · bar on` and opens a view with
  `Usage line` and `Usage in bar` (`[ai.usage] line` and `statusline`)
  and `claude billing`, `codex billing` and `gemini billing`
  (`[ai.usage.billing]`: `subscription` or `per-token`); local engines have no
  row. Changes are a draft until `s` saves, only the changed keys are written,
  and `Esc` goes back keeping the draft; any other key inside the view says
  `Esc first to leave Usage`. The README now says what the usage history
  stores: numbers only, never message text, pruned after 90 days.
- Usage limits (alpha). Three optional limits add a passive note and never
  block, delay or cancel a send: `turn_tokens` (a finished turn used more
  tokens than the limit: `turn used 1200 tokens (limit 1000)`), `day_tokens`
  (`today's usage reached 5200 tokens (limit 5000)`) and `day_usd` (`today's
  spend reached $5.01 (limit $1.00)`, counting only the engines in `per-token`
  mode; list-price equivalents and local engines never count). The turn note
  shows on every turn over the limit; the two day notes show once per local day
  per run. The Usage view gets the rows `Turn tokens`, `Day tokens` and `Day
  $`: `Enter` edits, digits type (one `.` and two decimals for dollars, at most
  12 digits; a paste of valid digits is accepted), `Enter` keeps it, `Esc`
  cancels, and `0` or an empty field is `off` (the key is removed from
  `config.toml`). A stored value with more decimals than you can type (`day_usd
  = 0.125`) is shown as it is and kept unless you change it. A key that cannot
  be typed shows a footer notice. When the first status read of the day finds a
  day limit already reached, its note shows at startup.
- Usage charts (alpha). `/cost` ends with two text charts of the last 7 days
  when there are records: `per day` (one glyph per local day from `▁` to `█`
  scaled to the biggest day, `·` for a day with no known total, ending in `max
  <N>`) and `per engine` (a bar of up to 20 `█` per engine, or `no data` when
  it reported no totals). They chart tokens only: a missing count is a gap,
  never zero, and no dollars are drawn. They wrap like any other line in a
  narrow pane and are not shown with the history off. A ` *` after a figure
  means some turns did not report both counts, so it is a minimum.
- AI usage of orchestrator workers (alpha, slice 5 of #440). Each worker turn
  is now stored in the usage store with the role `worker`, the worker's engine
  and model and its `<run>.<worker>` id, under the same rules as a chat turn:
  nothing is stored with the history off (`/cost` still counts it in `this
  session`), a count the engine did not report stays `no data`, a worker that
  failed, was cancelled or ran past its deadline is stored `partial`, a failed
  write shows `Usage not saved: <reason>` once per run, and deleting the chat
  keeps the records. They count in `today`, `last 7 days`, the charts, the
  status bar's `today: N tok` and the day limits; the per-turn limit never
  fires for a worker. `/cost` adds `of which workers: <tokens>` after `last 7
  days` when there are worker records, and on the coordinator's orchestrated
  turn its usage line reads `usage (coordinator): ...` so it is not taken for
  the run total. There is no per-worker or per-run breakdown yet. Usage that
  arrives after its pane was closed is still stored (unlinked); if that write
  fails there is no pane to tell.
### Changed

- The `Orchestrator` row of the menu is now in the Tools tab (it was in the AI
  tab before the tabs split).

### Fixed

- Orchestrator worker rows count the prompt as input plus cache reads and
  writes, so it means the same for every engine. A worker on llama.cpp now
  shows its usage, and a failed `claude` worker keeps the usage it reported. A
  report cut short is marked partial and shows no cost.

## [1.1.0] - 2026-11-01

### Added

- Agent orchestrator (experimental, off by default). One coordinator model
  splits a job across workers, each running on the engine and model the menu
  assigns to its task, and every step is recorded. Turn it on with the
  `Orchestrator` row of the AI tab (`[ai.orchestrator] enabled`), then press
  `Ctrl+O` in a chat pane to switch its ORCH mode on; the new `orchestrate`
  row picks the coordinator's engine (it falls back to `general`, and Ollama
  is marked `not recommended`). Workers run in parallel (`max_parallel`,
  `max_workers`, `run_timeout_mins`, `max_turns`, `worker_timeout_secs`), read
  the chat folder without asking and ask for anything outside it; a `code`
  worker only stages files, which reach the disk through your batch review. A
  high-risk change asks your consent first and then gets a lens review and a
  receipt in the batch review, which you always decide. `Alt+Down`/`Alt+Up`
  select a worker, the first `Ctrl+C` cancels only it and a second one cancels
  the run, `Ctrl+Space W` opens its transcript, and an interrupted run is
  named after a restart and resumed or discarded with `Ctrl+Space r r` and
  `r d`. Handoffs and receipts go to engram when memory allows it. Providers
  recommend API keys for automation and consumer-account logins carry risk; no
  key is required. Known limit: workers cannot run commands or tests yet, so
  the coordinator asks you to run them (test-first evidence is planned for
  1.2). The ODD flow (classify, feature document, one question at a time)
  depends on the model following its instructions and is not reliable yet; a
  later version will enforce it. See the README, "Orquestador de agentes".
- Installed Ollama models in the model picker show a tool mark: `🔧` beside
  `✓` for a model of 3B parameters or more that declares tool support, and
  `tools: unreliable` for a smaller one. A model without tool support, or with
  unknown support or size, shows no mark. The mark is informational (the model
  stays selectable) and shows even with `[ai.tools] enabled = false`. On
  `TERM=linux` or a non-UTF-8 locale, `🔧` is spelled `tools`.
- Detected `claude`, `codex` and `gemini` engine rows in the model picker show
  the same `🔧` mark (`tools` on `TERM=linux` or a non-UTF-8 locale), shown
  even with `[ai.tools] enabled = false`. `opencode`, `ollama`, engines not
  detected and CLI model rows carry none.
- The Ollama download screens mark models that declare tools: a Popular row
  gets `🔧` (or `tools: unreliable` under 3B) judged by the largest size that
  fits this PC, and each Sizes row by its own tag. No new request is made.
- Ollama detection now sends one `POST /api/show` per installed model to the
  local Ollama server (127.0.0.1 only; at most 32 per detection, 4 at a time,
  2 seconds in total, cached by digest) to read whether it declares tools.
- Copy now sends OSC 52 and also runs a local tool when one exists (`wl-copy`,
  `xclip`, `xsel`, or `clip.exe` in WSL; none over SSH). The notice says what
  happened, short and action first so it fits an 80-column statusline:
  `Copied (<tool>)`, `<tool> failed; sent via OSC 52 (<reason>)`,
  `<tool> failed; not copied: too large for OSC 52 (<reason>)`, or
  `No paste? Install wl-copy or xclip (OSC 52 sent)`.
- `Ctrl+]` (and `Ctrl+5`) works as an alias of the `Ctrl+Space` prefix.
- First-run prefix check: if `Ctrl+Space` does not reach lazarobox, it says to
  use `Ctrl+]`. The result is kept per terminal; `--check-prefix` runs it
  again.
- `install-desktop` asks which terminal to use when run on a TTY without
  `--terminal`, marks terminals with a test record, and prints notes for WSL.
- `docs/terminals/` holds the support policy, the checklist and the record
  templates. A terminal is supported only with a filled record.
- Terminal records for Kitty 0.41.1 and zutty 0.16.2 (2026-11-01), with the
  known issues they found; `zutty` joins `RECORDED_TERMINALS`.

### Fixed

- Alt+Enter is no longer lost in zutty, which sends it as `CSI 27;3;13~` while
  xterm modifyOtherKeys is on (the input library cannot parse that form):
  startup asks the terminal to turn that mode off (`CSI > 4 ; 0 m`) and exit
  asks it to return to its initial value. zutty then sends ESC LF, which the
  chat now treats as a newline (Alt+Ctrl+J).
- When the jail refuses the tools shim (or an interpreter) because it or its
  folder is writable, the notice now names the cause and the fix instead of the
  generic "they could not be set up": `tools are off: ~/.cargo/bin and
  ~/.cargo/bin/lazarobox-shell are writable by its group; run: chmod g-w
  ~/'.cargo/bin' ~/'.cargo/bin/lazarobox-shell'`. With umask 002 (Ubuntu,
  Debian, Parrot) `cargo install` leaves those at 775, so a default install
  needs that one chmod (#496). Paths you do not own are listed apart for their
  owner. A path that cannot be inspected says `cannot check <path>: permission
  denied` instead of being called writable. The trust rule itself is
  unchanged.

### Changed

- A single `Ctrl+]` or `Ctrl+5` no longer reaches the program in a pane: it is
  the prefix alias now and takes two presses (`Ctrl+] Ctrl+]` sends 0x1D). This
  affects vim tag jump and the telnet escape.
- `install-desktop` no longer says lazarobox is "tested in Kitty"; it says
  whether the terminal has a test record, and never refuses to install.
- README: Windows is described as WSL only, and the terminal record policy is
  linked.

### Internal

- Agent orchestrator groundwork (S1, behind the `[ai.orchestrator] enabled`
  gate, off by default): the ORCH badge of an orchestrating chat pane, worker
  rows coloured by status, the provider notice and the failure alert above the
  composer, and the final `orchestrate` prompts of both profiles.
  The `handoff` tool also closes an ODD step (`step` instead of `id`): steps
  close in order, each with its handoff, and a goal authorized `read_only`
  refuses `code` workers. A large goal gets a feature document (the user's
  requirements in their own words, tasks and a log that starts with the goal),
  saved to engram under one key per run and kept in the run ledger; a
  correction appends to the log and rewrites one requirement. The
  `orchestrate` prompts gain a playbook per step. A risk tier for staged
  changes (S7b): build, CI, dependency, lock, config and secret
  paths are `high`, and the orchestrator can only raise it with the new `raise`
  tool. The review of a `high` change (S7b): a `code` worker's `propose_batch`
  asks an inline consent first; on `y` read-only lens workers and a refuter
  look at the staged change and at most one correction worker fixes the
  blockers that survived, all under the run limits; on `n` the change goes on
  to your batch review marked not reviewed. Nothing approves the batch. Each
  review leaves a receipt (tier, consent, lenses, findings, outcome, paths not
  reviewed), saved to engram, and a one-line note in the batch review header.
  Runs are stored locally (S9, first part): a run is saved in the chat database as
  its workers change, and after a restart a run that was live is marked
  interrupted and named in the pane the first time ORCH is turned on; nothing
  runs again on its own. Run ids start above the highest one stored. With a
  worker selected (Alt+Up/Down), `Ctrl+Space W` opens a full-pane, read-only
  transcript of it (its output and tool rows, the last 128 KiB, cleaned of
  control characters); Esc or `q` close it and it follows new output.
  An interrupted run is named again at each start until you answer it, in the
  folder it started in only (runs of other folders are counted, never shown):
  `Ctrl+Space r r` starts a new run from its goal, with its read scope set again
  from the chat folder and the orchestrator told to relaunch no worker until you
  say so, and `Ctrl+Space r d` discards it. The old run closes when the new one
  has a worker; one older than 30 days is closed unseen. An approval refused
  between two orchestrator turns now says so in the pane.
- Agent orchestrator write integration (S7, same gate): a `code` worker gets
  `create_dir`, `write_file`, `edit_file` and `propose_batch`. Its writes are
  staged in the pane's batch, only inside the run's read scope, and reach the
  disk only when the user approves the batch, whose review header names the
  worker. `collect` lists the worker's `files changed` (path, size, diff
  hash, applied or not applied).
- Agent orchestrator usage (S6, same gate): the claude, codex, gemini and
  Ollama decoders report the tokens (and the cost, where claude gives one) of
  a turn. A worker row shows its own numbers, or `n/a` when its engine
  reported none, and a `run` row shows the sum. Nothing is estimated.
- Agent orchestrator multiple workers (S3, same gate): read grants belong to
  one worker, worker approvals queue one prompt at a time, each worker has its
  own session, and `[ai.orchestrator]` `max_parallel`, `max_workers` and
  `run_timeout_mins` bound the run. With a worker row selected (Alt+Down,
  Alt+Up) the first Ctrl+C cancels only that worker and a second one within
  two seconds cancels the run.
- Agent orchestrator handoffs (S4, same gate): the orchestrator records each
  finished worker with a `handoff` (verdict, reason, done, left, decisions)
  and ends the run with `finish`; an `off_course` or `blocked` verdict shows
  in the transcript. Missing handoffs are filled in by lazarobox, never from
  worker text. Each handoff and the closed run ledger are saved to engram
  under `orchestra/<run>/...` keys; with memory off the run turns it on for
  itself only (`memory on for this run`), and when engram cannot be used the
  run goes on with `handoffs not persisted`, said once.

### Known limits

- Chat with `codex` and `gemini` is not fully isolated from the user's own
  setup: they still load the global instruction files (`~/.codex/AGENTS.md`,
  `~/.gemini/GEMINI.md`) and, for gemini, the user's skills in
  `~/.gemini/skills/`. That text reaches the provider and can change
  answers. `claude` is isolated. It predates 1.1.0 and is planned for 1.1.1
  (#519).

### Not yet checked by hand

These parts passed automated tests but have not been tried by hand yet.
Reports are welcome.

- Terminals: WSL and WezTerm have no manual record yet (#490).
- Kitty: keys, paste refusal, and typing commands in bash and zsh (including
  auto mode) were checked. fish, launching from rofi and the desktop entry
  `Path=` in other terminals were not.
- Orchestrator, MCP: a hand-started session against the lazarobox MCP server
  (P04, P05) has not been checked.
- Orchestrator, model-dependent steps: classification, the feature document
  and correcting a written spec (T21 to T25) depend on the coordinator model
  following its instructions (#503).

## [1.0.1] - 2026-10-31

### Changed

- The batch review no longer reads `y locked` in the counter: it says
  `N remaining · ↓ to unlock y`, and the keys row `y after scrolling` (`y
  locked` in panes too narrow for it). Pressing `y` with lines unseen still
  does nothing, but for 2 seconds the counter says what to do first, `scroll ↓
  · N lines unseen · y needs all` (`scroll ↓ · N unseen` in narrow panes). On a
  batch that cannot be approved (too large or empty) `y` says `y can't approve
  this batch · press n`, and an empty batch no longer reads "too large to
  review".
- Write works on Linux and macOS (APFS and HFS+); on any other system an
  approved batch writes nothing and says so. On macOS, a filesystem without an
  atomic no-replace rename (exFAT, SMB, NFS) answers "this system or volume does
  not support a safe no-replace write" and whatever was done is undone, and
  replacing a file drops its extended attributes, ACLs, quarantine flag and
  Finder tags, and the new file takes its group from the parent folder (BSD
  semantics).

### Internal

- Releases can be started with one click: Actions -> Release -> Run workflow
  bumps the version, moves `[Unreleased]` under it, commits, tags and
  publishes, and `dry_run` rehearses it. Pushing a `vX.Y.Z` tag by hand still
  works. See `RELEASING.md`.

## [1.0.0] - 2026-10-30

First stable release. From here on, `config.toml`, the `chat.db` format, the
public CLI and the key bindings follow the stability promise described in the
README. It builds on 1.0.0-rc.1 with what testing the chat on local models
turned up: small Ollama and llama.cpp models now finish write tasks with fewer
stops (the batch review opens by itself, refusals name the chat's folder, a
tool call written as text or a step left unfinished gets one reminder), and
`y` on a model's command in run mode runs it.

### Changed

- With Ollama and llama.cpp, reasoning that a model leaks into its answer
  after a tool round, closed by a bare `</think>`, is now shown as reasoning
  instead of as part of the answer. In those rounds the answer appears whole
  at the end of the round.
- With Ollama and llama.cpp, a model that writes a tool call as JSON text
  instead of making it is asked once, in the same turn, to call the tool. The
  chat shows `the model wrote a tool call as text; it was asked to call the
  tool`. The text is never run, and the extra request counts toward the
  turn's limit.
- With Ollama and llama.cpp, a model whose answer ends by announcing its
  next action ("I'll create the test next") without a tool call is asked
  once, in the same turn, to do that step or to sum up. The chat shows `the
  model said what it would do next without a tool call; it was asked to do
  it`. A question, an offer, a courtesy or a summary does not count (#439).
- With Ollama and llama.cpp, an empty answer with tools on, or one whose
  last sentence starts with a sign-off such as "Ready for next steps." or
  "Listo para el commit.", gets the same single reminder; the chat shows `the
  model sent an empty answer; it was asked to go on` or `the model stopped
  before finishing; it was asked to go on` (#439).
- A turn that offers the write, exec or git write tools no longer tells the
  model it cannot change files, in the system prompt or in the `run_in_pane`
  description. The system prompt now names what its tools allow (write,
  test, commit), says it must do every step asked in the same turn, and must
  not claim a file, test run or commit that no tool result shows (#439).
- The result of an applied batch now ends with `Continue with any step the
  user asked for that is not finished yet.`, and the refusal of `run_command`
  and the git tools while changes are staged now says `propose them first
  with propose_batch, then call this tool again` (#439).
- A refused write (`write_file`, `edit_file`, `create_dir`, `propose_batch`)
  now names the chat's folder, so a model learns where its paths start:
  `path not available (paths are relative to the chat's folder ~/proj)`. The
  text is still the same for every reason. Only a missing parent folder of a
  path the policy allowed is named: `the folder src/ does not exist`.
- When a chat turn ends normally and the model left staged changes without
  calling `propose_batch` (common with small local models), the batch review
  opens by itself, with the same coverage check, arming delay and `y`/`n` as a
  proposed batch. A cancelled, timed-out or failed turn, or a chat that already
  waits on an approval, still leaves the orphan behind `Ctrl+G`.
- In approval mode `run`, `y` on a model's `run_in_pane` prompt now runs the
  command: lazarobox presses Enter, waits for it and returns its output to the
  model, like a safe command that runs on its own. The prompt says
  `y run it · n cancel · p next pane`. A command that cannot run (multi-line,
  unknown pane directory, a busy or half-typed pane, a non-UTF-8 locale) is
  typed without Enter as before, and the chat line says why. `ask`, `auto` and
  the `Ctrl+Space`, `e` confirm keep typing without Enter.

## [1.0.0-rc.1] - 2026-10-30

First release candidate for 1.0.0. It is published to crates.io, but cargo
does not install a pre-release unless asked for it (`cargo install
lazarobox-shell --version 1.0.0-rc.1`). Use it, report what turns up, and
1.0.0 follows from the last good candidate. From 1.0.0 on, `config.toml`, the
`chat.db` format, the public CLI and the key bindings follow the stability
promise described in the README.

### Added

- Chat folder: the chat now asks for its folder instead of silently using the
  folder it was launched from. When it opens (or resumes a conversation)
  with no usable folder, for example from a desktop launcher that starts in
  `$HOME`, a picker offers the last used project, the repository roots of the
  panes, recent chat folders and a typed path with `Tab` completion; `Esc`
  keeps the current folder. `/cd <folder>` changes the folder of an open
  chat: it is your command (it never reaches the model), asks `y` showing the
  old and the new folder, and waits while a write, an approval or a command
  is pending. The chat remembers its last folder and a resumed conversation
  opens in it. To send a message that really starts with `/cd`, type
  `//cd`.

- The codex row of the AI menu names how codex is installed after its version, for example `codex available · 0.153.4 · standalone` (also `npm` and `homebrew`; nothing for an unknown layout). It is a label for messages only: the jail rules still come from the real paths alone (#180).
- A menu save says where it wrote in the statusline (`saved to
  ~/.config/lazarobox/config.toml`), and the README has a section on where
  `config.toml` lives.
- `lazarobox-shell --version` (`-V`) and `--help` (`-h`) print and exit 0,
  before the terminal or the nesting guard are touched.
- Start folder: `lazarobox-shell --dir <folder>` and `[workspace] dir` in
  `config.toml` choose where the first shell starts (flag, then config, then
  the launch folder). An invalid folder does not stop the app: it starts in
  the launch folder and shows a notice inside lazarobox.
- `install-desktop --dir <folder>` writes `Path=` and `--dir=` in the
  launcher; running it again without `--dir` removes both.

### Changed

- When the jail refuses a codex layout, the full reason (`y` in the menu, and the chat) now names the layout and what to do, for example `codex (standalone) is installed in a layout the jail cannot isolate: … would be both run and written; reinstall it with the official installer, npm or Homebrew`. The short form on the menu row is unchanged.
- `codex --version` now runs through the jail, like the model list. Before, it ran outside it, and with the standalone install the binary sits in codex's own writable folder.
- An unknown option or command now prints one line on stderr and exits 2
  instead of silently starting the TUI.
- `chat.db` schema 3: the closed-list checks on `profile`, `role` and `status`
  are gone (the values are still validated when written), so a new profile,
  role or status can be added without rebuilding tables. Opening an older
  file rebuilds `conversations`, `messages` and `tags` once, in one
  transaction, keeping every row and id. A build that only knows schema 2
  refuses a schema 3 file and leaves it untouched. The migration drops
  messages or tags whose conversation no longer exists (they could never be
  shown) and vacuums the file afterwards. Resuming a conversation leaves out
  messages of a role this version does not know and says how many.

### Fixed

- The AI menu no longer shows `available` for codex or gemini when the jail will refuse to start them. The row reads `codex refused · …/bin is run and written`, cannot be chosen, and the chat explains why. claude keeps `available · no jail: …` where it really runs without a jail (#180).
- The nesting guard no longer treats the value of `--dir` as a subcommand.

### Docs

- README: the chat folder picker and `/cd`, and the key reference for both.
- README: new "Estabilidad y compatibilidad (1.0)" section (what is frozen:
  `config.toml`, the `chat.db` format with forward migration, the public CLI
  and the key bindings; what is not: `__*` subcommands, log text, UI layout),
  an upgrade note, a single key-bindings reference for every view (the
  `openspec/` link is gone), a security and threat-model index, and macOS
  stated as not officially supported in 1.0.
- `SECURITY.md`: supported versions and private vulnerability reporting.
- `RELEASING.md`: the post-1.0 SemVer rule and the pre-1.0 release checklist.
## [0.30.0] - 2026-10-29

### Added

- Ollama downloads: with the Ollama server running, its model level ends in
  `download from Ollama…`. It lists up to 20 of the most popular models of
  the Ollama library that fit this PC's memory (embedding and cloud-only
  models left out), then the exact size of each tag from its registry
  manifest, then a confirmation with the license title, a link to read it,
  the models folder and the free space. Nothing is pulled before `y`. The
  pull goes through the local Ollama server (`POST /api/pull` on
  `127.0.0.1`), with progress on the row and in the status bar, a cancel
  prompt that keeps what Ollama pulled, and one detection when it ends.
  When the library page cannot be read, the built-in list is shown instead.
- One download runs at a time across Hugging Face and Ollama: while one
  runs, the other entry row says `a download is already running`.

### Changed

- The loopback client may POST to `/api/pull`, for the Ollama download only;
  chat requests still refuse that path.

### Fixed

- A held key no longer repeats a decision: on prompts, confirmations and
  menus, a key held down acts once, and only a new press acts again.
  `Ctrl+H` works as Backspace in text fields.
- Tool approval prompts arm after 800 ms instead of 400 ms, so the first OS
  key repeat of a held key (about 600-760 ms) can never answer one.
- The feedback form's footers say what `Esc` does on every screen.

## [0.29.0] - 2026-10-28

### Added

- Chat COPY mode (`Ctrl+Space [` in a chat pane): `v` and `V` select, `y`
  copies through OSC 52 (75 000-byte limit, same notice as the shell) and
  returns to TERMINAL. The text is clean: no drawing prefixes, wrapped lines
  rejoined, no key hints. `Ctrl+Y` (last answer) and `Ctrl+B` (last code
  block) also work inside COPY mode.
- Run mode (`approval = "run"`):
  - `read_file` and `grep` inside the chat's folder run without asking. The
    path is confined to the root when the file is opened. A read outside the
    folder still asks, a sensitive file is still refused, and with a cloud
    engine every read asks when the chat's folder is `$HOME` or `/`.
  - Safe pane search commands (`cd`, `pwd`, `ls`, `tree`, `grep`, `rg`,
    `find`) run without asking while they stay inside the chat's folder.
    Leaving the folder, a broad root with a cloud engine or an oversized walk
    asks; reaching a sensitive file, a git config or a secret folder is
    refused. `rg` is checked with ripgrep's own walker (`ignore` crate).
- A skill for coding agents to report lazarobox defects to GitHub, with the
  user's consent, after searching existing issues.
- README: demo GIFs for panes, COPY mode, the AI menu, history and run mode.
- Git tools for the chat, behind two toggles that are off by default: `git`
  (`git_status`, `git_diff`, `git_log`) and `git_write` (`git_add`,
  `git_commit`). No other verb exists and no tool takes a free-form argument
  list. Linux only; git must be in a system place.
  - The read tools exclude secret file names by pathspec, redact the whole
    output and are refused while a write batch is staged or proposed. A
    refused path opens no prompt. A local engine never asks; a cloud engine
    asks only for `git_diff`.
  - `git_add` shows a one-line prompt with the paths and the file count. Paths
    that cannot be shown whole, or that contain `'`, are refused.
  - `git_commit` opens a full-pane review on every engine: the message, a
    `signing disabled for this commit` notice, the hooks that will run and the
    whole staged diff (`--text`). The commit is unsigned, has no trailer and
    runs its hooks inside the jail. It is refused on a secret in the index or
    a cut diff, and the index is checked byte for byte again after `y`. The
    commit has a 120 s timeout.
  - After the commit the committed tree is compared with the reviewed one. A
    difference (a hook changed files) is reported, never undone, with a
    `git reset --soft HEAD~1` hint. An edit of the message by a `commit-msg`
    hook is not detected.
- README: how the chat's folder is chosen (the focused pane's folder when you
  open the chat, fixed for its life; CLI engines run in a private folder) and
  a threat model for `run_command` and git (the allow-list bounds the verb,
  the network is on, `.env` can be exfiltrated, UNIX sockets stay reachable,
  daemonizing grandchildren, hooks that change a commit or are written by the
  model).

### Changed

- The `git` and `git_write` rows of the AI menu now offer real tools instead
  of only holding the switch.
- `mkdir` and `git` are no longer on the run-mode allow-list: they always
  ask.
- New dependency: `ignore` 0.4.

## [0.28.0] - 2026-10-28

### Added

- `llama-server` can be installed from lazarobox, the one documented exception
  to "lazarobox never installs anything". With llamacpp `not installed`, its
  model list offers `install llama-server…`: pick a build (CPU, Vulkan on
  Linux with a driver, Metal on macOS), confirm the download and the install
  with `y`. The archive comes from `github.com/ggml-org/llama.cpp`, is held
  to its published SHA-256 digest (and verified with `gh attestation verify`
  when `gh` is logged in; otherwise a warning is shown), and is unpacked
  under `$XDG_DATA_HOME/lazarobox/llama.cpp/`.
  A `llama-server` of your own on the `PATH` always wins. With an owned
  install the list offers `remove the llama.cpp install` (y/n), which stops a
  running managed server first, and opening llamacpp tells you once per run
  when a newer build exists (of the installed variant, Vulkan included; never
  with `[updates] check = false`, silent offline, and it never replaces
  another footer notice). The Hugging Face download row
  no longer needs `llama-server`.
- `run_command` asks before every command, in every approval mode, and the
  prompt now says what the user is approving: the timeout, that the network
  is on and a test can read and send out the project's `.env`, that the output
  goes to the model (and, on a cloud engine, to its provider), which project
  files the program runs (`build.rs`, `package.json` scripts, `conftest.py`,
  `go.mod`), and, for `npm`, `yarn` or `pnpm` `install` and `ci`, that the
  install scripts of the project and its dependencies run, with the project's
  own lifecycle scripts shown. The notice is part of what must fit on screen
  before `y` answers. Cloud engines can now run commands (Linux only).
- The Exec row of the AI menu reads `on · needs Linux sandbox` when the system
  has no Landlock jail.
- Arguments with shell characters (`; | & < > ( ) * ? # ~ { } !`) show quoted
  in the prompt.
- A cancelled run that touched git hooks or config is told in the next
  command prompt of that chat, and still reaches the model.
- Chat memory: the history sent to the model is now sized in tokens against
  the engine's context window (llama.cpp reports it through `/props`), and the
  status line shows `context N%` once the window is known.
- When a long chat overflows the window, the messages that drop out are folded
  into a rolling summary sent as an inert block, instead of being lost. The
  summary uses the `summarize` task only when you set it in
  `[ai.tasks.summarize]`, otherwise the chat's own engine and model (never the
  general one). `[ai.chat] compaction = "off"` turns it off (default `"auto"`).
- `[ai.memory] save_summaries = true` (off by default) also saves that summary
  to engram, as one note per conversation. The first time it would have saved
  one, a line in the chat explains what would be saved and how to enable it.
- llama.cpp gets the tool loop on an OpenAI-style wire profile. It stays off
  (`llamacpp` is not in the verified set) until it is checked against a real
  server.

### Fixed

- `npm`, `yarn` and `pnpm` commands (`test`, `run`, `install`, `ci`) are
  refused outside the folder that has the `package.json` (a manager would read
  a parent one that nothing showed).
- A run that fails after it started now reports the git hooks and config it
  changed instead of dropping them.

## [0.27.0] - 2026-10-27

### Added

- Hugging Face downloads open on a list you pick from with `j`/`k` and
  `Enter`, no typing needed: the most downloaded text-generation GGUF models
  that fit this PC's memory, each with its estimated size (about Q4_K_M, and
  at most 70% of the RAM). `Esc` opens the search, which shows everything
  and marks with `too big for this PC` the repos, and the files of a repo,
  that do not fit.

- Ctrl+O opens the link of the Hugging Face token field, and the prefilled
  issue of the Feedback form, in the browser (`xdg-open`; on WSL `wslview`,
  then `rundll32.exe url.dll,FileProtocolHandler`, since `explorer.exe` opens
  some links in the file manager; on macOS `open`). With no opener the link is
  copied, and
  when the opener fails the link is copied and the footer says so.
  With `Mouse` on, a left click on the token link line opens it too.

- With `[ai.tools] write = true`, chats get four tools to change files in the
  project: `create_dir`, `write_file`, `edit_file` and `propose_batch`.
  Nothing touches the disk while the model stages; `propose_batch` opens a
  full-pane review of the whole batch (new text, diffs, a warning on build
  files), and only `y` applies it, all or nothing:
  - `y` stays locked until every line has been drawn on screen and 800 ms have
    passed; `n` or `Esc` rejects the whole batch; `Ctrl+C` cancels the turn.
  - Writes stay beneath `$HOME`, inside the chat's folder, never through links,
    and never in `.git`, `node_modules` or secret files. Invalid UTF-8,
    control, bidi or zero-width characters and CRLF are refused, not cleaned.
  - Apply checks every target again and refuses the whole batch if one
    changed, writes through same-folder temp files and atomic renames, and
    undoes the files already written if a step fails (`partial` lists what
    could not be undone).
  - A batch left by a cancelled or timed-out turn stays as an orphan: the pane
    shows ` staged N ` or ` orphan N · Ctrl+G `, and `Ctrl+G` opens its review.
    Closing the pane or quitting discards the batches with a notice.
  - `read_file`, `glob` and `grep` see the staged changes, marked
    `(pending, not on disk)`. The model gets counts and paths, never the diff.
- The chat key table lists `Ctrl+G`.

### Changed

- On a cloud engine, `edit_file` asks like `read_file`, once per file per turn,
  because its answers reveal what a file contains.
- Write is Linux only: elsewhere an approved batch writes nothing and says so.
- Feedback: Ctrl+S now sends the issue with your own `gh`
  (`gh issue create`, the body on stdin, no shell), shaped like the issue
  forms and labelled `bug` or `idea`; the form shows `Sent:` and the issue's
  link, which Ctrl+O or a click opens. A `gh` failure keeps the form and shows
  its message. Without `gh`, or not logged in, Ctrl+S opens the prefilled page
  instead. Ctrl+O still opens the prefilled page. lazarobox stores no GitHub
  token.

## [0.26.0] - 2026-10-27

### Added

- The Hugging Face token field shows the link to create a read token
  (`https://huggingface.co/settings/tokens/new?tokenType=read`) on a line of
  its own, so terminals that detect URLs keep it clickable.
- A **Feedback** row at the end of the Settings tab opens a short form (a bug
  report or an idea, a title, a description) that attaches the lazarobox
  version, the OS and WSL, the shell, the terminal and the detected AI
  engines. `Ctrl+S` copies a link that opens the matching GitHub issue form
  filled in; lazarobox sends nothing itself. Secrets in the text are masked
  before any link.
- GitHub issue forms for bug reports and ideas; ideas go to the project's
  Goals board.

### Fixed

- The `downloaded <file>` notice draws control and format characters of the
  file name as `?`, like the rest of the Hugging Face screens.
- A repo id with `--`, or a part ending in `-` or `.`, is refused, as the Hub
  refuses it.
- A partial download longer than its file is removed and fetched again
  instead of failing on every resume.
- A relative `HF_TOKEN_PATH` is read from the working directory, as
  `huggingface_hub` reads it, instead of being ignored.
- Esc while a pasted Hugging Face token is being checked stops the check, so
  the token is not saved.
- `d` on the Hugging Face row disconnects a saved token even when its check
  fails (offline, or a token the Hub no longer accepts).

### Security

- Every copy of the Hugging Face token that curl is given (the token and its
  header line) is zeroed when it is dropped, as the pasted token already was.

## [0.25.1] - 2026-10-26

### Fixed

- On macOS, codex installed with its official standalone installer now runs in the jail (#180). That installer puts the binary under `~/.codex/packages/standalone/releases/<version>/bin`, inside codex's own state folder, so the jail refused to start with `… would be both run and written by the jail`.
  - `~/.codex` stays writable, but `~/.codex/packages` is now read and run only.
  - The state folder and the folders above `packages` cannot be renamed, removed or replaced from inside the jail, so a planted tree can never take the binary's place.
  - The exception applies only when the program itself is in a real `packages` folder with that exact spelling. Every other layout where the program could be written is still refused.
  - Inside the jail codex cannot update itself. Run it directly to update; lazarobox picks up the new version on its next start.

## [0.25.0] - 2026-10-26

### Added

- Download GGUF models from Hugging Face without leaving lazarobox (#282). The llamacpp model list ends in `download from Hugging Face…`:
  - search the Hub or paste `owner/repo`, then pick a quantization (split models count as one);
  - a confirmation shows the file, its size, the license with a link to read it, the target folder and the free space, and nothing downloads before `y`;
  - the download runs in the background with the system `curl` (HTTPS only), with progress in the list and the status bar, and it can be cancelled and resumed;
  - each file's size and sha256 are checked before it lands in the standard Hugging Face cache (`~/.cache/huggingface/hub`), where the model list finds it and the managed llama-server runs it.
- A `Hugging Face` row at the end of the AI tab connects an account for gated or private repos. The pasted token is checked with the Hub and saved where `hf auth login` saves it (`~/.cache/huggingface/token`, mode 0600), never in `config.toml` and never on a command line. `HF_TOKEN` still wins when set.

### Changed

- lazarobox may now download a model, but only from that Hugging Face flow and only after you confirm it. It still never pulls a model through a server.

## [0.24.0] - 2026-10-26

### Added

- The quit prompt names `llama-server (shared)` when the managed llama-server
  this lazarobox started is answering another lazarobox, since quitting cuts
  that reply; the idle stop waits for such a reply and counts the idle time from
  it. Tabs and panes of one lazarobox share its server and never count.
- Managed llama-server: one server per model, each on its own port (the
  configured one and the seven after it). A second lazarobox, or a
  `llama-server` you started in a terminal, serving the same model file is used
  as it is and never stopped by lazarobox; another model starts its own server
  on the next free port. Only a listening socket of your own user is used.
- The quit prompt names what still runs in the panes (jobs started with `&`,
  `nohup` or `disown`, or a program in the foreground):
  `Quit? still running: sleep 77, npm run dev (y/n)`, three names at most and
  fitted to the width. After quitting, the same jobs are written to stderr
  with their process group, also after `exit` in the last pane. lazarobox
  still never stops them: a pane behaves like any terminal.
- `[ai.servers.llamacpp] args` passes extra arguments to the managed
  llama-server, after the ones lazarobox sets: for example
  `args = ["-ngl", "99", "-c", "16384"]`. Each entry is one argument, with
  no shell. A list that sets `--host`, `--port`, `-m` or `--model` (also as
  `--port=8080`), or that is not a list of strings, is ignored whole with a
  startup notice.
- The managed llama-server stops after `[ai.servers.llamacpp] idle_minutes`
  without a turn (15 by default, 0 = never), freeing the GPU memory; the
  next llamacpp turn starts it again. A turn still answering is never cut:
  the idle time counts from its end. The Settings tab of the menu has a
  `llama-server idle` row (5 min, 15 min, 30 min, never) that applies at
  once, with no restart.

### Changed

- Managed llama-server: a port where something else already answers no longer
  fails the turn with `another program answers on 127.0.0.1:<port>`; the next
  free port is used, and the turn fails only when all eight are taken (`no free
  port on 127.0.0.1 from <port> to <port+7>`). With llamacpp managed, the model
  picker keeps listing the local models while another llama-server answers on
  the port.
- The managed llama-server checks its model file before each start. When
  it is missing nothing starts and the chat reads `llama-server not
  started: model file not found: <path>`, instead of the last line of a
  failed llama-server. It is not checked when the config loads, so a drive
  mounted later works on the next turn.

### Fixed

- Nothing lazarobox starts is left running once it ends, however it ends.
  A small watcher (`lazarobox-shell __watch`, in its own session; `ps` on
  Linux shows `/proc/self/exe __watch`) outlives it; every process group
  lazarobox starts (CLI chat turns, MCP servers, the managed llama-server,
  short probes) is listed with it and taken off once reaped. When lazarobox
  quits, crashes, is killed with SIGKILL or loses its terminal, the watcher
  sends SIGHUP and SIGTERM to the groups still listed, SIGKILL after 3 s, and
  exits. Before, a crash or SIGKILL left them all
  running, and on macOS the managed llama-server then held its port so every
  later start failed. A group is only signalled while its leader is still the
  listed process (same start time) or gone, so a reused pid is never hit.
  `engram serve` is never listed.
- Quitting is polite. It used to SIGKILL CLI chat turns, MCP servers and the
  managed llama-server at once. Now, as soon as the terminal is restored,
  everything lazarobox started gets SIGHUP and SIGTERM, and SIGKILL only if
  it still runs 2 s later, while the history and the memory are saved. A
  turn still streaming its answer keeps running until then instead of being
  cut at its next line. A group whose leader ends gets SIGKILL at once for
  what it left behind. If lazarobox is killed during the quit, the watcher
  repeats the same stop, where what a leader left behind gets the grace too.
- A managed llama-server that ends on its own, during its start or later,
  no longer leaves its children running: they get SIGKILL before the server
  is reaped.

## [0.23.0] - 2026-10-25

### Added

- `[ai.tools]` gains five family toggles, all `false` by default: `search`,
  `write`, `exec`, `git` and `git_write`. The AI tab of the menu shows them
  as rows (Search, Write, Exec, Git, Git write) with a caution line for
  Write, Exec and Git write; saving writes only those keys and keeps the
  rest of `config.toml`. A family is offered only while `[ai.tools] enabled`
  is on and the chat's engine is verified for tools. With every toggle off,
  the tools offered to the engine are the same as in 0.22.0.
- With `search = true`, chats get two tools:
  - `glob` lists the files of the project that match a pattern (`*`, `?`,
    `**`, `[...]`), sorted and capped at 100. It never asks for approval,
    like `list_dir`.
  - `grep` searches file contents with a regular expression, capped at 100
    matches or 20 KiB, 200 characters per line. On a cloud engine it asks
    first (`search <pattern> in <folder>`), in every approval mode; on a
    local engine it runs freely.
  - Both skip symlinks, `.git`, `node_modules`, `target` and other build
    folders, binary files and secret files (their names are left out and
    the result says so). `grep` matches against the redacted text, so a
    pattern cannot probe a secret. A folder the ask could not show exactly
    (a symlink, `~`, an absolute path, `..`, hidden characters) is refused.
  - `read_file` takes `start_line` and `max_lines` to read a window of a
    long file, and says where the next window starts.
- Ollama allows 24 tool rounds per turn when any of these families is on
  (8 otherwise).

### Changed

- The menu popup is 22 rows tall (was 16); the full popup needs a terminal
  of at least 21 rows, a smaller one shows the one-line `MENU` fallback.

### Security

- `read_file` refuses a git `config` or `config.worktree` file under any
  `.git` folder, which can hold remote URLs with credentials.

## [0.22.0] - 2026-10-23

### Added

- The model picker of a stopped llama-server lists the `.gguf` models found
  on the machine (the llama.cpp models folder, llama.cpp's cache and the
  Hugging Face cache), marked `lazarobox starts it`. Choosing one and saving
  writes `managed = true` and that `model` under `[ai.servers.llamacpp]`,
  keeping the other keys, and starts llama-server at once, with no restart.
  The statusline says when it is ready or why it did not start.

### Fixed

- macOS: the jail's check that no program folder is also writable ignores
  letter case too. A codex under `~/.codex/...` with `CODEX_HOME=~/.CODEX`
  (the same folder on the default file system) was accepted; now it is
  refused like `~/.codex`.
- With `[ai.servers.llamacpp] managed = true`, the model picker offers the
  configured model while llama-server is not running (marked
  `lazarobox starts it`), instead of the note that says to start it by hand.

## [0.21.0] - 2026-10-23

### Changed

- CI and release workflows: `actions/checkout` v4 to v7.0.1 (Node 24, no more
  Node 20 deprecation warning; it refuses fork checkouts under
  `pull_request_target` and `workflow_run`). Every action is pinned to a
  commit SHA with its version in a comment, and no checkout keeps the git
  token on the runner (`persist-credentials: false`). `rust-cache` v2.9.3,
  `crates-io-auth-action` v1.0.6.
- macOS: the startup check of the jail may take 10 s in total instead of 5 s,
  so a slow first `sandbox-exec` on a loaded Mac no longer turns codex and
  gemini off for the session.

### Fixed

- macOS: the jail's refusals ignore letter case, like the default file system
  does. A state dir or package root spelled `~/library/keychains`,
  `~/LIBRARY/Caches`, `~/.SSH` or `/system` was granted before; now it is
  refused like `~/Library/Keychains`. Linux still matches case.
- The outdated-CLI message names our flag also when the CLI rejects it with
  its value (`--model=<name>`, `--mcp-config=`, `--allowedTools=`,
  `--setting-sources=`): it says `--model`, never the value, instead of the
  generic `it rejected an option`.

## [0.20.0] - 2026-10-23

### Added

- `r` on the menu's Updates row checks crates.io now, ignoring the cache and
  the 24-hour limit. The row reads `checking…` until the answer comes. It does
  nothing with `[updates] check = false` or after an install in this run.

## [0.19.0] - 2026-10-23

### Added

- lazarobox can manage llama.cpp: with `managed = true` and `model = "<absolute
  path to a .gguf>"` under `[ai.servers.llamacpp]`, the first llamacpp chat turn starts
  `llama-server --host 127.0.0.1 --port 39281 -m <model>` and waits until it
  answers; later turns reuse it, and quitting lazarobox stops it (on Linux it
  also dies if lazarobox crashes). Nothing starts on launch or on detection.
  The port is 39281 unless `port` is set. If something already answers there,
  nothing starts and the chat says so. The server's output goes to
  `llama-server.log` in lazarobox's state folder, and a failed start shows its
  last line. Wrong values are ignored with a startup notice.
- A server engine can live on another port: `[ai.servers.<id>] port = N` in
  `config.toml` (for `ollama`, `lmstudio`, `llamacpp` and `vllm`). Detection
  and chat both use it, always on `127.0.0.1`; without it the usual port is
  used. A port that is not an integer from 1 to 65535 is ignored with a startup
  notice. The menu never writes the table and keeps it on save. It is read at
  startup.
- An installed server that is not running (its binary is on PATH, but nothing
  answers on its port) is now listed in the engine picker as `not running`.
  Choosing it shows where lazarobox looks and how to start it, for example
  `llamacpp not running on 127.0.0.1:39080 · start it, then press r:
  llama-server --port 39080 -m <model.gguf>`; Enter or `y` copies it.
  lazarobox does not start the server, unless it manages llama.cpp (below).

- Chat with LM Studio, llama.cpp (`llama-server`) and vLLM. They talk to their
  server's `/v1/chat/completions` on `127.0.0.1` and their usual port (1234,
  8080 and 8000), stream the answer over Server-Sent Events and send no
  credentials, like Ollama. The body holds the model, the messages and the
  stream flag only: no reasoning is shown and no tools are offered. llama.cpp
  is tested against a reply recorded from a real server; LM Studio and vLLM
  use the same format but are not yet verified against a real one. The menu no longer marks them
  `detected · not in chat yet`; only opencode still has no chat adapter.

### Changed

- The engine picker no longer lets you choose an engine that chat has no
  adapter for (today only opencode). Its row still reads `detected · not in
  chat yet`, dimmed, and Enter does nothing on it; before, you could assign it
  and chat then refused it.

### Fixed

- Closing the terminal tab that runs lazarobox no longer leaves the servers it
  started running (MCP servers, CLI chats and the managed llama-server).
  Restoring the dead terminal made lazarobox abort before its quit cleanup.

## [0.18.0] - 2026-10-20

### Added

- The macOS jail: codex and gemini now run on macOS inside a Seatbelt jail
  (`/usr/bin/sandbox-exec`), after a startup check that proves it restricts
  (one 5 s deadline for the whole check). If the check fails they are refused,
  never run without the jail. claude is never jailed on macOS, to keep its
  Keychain login. macOS 14 or later is supported; gemini is not yet verified on
  a real Mac. The jail never grants anything at or under `~/Library` (as
  requested or as resolved), except `~/Library/Application Support/fnm`, and
  the credential folders in it stay refused. It opens one mach lookup,
  `com.apple.SecurityServer`, which codex needs to verify TLS certificates. The
  lookup reaches securityd, which reads keychains outside the sandbox and can
  serve keychain items over that port subject to its own ACLs and prompts; the
  keychain files stay denied by the profile (accepted risk). It refuses a layout where a program sits inside
  a writable folder, and a jailed CLI can still reach loopback, the LAN and the
  metadata address (accepted risk). A step-by-step check for macOS testers is in
  `docs/macos-tester-checklist.md`.

### Changed

- The refusal on systems with neither Linux nor macOS now reads `the CLI jail
  needs Linux or macOS`.

### Internal

- The real codex test of the macOS jail runs in its own non-blocking CI job
  and judges only the server's 401 as success; an answer such as 403, 429,
  `unsupported_country` is an annotated skip.
- Integration tests run fake CLIs through the real macOS jail helper and
  `sandbox-exec` on the macOS CI runners: reads, writes and execs outside the
  granted folders, unix sockets and `bind` are denied, and `/dev/tcp`, the
  environment, odd paths (`=`, quotes, newlines), nested vendor scripts, node
  and an npm-style `#!/usr/bin/env node` shim work. A profile that fails to
  compile (status 65) is now shown as a jail error too.
- The macOS jail helper now builds the `/usr/bin/sandbox-exec` command from
  the rendered profile and replaces itself with it, leaving the environment
  untouched, and the startup probe checks the real profile instead of a
  constant one. A `sandbox-exec` failure (status 71 and its own first stderr
  line) is shown as a fixed jail error and its stderr is never echoed.
- The macOS jail profile renderer (`sbpl::render`): it turns the jail rules into
  a Seatbelt profile text and a list of `-D` values. Paths only ever travel as
  values, never inside the text. The fixed rules read `resolv.conf` at its
  macOS target and keep Homebrew `etc/` closed, like `var/`.
- The jail rules are now built per system (`jail_spec_for`): the Linux rules are
  unchanged, and the macOS rules name no Linux-only path and add the macOS
  system and Homebrew folders to the list that is never granted.

## [0.17.2] - 2026-10-19

### Internal

- The macOS jail groundwork: a startup probe that checks, with three controls,
  that `/usr/bin/sandbox-exec` really confines a program, and the fault types
  that map its result. The jail stays off (`SEATBELT_ENABLED` is `false`), so
  nothing changes for users: codex and gemini are still refused on macOS. CI
  now runs a raw `sandbox-exec` smoke test on both macOS runners.
- More macOS jail groundwork: claude is exempt from the jail on macOS (it
  keeps its Keychain login and stays text-only, as today), and outside Linux
  the jail helper is the resolved path of this program. Nothing changes for
  users yet.

### Changed

- The AI engines menu now marks a detected engine that has no chat adapter
  (lmstudio, llamacpp, vllm, opencode) as `detected · not in chat yet`
  instead of `available · <version>`, so it no longer reads as usable. The
  note comes from the adapter table, so it disappears when an adapter lands.

### Fixed

- The chat no longer fails with `EAI_AGAIN` (or hangs) on WSL2 and on systems
  where `/etc/resolv.conf`, `/etc/hosts`, `/etc/nsswitch.conf` or
  `/etc/localtime` link outside `/etc` (WSL's `/mnt/wsl/resolv.conf`,
  systemd-resolved). The Linux jail now grants the exact target of each link
  read-only, never its folder, for claude, codex and gemini (#267).
- Memory now offers to start engram on WSL2 with `networkingMode=mirrored`
  instead of saying `memory unavailable: engram did not answer`. There a
  connect to a closed loopback port outside WSL's ephemeral range is dropped
  instead of refused, so a timed-out connect now checks whether anything holds
  the port: a free port means engram is not running; a held one is still a
  server that did not answer, so a second server is never offered.

## [0.17.1] - 2026-10-18

### Changed

- On systems without the jail (macOS), codex and gemini now say what to do
  instead. The menu row and the model list read `needs Linux · use claude`;
  the chat and the engine guidance read `the CLI jail needs Linux; on this
  system use claude or Ollama for chat`. claude keeps the neutral text, since
  it runs without the jail there (#180).

### Fixed

- A claude (or codex, or gemini) older than lazarobox needs no longer ends a
  turn with a bare `error: unknown option '...'` (#180). When the last line
  the CLI wrote is that error, the chat now says the CLI is too old and how to
  update it. The option is named only when it is one of the flags lazarobox
  itself passes (`claude does not know the option --permission-prompts: this
  claude is older than lazarobox needs; update it (claude update) and try
  again`); for any other word the message does not repeat it. The secret
  redactor is unchanged. No flag is dropped: the text-only options stay on.
- **claude older than 2.1.259 no longer fails with `unknown option`** (#180).
  The text-only argv needs `--permission-prompts none`, added in claude
  2.1.259. The chat now refuses a turn with a detected claude below that
  version, with the message `claude <version> is too old for lazarobox (it
  needs 2.1.259 or newer); update it: ...; then refresh from the menu
  (Ctrl+Space m, then r)`, and the engine row in the menu
  reads `update needed: 2.1.259+`. A version that cannot be read does not
  block. The text-only flags are never dropped to fit an old claude.

## [0.17.0] - 2026-10-18

### Added

- **Your own MCP servers in the chat.** Declare them in the user config as
  `[[ai.mcp.servers]]` (`name`, `command`, `args`, `env`, optional `project`
  and `enabled`). Global entries apply everywhere; a `project` entry applies
  only to that git root, compared as a real (canonical) path, and an entry
  with `enabled = false` turns a global server off for that project. A
  repo-local file is never read.
  - A server starts only for a chat turn with tools on, and only for an
    engine that will get the tools: Ollama, or a CLI that runs jailed. Each
    start is announced (`mcp: started <name>`); a server that fails gives one
    notice and the others keep working. Servers stop when the chat closes, on
    a turn with tools off and on quit.
  - **Every call asks `y`, even in Chat approval mode `auto`.** The prompt
    shows the server, the tool and every argument key, and says how much of a
    long value is hidden. Arguments are capped at 4 KiB.
  - Results come back wrapped as untrusted data and capped. Tool names are
    qualified as `<server>__<tool>`; built-in tools are never shadowed and
    colliding names are reported.
  - `env` lists variable names only; values are refused. Servers run with a
    scrubbed environment, `HOME` as their working directory, no shell, and
    never receive the session token or `LD_*`/`DYLD_*` variables.
- Releases are published by a GitHub workflow when a `vX.Y.Z` tag is pushed:
  it checks the version and CHANGELOG, runs the tests, publishes to crates.io
  through Trusted Publishing and creates the GitHub Release.

### Fixed

- macOS: the chat's read tools (`read_file`, `list_dir`, `find_dir`) always
  failed, because the random delimiter that wraps a tool result was only
  generated on Linux. It is now read from `/dev/urandom` on every system.
- macOS: the `/private/tmp` and `/private/var/folders` scratch folders are
  now treated as untrusted like `/tmp`, process names are read with `ps`
  where there is no `/proc`, and the MCP shim handshake is always bounded.
- CI runs the tests on Linux and on macOS (arm64 and Intel), with a pinned
  toolchain and an MSRV check.

## [0.16.2] - 2026-10-18

### Fixed

- The update now installs exactly the version the **Updates** row announces
  (`cargo install lazarobox-shell --locked --version <version> --root <root>`).
  It used to install whatever was newest on crates.io, so when that was the
  version already installed cargo did nothing, exited 0 and lazarobox still
  said "Updated to X". Now cargo installs exactly that version or fails,
  instead of installing a different one. A version that is not plain
  `major.minor.patch` is never passed to cargo and the command to run by hand
  is shown instead. The command shown for copying is unchanged: it asks for
  the newest version.

## [0.16.1] - 2026-10-18

### Fixed

- Enter on **Updates** did nothing visible when `~/.cargo` and `~/.cargo/bin`
  are group-writable (mode 0775, the default with umask 002 on Debian and
  Parrot). The update install refused that `cargo` with the strict rule meant
  for the AI CLIs and fell back to a footer that was cut off. The install now
  accepts a `cargo` whose folders are group-writable only when the folder is
  yours and your group is private to you (named like your user, with no other
  member). A shared group (`users`, `staff`) and root-owned folders that the
  group can write are refused, and so is anything others can write or anything
  under `/tmp`. The folders that hold the link to `cargo` (such as
  `~/.cargo/bin`) are checked too, and the check runs again when you press
  Enter. The AI CLI jail keeps its strict rule.

### Changed

- The update texts are in plain words. The menu note says which version is
  available and to go to **Updates**; when lazarobox cannot update itself it
  says so, shows the command to copy and the reason on the footer (installed
  another way, Rust is not installed, Rust folder is too open, or an unusual
  folder name). Without Rust, the note says to install it first (rustup.rs).
  Enter on **Updates** closes the menu, as Esc does, so focus lands on the
  update tab. The update tab starts by saying that it can take a few minutes
  and that lazarobox must stay open, and ends with
  `Updated to X.Y.Z. Restart lazarobox to use it.` A failure says what
  happened and gives the command to run by hand.
- README: short install (with the link to rustup) and update instructions.

## [0.16.0] - 2026-10-17

Agent tools: the chat can now ask lazarobox to look at the user's files and to
use the user's terminal panes. Tools are active on claude, codex, gemini and
Ollama, and each engine was verified manually before this release. They are on
by default, also for existing users who upgrade (see **Changed**).

### Added

- Run a command from the chat. When the last finished answer of a chat ends
  with a shell block (`bash`, `sh`, `shell` or `zsh`), a hint under the block
  names `Ctrl+Space`, then `e`. It opens a confirmation over the composer
  (`run in pane N (<cwd>)`, the exact command and the keys): `y` types the
  command into the target pane and never presses Enter, `n` or `Esc` declines,
  `p` moves the target to the next shell pane of the tab, and `Ctrl+C` cancels
  the answer and declines. The target is the shell pane of the current tab used
  last. Commands with control characters, escape sequences, invisible or
  spoofing characters, or non-ASCII whitespace are refused, never cleaned; a
  multi-line command needs bracketed paste; a pane running a full-screen app
  is refused. It works with every engine because it reads the block of the
  answer.
- Built-in agent tools: `find_dir`, `list_dir`, `read_file` and `run_in_pane`,
  with strict argument parsing (4096 bytes at most per argument). The read
  tools run on the blocking pool with a time limit and cancellation, wrap what
  they return in an untrusted-output block with a per-call nonce (32 KiB cap),
  and redact secrets before the model sees them. `read_file` opens below
  `$HOME` without following symlinks (`openat2`, with a per-component fallback)
  and refuses hard-linked and special files; `list_dir` and `find_dir` leave
  out names that look like secrets and say so.
- Path policy for the read tools. Anything outside `$HOME` is refused, and so
  are the secret roots: `~/.ssh`, `~/.gnupg`, the credential folders of other
  CLIs, `~/.password-store`, `~/.config/gh`, `~/.config/gcloud`, the keyrings,
  lazarobox's own config and data folders (they follow `XDG_CONFIG_HOME` and
  `XDG_DATA_HOME`), and at any depth `.env`, `.env.*`, `.envrc`, `.netrc`,
  `.git-credentials`, `.pgpass`, `.npmrc` and the shell and REPL histories. A
  path is judged as requested and as resolved, so a link to a secret folder
  does not get around it. Every call fails closed when `$HOME` or a secret
  root cannot be read. The list is best effort and is documented as such.
- MCP for the CLI engines. A hand-rolled MCP JSON-RPC core, a per-chat unix
  socket session behind a token, and a stdio shim that relays a chat's MCP
  lines to that socket. The shim is the only extra binary the jail grants,
  together with one socket, and it sends its token only to a private socket
  owned by the user. A tool session lives for one turn and ends at once when a
  turn falls back to text.
- Per-engine wiring, active and verified on each engine: claude (a per-turn MCP
  config, `--allowedTools=mcp__lazarobox`, an empty `--setting-sources`),
  codex (the server through `-c` overrides and its own environment) and
  gemini (a per-turn work folder with an admin policy). Each decoder lets
  through only the lazarobox tool calls, and a failed wiring falls back to a
  text-only turn. A turn with tools tells the model it has them, only on the
  attempt that is wired. The per-turn CLI files are refused to `read_file`.
- Tool rows in the transcript for each call and result, and an approval
  overlay answered per pane, turn and call. Reads from a cloud engine ask
  first with the resolved path, because what a tool returns goes to the
  provider; Ollama is local and reads without asking. A prompt the model opens
  ignores `y`, `n` and `Esc` for a short arming delay, and the timers of a
  turn pause while a call waits for the user. Turning tools off ends the tool
  session of every chat that has one.
- Approval modes `ask`, `auto` and `run`. `ask` (the default) needs a `y` for
  every command. In `auto` a model's `run_in_pane` is typed into the target
  pane without a prompt, but Enter is still never pressed. `Ctrl+Space`, then
  `a` cycles the mode of the focused chat, and a system line says which one is
  on. A chat in `auto` wears an amber ` AUTO ` badge and one in `run` a red
  ` RUN ` badge. `approval = "ask" | "auto" | "run"` in `[ai.tools]` sets the
  starting mode; anything else falls back to `ask` with a notice. The mode
  lasts for the conversation. Auto-typing needs the shell in the foreground of
  an unfocused, visible pane with no earlier unsent auto-typed line; otherwise
  it falls back to the normal prompt, and so it does after five commands in a
  turn.
- `run` mode (auto-execute). For a single command from a short safe list
  (`cd`, `pwd`, `ls`, `tree`, `mkdir`) lazarobox types the command, presses
  Enter, waits for it to settle and returns the output to the model. Words
  accept Latin letters (accented too), digits and `. _ / @ % + : , = ~ -`;
  paths may be unquoted or inside single or double quotes. `&&`, `;`, `|`, `$`,
  redirections, globs and long options are not accepted and use the normal
  prompt. At most 5 commands run in a turn, and three consecutive refusals by
  grammar or path pause `run` until the turn ends. The confirmation of a
  command that did not run directly shows a reason line with the piece that
  caused it, with invisible characters drawn as `?`. The model is told which
  commands run directly and how to ask for them.
- Output of a run goes back to the model as the screen text left after the
  command: 8 KiB and 200 rows at most (head and tail, with the cut marked),
  cleaned, redacted and wrapped like the output of the read tools. A command
  that does not end in 10 s returns `still running` with what is visible. A
  `cd` also reports the new directory in a system line. The result never
  includes the exit code, and a shell that rewrites the typed line (fish
  abbreviations) gives `output not available`.
- `[ai.tools] enabled` (default on) and `approval` in `config.toml`, with a
  **Tools** row and an **Approval** row in the AI menu. The menu writes only
  those two keys and never touches `[ai.mcp.*]`.
- Tools with Ollama. Because Ollama has no CLI that runs tools, lazarobox runs
  the tool loop itself over `/api/chat`: it sends the tools in the `tools`
  field, runs the model's calls through the same registry, approvals and modes
  as the CLIs, and sends the results in a new request of the same turn. At
  most 8 requests per turn; past that the turn ends with a system line and
  keeps the partial answer. A model that does not support tools (a 400 on the
  request with `tools`) is retried once without them, before any round. Tool
  messages live only inside their turn. The loop is active and verified with
  a real model.

### Changed

- Tools are on by default (`[ai.tools] enabled = true`), so an upgrade turns
  them on for existing users. Turn them off with the **Tools** row of the AI
  menu or with `[ai.tools] enabled = false`.
- When the AI section is shown, the config menu groups its rows into two
  tabs, **Settings** and **AI**, switched with `Tab` and `Shift+Tab`. `j` and
  `k` wrap inside the active tab and each tab remembers its row.

### Fixed

- The Gemini workspace trust: the folder is now trusted by environment
  variable and the policy rule carries a `toolName`, so gemini loads the
  lazarobox tools. The env, context, hooks and skills a trusted folder would
  load are switched off, and the inherited trust variable is scrubbed.
- A zoomed tab, an update pane, a half-typed line and a paste that does not
  run the line no longer let an auto-typed command land in the wrong place:
  these cases prompt or clear the unsent mark as appropriate.
- Approvals carry an id drawn from a process-wide counter, so a stale answer
  or withdraw is ignored, and a shell block is offered only from a finished,
  successful answer of the last turn.
- A dispatch that panics no longer leaves the terminal broken, and a client
  that half-closes the MCP connection still gets the replies of its calls in
  flight.

### Security

- Tool output and everything the model can read passes the same cleaning and
  redaction; secrets are removed even when they sit across the read cap or at
  the cut edge of a window.
- Commands are refused, not cleaned, when they contain invisible, bidi or
  spoofing characters, and the text of a model that opens a continuation
  (unclosed quote, `$(`, heredoc, trailing `\`) is refused in `run`.
- The MCP session token is never printed (not in `Debug`, not in the stderr
  tail of a failed CLI), `mcp.json` is wiped after a wired turn, and the
  shim token goes only to a private socket of ours.
- In `run` the pane output is sent to the model, also with cloud engines; in
  `ask` and `auto` the model receives none of it. `cd` and `ls` may go
  outside `$HOME` (secret folders stay refused, also through symlinks) and
  expose only names and `ls -l` metadata; `tree` and `mkdir` stay inside it.
- Residual risks of `auto` and `run` (typing after a half-typed line, `cd`
  hooks, `CDPATH`, the path-check window, PS2 continuation, prompt lines in
  the output) are listed in the README.

## [0.15.4] - 2026-10-17

### Added

- Updates row in the Settings section of the menu. It shows the running
  version (`Updates  0.15.3`); when the update check knows a newer one the
  name carries a `*` and the value reads `0.15.4 available`. Enter acts only
  when an update is available. If the running binary (canonicalized) is under
  `$CARGO_HOME/bin` (default `~/.cargo/bin`), it opens a new tab that runs
  `cargo install lazarobox-shell --locked` directly as the pane's program (no
  shell), with `cargo` resolved on the login `PATH` and run from `$HOME`. On
  exit 0 the tab closes, a notice says `lazarobox X.Y.Z installed: restart
  lazarobox to use it` and the row reads `X.Y.Z installed: restart`. On a
  failure the tab stays open with `update failed (exit N)` so the error can
  be read. Any other install runs nothing: the footer shows the command, and
  Enter or `y` copies it (OSC 52). `[updates] check` stays config-only.
- Spawning a pane with a program and its arguments, and an exit-status event
  (`PtyEvent::Status`) that is sent before `Exited` for panes that ask for it.

### Changed

- The Notify and Sound rows moved from the AI section to Settings (after
  Mouse). Their behaviour (h/l cycling, `t` test and its footer report,
  saving) and the config keys `[ai] notify` and `[ai] notify_sound` are
  unchanged.
- The update check no longer waits for 24 h when the cache was written by
  another lazarobox version and its latest version is not newer than the
  running one (for example right after an upgrade): the next start checks at
  once. The cache now records the checking version (`checked_by`; an old file
  without it counts as another version). A cache written by the running
  version keeps the once-a-day rule, as do a newer cached version and the
  one-hour retry after a failure.
- The update pane trusts its `cargo` like the AI CLIs are trusted. It prefers
  the `cargo` beside the running binary (`<cargo bin dir>/cargo`), else the
  first on the login `PATH`, kept as found for the rustup proxy; one under
  `/tmp`, or writable by group or others (or in such a folder), is refused and
  the command is shown instead. The pane's first line names what runs, a
  non-UTF-8 `PATH` is passed through unchanged, and the install gets
  `--root <parent of the cargo bin dir>` so it always replaces the running
  binary. A symlinked `~/.cargo` is compared by where it points. A child that
  ends without an exit status is a failure (`update failed (no exit status)`)
  and its pane stays open.
- The menu has one more row (Updates), so the smallest full popup is now 8
  rows high (it was 7); below that the one-line fallback is shown.
- The install command shown by the menu note, the statusline hint and the
  footer is `cargo install lazarobox-shell --locked` everywhere.

## [0.15.3] - 2026-10-16

### Added

- Desktop notification for finished chat turns, independent of the outer
  terminal. `[ai] notify` now takes `"auto"`, `"desktop"`, `"bell"` or
  `"off"` (the Notify row of the AI menu cycles them). `"auto"` always rings
  the bell and also sends a desktop notification when `DISPLAY` or
  `WAYLAND_DISPLAY` is set and `notify-send` is on the login `PATH`.
  `"desktop"` sends only the notification and rings the bell if it is
  unavailable or fails. `notify-send` runs without a shell, detached, with a
  2 s limit; its title is `lazarobox` and its text is a fixed line naming the
  pane, never any of the answer. The old booleans keep working (`true` is
  `"auto"`, `false` is `"off"`), and the default stays off.
- The desktop notification is hardened. The child gets an empty environment
  plus an allow-list (`PATH`, `HOME`, `LANG`, `LC_*`, the session bus and
  display variables), never secrets. A `notify-send` under `/tmp` or writable
  by group or others is skipped, like the CLI interpreters. The availability
  check runs off the UI thread. The desktop counts as unavailable over SSH or
  without a display and a session bus. At most one send is in flight, so a
  burst of finished turns is coalesced (`"auto"` still rings the bell for
  each).
- lazarobox plays its own sound with the notification in `"auto"` and
  `"desktop"` (`[ai] notify_sound`, default true, and a Sound row in the AI
  menu): `canberra-gtk-play -i message-new-instant`, else `pw-play` or
  `paplay` with the first of the freedesktop `message-new-instant.oga` and
  `complete.oga`. It follows the same rules as `notify-send` and does
  nothing when no player is found.
- `t` on the Notify or Sound row of the AI menu fires one test notification
  now, with the mode shown (saved or not) and ignoring the focus rule. The
  footer says what happened (`sent: bell + desktop + sound`) or why not
  (`desktop unavailable: no session bus`).
- How the notification machinery works, and its guard rails. The machine is
  probed once at startup, in the background, and again by the `t` test: the
  session (display, session bus, not over SSH), the trusted `notify-send` and
  the sound players. No send ever waits for a probe; if the result is not
  there yet, or the probe failed, the desktop and the sound are unavailable
  for that event and `desktop` mode rings the bell. A tool installed after
  startup is seen after `t` or a restart. Each probe takes a ticket and a
  slow or old probe never overwrites a newer plan; a probe that panics is
  caught (without tearing the terminal down) and recorded as failed, and `t`
  reports "the check failed" and probes again. Just before an exec the real path
  is re-checked (exists, not under `/tmp`, not group- or world-writable),
  and that is the path that runs, with the found name as `argv[0]`. The
  child gets an empty environment plus an allow-list (`PATH`, `HOME`, `USER`,
  `LOGNAME`, `LANG`, `LANGUAGE`, `LC_*`, the session bus, display, audio and
  XDG variables), never secrets. SSH only disables the desktop when no local
  display is set, so a stale `SSH_CONNECTION` in a local tmux does not.
  Limits: 2 s per program (a program still running is killed and counts as
  failed), 4 s per send, 12 s for the sound chain, 3 s for the test probe.
  At most one send, one sound and one test run at a time; a send dropped
  because another is running rings the bell in `desktop` mode, as does an
  unavailable, untrusted or failed one, even if the send task dies. The
  sound tries canberra, then `pw-play` and `paplay` with each existing sound
  file; only an exit 0 within the limit is success, anything else moves to
  the next candidate, and the test report names the player that worked or
  why all failed. The test report lives in its own footer field until the
  next key press and is sent after its in-flight flag is cleared. In
  `desktop` mode a missing `notify-send` rings the bell and the sound may
  play too: they overlap by design. A turn that ended by cancel never
  notifies; its fallback body would read "Turn ended in pane N". The
  runtime is held by a guard that bounds its shutdown to 1 s on every way
  out, panics included.

## [0.15.2] - 2026-10-16

### Added

- The AI picker now says why an engine is degraded, and lets you copy it. A
  codex model list that could not be read shows `model list unavailable:
  <reason>` wrapped over up to four rows instead of one clipped row. Move onto
  the note and press Enter or `y` to copy the whole, sanitized reason through
  OSC 52 (the usual copied notice appears). The note is never a model choice
  and never changes the saved model. On the engine list, `y` on an engine
  that runs without the jail copies its full `no jail` reason; Enter still
  picks the engine.
- A background check for a newer version, made with the system `curl` at most
  once a day. When one exists, a text mark shows in the statusline and a note
  in the menu. Nothing is sent but one `GET` to crates.io; with no `curl` it
  does nothing.
- Diagnostics for the AI CLIs: the engine list shows why a CLI runs without
  the jail (`available · no jail: <reason>`), a failed codex model listing
  says why, and a failed chat turn shows the CLI's last stderr line with
  credentials redacted. The suggested `chmod` command ends its options with
  `--`.

### Fixed

- The login-shell PATH lookup works from a launcher again. The shell was
  started with the launcher's bare PATH, so rc files that call `tr` or
  `readlink` (`brew shellenv`, nvm) failed and linuxbrew and nvm never reached
  the merged PATH. The probe shell now starts with the process PATH plus
  `/usr/local/bin:/usr/bin:/bin`. If `$SHELL` gives nothing, `/bin/bash -l -c`
  is tried once (not when `$SHELL` is bash already; it reads the login files
  but not `~/.bashrc`, so a PATH set only there is not seen). Both share one
  3 s deadline, reaping included, instead of 2.5 s for the first alone.
- The login-shell entries now come first in the merged PATH, then the process
  entries it lacks, so the launcher picks the program your terminal picks. A
  stale `/usr/local/bin/codex` no longer hides the one in linuxbrew.
- An empty PATH, or one with only relative entries, no longer reaches the
  current directory: relative and empty entries are dropped everywhere, and
  when nothing absolute is left the PATH is `/usr/local/bin:/usr/bin:/bin`.
- A failed chat turn's error line is sanitized before secrets are redacted,
  so a zero-width character cannot split a key out of the redaction, and an
  error code such as `context_length_exceeded` is no longer taken for a
  secret.

## [0.15.1] - 2026-10-16

### Fixed

- Started from a desktop launcher or rofi, lazarobox inherited a bare PATH, so
  `claude`, `gemini` and `codex` were not detected. lazarobox now reads the
  login-shell PATH once at startup, with a bounded 2.5 s probe. If the probe
  fails or times out, it falls back to the process PATH. The probe runs your
  interactive rc files. The merged PATH is used for detection, `--version`,
  chat turns, the codex model listing and the engram server.

## [0.15.0] - 2026-10-15

### Added

- The codex model list now comes from your account. At detection time
  lazarobox runs `codex debug models` once (4 s timeout, 2 MiB output cap,
  stdin closed, private working folder, scrubbed environment, next to the
  version probe) through the same jail as the codex chat. Like running codex,
  that command may refresh the catalog from OpenAI with your login and update
  codex's own cache (`models_cache.json`). Without the jail it is not run. The
  picker offers `default (CLI picks)` followed by the models the account
  lists, with their display names as labels and the slugs as saved values. An
  older codex without the subcommand, or any failure, shows only `default`.
  The listing is kept in memory and refreshed at most every 10 minutes; the
  menu's `r` forces a new one. A failed refresh keeps the cached list while it
  is under 10 minutes old.
- A codex model that is assigned or typed in `custom…` but is not in the
  account's list is marked `not in your account's list` in the picker, on the
  task row and in the footer. It is a warning only.
- The menu footer shows `unsaved changes · s to save · Esc discards` while the
  AI draft differs from the saved config.

## [0.14.0] - 2026-10-15

### Added

- Nesting guard, modelled on tmux's `$TMUX`: every shell pane child gets
  `LAZAROBOX=<pid of the instance>`. Starting `lazarobox-shell` with that
  variable set (and non-empty) prints `lazarobox-shell is already running in
  this terminal; use --force to nest anyway` to stderr and exits 1, before the
  terminal enters raw mode. `--force` (a top-level flag) skips the check.
  Subcommands, such as the internal `__jail-exec`, are never blocked.
- `lazarobox-shell install-desktop [--terminal <cmd>] [--uninstall]` installs
  the desktop launcher from the binary itself, so it works after
  `cargo install lazarobox-shell` and with any terminal. The terminal is
  `--terminal`, then `$TERMINAL`, then `xdg-terminal-exec`, then the first of
  kitty, ghostty, wezterm, alacritty, foot, gnome-terminal, konsole,
  xfce4-terminal and x-terminal-emulator in `PATH`, else `Terminal=true`. An
  unknown terminal is launched as `<cmd> -e <bin>`, and any terminal other
  than Kitty prints a compatibility notice.

### Changed

- `scripts/install-desktop.sh` is now a thin wrapper around the subcommand and
  accepts the same options. The launcher template moved from
  `packaging/lazarobox.desktop` into the binary.
- AI model picker: CLI models now show readable labels while the saved value
  stays the CLI value, so `config.toml` is unchanged. claude lists `default
  (CLI picks)`, `Opus 5.5`, `Sonnet 5.5`, `Haiku 5.5` and `Fable 5.1` (the
  CLI aliases `opus`, `sonnet`, `haiku` and `fable`). gemini lists `default
  (auto)`, `Flash 3.8`, `Flash 3.5` and `Flash Lite 3.5` as explicit ids
  (`gemini-3.8-flash`, `gemini-3.5-flash`, `gemini-3.5-flash-lite`)
  instead of the CLI aliases, which may point at retired 2.5 models. A saved
  value outside the list, such as `gemini-2.5-flash`, still loads and is shown
  as a custom value. Task rows and the chat statusline keep showing the saved
  value (`claude › opus`).

### Fixed

- AI chat with the `gemini` CLI no longer ends with `the engine stopped
  answering` when the model is slow. gemini prints its first line at once and
  can then stay silent for a minute or more while it retries a busy model
  (HTTP 503), which tripped the 90 s idle limit. A CLI turn now waits up to
  300 s between outputs (the 15 minute total limit is unchanged; servers keep
  their 60 s), and after 10 s of silence the pane shows `still waiting for the model…`
  instead of a silent spinner.

## [0.13.0] - 2026-10-14

### Added

- Chat memory: deleting a conversation from the list (`d`) can also delete its
  engram notes. With memory on, `ENGRAM_HTTP_TOKEN` set and at least one saved
  note, the prompt reads `Delete conversation? y · n · a also delete its N
  notes`: `y` keeps the notes, `a` deletes the conversation and then each note
  (`DELETE /observations/{id}?expected_project=P` with the bearer token), and
  any other key cancels. The result is one line (`deleted N notes from
  engram`, `deleted K of N notes from engram` or `could not delete notes:
  <reason>`). Without a token or with memory off, the notes stay and a line
  says why. Retention eviction never sends a delete.

### Changed

- Chat memory: resuming a conversation reuses the engram session (and project)
  its notes were written under, so new notes land in the same session and it is
  ended once. A resumed conversation that writes nothing ends no session.
- The conversation list carries the number of notes of each conversation.

## [0.12.0] - 2026-10-14

### Added

- AI chat with the `claude`, `codex` and `gemini` CLIs, using your own login
  (no API key is read, stored or asked for). The conversation is sent to the
  CLI's provider.
  - Text only: each CLI starts with fixed options that remove its tools, so it
    cannot run commands or edit files. gemini gets a deny-all tool policy file
    written into the turn's private temp folder, no extensions and no MCP
    servers (its `plan` mode alone can be left by the model).
  - Linux jail (Landlock) for every CLI turn: no access to `$HOME`, `~/.ssh`
    or other CLIs' folders, writes only in the CLI's own state folder and a
    private per-turn temp folder, and only the interpreter and the CLI can be
    executed. codex and gemini do not run without the jail
    (`<engine>: not available on this system yet`); claude does, and says so
    once in the pane.
  - Script CLIs (npm installs) start through their interpreter. A Homebrew node
    gets read-only access to exactly the library folders its own ELF lists and
    to its OpenSSL config and CA bundle.
  - A newer codex or gemini that adds an event kind, or a codex that drops a
    feature flag lazarobox disables, makes the chat fail with an error until
    lazarobox is updated.
  - Tested with claude 2.1.299, codex-cli 0.142.5 and gemini 0.63.0.
- Scrolling for every pane. The mouse wheel scrolls the pane under the pointer
  without moving the focus: three lines per notch in a shell pane's
  scrollback and in the AI chat transcript, and straight to the application
  when it asks for the mouse (`vim`, `htop`) or runs on the alternate screen
  (`less`, `man`, which get arrow keys). Typing in the pane or pasting returns
  it to the live screen.
- COPY mode on AI chat panes (`Ctrl+Space [`): `j`/`k`, `Ctrl+u`/`Ctrl+d`,
  `gg`, `G`, `Esc` or `q` to leave. The streaming answer keeps arriving while
  the view is scrolled.
- A `[+N]` badge on a scrolled pane, a position in the statusline
  (`TERMINAL ↑12/340`) and a key hint in COPY mode.
- The menu `Mouse` row is now a real `off`/`on` setting with a live preview;
  `Esc` reverts it and `s` saves it as the root key `mouse` of `config.toml`.

### Changed

- The jail no longer lets a CLI read `/usr/bin`, `/usr/sbin` or `/bin`: the
  dynamic loader could otherwise run any program there.
- A script CLI's interpreter in a group- or world-writable folder, or under
  `/tmp`, is refused inside the jail.
- The mouse is captured by default (`mouse = true`), but only the wheel is
  reported: hold `Shift` to select text with the terminal. Set `mouse = false`
  to turn the capture off. The terminal is released on quit, on exit of the
  last pane and on panic.

### Removed

- The `config: mouse is not supported yet` notice.

### Security

- Known limits, also in the README: the CLI state folders are writable
  (a configuration written there runs on your next unjailed run), network is
  not restricted, claude rewrites `~/.claude.json` in place under the jail,
  claude's system prompt is visible in `ps`, gemini keeps its sessions in
  `~/.gemini` outside `chat.db`, and `execve` limits are defense in depth.

## [0.11.0] - 2026-10-14

### Added

- Conversation history for the AI chat. Messages are saved to the private
  `chat.db` as you chat. An answer that stops early (an error or a cancel
  after some text arrived) is saved too, marked as partial. The model's
  reasoning is never stored.
- Conversation list: `Ctrl+Space c` now opens the list instead of an empty
  chat. It shows the conversations of the current project (the git
  repository), newest first, as `date · profile · first message`.
  - `a` shows or hides the conversations of all projects.
  - `n` or `Esc` starts a new conversation.
  - `Enter` resumes the selected one: the full history is loaded, new
    messages are appended to the same conversation, and its stored profile is
    used.
  - `d` then `y` deletes it (any other key cancels). A pane that has the
    deleted conversation open stops saving and says so.
- Tags: each conversation has a project tag plus up to 8 extra tags. Edit them
  with `t` on a list row or with `Ctrl+Space T` inside a conversation (one
  comma-separated line; the project tag comes first; `Enter` saves, `Esc`
  discards). A saved edit counts as activity, so the row moves to the top of
  the list.

### Changed

- When an answer fails after some text was received, the partial text is kept
  in the history with status `error`; the error line itself is not stored.
- Closing a pane (or a tab, or quitting) while an answer is streaming saves
  the text received so far as `cancelled`.

### Known limits

- Extra tags come back in alphabetical order after a conversation is reopened.
- Deleting the engram notes of a deleted conversation and resuming the engram
  memory session are not part of this release (they arrive with the
  `engram-memory` follow-up).
- `Ctrl+Space T` has not yet been checked by hand in Kitty.

## [0.10.0] - 2026-10-14

### Added

- Optional project memory with [engram](https://github.com/Gentleman-Programming/engram),
  shared with other agents that use it. It is off by default: turn it on in
  the AI menu (`Ctrl+Space m`) or with `[ai.memory]` in `config.toml`
  (`enabled`, `acknowledged`, `port`). A one-time privacy warning must be
  accepted first. It works in git projects only and needs an engram 3.x server.
- Memory block at the top of a chat: shown as `▸ memory (N notes)`, and
  `Ctrl+Space o` shows or hides it.
- Save a note from the chat with `Ctrl+Space n` (`Ctrl+S` saves, `Esc`
  closes). Recognizable secrets are redacted first, and the model's reasoning
  is never stored.
- Conversation summary with `Ctrl+Space s`: review the draft (`Ctrl+S`
  approves, `Esc` discards); an approved summary is sent to engram when the
  chat closes.
- When engram is not running, the chat offers to start `engram serve`, showing
  the absolute path of the binary. The server keeps running after you quit
  lazarobox.
- Statusline indicator `mem: ready`, `mem: checking` or `mem: unavailable`.
  Every wait on memory is bounded to 5 seconds, so a slow or missing server
  never blocks the chat.
- Chat notification: lazarobox rings the terminal bell when a chat answer
  finishes while you are not looking at it (another pane, tab or window is
  focused). Turn it on with `notify = true` in `[ai]` or with the new Notify
  row in the AI menu. It is off by default.

### Changed

- `chat.db` moves to schema version 2 to hold the memory data. An older
  lazarobox refuses a newer `chat.db` and leaves it untouched.
- The chat database is now opened lazily: a session that never uses the chat
  no longer creates `chat.db` or its directory.

### Known limitations

- Conversations are not saved yet; chat history arrives in a later release.
  Deleting notes from lazarobox and resuming conversations come with it.
- Only Ollama has a chat adapter, and only it receives memory. Other engines
  are detected but cannot chat.
- Memory requires engram 3.x.
- The chat talks to `127.0.0.1` only and sends no credentials.

### Notes

- Still pending manual verification in Kitty: the `engram serve` offer, the
  note keys, the memory line toggle, the summary keys and the notification
  bell.

## [0.9.0] - 2026-10-13

### Added

- AI profiles for the chat: `analyst` and `programmer`, set with `profile` in
  the `[ai]` section of `config.toml` (default `programmer`) or with the new
  Profile row in the AI menu (`Ctrl+Space m`). An invalid value is reported and
  the default is kept.
- Built-in system prompts: ten in total, one for each profile and task
  (`general`, `code`, `chat`, `review` and `summarize`). The chat uses the
  `chat` prompt of the chosen profile.
- Prompt overrides: put your own prompt in
  `~/.config/lazarobox/prompts/<profile>/<task>.md`. The task file wins, then
  `general.md`, then the built-in prompt. Files must be regular UTF-8 text of
  at most 16 KiB; one that cannot be used is skipped with a notice.
- Workspace context in the system prompt: a short block with the project, git
  branch, current directory relative to the repository root, OS, shell and
  date. It never includes absolute paths, environment variables, file contents
  or terminal output; your home directory is shown as `~`, and git is never
  run. Turn it off with `context = false` in `[ai]` or with the new Context
  row in the AI menu. It is on by default.
- If the context cannot be prepared within 2 seconds, the chat falls back to
  the built-in prompt and shows the notice `workspace context unavailable`.

### Fixed

- Closing a chat pane while its first message was waiting for the prompt no
  longer sends that message.

### Known limitations

- Conversations are not saved yet; chat history arrives in a later release.
  Starting the app still creates an empty, private database file at
  `$XDG_DATA_HOME/lazarobox/chat.db` (default `~/.local/share/lazarobox/`),
  for that future history; nothing is written to it yet.
- Only Ollama has a chat adapter. Other engines are detected but cannot chat.
- The chat talks to `127.0.0.1` only and sends no credentials.

## [0.8.0] - 2026-10-12

First release published to crates.io.

### Added

- **Experimental** AI chat pane: `Ctrl+Space c` opens a chat with a local
  model inside the workspace. It works only with Ollama for now.
- Streamed answers: the reply appears as the model writes it, with wrapped
  prose and fenced code blocks.
- Reasoning block for models that "think" (for example `qwen3`): the thinking
  is kept apart from the answer and `Ctrl+R` shows or hides it. A statusline
  indicator shows while a model is thinking, even if another pane is focused.
- Composer keys: `Enter` sends, `Alt+Enter` or `Ctrl+J` adds a newline,
  `Esc` or `Ctrl+C` cancels the running answer, `Ctrl+U` clears the input,
  `PageUp`/`PageDown` scroll the transcript, `Ctrl+Y` copies the last answer
  and `Ctrl+B` its last code block (through OSC 52).
- The chat uses the engine and model of the `chat` task (`[ai.tasks.chat]` in
  `config.toml` or the AI menu, `Ctrl+Space m`); without its own setting it
  falls back to the general engine. The pane explains what is missing when
  the engine is not set, not running or not supported.
- Closing a chat pane or tab (for example `Ctrl+Space w q`) cancels its running
  answer.

### Known limitations

- Conversations are not saved yet; chat history arrives in a later release.
- Only Ollama is supported. Other engines are detected but have no chat
  adapter yet.
- One fixed system prompt; profiles come later.
- The chat talks to `127.0.0.1` only and sends no credentials.

## [0.7.0] - 2026-10-12

0.7.0 is the baseline before the first crates.io release and was not published.

### Added

#### Terminal panes and tabs

- Full-screen terminal pane running your `$SHELL` in a PTY, with a VT100
  emulation layer and TrueColor. Linux support only.
- Modal input: TERMINAL, PREFIX (`Ctrl+Space`) and COPY modes, with a quit
  confirmation.
- Cursor shape passthrough and a statusline with the current mode, directory
  and shell.
- Splits, geometric focus, sticky resize and zoom for panes.
- Tabs with a tab bar that appears when there is more than one tab.
- Which-key style prefix groups behind `Ctrl+Space`.
- New panes and tabs inherit the working directory of the focused pane, tracked
  through OSC 7 with a `/proc` fallback.
- A pane closes when its shell exits; the last tab closing quits the app.

#### Configuration menu

- Config menu on `Ctrl+Space m` with a Settings section: statusline and tab bar
  position (top or bottom), previewed live.
- `s` saves to `config.toml` keeping the user's comments, line endings and
  key order; `Esc` reverts. `Enter` cycles a Settings value or opens the
  picker, it never saves. Writes are atomic and refused when the file has
  errors.

#### Copy selection

- COPY mode with scrollback navigation using vim-style motions.
- `v` selects characters, `V` selects whole lines.
- `y` copies the selection to the system clipboard through OSC 52 (selections
  over 75,000 bytes are not copied) and returns to the terminal.
- Selection highlight and a VISUAL / V-LINE marker in the statusline.
- A held COPY exit key no longer re-triggers after leaving COPY; empty yanks
  are skipped and the COPY cursor resets on screen switch or resize.

#### Desktop launcher

- Desktop launcher (`packaging/lazarobox.desktop`) and a per-user installer
  script (`scripts/install-desktop.sh`) that opens the workspace in Kitty from
  rofi or the application menu.

#### AI engine detection

- Detection of local AI engines: `ollama`, `lmstudio`, `llamacpp` and `vllm`
  (loopback servers on their usual ports) and the `claude`, `codex`, `gemini`
  and `opencode` CLIs found in `PATH`.
- AI section in the config menu, with the detected engine status, and `[ai]`
  engine and model keys in `config.toml`.
- Detection is read-only, uses `127.0.0.1` only and sends no credentials; the
  only command run is `--version` on the CLIs found. Engine responses are
  parsed defensively and version probes are bounded.
- `r` re-detects the available engines from the menu.

#### Per-task AI model selector

- Per-task AI model selection in the config menu: a general "Set all tasks"
  engine plus one row for each of the five AI tasks (general, code, chat,
  review, summarize).
- Two-level picker (engine, then model). The CLI engines end with a `custom...`
  row to type or paste a model name; custom names reject format characters.
- Curated Ollama and CLI model catalogs. For server engines, models that are
  not installed are marked `download required` and cannot be chosen; nothing is
  downloaded.
- `s` saves the choice to the `[ai]` and `[ai.tasks.*]` tables of
  `config.toml`; `Esc` discards it. `Enter` never saves.

[Unreleased]: https://github.com/pichu2707/lazarobox-shell/compare/v1.20.0...HEAD
[1.20.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.19.0...v1.20.0
[1.19.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.18.0...v1.19.0
[1.18.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.17.0...v1.18.0
[1.17.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.16.0...v1.17.0
[1.16.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.15.2...v1.16.0
[1.15.2]: https://github.com/pichu2707/lazarobox-shell/compare/v1.15.1...v1.15.2
[1.15.1]: https://github.com/pichu2707/lazarobox-shell/compare/v1.15.0...v1.15.1
[1.15.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.14.4...v1.15.0
[1.14.4]: https://github.com/pichu2707/lazarobox-shell/compare/v1.14.3...v1.14.4
[1.14.3]: https://github.com/pichu2707/lazarobox-shell/compare/v1.14.2...v1.14.3
[1.14.2]: https://github.com/pichu2707/lazarobox-shell/compare/v1.14.1...v1.14.2
[1.14.1]: https://github.com/pichu2707/lazarobox-shell/compare/v1.14.0...v1.14.1
[1.14.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.13.0...v1.14.0
[1.13.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.12.0...v1.13.0
[1.12.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.11.0...v1.12.0
[1.11.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.10.0...v1.11.0
[1.10.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.9.0...v1.10.0
[1.9.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.8.0...v1.9.0
[1.8.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.7.1...v1.8.0
[1.7.1]: https://github.com/pichu2707/lazarobox-shell/compare/v1.7.0...v1.7.1
[1.7.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.6.0...v1.7.0
[1.6.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.5.0...v1.6.0
[1.5.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.4.0...v1.5.0
[1.4.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.3.1...v1.4.0
[1.3.1]: https://github.com/pichu2707/lazarobox-shell/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.2.4...v1.3.0
[1.2.4]: https://github.com/pichu2707/lazarobox-shell/compare/v1.2.3...v1.2.4
[1.2.3]: https://github.com/pichu2707/lazarobox-shell/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/pichu2707/lazarobox-shell/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/pichu2707/lazarobox-shell/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.0.1...v1.1.0
[1.0.1]: https://github.com/pichu2707/lazarobox-shell/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.0.0-rc.1...v1.0.0
[1.0.0-rc.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.30.0...v1.0.0-rc.1
[0.30.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.29.0...v0.30.0
[0.29.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.28.0...v0.29.0
[0.28.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.27.0...v0.28.0
[0.27.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.26.0...v0.27.0
[0.26.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.25.1...v0.26.0
[0.25.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.25.0...v0.25.1
[0.25.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.24.0...v0.25.0
[0.24.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.23.0...v0.24.0
[0.23.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.22.0...v0.23.0
[0.22.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.21.0...v0.22.0
[0.21.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.20.0...v0.21.0
[0.20.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.19.0...v0.20.0
[0.19.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.17.2...v0.18.0
[0.17.2]: https://github.com/pichu2707/lazarobox-shell/compare/v0.17.1...v0.17.2
[0.17.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.17.0...v0.17.1
[0.17.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.16.2...v0.17.0
[0.16.2]: https://github.com/pichu2707/lazarobox-shell/compare/v0.16.1...v0.16.2
[0.16.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.16.0...v0.16.1
[0.16.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.4...v0.16.0
[0.15.4]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.3...v0.15.4
[0.15.3]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.2...v0.15.3
[0.15.2]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.1...v0.15.2
[0.15.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.0...v0.15.1
[0.15.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/pichu2707/lazarobox-shell/releases/tag/v0.7.0
