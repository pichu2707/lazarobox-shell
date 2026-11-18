# Proposal: Agents overview (every AI agent at work: who, state, where, how long, cost, and whether it waits for you)

## Intent

Issue #493, "Goal: overview of the AI agents at work (state, place, waiting for you)":

> "One place that shows every AI agent working in lazarobox: what it is doing, where it is working, and whether it needs you. Inspired by how [herdr](https://github.com/ogulcancelik/herdr) lists the agents running across terminal panes."

The owner, 2026-11-10, asked for this proposal to be written and published so someone else can pick it up (verbatim): "si hazlo y lo subimos a github por si no nos da tiempo a nosotros a ponernos con ello". So this proposal names every fact it rests on, every open decision with a recommendation, and a slice plan that a new contributor can start from.

Today the user learns that an agent needs them only by looking at it: a worker's ask sits in its chat pane's approval queue, a failed worker halts its run behind an alert, a chat answer ends in a pane on another tab, and a `claude` or `codex` session in a shell pane waits at its own prompt. This change adds one view that lists them all, a key to jump to each, and an alert when one starts waiting.

## Status of this document

Proposal only. No decision below is the owner's unless marked so; the "Open questions" section holds the decisions, each with options, tradeoffs and a recommendation. The spec deltas are written for the recommended options and mark each requirement that depends on an open question with `(pending Qn)`.

Code references were taken on `main` at 2f20e9df (2026-11-10). V = verified in the code or the specs on that commit. U = unverified (external CLI behaviour, recalled and not checked against a pinned version). "Inferred" = a conclusion drawn from the code, not a behaviour seen running.

## Facts this proposal rests on

### Orchestrator workers (V)

- **What a worker is.** `WorkerSpec` holds the task (role), the engine and model in effect at spawn time (empty when unavailable), the toolset and the deadline (src/core/ai/orchestra/worker.rs:12-23). `WorkerResult` keeps the end status, the elapsed time, the error class and the summed usage (worker.rs:90-104).
- **Worker states.** `WorkerStatus` is `Queued`, `Running`, `Done`, `Failed`, `Cancelled`, `TimedOut`, `Refused` (worker.rs:33-42), with the ids `queued`, `running`, `done`, `failed`, `cancelled`, `timed_out`, `refused` (worker.rs:44-57). There is no "waiting for approval" status.
- **Run states.** `RunState` is `Open`, `Halted { worker, class, text }` ("A worker failed: nothing new starts until the user decides"), `Finished`, `Cancelled` (src/core/ai/orchestra/run.rs:90-101).
- **Inside the run.** Each slot keeps `started: Option<Instant>`, the summed usage and a `parked` flag ("Waiting on its own review", not counted as running) (run.rs:183-206). The `started` instant stays in the run manager; it is not sent to the app (inferred from the `WorkerRow` fields below).
- **What the app is told.** The run manager talks to the app over its own channel, `OrchestraEvent` (src/core/ai/orchestra/event.rs:111). `OrchestraEvent::Worker(WorkerRow)` (event.rs:120) carries run, worker id, kind, engine, model, status, `elapsed` ("How long it ran, once it ended"), usage, `partial` and `overridden` (event.rs:52-72). `Halted` (event.rs:132) tells the app a worker failed and the run waits for the user.
- **Where the app keeps them.** `App::on_orchestra` routes each event to the chat pane that owns the run (src/app/orchestra.rs:202). On a `Worker` event it marks the worker over or not, drops the selection of an ended worker, and stores the row (orchestra.rs:264-290). `ChatState::worker_row` keeps the row as a transcript entry `Entry::Worker`, updated in place (src/core/ai/chat/mod.rs:995-1010). So the workers of every run live inside their chat pane's state; there is no app-wide list (inferred: no other store found).
- **The worker row.** `worker_line` draws `review codex>gpt-5 running`, then `done 12.3s` and the usage label once known, `n/a` for a done worker that reported nothing (src/core/ai/chat/transcript.rs:150-166).
- **Worker transcript.** `Ctrl+Space W` opens a full-pane read-only transcript of the selected worker (src/app/run_view.rs:1-21; `view_worker`, run_view.rs:32; modal-input spec "Worker transcript key", openspec/specs/modal-input/spec.md:1031). A worker is selected with Alt+Up/Down among the queued or running ones, and the selection ends with its worker (modal-input spec:1008-1009; orchestra.rs:271-273). The transcripts of the current run are kept, bounded, with eviction (`trace`, src/core/ai/chat/run_trace.rs:241-260). Inferred: a jump to the transcript of a worker that already ended needs the view to accept an ended worker, which it does not today.
- **Folder of a worker.** A run's read scope is set from the chat folder (agent-orchestrator spec "Read scope of a run", openspec/specs/agent-orchestrator/spec.md:179). No orchestrator code creates a git worktree (no match for `worktree` in `src/core/ai/orchestra/` outside tests), so a worker's "where" is its chat pane and the chat folder.

### The approval queue (V)

- A chat has one pending approval and the worker asks queued behind it, oldest first (`Approvals`, src/core/ai/chat/approval.rs:1098-1123), at most `MAX_QUEUED` = 16 (approval.rs:65).
- `ApprovalAsk::is_worker` and `ApprovalAsk::worker` name the worker whose session an ask belongs to (approval.rs:286-297). So "worker N waits for an approval" can be derived by joining the queue with the worker rows (inferred; no such join exists).
- The chat keeps its approvals in `ChatState.approvals` (mod.rs:445-446; accessor `approvals`, mod.rs:1267).

### Chat pane states (V)

- `Phase` is `Idle`, `Waiting { slow }` ("Sent, no byte yet") and `Streaming` (src/core/ai/chat/mod.rs:287-297; accessor `phase`, mod.rs:1415).
- `ChatState.started` is "When the stream in flight was sent" (mod.rs:360-361). There is no "idle since" stamp (inferred: none found).
- The engine and model of the request in flight are `request_engine` and `request_model` (mod.rs:367-368, 425-426).
- ORCH mode, the run of the pane and its alert: `orch` (mod.rs:656), `orch_turn_in_flight` (mod.rs:674), `orch_run` (mod.rs:679), `orch_alert` (mod.rs:547, `set_orch_alert` mod.rs:702).
- The chat folder is set with `set_folder` (src/core/ai/chat/folder.rs:208); the cwd the chat was opened from is `origin_cwd` (mod.rs:1373).

### Branch and worktree (V)

- The workspace block reads the branch without running `git`: it walks up from the origin cwd and reads the `HEAD` file (src/runtime/workspace.rs:1-3; `git_root`, workspace.rs:95). A linked worktree's gitdir is mapped to its main repository (`main_repo_of`, workspace.rs:143-148). `parse_head` gives a branch or a detached head (src/core/ai/chat/workspace.rs:72-82). This can be reused for a "branch" column, off the UI loop.

### Pane foreground program (V)

- **The probe.** `PtyHandle::probe` takes the foreground process group with one ioctl on the loop, and returns a closure that reads `/proc/<pgid>/cmdline` and `/proc/<pgid>/cwd`, never the environment (src/core/pty/mod.rs:106-125; `read_foreground`, src/core/pty/portable.rs:66-76; `shell_in_foreground` by `tcgetpgrp`, portable.rs:112-117).
- **The roster.** Every 1 s (`CWD_POLL`, src/runtime.rs:129) the loop calls `refresh_roster_in_background` (runtime.rs:1213-1218), which runs the probes on the blocking pool and never stacks a refresh on a running one (runtime.rs:1527-1543). `publish_roster` stores one already-redacted line per pane; "the raw argv never leaves this function" (runtime.rs:2384-2405). `Roster` holds those lines and their age (src/core/ai/tools/panes.rs:57-77).
- **What is kept.** Only redacted text: the program's file name, flags without values and paths, `…` for every other argument; other panes show the program name only; a program title with spaces or `:` shows as `…` (`safe_argv`, panes.rs:145-182; agent-tools spec "list_panes tool", openspec/specs/agent-tools/spec.md:670-671). On macOS the program is `unknown` (agent-tools spec:671). So no structured "program name per pane" exists today; the overview needs one more field from the same probe (inferred).
- **`__watch` is not a detector.** The `__watch` process kills the process groups lazarobox started when lazarobox ends (src/runtime/watch.rs:1-23). The "process watcher used to name jobs on quit" of #493 is `runtime::jobs`, which lists a pane session's process groups on demand for the quit prompt (src/runtime/jobs.rs:1-13; archived change 2026-10-31-quit-names-pane-jobs), not on a poll. The foreground detection to reuse is the roster probe above.
- **Pane environment.** Every pane child gets `LAZAROBOX=<pid of the owning instance>` (src/core/nesting.rs:1-7). No variable names the pane (no `LAZAROBOX_PANE` in `src/`).

### Pane output, bells and titles (V)

- Pane output goes through `App::on_pty`, which feeds the emulator and stamps `now` only on a `run_in_pane` slot (src/app.rs:3572-3590). No per-pane "last output" instant exists (inferred: none found).
- The emulator is `vt100` 0.16.2 (Cargo.toml:43). Its `Callbacks` trait offers `audible_bell`, `set_window_title` and `unhandled_osc` (vt100-0.16.2 src/callbacks.rs:6, 23, 66). lazarobox implements only `unhandled_osc` (for OSC 7) and `unhandled_csi` (src/core/pane.rs:124-135). So a BEL, a window title or an OSC 9 / OSC 777 notification sent by a program in a pane is dropped today.

### Usage and cost (#440) (V)

- The `ai-usage` capability exists (archived change 2026-11-02-ai-cost; openspec/specs/ai-usage/spec.md). Usage is engine-reported only: "lazarobox holds no price table and never computes dollars", and a missing value is `None`, shown as `no data`, never 0 (ai-usage spec:5).
- Worker turns are recorded with the role `worker` (ai-usage spec "Worker usage", spec:315-318). The worker row carries the summed `Usage` (event.rs:63-65), drawn by `Usage::label` as `<in> in <out> out` plus `$x.xxxx` when the engine reported a cost (src/core/ai/chat/engine.rs:315-326).
- A chat keeps the last turn's usage (`turn_usage`, mod.rs:427-429) and the pane's session usage (`session_usage`, mod.rs:434-435).
- Usage of a CLI agent running in a shell pane is not known to lazarobox (inferred: it is not one of lazarobox's engine streams). OxideGate (#440) could know it only if the user routes that CLI through it.

### Notifications (V)

- `[ai] notify` is `auto`, `desktop`, `bell` or `off`, default `off` (`NotifyMode`, src/core/ai/config.rs:107-121), with `notify_sound` (config.rs:424-425).
- `should_notify` is pure: mode not `off`, the turn not cancelled, and the pane not watched (another pane focused or the window unfocused) (src/core/ai/chat/notify.rs:20-30). The desktop body is fixed text, never the answer: `Answer ready in pane N`, `Turn failed in pane N` (notify.rs:140-148).
- The only trigger is the end of a chat turn (src/app.rs:3220-3235, the single `Effect::Notify` site in `src/app*`). An approval that arrives, a worker ask that queues, or a run that halts does not notify today.
- The desktop half runs `notify-send` off the UI thread, from a plan probed once at start (src/runtime/notify.rs:1-8).

### Keys and menu tabs (V)

- The prefix root (`Ctrl+Space`) binds `w t g b [ m c n q` (hinted), and `h j k l o s e a T W r u ?` (unhinted), plus the prefix and alias literals (src/core/prefix.rs:230-275). The root hint is full: new root keys are unhinted (prefix.rs:248-249). Capitals are used when the lower case is taken: `T` tags, `W` worker transcript (prefix.rs:261-263). Free root letters include `A`, `d`, `f`, `i`, `p`, `v`, `x`, `y`, `z`. A test checks that chords are unique in every group (prefix.rs:422).
- `Ctrl+Space u` opens LINKS, a list overlay with row keys, Enter, `j`/`k` and Esc (modal-input spec "LINKS mode", spec:426; `open_link_list`, src/app/links.rs:125). It is the nearest precedent for an overlay list with a jump.
- The menu tabs are a static table of 5: `Settings`, `Models`, `Tools`, `Connectors`, `Chat` (`TAB_COUNT`, src/core/menu.rs:744; `TABS`, menu.rs:750-785). `Connectors` is hidden while it has no rows (config-menu spec "Menu tabs", openspec/specs/config-menu/spec.md:760). The jump digits `1` to `5` are fixed positions (config-menu spec "Jump keys", spec:1626). The popup body has a fixed height (`VIEW_BODY`, menu.rs:859-870), and the menu is a modal config popup (inferred: the user cannot type into a pane while it is open).
- Focus moves with `set_focus_in(tab, pane)` and tabs with `activate` (src/app.rs:2884-2886, 3087-3100), so a jump to a pane in another tab can reuse them.

### Related goals (V, issue text)

- #570 (team work in the cloud): its "Activity" item is "the shared version of the overview in #493". This change stays local; it keeps the row model pure and free of content so a later change could share it.
- #654 (remote panes): `list_panes` shows the local folder, and an `ssh` pane shows as `ssh`. A CLI agent run inside `ssh`, `docker exec`, `tmux` or `mosh` in a pane is invisible to the foreground probe (inferred: the foreground program is the client, not the agent).

## Scope

### In Scope

1. **Agent rows (pure model).** One row per agent, built by the app from state it already holds, with no new IO for chats and workers:
   - **Who:** an orchestrator worker (its role, engine and model, worker `n` of run `r`), a chat pane (its engine and model), or a CLI agent in a shell pane (its program: `claude`, `codex`, `gemini`, `opencode`, from a fixed table).
   - **State:** `working`, `waiting for you` (a halted run, a gate offer, a CLI agent that asked for attention), `waiting for approval` (the chat's pending approval or a queued worker ask), `idle`, `done`, `failed` (also `cancelled`, `timed out`, `refused` for workers, shown as their own ids). For CLI agents the state is what the chosen detection can tell (Q2) and is marked as a guess when it is one.
   - **Where:** tab number, pane number, folder (home as `~`), git branch or `detached <sha>`, `(worktree)` when the folder is a linked worktree.
   - **How long:** since the current state began (working for 2m, waiting for 40s, done 12.3s).
   - **Tokens and cost:** the engine-reported usage when known (`Usage::label`), `no data` otherwise, never estimated; nothing for CLI agents in panes.
2. **The overview view.** A list of the rows, waiting ones first, then working, then the rest, opened by a key (Q1). Read-only.
3. **Jump.** Enter on a row (and a row key, as LINKS) goes to the agent: the pane (switching tab), and for a worker the chat pane with that worker's transcript open when it still has one.
4. **Alert when an agent starts waiting.** When a row enters `waiting for approval` or `waiting for you` and the user is not watching that pane, one notification through `[ai] notify` (Q3), with fixed text that names the agent and the pane, never content.
5. **CLI agent detection in panes.** From the existing 1 s roster probe, the foreground program's name, matched against a fixed table, plus the signals the chosen detection uses (Q2).
6. **Docs.** README section for the view and its key, the key in the help and in the modal-input spec, the privacy note, and the limits of CLI detection.

### Out of Scope

- Agents outside lazarobox (other terminals, IDEs, remote hosts): recommended out of scope (Q5).
- Sending anything about the rows to a model, a log or the network. The overview is local UI.
- Reading other panes' screen text by default, or any per-agent output pattern matching (see Q2).
- The shared, cloud version of the overview (#570) and remote pane awareness (#654).
- Usage or cost of CLI agents in panes (they are not lazarobox engine streams; OxideGate, #440, is a later route).
- Controlling agents from the overview (cancel, approve, retry): the existing keys in the chat pane stay the only way; the overview only shows and jumps.
- New orchestrator states in the run manager. The rows are derived in the app.

## Privacy

- **Chats and workers:** the rows use only state lazarobox already holds about its own agents (statuses, engine and model ids, folder, usage counts). No message, prompt, answer, tool argument or error text appears in a row (the failure alert keeps its text; the row says `failed` and the error class id).
- **Other panes:** only the foreground program's file name is matched, from the probe that already runs for `list_panes`, which reads `/proc/<pgid>/cmdline` and `cwd` and never the environment (pty/mod.rs:106-125). Arguments are not kept for the overview. If Q2 picks bells or OSC notifications, their payload text is not shown or kept: only the fact that one arrived and when.
- **Nothing is sent to a model.** The rows are never part of a prompt, a tool result or `list_panes`. A test pins that the tool texts are unchanged.
- **Logs:** no row text is logged; a log line may carry ids and state names only, as the orchestrator's "No content in logs" rule (agent-orchestrator spec:592).
- **Notifications:** fixed text naming the agent kind and the pane (`Worker 2 waits for approval in pane 3`), as `notify::body` does today (notify.rs:140-148).

## Performance

- Chats and workers: rows are rebuilt on the events that change them (`OrchestraEvent::Worker`, `Halted`, approvals, phase changes), or only while the view is open. No timer is added for them.
- CLI agents: no new poll. The roster probe already runs every 1 s on the blocking pool and does not stack (runtime.rs:1527-1543); this change adds one structured field (the program's file name) to what it already reads.
- Branch: read with the existing `HEAD`-file walk (no `git` process), off the UI loop, only when a folder changes or the view opens.
- Quiet time (if Q2 picks it): one `Instant` stored per pane in `on_pty` (app.rs:3574), no extra work per byte beyond that store.
- The view redraws on the existing animation tick only while it is open (for the "how long" column).

## Limits of detecting a CLI agent's state from outside

- **Presence is reliable, state is not.** The foreground program says that `claude` runs in pane 3; it does not say whether it is thinking, running a tool, or waiting at its prompt.
- **Wrappers.** A CLI installed through npm may run as `node …/bin/claude` or `node …/gemini.js`, and an npm wrapper may start a native child (U for each CLI and version). Matching must look at `argv[0]` and, for `node`, `bun` and `deno`, the file name of `argv[1]`; a wrapper the table does not know shows nothing (a miss, not a false alarm).
- **Hidden agents.** An agent inside `ssh`, `docker exec`, `tmux`, `zellij`, `screen` or `mosh` shows as that program, not as the agent.
- **Quiet time is a guess.** Silence for N seconds can be a long tool call, a slow model, a full-screen pager or a wait for input. As an alert source it gives false alarms; as a display ("quiet 2m") it is honest.
- **Bells and OSC notifications.** Some CLIs can ring the bell or send OSC 9 / OSC 777 desktop notifications when they need input (U: Claude Code's notification channel setting, Codex's TUI notifications; versions not checked). They are explicit "look at me" signals, but they are opt-in in those CLIs, other programs ring bells too (shell completion, `vim`), and the bell does not say why.
- **Hooks.** Claude Code (`Notification`, `Stop` hooks), Codex (`notify` program) and others can run a command on events (U). That is the only exact signal, but it needs the user to configure each CLI and lazarobox to accept a message from inside a pane, which is a new local channel (see Q2, option E).
- **macOS.** The probe reports `unknown` there today (agent-tools spec:671), so CLI detection is Linux and WSL first.

## Capabilities

### New Capabilities

- `agents-overview`: the agent rows (who, state, where, how long, usage), the overview view and its ordering, the jump, the CLI agent table and detection, privacy and performance rules.

### Modified Capabilities

- `modal-input`: a new root prefix key for the view and its keys while open (ADDED "Agents overview key", "Agents overview keys"). Pending Q1.
- `chat-notify`: a second trigger, "an agent starts waiting", under the same `[ai] notify` mode and the same "not watched" rule (ADDED "A wait asks for one notification"). Pending Q3.

Not modified: `agent-tools` (`list_panes` text unchanged), `agent-orchestrator` (no new run states), `ai-usage` (read only), `config-menu` (unless Q1 picks the menu tab, then "Menu tabs" and "Jump keys" are MODIFIED), `configuration` (unless Q3 picks a new key).

## Approach

1. **Core model (pure).** `src/core/ai/agents.rs` (new): `AgentRow { who, state, since, place, usage }`, an ordering function, and a builder that takes plain inputs (chat phases, worker rows, approval owners, run halts, pane programs) and returns rows. Tested without IO.
2. **App wiring.** The app builds the inputs from `PaneState::chat_state` of every pane in every tab (worker rows from `Entry::Worker`, mod.rs:995-1010; approvals, mod.rs:1267; phase, mod.rs:1415; `orch_alert`, mod.rs:547) and keeps a per-row "since" instant, stamped when the derived state changes (workers' running time is not sent by the run manager, event.rs:61-62).
3. **View.** An overlay drawn like the link list (`src/ui/components/link_list.rs`, app/links.rs:125), opened by the Q1 key, a row per agent, row keys `1`–`9`, Enter to jump, Esc to close.
4. **Jump.** `activate(tab)` and `set_focus_in(tab, pane)` (app.rs:2884-2886, 3093). For a worker row, select the worker and open its transcript through the existing `view_worker` path (run_view.rs:32) when it is queued or running; for an ended worker, focus its chat pane (or extend the view to ended workers whose trace is kept, run_trace.rs:241-260; design decision).
5. **Alerts.** The transition into a waiting state of a row is computed in the app; `should_notify` is reused with an `Ended`-like reason (notify.rs:20-30), and a new fixed body. One notification per wait; a wait that ends and starts again may notify again after a short hold (assumption: 30 s) so a chatty approval stream does not ring every second.
6. **CLI detection.** `publish_roster` (runtime.rs:2384-2405) also returns, per pane, the file name of the foreground program (and of `argv[1]` for `node`/`bun`/`deno`), without arguments; the app matches it against a fixed table. Q2 picks which state signals are added.
7. **Branch.** Reuse `runtime::workspace` (workspace.rs:95, 143-148) through an effect, cached per folder.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/agents.rs` (new) | New: rows, states, ordering, CLI table |
| `src/app.rs`, `src/app/` (new `agents.rs`) | Modified / New: inputs, since-stamps, open, jump, alert transitions |
| `src/ui/components/` (new overview list, after `link_list.rs`) | New |
| `src/core/prefix.rs:230-275` | Modified: one root key (Q1) |
| `src/core/ai/chat/notify.rs`, `src/app.rs:3220-3235` | Modified: a wait trigger and its body (Q3) |
| `src/runtime.rs:2384-2405`, `src/core/ai/tools/panes.rs` | Modified: a structured program name next to the redacted lines; `list_panes` text unchanged |
| `src/app.rs:3574` (`on_pty`), `src/core/pane.rs:124-135` | Modified only if Q2 picks quiet time or bell/OSC signals |
| `src/runtime/workspace.rs` | Reused |
| README, help, specs | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| False alarms from CLI agent detection (quiet time, bells from other programs) | High if quiet time alerts | Q2 recommendation: quiet time is displayed, never alerted; bell/OSC alerts behind their own opt-in |
| Alert storms from many worker asks | Med | One notification per wait, a hold before a repeat, and the existing "not watched" rule |
| Turning on a new trigger under an existing `[ai] notify` changes what users who set it receive | Med | Q3 options; README and CHANGELOG say it; `off` stays the default (config.rs:118-120) |
| A row leaks content (error text, argv) to the screen or a log | Low | Rows hold ids, states and counts only; a test lists the row fields like ai-usage's "No text in the record" |
| A pane program name is free text (a program can set its own argv) | Low | Matched against a fixed table only; an unmatched name is never shown in the overview |
| The view grows the per-frame cost | Low | Rows rebuilt on events or while open; the roster stays the only probe |
| Key collision | Low | Q1 key checked against prefix.rs:230-275 and the uniqueness test (prefix.rs:422) |
| The worker transcript jump fails for an ended worker | Med | Fall back to focusing the chat pane, with a notice (design decision in S2) |

## Rollback Plan

Each slice reverts on its own. No config key is added unless Q3 picks one; no stored data or format changes; `list_panes` output stays byte-identical.

## Dependencies

- `agent-orchestrator` (shipped in 1.1.0, #435) and `ai-usage` (archived 2026-11-02): read only.
- `chat-notify`: S3 builds on it.
- None blocking. #570 and #654 are later goals that may reuse the row model.

## Success Criteria

- [ ] `cargo test --locked`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo fmt --all -- --check` pass on every slice.
- [ ] With two chats on two tabs and a run of three workers, the view lists five rows with the right who, state, where and usage, waiting ones first.
- [ ] Enter on a worker row in another tab switches to its tab, focuses its chat and opens its transcript while it runs.
- [ ] A worker ask that queues in an unwatched pane gives one notification `Worker 2 waits for approval in pane 3` when `[ai] notify` is on, none when it is `off`.
- [ ] A `claude` session in a shell pane appears as a CLI agent row within about 1 s of starting, and disappears when it exits.
- [ ] No row text reaches a model, a log or `list_panes`; `list_panes` output is unchanged.
- [ ] No new timer or `/proc` read beyond the existing 1 s roster probe.

## Slices and Size Forecast (production lines, each PR under about 400; tests and docs may exceed; advisory heuristic)

| Slice | Content | Depends on | Size |
|---|---|---|---|
| S1 | Core rows for chats and workers (who, state, where without branch, how long, usage), ordering; the overlay on the Q1 key; Enter and row keys jump to the pane (tab switch and focus); README and modal-input. No CLI detection, no alerts | Q1, Q4 | 300–390 |
| S2 | Worker transcript jump (select the worker and open `Ctrl+Space W`'s view; ended workers per design), branch and worktree column through `runtime::workspace`, `waiting for approval` per worker from the queue owners | S1 | 180–280 |
| S3 | Alert when an agent starts waiting: the transition, the hold, the fixed bodies, through `[ai] notify` | S1, Q3 | 120–200 |
| S4 | CLI agents in panes: a structured program name from the roster probe, the fixed table with the `node`/`bun`/`deno` rule, rows with `running` and (if Q2 picks it) `quiet <time>` from a per-pane last-output stamp | S1, Q2 | 180–280 |
| S5 (Q2-dependent) | Bell and OSC 9 / OSC 777 attention signals from panes: `audible_bell` and `unhandled_osc` in the emulator callbacks (pane.rs:124-135), a `waiting for you (bell)` state, alerts behind an opt-in | S4, Q2, Q3 | 150–250 |
| S6 (later, Q2 option E only) | Hook bridge: a `LAZAROBOX_PANE` variable, a `lazarobox-shell __agent-event` subcommand that tells the owning instance a pane's agent state over a local socket, guides for each CLI's hooks | S4, Q2 | 300–400 (U) |

S1–S4 total about 780–1,150 lines. S1 alone is the smallest useful slice: it answers "what are my agents doing and where" for everything lazarobox runs itself.

## Open questions

### Q1. Where the overview lives

- **A. Overlay on a prefix key (`Ctrl+Space A`).** A list over the focused pane, like LINKS (`Ctrl+Space u`). `a` is the approval toggle, so the capital follows the `T` and `W` precedent (prefix.rs:261-263); `A` is free at the root. Pros: no layout change, reachable from any pane, smallest code, matches an existing pattern. Cons: not visible unless opened; needs the alert (S3) to tell the user to look.
- **B. Sidebar.** A column at the side of every tab, always visible, toggled by a key. Pros: always in view, like herdr. Cons: takes columns from every pane (resize of every PTY on toggle), layout code changes, the largest slice; on an 80-column terminal it hurts.
- **C. A tab in the AI menu.** A sixth menu tab, `Agents`, digit `6`. Pros: no new root key; the menu is where AI things live. Cons: the menu is a modal config popup with a fixed body height (menu.rs:859-870) and needs the AI section; opening it to look at agents mixes config with live status; the jump would close the menu; `TAB_COUNT` and the fixed digit list change (menu.rs:744; config-menu spec:1626).
- **Recommendation: A**, with an optional, off-by-default status-bar count (`2 waiting`) as a later small addition that gives B's "always in view" for the case that matters.

### Q2. How to detect a CLI agent's state in a pane

- **A. Foreground program only.** Presence (`claude in pane 3`), state `running` while it is in the foreground. Pros: no false alarms, no new reads. Cons: never says "waiting".
- **B. A plus quiet time.** `quiet 2m` after N seconds without output. Pros: cheap, works for every CLI. Cons: a guess; as an alert source it gives false alarms (long tool calls, pagers).
- **C. A plus bell and OSC 9 / OSC 777 signals.** The program asks for attention explicitly. Pros: an explicit signal, no screen reading, cheap (emulator callbacks). Cons: opt-in in each CLI (U), other programs ring bells too, no reason given.
- **D. Screen pattern matching** (look for each CLI's prompt or approval box in the pane's screen). Pros: can tell "waiting for approval" from "idle". Cons: breaks on every CLI update, reads other panes' screen text, the most false alarms over time.
- **E. Hooks reporting state.** Each CLI's hooks run a lazarobox subcommand that tells the instance the pane's state. Pros: exact. Cons: user setup per CLI, a new local channel that a program in a pane can write to (must be guarded like the MCP shim's session token, src/core/ai/tools/wiring.rs:20), the largest slice.
- **Recommendation: A + B for display (S4), C for alerts (S5) behind an opt-in; E later only if users ask; never D by default.**

### Q3. Whether the alert reuses `[ai] notify`

- **A. Reuse `[ai] notify` as is.** The same mode, the same delivery (bell, desktop, sound) and the same "not watched" rule, with a new trigger and body. Pros: no new key, one place to turn notifications on. Cons: users who turned it on for answers now also get waits (a behaviour change of an existing key; say it in the CHANGELOG).
- **B. A separate key** (`[ai] notify_waiting`, same values). Pros: independent control. Cons: a new config key and menu row; `configuration` and `config-menu` change.
- **C. In-app only** (the status-bar count and a mark on the tab). Pros: no desktop noise. Cons: misses the user who is in another window, which is the main case.
- **Recommendation: A for chats and workers** (an approval or a halted run is as worth a notice as a finished answer), **and CLI-agent alerts off unless the user opts in** (they are guesses or third-party signals), the opt-in decided with S5.

### Q4. What the first slice is

- **A. Chats and workers, list and jump (S1 above).** Pros: smallest useful, no guesses, no new IO. Cons: CLI agents wait for S4.
- **B. A plus alerts.** Pros: the "waiting for you" value arrives at once. Cons: about 120–200 more lines, over the 400 heuristic in one PR.
- **C. CLI detection first.** Pros: the herdr-like part. Cons: the least reliable part first, and it still needs the view.
- **Recommendation: A**, then S3 (alerts) before S4 (CLI detection).

### Q5. Agents outside lazarobox

- **A. Out of scope.** Pros: nothing to read outside lazarobox's own panes. Cons: an agent in another terminal is not listed.
- **B. Scan the machine's processes for known CLIs.** Pros: complete list. Cons: reads every process on the machine, no place to jump to, privacy cost.
- **Recommendation: A.**

### Q6. Which ended agents stay listed

- **A. Workers of each chat's current run, ended ones included, until the run is replaced; chats always.** Pros: matches what the chat pane shows. Cons: a long run keeps many done rows.
- **B. Only live and waiting agents; ended ones for a short time (for example 60 s).** Pros: a short list. Cons: the user misses a failure that happened while away (the alert covers part of it).
- **Recommendation: A**, ended rows sorted last.
