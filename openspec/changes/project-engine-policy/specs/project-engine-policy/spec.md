# Project Engine Policy Specification

## Purpose

Let the user say that a project requires local AI engines, so that a chat working in that project uses no cloud engine for any task, while chats in other folders keep their task assignments. The rule comes from the user's own `config.toml` and, tighten-only, from a file in the repository. It is selectable by the user, like every setting; locking is a separate goal (#659). Part of goal #658; builds on local-only-mode (#657, change `local-only-mode`), whose refusal, guidance and terms it reuses. Slice tags: [P1] user entries and the check, [P2] badge and menu mark, [P3] repository file, [P4] orchestrator runs, folder changes and resume, [P5] per-chat model calls. Scenario tags: [auto] = `cargo test` (pure config, temporary directories, fake detection, fake CLIs, `TestBackend`); [manual] = a real machine. Items that depend on an open question of the proposal name it (Q1-Q8) and follow its recommendation.

Terms: "chat folder" as in chat-folder. "Cloud engine", "local engine", "task" as in local-only-mode. "Project entry" = one `[[ai.projects]]` table of the user's `config.toml` (see configuration, Project entries). "Repository file" = `.lzr/policy.toml`. "Covers" = a project entry covers a chat folder when the folder is its `root` or lies under it, comparing resolved paths. "The chat's rule" = local required, or not, for one chat, with the source that requires it.

## Requirements

### Requirement: Sources and how they combine [P1, P3]
The chat's rule MUST require local engines when any of these says so, and MUST NOT require them otherwise: the user's `[ai] local_only` (local-only-mode); a project entry with `local_only = true` that covers the chat folder; from P3, a repository file with `[ai] local_only = true` found for the chat folder (see A repository file can only tighten). No source MUST be able to make the rule less strict than another source makes it: `local_only = false`, in any source, means only "no rule from this source" (Q2, recommended a). When several sources require local, the one named to the user MUST be the first in this order: the user's `[ai] local_only`, a project entry, a repository file (a system policy, #659, will come first). With no source requiring local, every behaviour MUST be exactly the same as without this capability.

#### Scenario: Global wins over a project saying cloud [auto]
- GIVEN `[ai] local_only = true` and a project entry for `/w/oss` with `local_only = false`
- WHEN a chat in `/w/oss` sends on `claude`
- THEN it is refused with the global text of local-only-mode

#### Scenario: Project entry alone [auto]
- GIVEN `[ai] local_only` unset and a project entry for `/w/acme` with `local_only = true`
- WHEN a chat in `/w/acme/src` sends on `claude`, and a chat in `/w/oss` sends on `claude`
- THEN the first is refused naming `ai.projects root /w/acme`, the second runs on claude

#### Scenario: Nothing set [auto]
- GIVEN no `[[ai.projects]]` and no repository file
- WHEN every task runs on each engine
- THEN availability, guidance and requests are byte-identical to the ones without this capability

### Requirement: Project rule refuses cloud engines for that chat [P1]
While the chat's rule requires local, every task of that chat MUST get the result "refused by local only" of local-only-mode for a cloud engine, in the same place in the order of results: chat turns, the ORCH coordinator turn, compaction, the memory summary (`Prefix s`), every worker spawn, retry, relaunch and run-scoped override of a run started from that chat, and the resume of an interrupted run in that chat. When the source is a project entry or a repository file the guidance MUST read `{engine}: cloud engine, not used in this project (local only by {source}): pick a local engine in the menu (Ctrl+Space m)`, where `{source}` is `ai.projects root {root}` (the root as written) or the absolute path of the repository file. Another chat whose rule does not require local MUST NOT be affected. The refusal MUST NOT select another engine and MUST NOT change any assignment.

#### Scenario: Chat on a cloud CLI in a marked project [auto]
- GIVEN a project entry `root = "~/clients/acme"`, `local_only = true`, `HOME=/h`, and `chat` `claude` / `sonnet` available
- WHEN a chat whose folder is `/h/clients/acme` sends a message
- THEN no claude process starts, the message stays in the composer and the guidance reads `claude: cloud engine, not used in this project (local only by ai.projects root ~/clients/acme): pick a local engine in the menu (Ctrl+Space m)`

#### Scenario: Two chats, two folders [auto]
- GIVEN the same entry and two chat panes, one in `/h/clients/acme`, one in `/h/oss`, both with `chat` `codex`
- WHEN both send
- THEN the first is refused, the second runs on codex

#### Scenario: Workers of a marked chat [auto]
- GIVEN a chat in a marked project, `orchestrate` `ollama` / `qwen3:14b`, `code` `codex`, `review` `ollama` / `qwen3:14b`
- WHEN the coordinator spawns a `code` and a `review` worker
- THEN the `code` worker is refused with the project guidance as its reason and the `review` worker runs

### Requirement: Where the chat's rule comes from [P1, P3]
The chat's rule MUST be computed when the chat's folder is set (open, `/cd` confirmed, picker, resume), from the resolved folder, together with the folder check of chat-folder, and MUST be kept with the chat until its folder changes again. A chat with no folder yet MUST have only the global rule (it takes no message until its folder is known). A project entry's `root` and the chat folder MUST both be compared as resolved paths; a root that cannot be resolved MUST also be compared by its literal text, so a symlink never makes a covered folder uncovered. A change of `[[ai.projects]]` applies at the next start, or at once to every open chat after a menu save that writes it (P6). A repository file edited while the chat is open applies at the chat's next folder set or the next start. When a folder change makes the chat's rule require local because of a project source, the pane MUST add, after the `folder changed: <old> -> <new>` line of chat-folder, the system line `local only in this project ({source})` (not stored, not sent).

#### Scenario: Subfolder covered [auto]
- GIVEN an entry for `/w/acme`
- WHEN chats open in `/w/acme`, `/w/acme/src/deep` and `/w/acme-old`
- THEN the first two are covered and `/w/acme-old` is not

#### Scenario: Symlinked root [auto]
- GIVEN `/w/link` pointing to `/w/acme` and an entry with `root = "/w/link"`
- WHEN a chat opens in `/w/acme/src`
- THEN it is covered

#### Scenario: Folder change [auto]
- GIVEN a chat in `/w/oss` on `claude` and an entry for `/w/acme`
- WHEN `/cd /w/acme` is confirmed and a message is sent, then `/cd /w/oss` is confirmed and a message is sent
- THEN after the first change the transcript has `local only in this project (ai.projects root /w/acme)`, the first send is refused naming the entry, and the second runs on claude

### Requirement: Menu task-row mark [P2]
While the menu is open over a focused chat whose rule requires local because of a project source only, a task row whose effective engine that chat would refuse MUST end with the mark ` · off in this project`, like the other marks a row may carry; the row stays selectable and the assignment is unchanged. When the global switch is on, the mark of local-only-mode (` · off: local only`) is shown instead. The pickers are not filtered by a project rule: the menu edits the global assignments, used by chats in every folder.

#### Scenario: Mark for a marked chat [auto]
- GIVEN local-only mode off, a focused chat in a marked project and `general` `claude` / `sonnet`
- WHEN the menu opens on the Models tab
- THEN the `general` row ends with ` · off in this project` and the picker of that row still lists claude

### Requirement: A repository file can only tighten [P3]
The repository file is `.lzr/policy.toml` (Q1, recommended a). It MUST be looked for in the chat folder and in each parent up to and including the git root of the chat folder (the walk of the workspace block); outside a git repository only in the chat folder itself. Its only key is `local_only` in the table `[ai]`; `true` makes the chat's rule require local, `false` or a missing key adds no rule. The file MUST NOT be able to enable an engine, a tool, an MCP server, a network call or a process, and MUST NOT loosen any other source. Its content MUST NOT be sent to any engine or written to a log. It MUST be read with a cap of 4 KiB, without blocking, and a symlinked `.lzr` directory or `policy.toml` MUST NOT be followed. A file that cannot be read, is not a regular file, is larger than the cap, is not valid TOML, has `local_only` of another type than boolean, or has any other key MUST make the chat's rule require local (fail closed, Q5) and MUST add, once per folder set, the system line `{path}: {problem} (this project is local only until this is fixed)`, with `{problem}` one of `cannot be read`, `not a regular file`, `larger than 4 KiB`, `not valid TOML`, `ai.local_only: expected true or false`, `unknown key: {key}`.

#### Scenario: File in the repository root [auto]
- GIVEN a git repository `/w/acme` holding `.lzr/policy.toml` with `[ai]\nlocal_only = true`
- WHEN a chat in `/w/acme/src` sends on `gemini`
- THEN it is refused naming `/w/acme/.lzr/policy.toml`

#### Scenario: Not above the git root [auto]
- GIVEN `/w/.lzr/policy.toml` with `local_only = true` and a git repository `/w/acme` with no file
- WHEN a chat in `/w/acme` sends on `claude`
- THEN it runs on claude

#### Scenario: Outside git, the folder only [auto]
- GIVEN `/h/.lzr/policy.toml` with `local_only = true` and a non-git folder `/h/notes`
- WHEN a chat in `/h/notes` sends on `claude`
- THEN it runs on claude

#### Scenario: A repository cannot loosen [auto]
- GIVEN an entry for `/w/acme` with `local_only = true` and `/w/acme/.lzr/policy.toml` with `local_only = false`
- WHEN a chat in `/w/acme` sends on `claude`
- THEN it is refused naming the entry

#### Scenario: Unknown key fails closed [auto]
- GIVEN `/w/acme/.lzr/policy.toml` with `[ai]\nengines = ["ollama"]`
- WHEN a chat opens in `/w/acme` and sends on `claude`
- THEN the system line `/w/acme/.lzr/policy.toml: unknown key: ai.engines (this project is local only until this is fixed)` is shown once and the send is refused naming that file

#### Scenario: Symlink and FIFO [auto]
- GIVEN in one run `.lzr/policy.toml` a symlink to a valid file, in another a FIFO
- WHEN a chat opens there
- THEN nothing blocks, the line names `not a regular file` and the chat is local only

### Requirement: Orchestrator runs [P4]
A run started from a chat MUST keep the chat's rule at the run start, computed for its origin folder. Every later spawn, retry and relaunch of that run MUST use a rule that requires local when the rule at start, the global switch now, or the chat's current rule requires it: a run never becomes less strict while it lives. When the chat's folder changes during a run to a folder whose rule requires local and the run's rule did not, every running worker on an engine that is now refused MUST be cancelled with the reason `stopped: this project is local only`, through the cancel path of local-only-mode; workers on local engines go on.

#### Scenario: Move into a marked project mid-run [auto]
- GIVEN a run in `/w/oss` with a running `codex` worker and a running `ollama` worker, and an entry for `/w/acme`
- WHEN `/cd /w/acme` is confirmed between coordinator turns
- THEN the codex worker ends with `stopped: this project is local only` and the ollama worker goes on

#### Scenario: Move out does not loosen [auto]
- GIVEN a run started in a marked `/w/acme`
- WHEN the chat moves to `/w/oss` and the coordinator spawns a `code` worker on `codex`
- THEN the worker is refused with the project guidance naming the entry for `/w/acme`

### Requirement: Resumed conversations [P4]
Nothing about the rule MUST be stored with a conversation, a run or a usage row. A resumed conversation MUST get the rule of the folder it resumes in, read fresh; its stored messages, whichever engine answered them, are kept unchanged.

#### Scenario: Resume from another pane [auto]
- GIVEN a conversation stored in `/w/acme` (marked) answered by `claude`, resumed from a pane in `/w/oss`
- WHEN a message is sent
- THEN the chat folder is `/w/acme`, the send on `claude` is refused naming the entry, and the stored rows are unchanged

### Requirement: Per-chat model calls follow the chat's rule [P5]
The GitHub read tools and the user MCP servers that local-only-mode turns off while the mode is on MUST also be off for a chat whose rule requires local because of a project source: the GitHub family is not offered and `/github` answers `GitHub tools are off in this project (local only by {source})`; only MCP servers with `local = true` start, and the notice of local-only-mode ends with `(local only by {source})`. App-wide calls (update checks, CLI model listings) MUST follow only the global switch.

#### Scenario: GitHub in a marked project [auto]
- GIVEN `[ai.tools] github = true`, a chat on `ollama` in a marked project and another in an unmarked folder
- WHEN each builds its tool list
- THEN only the second offers `gh_` tools

### Requirement: Shell panes [P1]
A shell pane MUST NOT get a project rule: it runs no AI engine. Its status bar shows the global prefix of local-only-mode only.

#### Scenario: Shell in a marked folder [auto, TestBackend + insta]
- GIVEN a shell pane whose cwd is under a marked root and `[ai] local_only` off
- WHEN it is rendered focused
- THEN the statusline is the one rendered without this capability

### Requirement: Selectable, stated in the README [P1, P3]
The rule MUST be changed only by the user: by editing `[[ai.projects]]` (or, from P6, the menu) and by adding, editing or deleting the repository file. lazarobox MUST NOT create, edit or delete either by itself. No refusal text MUST tell the user how to remove a rule. The README section "🛡️ Tus datos: qué sale de tu máquina" MUST say, in Spanish: that the project rule is a choice the user makes, not a lock, and that a locked version is planned (#659); that it never allows a cloud engine the global switch refuses; how to write an entry; from P3, that a repository file can only make a project stricter, that deleting it removes the rule, and that builds before P3 ignore it in silence; from which version each is enforced; that the rule follows the chat folder, not files read from other chats (Q7).

#### Scenario: README states it [manual]
- GIVEN the README after P3
- WHEN the section is read
- THEN it holds each item above and the "Planificado" list no longer lists #658
