# Tasks: chat-folder (picker, `/cd`, start dir)

Inputs: `proposal.md`, `exploration.md` (#3430), amended `specs/` (12 files), amended `design.md` (D1-D12), decisions #3431, #3435, #3436. Amended in this phase (user decision 2026-10-28): spec `chat-folder` (broad = HOME, `/`, every ancestor of HOME, folders the tools refuse as roots, unknown; typed choice follows tool refusal; busy adds "a note is still being saved"), spec `start-dir` (invalid or missing `--dir` starts in the launch cwd with an in-app notice, no exit 1; `install-desktop --dir` writes `Path=` AND `--dir=` in `Exec`), design D3, D9, D11, D12.

Mode: STRICT TDD (`cargo test`). Every implementation task is a RED (failing test) then GREEN (smallest production code) pair, then an optional REFACTOR inside the same commit. One pair = one work-unit commit. Estimates are PRODUCTION lines only (tests, inline `#[cfg(test)]`, snapshots and fixtures are excluded, user rule). Conventional commits, no AI attribution. The `host/*.rs` files stay untouched (rebase point with agent-orchestrator S1c-2).

## Review Workload Forecast

| Field | Value |
|---|---|
| Estimated changed lines (production) | ~1,570 (A1 345, A2 320, A3 290, B1 330, B2 285) |
| 400-line budget risk | **High** as one PR (every slice is Low-Medium: 285-345, under 400) |
| Chained PRs recommended | **Yes** (5 PRs) |
| Suggested split | A1 -> A2 -> B1 -> B2, with A3 independent off `main` |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main (PROVISIONAL, to be confirmed with the user) |
| Hard stop | 600 production lines per PR, soft 400 |

Decision needed before apply: Yes (chain strategy confirmation: stacked-to-main vs feature-branch-chain)
Chained PRs recommended: Yes
Chain strategy: pending
400-line budget risk: High

Re-check against the amended spec: the broad criterion adds the `Outside` verdict to the picker trigger (~+10 in A1) and the in-app `--dir` notice plus the `Exec` slot (~+30 in A3). No slice crosses 400.

### Suggested Work Units (stacked-to-main)

| Unit | Goal | PR | Base and notes |
|---|---|---|---|
| 1 | Validator, probe, `set_folder`, busy, memory epoch, session relink | PR A1 (~345) | `main`. No user-visible change; fully tested |
| 2 | `/cd` command, confirm, chat.db `folder`, resume (no picker) | PR A2 (~320) | `main` after A1 merges (branch off A1 until then, then retarget) |
| 3 | `--dir`, nesting fix, in-app notice, `[workspace] dir`, `install-desktop --dir` | PR A3 (~290) | `main`; independent of A1/A2, can run in parallel |
| 4 | Picker core, candidates, recent folders, git walk, completion | PR B1 (~330) | `main` after A2 merges (branch off A2 until then) |
| 5 | Picker integration, keys, render, `/cd` Tab, README | PR B2 (~285) | `main` after B1 and A3 merge (branch off B1) |

Order: A1 -> A2 -> B1 -> B2 sequential; A3 any time before B2. Each PR keeps `cargo test` green and the app usable (A1 changes nothing visible; A2 ships `/cd`; A3 ships start dir; B ships the picker).

## PR A1: validator, probe, setter, busy, memory (~345, base `main`)

- [x] 1.1 RED: `core/ai/chat/folder.rs` tests for `judge`: Concrete, Home, AboveHome (`/home`), Outside (`/usr`, `/srv/x`), `/` Root, secret, plus typed-flow mapping (`Outside` -> `not allowed`, `/` -> `/ is not a valid chat folder`). (~0 prod)
- [x] 1.2 GREEN: create `folder.rs` with `Verdict`, `Refusal`, `judge`, `is_broad(verdict)`, refusal texts; add `SecretRoots::holds_secret` in `core/ai/tools/paths.rs`. (~70)
- [x] 1.3 RED: `parse_command` (`/cd`, `/cd  x`, `/cdx`, `/etc/passwd`, `//cd` escape) and `expand` (`~`, `~/x`, relative, `..`). (~0)
- [x] 1.4 GREEN: `parse_command`, `expand`, `CdArg`. (~60)
- [x] 1.5 RED: `runtime/folder.rs` probe tests in a tempdir HOME: symlink resolved, missing, file, secret, equivalence with `root_of`/`concrete_root` for dirs under HOME. (~0)
- [x] 1.6 GREEN: `runtime/folder.rs::probe` over `tools::fs::load_roots` + `judge`; `Effect::ProbeFolder{purpose}` / `AppEvent::FolderProbed` variants (not yet emitted). (~70)
- [x] 1.7 RED: `ChatState::folder_busy` per reason in D4 order (approval, run slot, phase != Idle, staged/proposed/orphan, pending note save). (~0)
- [x] 1.8 GREEN: `folder_busy` plus the app-side run-slot check helper. (~40)
- [x] 1.9 RED: `set_folder` invalidations: prompt re-prepared, tag cleared on a fresh store, system line `folder changed: <old> -> <new>`, once-only model notice, `Effect::EndToolSession`, approvals not carried over. (~0)
- [x] 1.10 GREEN: `ChatState::set_folder -> FolderChange`, `invalidate_prompt()` (extracted from `adopt_profile`), `App::apply_folder`. (~60)
- [x] 1.11 RED: memory epoch: `reset_for_folder` clears readiness/injected/context/degraded, stale `Resolve/Context` replies with an old epoch ignored, `MemCmd::End` emitted when the session has notes, session relinked under the new project. (~0)
- [x] 1.12 GREEN: `ChatMemory::reset_for_folder` + `epoch` through `MemCmd` and replies (`runtime/engram/*`), `LinkSession{replace}` upsert in `chat_store`. (~45)

## PR A2: `/cd`, confirm, chat.db folder, resume (~320, base `main` after A1)

- [x] 2.1 RED: `ensure_folder` adds the nullable column on a v2 file, is idempotent, and an older-build open (`open_with(.., 2)`) still works; `Create` stores `folder`; `SetFolder` resolves like `SetTags`; `Resumed` returns `folder`. (~0)
- [x] 2.2 GREEN: `chat_store.rs` + `chat_store/worker.rs`: `ensure_folder` inside `migrate_to`, `Create.folder`, `StoreCmd::SetFolder`, `Resumed.folder`. (~85)
- [x] 2.3 RED: `chat_submit` with `/cd p`: no `ChatSend`, no message appended, composer kept on refusal, `usage: /cd <folder>`, `already in <f>`, `/cdx` and `/etc/x` sent as normal messages, `//cd` sent as `/cd`, each busy reason gives `/cd refused: <reason>`. (~0)
- [x] 2.4 GREEN: hook in `app/chat.rs::chat_submit` and new `app/folder.rs` (`/cd` flow, busy check, `ProbeFolder{Cd}`, probe reply handling, invalid target lines). (~95)
- [x] 2.5 RED: `ApprovalAsk::Folder` keys: only `y` confirms; `n`, Esc, `x`, `Y`, Enter cancel; busy re-checked at `y`; composer cleared on confirm, kept on cancel; `TestBackend` + insta 60x10 shows old and new folder. (~0)
- [x] 2.6 GREEN: `ApprovalAsk::Folder{from,to,note}` in `core/ai/chat/approval.rs`, render in `ui/components/chat/approval.rs`, confirm wiring calling `apply_folder`. (~70)
- [x] 2.7 RED: `apply_folder` emits `StoreCmd::SetFolder`; the next send carries the notice once; the same conversation continues; panes opened from the chat get the new cwd; `/cd` model-callable tool absent. (~0)
- [x] 2.8 GREEN: wire `SetFolder` and pane cwd (`effective_cwd`) in `apply_folder`. (~20)
- [x] 2.9 RED: `on_resumed`: stored concrete folder is set with no ask; `NULL`, vanished, `Outside` or broad (including explicit `$HOME`, twice) gives `no stored folder` / `stored folder not available: <f>` and the folder stays (picker arrives in B2); busy chat keeps its folder. (~0)
- [x] 2.10 GREEN: `ProbeFolder{Resume}` in `app/history.rs::on_resumed`. (~50)

## PR A3: start dir (~290, base `main`, independent)

- [ ] 3.1 RED: `core/start.rs` tests: `--dir X`, `--dir=X`, no value, `~`, relative to cwd, file, missing, precedence flag > config > cwd, invalid `--dir` does not fall to config, notice texts. (~0)
- [ ] 3.2 GREEN: `core/start.rs` (`parse_args`, `resolve -> StartDir{path, notice}`). (~80)
- [ ] 3.3 RED: `nesting::guarded_start` with `--dir /a` and `--dir=/a` still guarded. (~0)
- [ ] 3.4 GREEN: skip the value after `--dir` in `core/nesting.rs`. (~15)
- [ ] 3.5 RED: `[workspace] dir`: absolute, `~`, empty = unset, relative / non-string / non-table notices, missing folder gives `config: workspace.dir: folder not found`; startup notice from invalid `--dir` shown inside the app, no stderr, no exit. (~0)
- [ ] 3.6 GREEN: `core/config.rs` `read_workspace` + `Parsed.workspace_dir`; `main.rs` and `runtime.rs::run(start)` feed the start dir as launch cwd (no `chdir`) and surface the notice. (~95)
- [ ] 3.7 RED: `core/desktop.rs` `plan_install(choice, bin, dir)`: `Path=` and `--dir=<dir>` in `Exec` with a dir, neither without; escaping (`\\`, leading `\s`, control chars refused, spaces kept); `install-desktop --dir /nope` fails with one stderr line and writes nothing; reinstall without `--dir` removes both. (~0)
- [ ] 3.8 GREEN: `Options::Install{terminal, dir}`, `@PATH@` and `@DIR_ARG@` in `desktop/lazarobox.desktop`, `runtime/desktop.rs` validation. (~100)

## PR B1: picker core (~330, base `main` after A2)

- [x] 4.1 RED: `merge` order last-used, pane git roots, recent, last shell; dedupe by real path; keeps `Concrete` only (drops HOME, `/`, `/home`, `/usr`, deleted, secret); cap 12; row 0 pre-selected. (~0)
- [x] 4.2 GREEN: `core/ai/chat/folder_picker.rs`: `merge`, row type. (~70)
- [x] 4.3 RED: picker transitions: Up/Down, field edit, Ctrl+U, paste to field, Enter field wins over row, empty field + empty rows does nothing, Esc cancel, refused path keeps text and reason, stale completion dropped. (~0)
- [x] 4.4 GREEN: `FolderPicker` state machine and `on_key -> PickerOutcome`. (~110)
- [x] 4.5 RED: `StoreCmd::RecentFolders{limit}` returns folders newest first, groups duplicates, limit 12, store failure counts as empty. (~0)
- [x] 4.6 GREEN: `RecentFolders` SQL in `chat_store/worker.rs`. (~40)
- [x] 4.7 RED: runtime `.git` walk (dir and worktree file, stops at HOME and `/`), `ProbeFolders{paths, walk_git}` judges each path, completion lists directories only (hidden only with `.` prefix, cap 50). (~0)
- [x] 4.8 GREEN: `runtime/folder.rs` `probe_many`, `walk_git`, `complete`; matching `Effect`/`AppEvent` variants. (~110)

## PR B2: picker integration, keys, render, docs (~285, base `main` after B1 and A3)

- [x] 5.1 RED: open flow: broad (HOME, `/`, `/home`, `/usr`, unknown) opens the picker with detection still running; `begin_memory`, offer, `PreparePrompt`, `Store List` are deferred; concrete opens as before; inherited broad folder shows the picker; no silent choice on tick, draw or detection. (~0)
- [x] 5.2 GREEN: `open_chat_pane` defers steps, `ProbeFolder{Open}`, picker held in `ChatState`, keys routed in `on_chat_key` after the approval check, candidate gathering (tab shell cwds, `last_shell`, `RecentFolders`, `ProbeFolders`). (~120)
- [x] 5.3 RED: accept: Enter probes again, `set_folder`, list follows the tag of the chosen folder; Esc keeps the folder and adds `chat folder is <f>; use /cd <folder> to change it`; `~` typed gives HOME; typed `/usr` and `/` refused; resume opens the picker over the conversation, Esc keeps the pane folder. (~0)
- [x] 5.4 GREEN: accept/cancel paths in `app/folder.rs`, resume picker hook in `app/history.rs`. (~60)
- [x] 5.5 RED: render snapshots 60x10 (rows, empty with hint, error, gathering) and `/cd` Tab completion. (~0)
- [x] 5.6 GREEN: `ui/components/chat/folder.rs` render and `/cd` Tab completion via `complete`. (~85)
- [x] 5.7 Docs (picker and `/cd` part; `--dir`, `[workspace] dir` and `install-desktop --dir` stay with A3): Spanish `README.md`: replace the "cd first" workaround (~l.331-336) with the picker and `/cd` (confirmation, busy refusals, `//cd` escape); document `--dir`, `[workspace] dir`, precedence, `install-desktop --dir` (`Path=` and `Exec`), the in-app notice for a bad folder, and the SQLite note that folder paths are stored. (~20 lines, docs)

## Closing tasks

- [ ] 6.1 Verify (`sdd-verify`): run every `[auto]` scenario; manual checks: rofi launch with one Enter, `Path=` in kitty/gnome-terminal/wezterm, macOS without OSC 7, bad `--dir` notice from rofi.
- [ ] 6.2 Archive (`sdd-archive`) only AFTER `coding-tools` is archived: these specs modify unarchived coding-tools capabilities (agent-tools, command-runner, code-search, git-tools).

## Dependency summary

- A1 is the foundation (judge, probe, `set_folder`, epoch). A2 needs A1. B1 needs A1 (probe) and A2 (`folder` column for `RecentFolders`). B2 needs B1, and A3 for the "home start dir asks" scenario.
- A3 only needs `main`; it can be applied in parallel with any other slice by a different worker. Single writer rule: never two writers on the same checkout.
- `cargo test` stays green at every PR boundary; A1 is invisible to users.

## Risks

- Rebase against agent-orchestrator S1c-2: `runtime/chat.rs`, `app/chat.rs` and `app/history.rs` are hot files; `host/*.rs` stays untouched. Keep hook edits small.
- Memory epoch touches the engram worker (A1); a missed echo makes stale replies leak. The 1.11 test must cover `Resolve` and `Context`.
- The tools' refusal criterion is the source of truth for `Outside`: if `load_roots` changes in coding-tools, the equivalence test in 1.5 must be re-run.
- `Path=` behaviour differs per terminal (manual verify); `--dir=` in `Exec` is the safety net.
- Estimates are production lines; A1 (345) and B1 (330) are closest to the 400 soft limit and hold the most new logic.
- Open for the user: typed ancestors of HOME (for example `/home`) are assumed explicit-only like HOME; confirm or refuse.
