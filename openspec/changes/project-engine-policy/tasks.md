# Tasks: Project engine policy

Order follows the proposal's slices. Each slice is one PR under about 400 production lines (tests and docs may exceed; the size is an advisory heuristic). Every RED/GREEN task writes the failing test first through the public interface, then the code. Every slice keeps the rule user-selectable (owner, 2026-11-10) and adds its README and CHANGELOG lines. **Every slice needs #668 (local-only-mode) S1 merged**; other #668 dependencies are named per slice. The open questions (Q1-Q8) are decided before the slice that needs them: Q1 (user side), Q2, Q3, Q5, Q8 before P1; Q1 (repository side) before P3; Q6 before P6; Q4 and Q7 are later changes.

## P1 User entries, the check, the refusal, the status text — 220-320 lines (needs #668 S1)

- [ ] 1.1 RED/GREEN: `[[ai.projects]]` read: `root` absolute or `~`, `local_only` default `false`, wrong type fails closed with `ai.projects[N].local_only: expected true or false (local only is on for this project until this is fixed)`, bad root skips the entry with `ai.projects[N].root: expected an absolute path`, `ai.projects: expected an array of tables` (configuration, Project entries).
- [ ] 1.2 RED/GREEN: `projects` in the `[ai]` known list (src/core/ai/config.rs:1535-1550) and the entry keys walked like MCP entries (src/core/ai/config.rs:1588); unknown entry key reported as `unknown key: ai.projects[N].<key>`; the every-known-key test stays silent.
- [ ] 1.3 RED/GREEN: pure `LocalRule` / `LocalSource` and `local_rule(global, project)`: source order, no input loosens (design D1).
- [ ] 1.4 RED/GREEN: entry match on resolved paths: equal, under, sibling, symlinked root, literal fallback (design D2); done in the folder probe, carried in `Probed` (src/core/ai/chat/folder.rs:16-23) and stored on the chat by `change_folder` (src/app/folder.rs:79-122) (project-engine-policy, Where the chat's rule comes from).
- [ ] 1.5 RED/GREEN: `availability_for_with` takes the rule (design D3); `Availability::LocalOnly` carries `by`; `chat_availability(id)`, `turn_availability`, compaction, `Prefix s`, run resume pass the chat's rule; `roster` the run's rule (chat-engines, Engine resolution for chat).
- [ ] 1.6 RED/GREEN: guidance with the source: `{engine}: cloud engine, not used in this project (local only by {source}): pick a local engine in the menu (Ctrl+Space m)`; the global text wins when both apply.
- [ ] 1.7 RED/GREEN: every task through the app in a chat under a marked root: send (composer kept), ORCH turn, compaction, `Prefix s`, worker roster and its reason, run resume; the same chat moved by `/cd` to an unmarked folder runs again (project-engine-policy, Project rule refuses cloud engines for that chat).
- [ ] 1.8 RED/GREEN: status bar `local only (project) · ` for the focused chat; shell panes get nothing from the project rule; drop order as #668 (statusline, Local-only segment).
- [ ] 1.9 Characterization: no entries, no rule: request bytes, availability results, guidance and statusline snapshots unchanged; menu save byte-identical.
- [ ] 1.10 Docs: README "🛡️ Tus datos" (README.md:36-54): the project rule, how to write an entry, that it is a choice and not a lock (#659 planned), that it never loosens the global switch, first version that enforces it; move #658 out of "Planificado" only when P3 lands (the repository file is part of the goal). CHANGELOG.

## P2 Badge and menu mark — 80-140 lines (needs #668 S1; S2 for the task-row mark)

- [ ] 2.1 RED/GREEN: ` LOCAL ` badge (short `L`) on the chat's top row, left of the ORCH badge, while the chat's rule requires local, from any source; absent otherwise; never displaces the mode badge (chat-pane, Local badge).
- [ ] 2.2 RED/GREEN: TestBackend + insta at 20-120 columns with ORCH, AUTO and the batch badge (src/ui/components/chat.rs:34-35, 277-291).
- [ ] 2.3 RED/GREEN: task-row mark ` · off in this project` for the focused chat when only a project rule refuses the effective engine (`task_label`, src/core/ai/picker.rs:650-656); the global mark of #668 wins.
- [ ] 2.4 Docs: README names the badge.

## P3 Repository file `.lzr/policy.toml` (Q1, Q5) — 150-240 lines (needs #668 S1)

- [ ] 3.1 RED/GREEN: parse `[ai] local_only`: true is a rule, false or missing is none, wrong type, unknown key, not TOML each fail closed with the problems of design D5 (project-engine-policy, A repository file can only tighten).
- [ ] 3.2 RED/GREEN: search from the chat folder up to and including the git root (`git_root`, src/runtime/workspace.rs:95-97), chat folder only outside a repository; nearest local file named.
- [ ] 3.3 RED/GREEN: `read_regular` with a 4 KiB cap (src/runtime/bounded_read.rs:23-27); a symlinked `.lzr` or `policy.toml`, a FIFO, a directory, a file over the cap: fail closed, never block.
- [ ] 3.4 RED/GREEN: the system line `{path}: {problem} (this project is local only until this is fixed)` once per folder set; refusal and status name the file.
- [ ] 3.5 RED/GREEN: a file with `local_only = false` under a user entry or the global switch changes nothing; a cloned repository cannot enable any engine, tool, server or call.
- [ ] 3.6 Coordinate with #431: `.lzr/` private files and the committed `policy.toml` (ignore rule documented).
- [ ] 3.7 Docs: README: the file, that older builds ignore it in silence, that deleting it removes the rule (choice, not lock), that it is read at folder set; remove #658 from "Planificado". CHANGELOG.

## P4 Orchestrator runs, folder changes, resume — 100-170 lines (needs #668 S1, S4)

- [ ] 4.1 RED/GREEN: a run keeps the rule of its origin at start; later spawns, retries and relaunches use that rule OR the current one (design D6).
- [ ] 4.2 RED/GREEN: `/cd` into a local project while cloud workers run: they end with `stopped: this project is local only`; local workers go on; `/cd` to a looser folder changes nothing for the run.
- [ ] 4.3 RED/GREEN: a conversation stored in a marked folder resumed from an unmarked pane is local only; stored rows unchanged; nothing about the rule is stored (project-engine-policy, Resumed conversations).
- [ ] 4.4 Docs: README: what happens to a run when the chat moves.

## P5 Per-chat model calls (needs #668 S5, S6) — 60-120 lines

- [ ] 5.1 RED/GREEN: GitHub read tools not offered and `/github` refuses with `GitHub tools are off in this project (local only by {source})` in a local project.
- [ ] 5.2 RED/GREEN: the chat's MCP set starts only `local = true` servers in a local project, notice with the source (src/runtime/chat/external.rs:215).
- [ ] 5.3 RED/GREEN: update checks and detection listings ignore the project rule.

## P6 (optional) Menu row `This project` (Q6) — 150-250 lines (needs #668 S2)

- [ ] 6.1 Decide Q6 (b); spec the row, its place and its texts before code.
- [ ] 6.2 RED/GREEN: the row toggles an entry for the focused chat's git root; save adds or removes only that `[[ai.projects]]` entry, every other byte kept (config-persistence).
- [ ] 6.3 RED/GREEN: after the save, every open chat's rule is recomputed (design D4).

## Later (other changes)

- Q4 (b) engine allow-lists, Q4 (c) per-project task assignments, Q7 (b) reads of a local project from other chats.
- #659 lock: design D9. #654 SSH panes.
