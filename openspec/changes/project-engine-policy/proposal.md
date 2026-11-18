# Proposal: Project Engine Policy (a folder can require local engines)

## Intent

Goal #658, "Goal: engine policy per project (local-only or cloud allowed)". The owner's direction (2026-11-10, from an "Enterprise & Corporate Readiness" text the owner received), verbatim:

> "Dynamic Multi-LLM Gateway: Easily switch between local models and commercial APIs (OpenAI, Gemini, Codex) based on the environment's classification (e.g., local sandbox vs. production tracking)."

> "el 2 también podría ser un goal"

And, for every setting of this family (2026-11-10, recorded on #659 and #657):

> "que quede bloqueado puede ser también un goal, no es que ahora esté así, si no que es seleccionable, ese punto tiene que dejarse claro en el README"

#658 asks for "a classification per project or folder, for example `local` or `cloud-allowed`", "engine rules per class: a `local` folder uses only local engines, the same rule as the strict local mode (#657), but per folder", "clear feedback: the chat header shows the folder's class and the engine in use. A refused engine says which rule refused it", and names the rule "the global switch should always win".

This change builds on the open proposal for #657, `openspec/changes/local-only-mode/` (PR #668, not merged), which adds the global `[ai] local_only` switch and one refusal, "refused by local only", in the one availability check every task goes through. This change makes that same refusal apply **per chat folder**:

- the user marks a project as local only in their own `config.toml` (`[[ai.projects]]`, slice P1);
- a repository may also carry a file that marks itself local only (`.lzr/policy.toml`, slice P3), and such a file can **only make things stricter**, never allow a cloud engine;
- a chat whose folder is in a local-only project refuses every cloud engine, for every task, with the text of #668 plus the rule that refused it;
- the chat header and the status bar say when a chat is local only because of its project.

**Selectable, not locked.** Like every setting today, the project rule is the user's choice: it lives in the user's own `config.toml`, and a repository file is a file in the user's own checkout that the user can delete or edit. Nothing here stops a user from changing it. Locking is goal #659 (a system policy file), not this change; this design only keeps it possible (design.md, D9). The README must say this in so many words (owner, 2026-11-10).

## Decisions (owner, 2026-11-10, settled)

- **O1 The global switch always wins** (#658, "To decide when this starts"). A project rule can never use a cloud engine while `[ai] local_only` is on.
- **O2 Selectable by the user, lock is #659.** Same as #668 O1/O2.
- **O3 The README says it plainly.** "Hoy es una elección, no un bloqueo" (README.md:38) stays true; the section says the project rule is a choice and that a locked version is planned (#659).

Everything else below is a proposal. The open product decisions are at the end (Q1-Q8), each with options, tradeoffs and a recommendation.

## Facts this proposal rests on

V = verified in the code at e480992e (the source tree of this branch's base, 4760d0dd, is identical: `git diff --stat e480992e 4760d0dd -- src` is empty). Inferred = read from names, comments or docs, not traced end to end; marked so. #668 = the unmerged local-only proposal; what it adds does not exist in the code yet.

### What "project" means in lazarobox today

There is no single "project" object. Four related notions exist, all derived from the **chat folder**:

- **Chat folder.** The directory a chat uses for its tools, context block, memory project and new panes ("origin cwd" before chat-folder), the resolved absolute path (openspec/specs/chat-folder/spec.md:9, 13). Stored on the chat as `origin_cwd` (src/core/ai/chat/mod.rs:355, accessor 1373-1375; `chat_folder`, src/app/run_resume.rs:61-64). V
- **It changes only by the user.** Set at open from the source pane's cwd or the picker, by `/cd` after a `y`, or by a resume (chat-folder spec "The `/cd` command", "After a folder change", "Inheritance between chats"). Every one of these goes through one probe, `Effect::ProbeFolder` answered by `AppEvent::FolderProbed { pane, purpose, result: Result<Probed, Refusal> }` (src/app.rs:199-205, 522-528), with purposes `Cd`, `Confirm`, `Resume`, `Pick`, open (src/core/ai/chat/folder.rs:27-40); a resume probes the stored folder (src/app/folder.rs:296-304). After a change, "Every other consumer reads the folder when it needs it" (src/app/folder.rs:66-68). V
- **`/cd` is refused while busy**: an approval pending, a command running, the model answering or compacting, writes staged, a note being saved (`folder_busy`, src/core/ai/chat/folder.rs:283-300; src/app/folder.rs:33-43). No orchestrator-run check is in that list, so (inferred) `/cd` is accepted between coordinator turns while workers of a run still run.
- **Git root.** Found by walking up from the chat folder, at most `WALK_LIMIT = 64` levels, without running `git`; a symlinked `.git` is not a marker (src/runtime/workspace.rs:1-3, 17, 95-102, 104-109). `project_tag` is the repository's name (the main repository for a linked worktree), else the folder's name (src/runtime/workspace.rs:62-73). V
- **Project tag in the store.** Conversations are tagged with that name (`project_tag`, src/runtime/chat_store/worker.rs:121, 311) and keep the last chat folder in `conversations.folder` (src/runtime/chat_store.rs:427-437). A resume restores the stored folder; the engine is the current `chat` assignment, not the stored one (openspec/specs/chat-history/spec.md:255). V
- **Memory project.** engram's project comes from engram's own reply for the folder; a folder that is not a git project gets `memory needs a git project` (`ProjectError::NotGit`, src/core/ai/memory/reply.rs:14-21; src/core/ai/chat/memory.rs:275). Inferred: how engram names the project is engram's.
- **Orchestrator folder.** A run is handed `OrchScope { origin: PathBuf, roots, .. }`, "The chat's origin: what a worker's relative path is read against" (src/runtime/orchestra.rs:298-312); its read-scope roots default to the chat folder and "MUST come only from the chat origin (and, later, from the user)" (openspec/specs/agent-orchestrator/spec.md:180). Workers run in that folder; no worker gets a folder of its own. V
- **Shell panes** have an effective cwd (OSC 7 or poll); an unknown cwd makes a chat opened from it show the picker (chat-folder spec:296-303). A shell pane runs no AI engine. V
- **`.lzr/` does not exist.** `grep -rn '\.lzr' src` finds nothing. #431 (open) proposes it for per-folder chat context and memory ("directorio .lzr/ … oculto", "también fuera de git"). V

### Per-folder configuration today

- **One per-project setting exists: MCP servers.** An `[[ai.mcp.servers]]` entry may carry `project`, "the absolute git root the entry is limited to" (src/core/ai/config.rs:381-393); an unreadable `project` skips the entry, because "reading it as global would widen where the server runs" (src/core/ai/config.rs:1228-1237). A chat's servers are the global list plus the entries whose `project` equals the git root of the chat folder, both sides canonicalized so a symlink never fails open (`active_servers`, src/runtime/mcp/active.rs:1-8, 20-61; `resolve`, src/core/ai/tools/servers.rs:21-43; called at src/runtime/chat/external.rs:215). V
- **Repository files are never trusted for that.** mcp-servers: per-project extras "MUST NOT be read from inside any repository or from the working directory: cloning or opening a repository MUST NOT be able to add a server or start a process", with the scenario "Repository file is never read" (`.lazarobox/mcp.toml`) (openspec/specs/mcp-servers/spec.md:11-38). CLI engines run in a private empty folder "so a project file (`CLAUDE.md`, `AGENTS.md`) cannot steer the CLI" (src/runtime/chat/cli.rs:7-9). V
- **lazarobox already reads one repository file for a folder, with no network:** the git `origin` for the GitHub tools (`Effect::ProbeOrigin`, src/app.rs:206-211, 529-531; src/runtime/tools/git.rs:554). Bounded, type-checked reads exist: `read_regular` opens without blocking and refuses a symlink to a device before reading, at most `cap + 1` bytes (src/runtime/bounded_read.rs:23-27). V
- **Everything else is global.** `[ai]` and `[ai.tasks.<task>]` are one set for the machine (src/core/ai/config.rs:1, 62-66); `[workspace] dir` is only the app's start folder (openspec/specs/configuration/spec.md:537). The config file is read once at start, no hot reload (configuration spec:9-10). V
- **`[ai]` is an owned table**: its known keys are listed in `note_unknown_ai_keys` (src/core/ai/config.rs:1522-1550), so a new `ai.projects` key is reported as `unknown key` by every build from 1.11.0 that predates it. A new top-level table would be silent (`OWNED_SECTIONS`, src/core/config.rs:367). V

### Where the engine is decided

- `availability_for_with(task, cfg, detection, jail, gated_off, adapter_exists)` takes **no folder** (src/core/ai/chat/engine.rs:479-486). #668 D2 puts its new check there, reading `cfg.local_only()`. V (signature); #668 (the check).
- The callers that know the pane: `turn_availability(id)` (src/app/orchestra.rs:1184-1195), compaction `start_compaction(id)` (src/app/compact.rs:19-32), `chat_summary_key(id)` (src/app/summary.rs:32-40), the run resume (src/app/run_resume.rs:179). `chat_availability()` takes no pane (src/app/chat.rs:480-482) and is reached from `turn_availability`, `chat_summary_key` and `status_context` (src/app/chat.rs:232-239). The worker roster `roster(config)` takes no pane either (src/app/orchestra.rs:797-806). V
- The chat pane's top row holds the mode badge, the ORCH badge and the batch badge (src/ui/components/chat.rs:34-35, 277-291; chat-pane spec "Per-pane ORCH mode and badge", openspec/specs/chat-pane/spec.md:887). V

## The rule, in one line

**A chat requires local engines when any source says so; no source can loosen another.** The sources, in the order the UI names them:

1. the system policy (#659, later);
2. the user's `[ai] local_only` (#668);
3. a `[[ai.projects]]` entry in the user's `config.toml` whose `root` holds the chat folder (P1);
4. a repository file `.lzr/policy.toml` in the chat folder or one of its parents up to the git root (P3).

A source that says "cloud allowed" (`local_only = false`, or no rule) never overrides one that says local. With nothing set, behaviour is exactly today's.

## What the user sees

| Where | Global switch on (#668) | Only a project rule on (this change) |
|---|---|---|
| Refusal (guidance, worker reason) | `{engine}: cloud engine, not used while local only is on: pick a local engine in the menu (Ctrl+Space m)` | `{engine}: cloud engine, not used in this project (local only by {source}): pick a local engine in the menu (Ctrl+Space m)` |
| Status bar, focused chat | `local only · ollama › llama3:8b` | `local only (project) · ollama › llama3:8b` |
| Chat pane top row | ` LOCAL ` badge (P2) | ` LOCAL ` badge (P2) |
| Shell pane status | `local only · zsh` | nothing added (no AI runs there) |

`{source}` is `ai.projects root /w/acme` for a user entry, or the file path `/w/acme/.lzr/policy.toml` for a repository file (design.md, D5).

## Scope

### In Scope

- `[[ai.projects]]` entries in the user's `config.toml`: `root`, `local_only` (P1).
- The chat folder's rule threaded into the one availability check, for every task of that chat, the ORCH coordinator, its workers and the run resume (P1, P4).
- Status bar text, the ` LOCAL ` chat badge, the source in the refusal text (P1, P2).
- The repository file `.lzr/policy.toml`, tighten-only, fail closed (P3).
- Folder changes, orchestrator runs and resumed conversations (P4).
- The per-chat model calls of #668 S5/S6 (GitHub read tools, MCP servers not marked local) following the chat's rule (P5).
- README "🛡️ Tus datos", CHANGELOG.

### Out of Scope

- Locking (#659); team sync (#570); the class of a remote server in an SSH pane (#654): the chat folder is local to the machine running lazarobox.
- Per-project engine assignments ("in this project, `chat` is claude") and engine allow-lists: Q4, a later change.
- Refusing cloud chats elsewhere from reading files under a local project: Q7, a later slice.
- Anything app-wide that is not per chat (update checks, detection listings): they follow only the global switch (#668 S5).

## Capabilities

### New Capabilities

- `project-engine-policy`: the sources of the project rule, how they combine, what a repository file may do, folder changes, orchestrator runs, resumes, texts.

### Modified Capabilities

- `configuration`: ADDED "Project entries".
- `chat-engines`: MODIFIED "Engine resolution for chat" (as #668 leaves it: the refusal applies when the chat's rule requires local, not only the global switch).
- `statusline`: MODIFIED "Local-only segment" (as #668 adds it: the project form).
- `chat-pane`: ADDED "Local badge".

## Slices and size forecast

Production lines (code, not tests or docs), advisory only. Each slice ships its tests, README and CHANGELOG lines. Every slice needs #668 S1 merged (the `LocalOnly` refusal, its guidance and its position in `availability_for_with`).

| Slice | Content | Prod. lines | Needs from #668 |
|---|---|---|---|
| P1 | `[[ai.projects]]` read (fail closed, unknown keys); pure `project_rule(entries, folder)`; the rule carried with the probed folder; `availability_for_with` takes a `LocalRule` instead of reading `cfg.local_only()`; every per-chat caller passes the chat's rule; refusal text with the source; status bar `local only (project) · `; README | 220-320 | S1 |
| P2 | ` LOCAL ` badge on the chat's top row; task-row mark ` · off in this project` in the menu for the focused chat | 80-140 | S1, S2 (mark) |
| P3 | `.lzr/policy.toml` read in the folder probe: walk to the git root, bounded read, no symlink, fail closed; notice naming the file; refusal names it | 150-240 | S1 |
| P4 | Orchestrator runs keep the rule of their origin, OR the current rule; `/cd` into a local project stops cloud workers of a running run; resume re-reads | 100-170 | S1, S4 (cancel path) |
| P5 | GitHub read tools and MCP servers not marked `local` follow the chat's rule | 60-120 | S5, S6 |
| P6 (optional) | Menu row `This project` that adds or removes the `[[ai.projects]]` entry for the focused chat's git root | 150-250 | S2 |

Total about 760-1240 lines over five or six PRs. **P1 is useful alone:** a user who writes one `[[ai.projects]]` entry by hand gets every cloud engine refused in chats under that folder, with a status bar that says so. P1 is smaller if #668 S1 already passes the switch as a parameter (design.md, D3).

## Risks

- **Older builds ignore it.** `[[ai.projects]]` is reported as `unknown key: ai.projects` by 1.11.0 and later builds that predate P1, and not enforced; a `.lzr/policy.toml` is ignored **in silence** by every build before P3. The README names the first version that enforces each, and a team relying on a repository file must check the version.
- **A deleted or ignored file.** A repository file is a file in the user's checkout: deleting it, or `.gitignore`-ing `.lzr/` (which #431 may suggest for private context), removes the rule. That is the "choice, not lock" of the owner's direction; the README says it.
- **Data of a local project read from elsewhere.** A chat in another folder on a cloud engine can still read files of a local project with approval (Q7). Documented until a slice covers it.
- **Folder known late.** A chat whose folder is still being probed or picked takes no message (`folder_pending`, src/app/picker.rs:30-33; src/app/chat.rs:279, 432), so no turn can start before the rule is known. V.
- **False sense of safety.** As in #668: the rule covers AI engines and lazarobox's own calls for that chat, not the user's shell or approved commands.

## Open questions

### Q1 File name and location (user file and repository file)

- **User side.**
  - (a) `[[ai.projects]]` with `root` and `local_only` in `config.toml`. + One file; reported as unknown by older builds; same key name as #668 and #659 (`local_only`). − Paths in the user's file, like MCP `project`.
  - (b) `[ai.projects."/w/acme"] local_only = true` (keyed table). + Shorter. − Quoted path keys, `~` awkward, no room for a later `engines` list per entry without nesting.
  - (c) Extend `[[ai.mcp.servers]]`-style `project` keys on every setting. − Scatters the rule.
  - **Recommendation: (a).**
- **Repository side.**
  - (a) `.lzr/policy.toml` with `[ai] local_only = true`. + The folder #431 chose ("directorio .lzr/ … oculto"); same table and key as the user file and #659. − #431 may put private, ignored files in `.lzr/`; a team must commit `policy.toml` while ignoring the rest (`.lzr/*` + `!.lzr/policy.toml`).
  - (b) `.lazarobox.toml` at the repository root. + One visible file. − A new name; the mcp-servers spec already uses `.lazarobox/mcp.toml` as the example of a file that is never read, which invites confusion.
  - (c) No repository file at all. + No trust question. − No way for a team to ship the rule with the code, which is the compliance case of the owner's text.
  - **Recommendation: (a)**, coordinated with #431 so its private files live under `.lzr/` with their own ignore rule and `policy.toml` is meant to be committed.
- **Where to look.** (a) the chat folder and each parent up to and including the git root, any `local_only = true` wins; outside a repository, the chat folder only. (b) the chat folder only. (c) up to `$HOME`. **Recommendation: (a)**: a chat opened in `src/` of a marked repository is covered; outside git nothing above the folder is read, so a stray `~/.lzr/policy.toml` or `/tmp/.lzr` cannot mark unrelated folders (it could only tighten, but it would surprise).
- **User entry match.** (a) the chat folder is the `root` or under it, both canonicalized. (b) exact git root equality, as MCP `project`. **Recommendation: (a)**: tightening by prefix is safe, and `~/clients` can cover every client repository.

### Q2 May a project rule loosen the user's setting?

- (a) **Never** (strict wins): any source saying local makes the chat local; `local_only = false` anywhere has no effect beyond "no rule here". + O1; a cloned repository can never send code to the cloud; simple to explain. − A user who wants "local everywhere except ~/oss" cannot say it.
- (b) A **user** entry may loosen the user's own global switch; a repository file never. + Expresses "except ~/oss". − Contradicts O1 ("the global switch should always win"); #659's lock would have to forbid it anyway.
- (c) Repository files may loosen too. − A cloned repository could turn the cloud on: rejected.
- **Recommendation: (a).** (b) can be its own goal later, designed with #659.

### Q3 What "project" is for a pane, the chat and the orchestrator

- **Chat:** the chat folder, read at every folder set (open, `/cd`, resume) together with the folder probe. **Recommended.**
- **Shell pane:** no rule; it runs no engine. Its status bar shows only the global prefix. **Recommended.** (Alternative: show the project prefix for a shell pane whose cwd is in a local project. − Costs a probe per cwd change for a label that changes nothing.)
- **Orchestrator run:** its `OrchScope.origin`, fixed at run start; while it runs, the rule of that origin OR the chat's current rule OR the global switch. A later root added by the user (agent-orchestrator spec:180, "later, from the user") adds its own rule. **Recommended.**
- **Resumed conversation:** the rule of the folder it resumes in, read fresh; nothing about the rule is stored. **Recommended.**

### Q4 Can the file pin allowed engines or models, not only local/cloud?

- (a) **Only `local_only`** now. + Smallest; matches #668's refusal. − No "only ollama" or "only this model".
- (b) An allow-list `engines = ["ollama", "llamacpp"]` (and later `models`), tighten-only: the allowed set is the intersection of every source. + Real "classification" of #658. − New refusal text, picker work; more to explain.
- (c) Per-project **task assignments** ("here `chat` is ollama › qwen3, there claude"), user file only. + The "picks the engine to match" of #658. − Breaks "resolution never picks another engine" unless framed as a project-level assignment; menu work; a repository must never set it (a repository picking `claude` would loosen).
- **Recommendation: (a)** now; (b) as a follow-up change once (a) is used; (c) only from the user's file, as its own change. An unknown key in a repository file fails closed (Q5), so an older build that meets a future `engines` list treats the project as local only instead of ignoring the restriction.

### Q5 Unknown or invalid content

- (a) **Fail closed:** a repository file that cannot be read, is not TOML, is larger than the cap, is a symlink, has `local_only` of the wrong type or has any unknown key makes the project local only, with the startup-style notice `{path}: {problem} (this project is local only until this is fixed)`. A user entry with a bad `local_only` counts as `true` (as #668 Q6); one with a bad `root` is skipped and reported (it cannot be applied anywhere). + A typo never opens the cloud; a future tightening key is never ignored. − A broken file blocks cloud engines in that repository until fixed (visible, named).
- (b) Ignore the bad file with a notice. + Never blocks. − A typo in a compliance file silently allows the cloud.
- **Recommendation: (a).**

### Q6 How the user marks a project

- (a) By hand in `config.toml` (P1), as MCP servers today ("The menu has no row for it in this change: it is edited by hand", configuration spec:440). + Smallest. − Paths typed by hand.
- (b) A menu row `This project: local only / no rule` for the focused chat's git root (P6). + Discoverable. − The menu writes an array-of-tables entry, which it does not do today (it never writes `ai.mcp.*`).
- (c) A user-only chat command `/local on|off`, like `/cd`. + In context. − Another command; must be unreachable by the model.
- **Recommendation: (a) first, (b) as P6.**

### Q7 Does the rule also protect the project's files from chats elsewhere?

- (a) No: the rule follows the chat folder only; documented. + Simple, P1-P5 as listed. − A chat in `~/oss` on claude can read `~/clients/acme/x.rs` with approval.
- (b) Tools refuse, for a cloud engine, any read under a root of a user `[[ai.projects]]` entry (prefix match in memory, no I/O), and the pane-screen and pane-list tools hide panes whose cwd is there. + Closes the main leak. − Touches the path policy of every read tool; repository files cannot be found for arbitrary paths without a walk per call.
- **Recommendation: (a)** in this change, (b) as a follow-up slice for user entries only.

### Q8 The first slice

- (a) **P1 as listed** (user entries, the check, the refusal text, status prefix). About 220-320 lines.
- (b) P3 first (repository file). + The team case. − Needs the trust rules first; nobody can mark a non-git folder.
- (c) P1 + P2 together. − Over the budget.
- **Recommendation: (a).**
