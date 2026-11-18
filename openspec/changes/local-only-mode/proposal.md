# Proposal: Local-Only Mode (one switch that refuses every cloud AI engine)

## Intent

Goal #657, "Goal: strict local mode (no cloud AI engines)". The owner's direction (2026-11-10, from an "Enterprise & Corporate Readiness" text the owner received), verbatim:

> "Zero-Data-Leak Policy (Local AI): By default, the tool operates with Ollama. All terminal outputs, logs, and SSH context are processed 100% locally. Ideal for companies bound by GDPR, HIPAA, or strict internal compliance."

> "el punto 1, el 3 como goal y el 2 también podría ser un goal"

> "que quede bloqueado puede ser también un goal, no es que ahora esté así, si no que es seleccionable, ese punto tiene que dejarse claro en el README"

Today "processed 100% locally" holds only when the user picks a local engine for every task; nothing enforces it (#657, "Today"). This change adds **one user setting, `[ai] local_only`**. While it is on:

- no cloud AI engine is used for any task, and the work that would use one does not start and says why;
- the cloud engines do not show in the pickers, and a task already assigned to one says so in the menu;
- the outbound calls lazarobox makes on its own, or that a model starts, are off; the ones the user starts by hand stay, and the README says which is which;
- the status bar and the menu show that the mode is on.

**Selectable, not locked.** Like every setting today, it lives in the user's own `config.toml` and the user turns it on and off from the menu. Locking it so a user cannot turn it off is goal #659 (a system policy file), not this change. The README must say this in so many words (owner, 2026-11-10). The design leaves room for #659 (see design.md, D8).

**Related goals.** #658 (engine policy per project: a folder marked `local` uses the same rule; "the global switch should always win") reuses the check this change adds. #570 (team work in the cloud) is the opposite direction: whatever it ships must be off while this mode is on (design.md, D9).

## Decisions (owner, 2026-11-10, settled)

- **O1 One switch, user-selectable.** The mode is a setting the user selects, "like every setting today". It is not locked.
- **O2 Lock is a separate goal (#659).** This change only has to not block it.
- **O3 The README says it plainly.** The "🛡️ Tus datos: qué sale de tu máquina" section (README.md:36-54, merged in #660) already says "**Hoy es una elección, no un bloqueo**" (README.md:38). When the mode ships, the README says that the mode is a choice the user makes in their own file and menu, and that a locked version is planned (#659).

Everything else below is a proposal. The open product decisions are at the end (Q1-Q6), each with a recommendation.

## Facts this proposal rests on

V = verified in the code on `main` at e480992e (2026-11-10). Inferred = read from names, comments or docs, not traced end to end; marked so. U = unverified.

### Engines and locality

- **Engine catalog.** Four loopback servers (`ollama`, `lmstudio`, `llamacpp`, `vllm`) and four CLIs (`claude`, `codex`, `gemini`, `opencode`) (`CATALOG`, src/core/ai/mod.rs:90-105). V
- **Locality comes from the kind.** `Kind::Server` is `Locality::Local`, `Kind::Cli` is `Locality::Cloud` (src/core/ai/mod.rs:53-58; `Locality`, src/core/ai/tools/mod.rs:56-61; test `a_loopback_server_is_local_and_every_cli_is_cloud`, src/core/ai/mod.rs:703). The comment names the known limit: "an Ollama `*-cloud` model goes through the local daemon to ollama.com and still counts as `Local`. Accepted. If a host or URL setting is ever added, `Local` must mean 'loopback AND not forwarding'" (src/core/ai/mod.rs:48-52). V
- **Loopback only.** Every server turn goes to `127.0.0.1` (`http_turn`, src/runtime/chat.rs:128-137); only the port is configurable, "The host is always `127.0.0.1`" (`ServerPorts`, src/core/ai/mod.rs:192-195). There is no host or URL setting. V
- **Ollama cloud models.** `forwards_off_machine` says an Ollama model named `*-cloud` or tagged `cloud` sends tool results off the machine; a missing model counts as cloud (fail closed); other engines are judged by kind, never by name (src/core/ai/tools/github.rs:232-249). Today it is used only by the GitHub tools (src/runtime/chat.rs:985; src/runtime/tools/host/github.rs:128-129) and the orchestrator's shared-engine check (src/core/ai/orchestra/run.rs:392-399). The README says such a model "cuenta como local para `list_panes`" but as cloud for the GitHub tools and the reads after a GitHub read (README.md:1162, 1196, 1212). The Ollama download screens hide a `cloud` model that has no size (`listable`, src/core/ai/ollama_lib.rs:303-308), but an installed `*-cloud` model is listed and usable like any other (inferred from the picker reading Ollama's tag list). V except the last clause.
- **No default engine.** `Assignment` is an engine and a model, both unset by default (src/core/ai/config.rs:284-287). An unassigned task falls back to `general` (`resolve`, src/core/ai/config.rs:762-769). V

### Every task goes through one availability check

- `availability_for_with` resolves a task's effective assignment and returns `Ready` or one refusal (`Unassigned`, `Undetected`, `Unsupported`, `NoJail`, `Refused`, `GatedOff`, `Outdated`, `NoModel`, `ModelMissing`) (src/core/ai/chat/engine.rs:392-435, 479-560). It never picks another engine. V
- Its callers cover every task:
  - chat turns (`chat_availability`, src/app/chat.rs:480-481);
  - the orchestrator coordinator in an ORCH pane (`turn_availability`, src/app/orchestra.rs:1184-1195);
  - compaction, on the `summarize` engine when set, else `chat` (src/app/compact.rs:19-31);
  - the memory summary `Prefix s`, on the `chat` engine (src/app/summary.rs:32-40);
  - the resume of an interrupted orchestrator run (src/app/run_resume.rs:179);
  - the worker roster of every task (src/app/orchestra.rs:797-806), whose refusals become the worker's reason through `guidance` (`resolve`, src/core/ai/orchestra/run.rs:654-663), also on a retry or relaunch (run.rs:552, 1221).

  V
- Each refusal has one line of guidance, for example `{engine}: not available on this system yet: {reason}` (`guidance`, src/core/ai/chat/guidance.rs:79-111). V
- The menu's task rows and the picker build their marks from the same config and detection (`task_label`, src/core/ai/picker.rs:650-656; `engine_rows`, picker.rs:208-279; the detected CLIs are pushed at picker.rs:239-251, assigned but undetected engines at picker.rs:265-277). V

### Configuration

- `[ai]` is an owned table: an unknown key in it is reported at start as `config: unknown key: ai.<key>`, the file still loads and the key is ignored (`note_unknown_ai_keys`, src/core/ai/config.rs:1522-1551; README and CHANGELOG 1.11.0, CHANGELOG.md:291-298). The top-level owned tables are only `build`, `statusline`, `tabbar`, `updates`, `workspace` (`OWNED_SECTIONS`, src/core/config.rs:367-373); any other top-level table is the user's own and never reported. V
- `[updates] check` is read by `read_update_check` (src/core/config.rs:461-476), default `true` (src/core/config.rs:64). V
- The menu applies a saved draft at once; a hand edit of `config.toml` is read at start only (no file watcher found: `grep` for a watcher finds only `ConfigSaved` after a menu save, src/app.rs:2151-2173). Inferred.

### Outbound calls today

See "Inventory of outbound calls" below; each row cites its code.

## Inventory of outbound calls, and what the mode does with each

"Who starts it": **auto** = lazarobox on its own; **model** = a model's tool call; **user** = an explicit user action (a key, a form send, a click). The rule proposed (design.md, D6): **auto and model calls are off; user calls stay**, and the README names each one.

| # | Call | Who starts it | Code | In local-only mode |
|---|---|---|---|---|
| 1 | CLI engine turns (claude, codex, gemini; opencode has no adapter) for chat, compaction, `Prefix s` summary, ORCH coordinator, workers (code, review, summarize, general), resume of a run | user picks the engine | `CATALOG` src/core/ai/mod.rs:90-105; availability src/core/ai/chat/engine.rs:479-560 | **Refused** at availability, with guidance; never another engine (S1) |
| 2 | CLI model listings during detection: `codex debug models` "may refresh the catalog from OpenAI with the user's own codex login", and `claude -p /model` | auto, on every detection when the CLI is installed | src/runtime/ai_detect.rs:569-583, 585-595, 873-885 | **Off** (S5). The local `--version` probe stays (inferred: no network) |
| 3 | Ollama `*-cloud` / `:cloud` models through the local daemon to ollama.com | user picks the model | src/core/ai/tools/github.rs:232-249; src/core/ai/mod.rs:48-52 | **Refused** if Q1 is (a) (S3) |
| 4 | A loopback port that forwards elsewhere (SSH `-L`, a proxy posing as vLLM or LM Studio) | user | host fixed at src/runtime/chat.rs:136, src/core/ai/mod.rs:192-195 | **Counts as local**, documented (Q2) |
| 5 | Update check to crates.io at start (once a day) and `r` on the Updates row | auto / user `r` | `API_URL` src/core/update.rs:18; `update::begin` src/runtime.rs:408-415; `can_check_update` src/core/menu.rs:1563-1566 | **Off**, as if `[updates] check = false` (S5) |
| 6 | Newer llama-server build check to api.github.com | auto, once per run, gated by `[updates] check` | src/app/install.rs:12-22; src/core/ai/install.rs:243-248; src/runtime/hf.rs:37, 97-100 | **Off** (S5) |
| 7 | Update install pane (`cargo install lazarobox-shell --locked`) | user Enter | src/app.rs:1965-1978; src/core/update.rs:22 | Stays |
| 8 | Hugging Face search, downloads and the token (`HF_TOKEN`, `HF_TOKEN_PATH`) to huggingface.co | user | `HUB` src/core/ai/hf.rs:16; `Hub::new` src/runtime/hf.rs:868-880; `token_env` src/runtime/hf.rs:742-757; token page src/core/menu/account.rs:18 | Stays (Q4) |
| 9 | Ollama library pages (ollama.com) and registry (registry.ollama.ai); `ollama pull` through the local daemon | user | `LIBRARY`, `REGISTRY` src/core/ai/ollama_lib.rs:17, 20; `curls` src/runtime/ollama_lib.rs:43-48; pull src/runtime/ollama_lib.rs:87-103, src/runtime/loopback/pull.rs:57-60 | Stays (Q4) |
| 10 | Managed llama-server install: release list and asset from GitHub | user, in the wizard | src/runtime/llama_install/installer.rs:114-116; src/runtime/hf.rs:97-105 | Stays |
| 11 | GitHub read tools `gh_issue_list`, `gh_issue_view`, `gh_pr_view` (argv: repo slug, state, limit, number only) | model | src/runtime/tools/host/github.rs:1-5; `argv` src/core/ai/tools/github.rs:257-298 | **Off**: not offered, `/github` refuses (S5; Q4) |
| 12 | Feedback form: similar-issue search (`gh issue list --search <words>`), `gh issue create`, `gh issue comment`, or the prefilled github.com page | user Ctrl+S | src/runtime/gh.rs:1-17, 57, 208; `GITHUB` src/core/menu/feedback.rs:129 | Stays (the user's own text, shown before the send) |
| 13 | `gh_issue_draft` | model | pure, "nothing reaches GitHub" (src/core/ai/tools/issue_draft.rs:1-3) | Stays (it only fills the form of row 12) |
| 14 | engram memory | auto, when memory is on | loopback `request` (src/runtime/engram/worker.rs:88-100; src/runtime/loopback.rs:1-2) | Stays: loopback. Inferred: what engram does with its own store (any sync of its own) is engram's, outside lazarobox |
| 15 | User MCP servers `[[ai.mcp.servers]]`: processes the user declared, outside the jail, that may call out themselves | model | spawn src/runtime/mcp/client/spawn.rs:108-113; chat set src/runtime/chat/external.rs:132-150; keys src/core/ai/config.rs:1585-1594 | **Not started unless the entry says `local = true`** if Q3 is (b) (S6) |
| 16 | Planned MCP connectors (Clarity, GA4, GSC, Gmail, Docs, Sheets, Apps Script) with values in `secrets.toml` | model | openspec/changes/mcp-connectors/ (proposal, mcp-catalog spec); src/core/ai/mcp_secrets.rs | Off: every one is a cloud service, so its catalog entry is never `local` (Q3) |
| 17 | Browser opener (pane links, guide links, the HF token page, the Feedback page) | user | src/core/opener.rs:8-34; `MenuCommand::Open` src/app.rs:2536-2543; src/runtime.rs:620 | Stays |
| 18 | `run_command`, `run_in_pane`, typed commands: a command can reach the network (`curl`, `ssh`) | model, after the user's `y`; the `run` allow-list has no network verb | src/core/ai/tools/exec.rs:1-10 | Stays; README states the limit |
| 19 | git tools: `status`, `diff`, `log`, `add`, `commit`; no fetch, pull or push | model | verb list pinned by the test at src/core/ai/tools/git.rs:863 | Stays: no network |
| 20 | lazarobox's own MCP shim for CLI engines | engine | openspec/specs/lazarobox-mcp-server/spec.md:5 (unix socket) | Unused: CLIs are refused |

### Work already running or stored

| Situation | Code | In local-only mode |
|---|---|---|
| A task assigned to a cloud engine | src/core/ai/config.rs:762-769 | The assignment is kept as written; the menu row says `off: local only`; the task is refused when used (S1, S2) |
| A chat turn streaming on a cloud engine when the mode is turned on | cancel effect `Effect::ChatCancel` src/app.rs:417 | Stopped, with a transcript notice (S4) |
| An orchestrator run with cloud workers | worker cancel src/core/ai/orchestra/run.rs:1152, 1264; re-resolve run.rs:552, 1221 | Running cloud workers stopped; later spawns, retries and relaunches refused (S4) |
| A compaction run on a cloud engine | src/app/compact.rs:19-31 | Stopped; the truncation marker stands (S4) |
| A saved conversation that used a cloud engine | `messages.engine` column src/runtime/chat_store.rs:42 | Kept on disk as it is; the next message goes to the current, local, `chat` engine |
| An interrupted run to resume | src/app/run_resume.rs:179 | Resumes only on a local `orchestrate` engine |

## Scope

### In Scope

- `[ai] local_only` (boolean, default `false`), read at start and written by the menu; reported as an unknown key by builds 1.11.0 and later that predate it (D1).
- Refusal of every cloud engine for every task, through the one availability check (D2), with the exact texts of D4.
- Ollama cloud models, as Q1 decides.
- The menu row `Local only` (Models tab, last row), the task-row mark, the pickers without cloud engines, and the `local only` status bar segment (D3).
- Turning it on stops cloud work in flight (D5).
- The auto and model outbound calls of the inventory off (rows 2, 5, 6, 11, and 15-16 as Q3 decides) (D6).
- README "🛡️ Tus datos" and "Seguridad / modelo de amenazas" (README.md:1551-1560), CHANGELOG.

### Out of Scope

- Locking the setting (#659), the per-project policy (#658), team sync (#570).
- Blocking what the user types in a shell pane: the terminal is the user's.
- Proving that a loopback port is not forwarded (Q2).
- Changing what the user-started calls send (rows 7-10, 12, 17).

## Capabilities

### New Capabilities

- `local-only-mode`: the switch, which engines count as local, the refusals, what stays and what goes off, work in flight, stored data, the lock hook.

### Modified Capabilities

- `configuration`: ADDED "The local-only key" and, in S6, "The local key of an MCP server" (which amends MCP server lists).
- `chat-engines`: MODIFIED "Engine resolution for chat" (a new result, refused by local only).
- `config-menu`: ADDED "Local only row" and "Updates and GitHub rows under local only"; MODIFIED "Sections and items are data", "Navigation" and "Models tab" (the new last row of the Models tab).
- `statusline`: ADDED "Local-only segment".

## Slices and size forecast

Production lines (code, not tests or docs), advisory only. Each slice ships its tests, README and CHANGELOG lines.

| Slice | Content | Prod. lines |
|---|---|---|
| S1 | Key, fail-closed read, unknown-keys walk; `LocalOnly` refusal for cloud CLIs in `availability_for_with`; guidance text; status bar segment; README section rewritten (choice, not lock) | 170-240 |
| S2 | Menu row `Local only`; task-row mark `off: local only`; cloud CLIs hidden in the picker with a note row | 150-230 |
| S3 | Ollama cloud models refused (Q1), hidden in the model level with a note row | 80-140 |
| S4 | Turning it on stops cloud chat turns, compaction runs and cloud workers, with notices | 120-200 |
| S5 | Auto and model calls off: update checks (start, `r`, llama-server newer build), CLI listings in detection, GitHub tools and `/github` | 140-200 |
| S6 | MCP servers as Q3 decides (`local = true` key, unknown-keys walk, servers not started, notice) | 120-180 |

Total about 780-1190 lines over six PRs. S1 is useful alone: a user who sets the key in the file gets every cloud engine refused and sees `local only` in the status bar.

## Risks

- **Older builds ignore it.** A file written for this version, read by 1.11.0 up to the last build before S1, shows `config: unknown key: ai.local_only` and does NOT enforce the mode; before 1.11.0 it is silent. The README says from which version the mode works. A top-level table (`[privacy]`) would be silent in every older build (src/core/config.rs:367-373), which is why the key goes under `[ai]` (D1).
- **Name-based Ollama rule.** A `*-cloud` model copied to another name (`ollama cp`) would pass the name check (inferred). Q1 names a stronger check to verify.
- **A forwarded loopback port** looks local (Q2). Documented, not detected.
- **A false sense of safety.** The mode covers the AI engines and lazarobox's own calls, not the user's shell, the commands the user approves, or MCP servers marked local. The README says what lazarobox guarantees and what it cannot (#657, "Wording for the README").
- **A refusal nobody expects.** A user with `general = claude` who turns the mode on gets every task refused until they pick a local engine. The guidance and the row mark say so; nothing is changed in their file.

## Open questions

### Q1 Ollama `*-cloud` models

Ollama serves these models from its cloud through the local daemon; lazarobox already recognizes them by name and fails closed on a missing model (src/core/ai/tools/github.rs:232-249).

- **(a) Refused** like a CLI. + Keeps the mode's promise ("no engine outside the machine"); reuses an existing rule. − Name-based: a renamed copy passes (inferred).
- **(b) Allowed, as today.** + No change. − The mode's name would be false for the one engine people think of as local.
- **(c) Ask once per conversation.** + Flexible. − A strict mode that asks is not strict; one more prompt.

**Recommendation: (a).** Also verify (U) whether Ollama's `/api/show` reply marks a cloud model (for example a remote host field); if it does, refuse by that field too, which closes the rename gap. lazarobox already calls `/api/show` for each model (`ShowCache`, src/runtime/ai_detect.rs:729-740).

### Q2 A local engine on another host, or through an SSH tunnel

lazarobox only talks to `127.0.0.1` (src/runtime/chat.rs:136). A port forwarded with `ssh -L 11434:gpu-box:11434`, or a proxy on a loopback port, sends the data wherever it points, and lazarobox cannot see that.

- **(a) Loopback counts as local; document it.** + Honest, no new setting. − The user must know their own forwards.
- **(b) A declaration per server**, for example `[ai.servers.ollama] remote = true`, refused while the mode is on. + Explicit for teams with a GPU box on the LAN. − Relies on the user writing it; one more key.
- **(c) Try to detect forwards.** − Not reliable (a forward looks like a local listener).

**Recommendation: (a)**, with the README line "local means an engine that answers on 127.0.0.1 of this machine; if you forward that port to another machine, the data goes there". If a host setting is ever added, a non-loopback host is refused while the mode is on (src/core/ai/mod.rs:50-52 already says so). #654 (SSH panes) and #658 can revisit (b).

### Q3 User MCP servers

They are processes the user declared, outside the jail (spawn.rs:8-12, per openspec/changes/mcp-connectors/proposal.md), and may call out with what the model sends them. Every call already needs `y`.

- **(a) As is**, documented. + No change. − A Gmail or GA4 server sends the model's arguments to Google while "local only" is on.
- **(b) Not started unless the entry says `local = true`.** + The user states which servers stay on the machine; the catalog connectors are cloud services and never carry it; #659 can later lock the allowed list. − A new key; the claim is the user's, not checked.
- **(c) All off.** + Simplest and strictest. − Kills local servers (a filesystem or SQLite server) that never leave the machine.
- **(d) Ask once per server per chat.** + Flexible. − Another prompt on top of the per-call `y`.

**Recommendation: (b).** An older build reports `ai.mcp.servers[N].local` as an unknown key (src/core/ai/config.rs:1585-1594), so the key is never silent.

### Q4 GitHub, update and download calls

- **Update checks (rows 5, 6):** off, not asked. They run on their own and the user did not start them. **Recommendation: off.**
- **GitHub read tools (row 11):** (a) off; (b) ask per repository and turn, as on cloud engines today (README.md:1196); (c) as is. The requests carry only the repo slug, a state, a limit and a number (src/core/ai/tools/github.rs:257-298), so (b) is a fair alternative. **Recommendation: (a) off**, for one simple rule: no call a model starts leaves the machine.
- **Feedback form (row 12):** the user's own text, shown before Ctrl+S. **Recommendation: stays.**
- **Hugging Face, Ollama library and pull, llama-server install (rows 8-10):** they send search words, model names and, for Hugging Face, the user's token; no conversation, file or terminal text. They are also how a user gets local models, so turning them off would make the mode hard to use. Options: stay, ask once per session, off. **Recommendation: stay**, named in the README; #659 can lock them off for an administrator who wants it.

### Q5 The first slice

- **(a) S1 as listed:** the key (hand-edited), every cloud CLI refused, the status segment, the README. About 170-240 lines.
- **(b) S1 without the status segment**, smaller (about 110-160). − A privacy switch nobody can see is on.
- **(c) S1 and S2 together**, the menu row included. − About 320-470 lines, over the budget.

**Recommendation: (a).** The menu row follows in S2.

### Q6 A wrong value of `local_only`

- **(a) Fail closed:** `local_only = "yes"` or `1` counts as on, with the problem `ai.local_only: expected true or false (local only is on until this is fixed)`. + The user clearly meant on; a privacy switch must not open by a typo. − Unlike the other boolean keys, which fall back to their default.
- **(b) Default (off), with the problem.** + Consistent with `ai.notify`. − A typo silently allows the cloud (only the notice warns).

**Recommendation: (a).**
