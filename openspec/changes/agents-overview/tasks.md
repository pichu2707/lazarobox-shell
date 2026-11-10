# Tasks: Agents overview

Nothing here starts before the owner answers the proposal's open questions Q1 (where it lives) and Q4 (first slice); S3 needs Q3, S4 and S5 need Q2. Each slice is one PR under about 400 production lines (tests and docs may exceed; the size is an advisory heuristic). Every RED/GREEN task writes the failing test first through the public interface, then the code. Every slice keeps the `list_panes` text byte-identical and sends nothing about the rows to a model or a log. References are on `main` 2f20e9df.

## S0 Decisions

- [ ] 0.1 Owner answers Q1 (where), Q4 (first slice), Q5 (outside agents), Q6 (ended rows). Rewrite the modal-input delta if Q1 is not the overlay.
- [ ] 0.2 Owner answers Q3 (alerts) before S3 and Q2 (CLI detection) before S4.

## S1 Chats and workers: rows, view, jump (Q1, Q4) — 300–390 lines

- [ ] 1.1 RED/GREEN: the row type holds only the allowed fields; a failed worker shows its class id, never its text (agents-overview, Agent rows hold no content).
- [ ] 1.2 RED/GREEN: pure builder from plain inputs: worker rows (`WorkerRow`, src/core/ai/orchestra/event.rs:52-72), chat phase (`Phase`, src/core/ai/chat/mod.rs:287-297), the chat's own pending approval (`Approvals::pending`, src/core/ai/chat/approval.rs:1115), run halted (`RunState::Halted`, src/core/ai/orchestra/run.rs:94; the app learns it from `OrchestraEvent::Halted`, event.rs:132) (Who, States without the per-worker approval wait).
- [ ] 1.3 RED/GREEN: ordering and the empty text (The overview view).
- [ ] 1.4 RED/GREEN: since-instants kept per row across rebuilds with a fake clock; ended workers use their `elapsed` (How long).
- [ ] 1.5 RED/GREEN: usage label, `(partial)`, `no data` never 0 (Usage; `Usage::label`, src/core/ai/chat/engine.rs:315-326).
- [ ] 1.6 RED/GREEN: app gathers inputs from every pane of every tab (`PaneState::chat_state`; worker entries `Entry::Worker`, mod.rs:995-1010); where = tab, pane, folder with `~` (Where, without branch).
- [ ] 1.7 RED/GREEN: `Ctrl+Space A` root leaf, unhinted, unique (src/core/prefix.rs:230-275, uniqueness test prefix.rs:422); `a` unchanged; refused while the worker transcript is open (modal-input, Agents overview key).
- [ ] 1.8 RED/GREEN: the overlay (TestBackend + insta), after the link list (`src/ui/components/link_list.rs`; app/links.rs:125); keys and swallowed keys (modal-input, Agents overview keys).
- [ ] 1.9 RED/GREEN: jump to another tab and pane (`activate`, `set_focus_in`, src/app.rs:2884-2886, 3093); closed pane notice (Jump).
- [ ] 1.10 RED/GREEN: `list_panes` and every tool text unchanged with rows present (Nothing reaches a model or a log).
- [ ] 1.11 Docs: README section for the overview and its key; key list; CHANGELOG.

## S2 Worker transcript jump, branch, per-worker approval wait — 180–280 lines

- [ ] 2.1 RED/GREEN: `waiting for approval` for a worker whose ask is pending or queued, joined through `ApprovalAsk::worker` (approval.rs:286-297) (States).
- [ ] 2.2 RED/GREEN: Enter on a queued or running worker selects it and opens its transcript through `view_worker` (src/app/run_view.rs:32); an ended worker focuses the chat with `Worker <n> has ended` (Jump).
- [ ] 2.3 [design-confirm] whether the transcript view accepts an ended worker whose trace is kept (`trace`, src/core/ai/chat/run_trace.rs:241-260).
- [ ] 2.4 RED/GREEN: branch, `detached <sha>` and `(worktree)` through `runtime::workspace` (src/runtime/workspace.rs:95, 143-148; `parse_head`, src/core/ai/chat/workspace.rs:72-82), read by an effect off the loop and cached per folder (Where).
- [ ] 2.5 Docs: README.

## S3 Alert when an agent starts waiting (Q3) — 120–200 lines

- [ ] 3.1 RED/GREEN: transition into a waiting state computed from two row sets; one notification per wait; the 30 s hold; `+N more` (chat-notify, A wait asks for one notification).
- [ ] 3.2 RED/GREEN: reuses `should_notify` (src/core/ai/chat/notify.rs:20-30) and the delivery of the mode; fixed bodies next to `body` (notify.rs:140-148); `off` silent; watched pane silent.
- [ ] 3.3 RED/GREEN: characterization: a finished turn notifies exactly as before (src/app.rs:3220-3235).
- [ ] 3.4 Docs: README `[ai] notify` paragraph says waits notify too; CHANGELOG states the behaviour change.
- [ ] 3.5 [manual] Desktop notification on Linux and WSL for a worker ask in another tab.

## S4 CLI agents in panes (Q2) — 180–280 lines

- [ ] 4.1 RED/GREEN: `publish_roster` (src/runtime.rs:2384-2405) also hands the app, per pane, the file name of the foreground program and, for `node`/`bun`/`deno`, of its first argument without extension; no argument kept; the redacted lines unchanged (agents-overview, CLI agent table; list_panes unchanged).
- [ ] 4.2 RED/GREEN: the fixed table `claude`, `codex`, `gemini`, `opencode`; unknown programs give no row; a chat pane is never a CLI row.
- [ ] 4.3 RED/GREEN (only if Q2 keeps quiet time): one last-output instant per pane, stored in `on_pty` (src/app.rs:3574); `quiet` after 30 s, drawn as a guess.
- [ ] 4.4 RED/GREEN: the probe count is unchanged with the view open (Cost of the view).
- [ ] 4.5 [manual] Real `claude`, `codex`, `gemini`, `opencode` sessions on Linux and WSL: record each CLI's foreground argv in the README (U today), and the known misses (`ssh`, `tmux`, `docker exec`, npm wrappers).
- [ ] 4.6 Docs: README limits of CLI detection; macOS shows no CLI rows (`unknown` program, agent-tools spec:671).

## S5 Bell and OSC attention signals (Q2 option C, Q3 opt-in) — 150–250 lines

- [ ] 5.1 RED/GREEN: `audible_bell` and OSC 9 / OSC 777 in the emulator callbacks (src/core/pane.rs:124-135; vt100 0.16.2 `Callbacks`), recorded as a flag and an instant, payload dropped; OSC 7 unchanged.
- [ ] 5.2 RED/GREEN: `waiting for you (bell)` only while a CLI agent is in the foreground; cleared by output or focus (Attention signals from a pane).
- [ ] 5.3 RED/GREEN: CLI alerts only under the opt-in Q3 decides; off by default.
- [ ] 5.4 [manual] Which CLIs emit a bell or an OSC notification when they wait, and with which setting (U today); README.

## S6 Hook bridge (later; only if Q2 picks option E) — 300–400 lines (U)

- [ ] 6.1 Proposal addendum: the channel (a `LAZAROBOX_PANE` variable next to `LAZAROBOX`, src/core/nesting.rs:1-7; a `__agent-event` subcommand; a local socket guarded by a token like the MCP shim, src/core/ai/tools/wiring.rs:20), the messages, and the per-CLI hook guides.
- [ ] 6.2 Spec and tasks written once 6.1 is accepted.
