# Tasks: Local-only mode

Order follows the proposal's slices. Each slice is one PR under about 400 production lines (tests and docs may exceed; the size is an advisory heuristic). Every RED/GREEN task writes the failing test first through the public interface, then the code. Every slice keeps the mode user-selectable (owner, 2026-11-10) and adds its README and CHANGELOG lines. The open questions of the proposal (Q1-Q6) are decided before the slice that needs them: Q5 and Q6 before S1, Q1 before S3, Q4 before S5, Q3 before S6; Q2 is documentation in S1.

## S1 Key, engine refusal, status segment (Q5, Q6) — 170-240 lines

- [ ] 1.1 RED/GREEN: `[ai] local_only` read: missing is off, `true`/`false`, a wrong value fails closed with `ai.local_only: expected true or false (local only is on until this is fixed)` (configuration, The local-only key).
- [ ] 1.2 RED/GREEN: `local_only` in the known list of `note_unknown_ai_keys` (src/core/ai/config.rs:1531-1551); the every-known-key test stays silent.
- [ ] 1.3 RED/GREEN: one accessor `AiConfig::local_only()` (and `local_only_source()` returning `User`), read nowhere else (local-only-mode, The switch is the user's; design D8).
- [ ] 1.4 RED/GREEN: `Availability::LocalOnly` in `availability_for_with` (src/core/ai/chat/engine.rs:479-560), right after `Unassigned`, for every `Kind::Cli` engine, `opencode` included (chat-engines, Engine resolution for chat).
- [ ] 1.5 RED/GREEN: guidance text (src/core/ai/chat/guidance.rs:79-111): `{engine}: cloud engine, not used while local only is on: pick a local engine in the menu (Ctrl+Space m)`.
- [ ] 1.6 RED/GREEN: every task through the app: chat send (composer kept), ORCH turn, compaction, `Prefix s`, worker roster and its refusal reason, run resume (local-only-mode, Cloud engines are refused for every task).
- [ ] 1.7 RED/GREEN: status bar prefix `local only · ` in chat and shell panes, drop order at widths 0-120 (statusline, Local-only segment).
- [ ] 1.8 Characterization: with the mode off, local-engine requests byte-identical; existing availability, guidance and statusline snapshots unchanged.
- [ ] 1.9 Docs: README "🛡️ Tus datos" (README.md:36-54): the mode is a choice, not a lock (#659 planned); first version that enforces it and what an older build does; what local means (127.0.0.1, forwarded ports, Q2); what stays on. Remove #657 from "Planificado". Row in "Seguridad / modelo de amenazas" (README.md:1551-1560). CHANGELOG.

## S2 Menu row, task-row marks, picker — 150-230 lines

- [ ] 2.1 RED/GREEN: `Local only` row, last on the Models tab; toggle, Esc, save writes only `ai.local_only`; not shown without the AI section (config-menu, Local only row).
- [ ] 2.2 RED/GREEN: MODIFIED Sections and items are data, Navigation, Models tab: descriptor integrity, nine presses wrap, `Local only` `off` in the rendered tab.
- [ ] 2.3 RED/GREEN: task-row mark ` · off: local only` for refused effective engines, following the draft (`task_label`, src/core/ai/picker.rs:650-656).
- [ ] 2.4 RED/GREEN: picker hides cloud CLIs (src/core/ai/picker.rs:239-251, 265-277) and adds the disabled note row `cloud engines hidden: local only is on` (local-only-mode, The pickers show no cloud engine).
- [ ] 2.5 Docs: README menu section names the row; CHANGELOG.

## S3 Ollama cloud models (Q1) — 80-140 lines

- [ ] 3.1 Verify (U) whether Ollama's `/api/show` marks a cloud model; if it does, add that field to the rule and record it in the spec.
- [ ] 3.2 RED/GREEN: move `forwards_off_machine` (src/core/ai/tools/github.rs:232-249) to a neutral module; GitHub tools tests unchanged.
- [ ] 3.3 RED/GREEN: `LocalOnly` for an `ollama` assignment with a set cloud model; `NoModel` stays first for an empty one; never by name on other engines (local-only-mode, Ollama cloud models are refused).
- [ ] 3.4 RED/GREEN: guidance `ollama: {model} runs in Ollama's cloud, not used while local only is on: pick a local model in the menu (Ctrl+Space m)`.
- [ ] 3.5 RED/GREEN: Ollama model level hides cloud models with the note row `Ollama cloud models hidden: local only is on`.
- [ ] 3.6 Docs: README "Tus datos" and the `*-cloud` paragraphs (README.md:1162, 1196, 1212) say the mode refuses them.

## S4 Work in flight — 120-200 lines

- [ ] 4.1 RED/GREEN: saving the mode on cancels streaming chat turns and compaction runs on refused engines (`Effect::ChatCancel`, src/app.rs:417) with `local only is on: the {engine} turn was stopped`; local turns go on (local-only-mode, Turning it on stops cloud work in flight).
- [ ] 4.2 RED/GREEN: running workers on refused engines cancelled with `stopped: local only is on` (src/core/ai/orchestra/run.rs:1152, 1264); queued workers refused at start.
- [ ] 4.3 RED/GREEN: turning it off restarts nothing; stored conversations and run records unchanged (local-only-mode, Stored data stays where it is).
- [ ] 4.4 Docs: README says what happens to work in flight.

## S5 Auto and model calls off (Q4) — 140-200 lines

- [ ] 5.1 RED/GREEN: effective update check `[updates] check && !local_only` at start (src/runtime.rs:408-415), on `r` (src/core/menu.rs:1563-1566) and for the llama-server newer build (src/app/install.rs:17); saved value never rewritten.
- [ ] 5.2 RED/GREEN: Updates row suffix and `r` footer text (config-menu, Updates and GitHub rows under local only).
- [ ] 5.3 RED/GREEN: detection skips the codex and claude listings (src/runtime/ai_detect.rs:873-885) and keeps the version probe; fake CLIs record their argv.
- [ ] 5.4 RED/GREEN: GitHub read tools not offered, `/github` refuses, GitHub row reads `off: local only`, `[ai.tools] github` kept.
- [ ] 5.5 RED/GREEN: user calls unchanged: Hugging Face, Ollama library and pull, llama-server install, Feedback send, opener, update install pane.
- [ ] 5.6 Docs: README "Sin telemetría" line (README.md:41) and the update section (README.md:450) say the mode turns the check off; "Tus datos" lists what stays.

## S6 MCP servers (Q3) — 120-180 lines

- [ ] 6.1 RED/GREEN: `[[ai.mcp.servers]] local` read, default `false`, wrong value `ai.mcp.servers[N].local: expected a boolean` and the entry kept; added to the entry keys of the unknown-keys walk (src/core/ai/config.rs:1585-1594) (configuration, The local key of an MCP server).
- [ ] 6.2 RED/GREEN: in the mode, the chat set (src/runtime/chat/external.rs:132-150) starts only `local = true` servers, and the notice names the others once per set (local-only-mode, MCP servers stay off unless declared local).
- [ ] 6.3 RED/GREEN: with the mode off, `local` changes nothing; the menu save tests that keep `ai.mcp.*` byte for byte pass unchanged.
- [ ] 6.4 Coordinate with openspec/changes/mcp-connectors/: no catalog snippet carries `local = true`.
- [ ] 6.5 Docs: README "Tus propios servidores MCP" (README.md:1266) documents `local` and what it claims.

## Later (other goals)

- #659 lock from a system policy file: design D8. Not specced here.
- #658 per-project policy: reuses the S1 check (design D9). Not specced here.
