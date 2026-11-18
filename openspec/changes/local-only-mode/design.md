# Design: Local-Only Mode

Code references are on `main` at e480992e (2026-11-10). Decisions marked "recommended" follow the proposal's open questions and change with them.

## D1 The key: `[ai] local_only`

- `[ai] local_only = true | false`, default `false`. Read next to the other `[ai]` keys in `src/core/ai/config.rs` and added to the known list of `note_unknown_ai_keys` (src/core/ai/config.rs:1531-1551), so this build never reports it.
- **Why under `[ai]` and not a top-level table.** `[ai]` is owned since 1.11.0: a build from 1.11.0 up to the last one before this change reports `config: unknown key: ai.local_only` at start, so a user who copied a newer file into an older build is told. A top-level `[privacy]` table is not in `OWNED_SECTIONS` (src/core/config.rs:367-373), so every older build would ignore it in silence. Builds before 1.11.0 ignore both silently; the README names the first version that enforces the mode.
- **A wrong value fails closed (Q6, recommended).** A value that is not a boolean counts as `true` and adds the problem `ai.local_only: expected true or false (local only is on until this is fixed)` through the existing startup notice.
- **One accessor.** `AiConfig::local_only() -> bool` is the only place the rest of the code reads. A second accessor `local_only_source()` returns `User` today; #659 adds `Policy` (D8).
- **Saving.** The menu writes only `[ai] local_only` through the existing draft and edit path (as `ai.notify_sound`, src/core/menu.rs:392-407); every other byte is kept (config-persistence).

## D2 One check, in `availability_for_with`

Every task reaches the engine through `availability_for_with` (src/core/ai/chat/engine.rs:479-560): chat, compaction, `Prefix s`, the ORCH coordinator, the worker roster, the run resume (proposal, Facts). The check goes right after `Unassigned` and before the adapter check, so `opencode` (no adapter) is also reported as refused by local only, not as unsupported:

```text
resolve(task) -> engine
local_only && engine is Kind::Cli                          -> Availability::LocalOnly { engine, model: None }
local_only && engine == "ollama" && model set && forwards_off_machine(engine, model)  [S3, Q1] -> Availability::LocalOnly { engine, model: Some(m) }
```

- The new variant carries the engine and, for an Ollama cloud model, the model, so the guidance can name it.
- It never selects another engine and never edits the assignment (chat-engines, "Never switches").
- `forwards_off_machine` (src/core/ai/tools/github.rs:232-249) moves to a neutral module when S3 uses it outside the GitHub tools (assumption: `src/core/ai/mod.rs` next to `locality`). Its fail-closed rule for a missing model is not used here: the check runs only when a model is set, so an `ollama` task with no model still gets `NoModel`, as today.
- The worker roster is built from `availability_for` (src/app/orchestra.rs:797-806), and a worker's refusal text is `guidance(other)` (src/core/ai/orchestra/run.rs:662). So spawns, retries, relaunches and run-scoped overrides are covered with no change in the orchestrator.

## D3 Visible state

- **Menu row** `Local only`, values `off`, `on`, the last row of the Models tab, after `Hugging Face` (assumption: the tab that holds the engines; the last place changes the fewest existing scenarios). On it, `h`, `l`, Left, Right, Space and Enter toggle the draft, and the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`, as on `Memory`. The draft has no live effect: the mode changes when the menu is saved (D5).
- **Task rows.** A task whose effective engine is refused gets the mark ` · off: local only` after its label, for example `general` `claude › sonnet · off: local only`. Like `· not recommended`, it is a mark; the row stays selectable.
- **Picker.** The cloud CLIs are not listed. When at least one was detected or is assigned, a disabled note row ends the list: `cloud engines hidden: local only is on`. The `none` / `(default · use general)` row stays. In S3 the Ollama model level does the same for cloud models: `Ollama cloud models hidden: local only is on`.
- **Status bar.** While the mode is on, the right-hand segment starts with `local only · ` in every pane, chat or shell (`local only · ollama › llama3:8b`, `local only · zsh`). It is dropped only after the memory and usage segments, and never cut in the middle. The chat pane gets no extra badge in this change (the ORCH badge already uses the top row, src/ui/components/chat.rs:34-35, 316).

## D4 Exact texts (UI copy in English)

| Where | Text |
|---|---|
| Guidance, a cloud CLI (chat, `Prefix s`, ORCH pane, worker reason) | `{engine}: cloud engine, not used while local only is on: pick a local engine in the menu (Ctrl+Space m)` |
| Guidance, an Ollama cloud model (S3) | `ollama: {model} runs in Ollama's cloud, not used while local only is on: pick a local model in the menu (Ctrl+Space m)` |
| Task row mark | ` · off: local only` |
| Picker note rows | `cloud engines hidden: local only is on`; `Ollama cloud models hidden: local only is on` |
| Status bar | `local only · ` before the engine or shell text |
| Wrong value problem | `ai.local_only: expected true or false (local only is on until this is fixed)` |
| Chat turn stopped by the switch (transcript notice) | `local only is on: the {engine} turn was stopped` |
| Worker stopped by the switch (worker reason) | `stopped: local only is on` |
| Updates row value suffix (S5) | ` · checks off: local only` |
| `r` on the Updates row (S5, footer notice) | `update checks are off while local only is on` |
| `/github` (S5) | `GitHub tools are off while local only is on` |
| GitHub row on the Tools tab (S5) | value `off: local only` |
| MCP server not started (S6, transcript notice, once per chat set) | `{name}: MCP server not started: local only is on (set local = true on its entry if it never calls out)` |

No text tells the user how to turn the mode off, so #659 needs no copy change when the value is locked.

## D5 Turning it on with work in flight

The menu save applies the draft at once. When `local_only` goes from off to on:

- every chat pane whose turn, or compaction run, is streaming on an engine that is now refused gets `ChatCancel` (src/app.rs:417) and the transcript notice of D4;
- every running worker of an orchestrator run whose spec is now refused is cancelled (the existing cancel path, src/core/ai/orchestra/run.rs:1152, 1264) with the reason `stopped: local only is on`; queued workers are refused when they would start;
- nothing else changes: assignments, saved conversations and run records are kept as they are.

A hand edit applies at the next start (no watcher, proposal Facts). Turning the mode off restarts nothing.

## D6 Outbound calls: auto and model off, user stays

| Class | Rows of the inventory | Mechanism |
|---|---|---|
| auto | 2 CLI listings | `detect_all` (src/runtime/ai_detect.rs:729-740) gets a flag; with it, `probe` (ai_detect.rs:873-885) skips the listing join and keeps the version probe |
| auto | 5 update check, 6 llama-server newer build | an effective `update_check() = [updates] check && !local_only` read by `update::begin` (src/runtime.rs:408-415), `can_check_update` (src/core/menu.rs:1563-1566) and `should_check_newer` (src/app/install.rs:17) |
| model | 11 GitHub tools | the family is not offered while on (the registry's family filter); `/github` refuses with D4's text; the menu row shows `off: local only` and keeps the saved toggle |
| model | 15-16 MCP servers (Q3 b) | `ExternalSet` start (src/runtime/chat/external.rs:132-150) filters the active list to `local = true` entries; the others get D4's notice |
| user | 7-10, 12, 17 | unchanged; listed in the README |

The saved `[updates] check` and `[ai.tools] github` values are never rewritten: turning the mode off brings them back.

## D7 What "local" means in this mode

An engine is local when its kind is `Server` (answers on `127.0.0.1`, src/core/ai/mod.rs:192-195) and, with Q1 (a), its model is not forwarded off the machine. A forwarded loopback port counts as local (Q2 a); the README says so. If a host setting is ever added, a host that is not loopback is refused while the mode is on.

## D8 How #659 can lock it later

- The policy file holds the same key in the same table (assumption: `[ai] local_only = true` in `/etc/lazarobox/policy.toml`), parsed by the same reader, so one rule decides the value.
- `local_only()` becomes `policy.or(user)`; `local_only_source()` becomes `Policy` when the policy sets it.
- The menu row, with source `Policy`, shows the lock and `set by your administrator` and does not toggle; the save never writes the key then.
- Nothing in D2-D6 reads the source, so the lock adds no change there. The texts of D4 do not mention turning the mode off.
- Later locked candidates named in #659 (the allowed MCP servers, the update check) map to D6's switches.

## D9 Relation to #658 and #570

- #658 adds a per-folder class. The same check of D2 runs with "local required" = `local_only() || folder_class == local`; the global switch always wins, as #658 says.
- #570 (team work in the cloud) is a new outbound call class. Whatever it ships is off while the mode is on, as an auto call.

## D10 Tests

- Pure: key read (default, true, false, wrong value fails closed, unknown-keys walk silent), `availability_for_with` for each task and each cloud CLI, `opencode`, a `general` fallback to `claude`, an Ollama cloud model (S3), the guidance texts, picker rows and marks, the status segment at widths 0-120 (TestBackend + insta).
- App: menu toggle then send on a cloud chat; ORCH pane; worker roster; compaction; `Prefix s`; run resume; work in flight stopped on save (S4).
- Runtime: detection with the flag runs no listing (fake codex and claude record their argv); update check not started; `/github` refused; MCP set filtered (S6).
- Characterization: with `local_only` off, every existing test stays green and the save output is byte-identical.
