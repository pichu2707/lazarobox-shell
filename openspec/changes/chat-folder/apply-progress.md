# Apply progress: chat-folder

## Batch 1: PR A1 (tasks 1.1-1.12), branch `feat/chat-folder-a1`, base `origin/main` (v0.30.0)

Mode: STRICT TDD (`cargo test`). Delivery: ask-on-risk, stacked-to-main (provisional).

| Task | Status | Commit |
|---|---|---|
| 1.1-1.2 `judge`, `Verdict`, `Refusal`, `is_broad`, `typed`, `SecretRoots::holds_secret` | done | `feat(chat): judge which folders a chat may work in` |
| 1.3-1.4 `parse_command`, `expand`, `CdArg`, `Command` | done | `feat(chat): parse the /cd command and expand typed folders` |
| 1.5-1.6 `runtime/folder.rs::probe`, `Effect::ProbeFolder`, `AppEvent::FolderProbed` | done | `feat(chat): probe a folder with the tool root policy` |
| 1.7-1.8 `ChatState::folder_busy(running)`, `App::folder_busy` | done | `feat(chat): say why the chat folder cannot change yet` |
| 1.9-1.10 `ChatState::set_folder`, `invalidate_prompt`, `App::apply_folder` | done | `feat(chat): change the chat folder and drop what the old one pinned` |
| 1.11-1.12 memory epoch, `reset_memory_for_folder`, `LinkSession{replace}` | done | `feat(chat): make memory follow the chat folder with an epoch and a relinked session` |

Verification: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (7081 passed, 0 failed, 1 ignored).

Production size: about 350 lines of code (372 added non-comment, non-blank lines including a few inline test hunks), under the 400 soft limit.

## Deviations and notes

- `folder_busy` takes a `running: bool` argument (the run slot is app state); `App::folder_busy(id)` computes it. The order of reasons is kept in one place.
- `typed(result)` is the typed-flow mapping (`Outside` -> `not allowed`); `judge` itself returns the raw verdict.
- The equivalence test in `runtime/folder/tests.rs` repeats the expressions of `root_of` and `concrete_root` instead of calling them: both are private to `host/*.rs`, which stays untouched.
- `parse_command` returns `Command::{Plain, Escaped, Cd}`; a multi-line text with `/cd` as the first token is a command and the argument is the first line after it.
- `normalize` in `core/ai/tools/paths.rs` is now `pub` (reused by `expand`); `ChatState::take_session_link` returns `(SessionRef, replace)`.
- The epoch travels through `MemCmd::{Resolve, Context}` and `MemReply::{Project, Context}`. A generic `MemReply::Unavailable` (no epoch) can still reach a new view; it was left alone because it only degrades the badge and a re-resolve fixes it.
- `Effect::ProbeFolder` now runs off the event loop (`runtime::folder::Prober`, `spawn_blocking` bounded by `TIMEOUT` = 3 s, answers `folder not found` on timeout) and answers through a dedicated `folder` inbox channel.
- `App::folder_busy` and `App::apply_folder` carry `#[allow(dead_code)]` until the `/cd` flow (PR A2) calls them. `Effect::ProbeFolder` is executed by the runtime but nothing emits it yet; the app ignores `AppEvent::FolderProbed` for now.
- `apply_folder` also ends the old engram session with `MemCmd::End{cwd: None}` when the session has notes, resets memory, and calls `begin_memory` so the new project is resolved. `StoreCmd::SetFolder` and pane cwd belong to PR A2.
- The memory-epoch plumbing and the `LinkSession` upsert were written with their tests in one pass (the tests for `reset_memory_for_folder`, the worker `replace` upsert and the app flows were written before the commit, but the plumbing compiled before they ran).

## Review fixes (batch 1b, on top of the A1 commits)

- W1: `MemReply::Unavailable` gained `epoch: Option<u32>` (set for failures of `Resolve`/`Context`); an older epoch is ignored.
- W2: the answers of saves sent before a folder change (`Saved`, `SaveFailed`, `SaveUnavailable`) are ignored via a per-chat `save_floor` (the save token is monotonic); `reset_memory_for_folder` also drops the pending saves.
- W3: probe off the loop with a 3 s `TIMEOUT` (see above).
- W4: `link_session` uses `ON CONFLICT(conversation_id) DO UPDATE` instead of `INSERT OR REPLACE`, so another conversation's row with the same session id is never deleted (the link fails instead).
- S1: `folder_busy` adds `a note or summary is open` and `a message is waiting to be sent` (parked send in `waiting_sends`); design D4 updated.
- S2: notes whose link row still waits for the conversation id survive `reset_memory_for_folder` (they link under their own project). Limitation: such a note's title can appear in the `Notes:` summary of the next session end.
- S3: the probe refuses a relative path and names `/` as `Root` before reading HOME.
- S4: the epoch uses `wrapping_add`.

## Batch 2: PR A2 (tasks 2.1-2.10), branch `feat/chat-folder-a2`, base `feat/chat-folder-a1`

| Task | Status | Commit |
|---|---|---|
| 2.1 `ensure_folder` in `migrate_to` (still schema v2) | done | `feat(chat): add a nullable folder column to chat.db without a schema bump` |
| 2.1-2.2 `Create.folder`, `StoreCmd::SetFolder`, `Resumed.folder` | done | `feat(chat): keep the chat folder with the conversation and read it back on resume` |
| 2.3-2.4 `/cd` hook, busy, usage, probe reply lines | done | `feat(chat): read /cd in the composer and probe the folder it names` |
| 2.5-2.6 `ApprovalAsk::Folder`, keys, prompt, confirm | done | `feat(chat): confirm a /cd with y and change the chat folder` |
| 2.7-2.8 `SetFolder` from `apply_folder`, notice once, pane cwd | done | `feat(chat): keep a confirmed folder with the stored conversation` |
| 2.9-2.10 resume probe and `on_resumed` | done | `feat(chat): take the stored folder of a resumed conversation when it is concrete` |

A1 review fixes merged with `git merge feat/chat-folder-a1` (no conflicts). `cargo fmt --check`, clippy `-D warnings` and `cargo test` (7133 lib tests) green.

Deviations and notes:
- `SetFolder` is silent (no reply): a lost write only makes a resume ask. A panic in it is swallowed without a reply.
- `ensure_folder` runs for every `target >= 2` open, not only when migrating, so an existing v2 file gains the column.
- The confirm reuses the approval slot (`Origin::Block`); its keys are handled before the generic approval keys (`on_folder_key`): only an unmodified `y` confirms, a bare modifier is ignored, busy is re-checked after the ask closes (the ask itself would otherwise count as "an approval is pending").
- Prompt/pane cwd needed no code: `effective_cwd` of a chat is its folder.
- `/cd` lines: `/cd refused: <reason>` for busy and invalid targets (typed verdicts, `Outside` -> `not allowed`); `already in <f>` compares the probed real path with the chat folder.
- Resume: `App.resume_folders` keeps `(conversation, stored)` while the probe runs; stale answers are dropped; the resume path does not write `SetFolder` again; busy gives `stored folder not used: <reason>`; same folder is a no-op. `on_resumed` takes `(session, folder)` as a tuple (clippy too_many_arguments).
- The `#[allow(dead_code)]` A1 left are removed.

## Batch 3: PR B1 (tasks 4.1-4.8), branch `feat/chat-folder-b1`, base `feat/chat-folder-a2`

| Task | Status | Commit |
|---|---|---|
| 4.1-4.2 `folder_picker::{Source, Row, merge}` | done | `feat(chat): merge folder candidates into picker rows` |
| 4.3-4.4 `FolderPicker`, `PickerOutcome`, `on_key`, `paste`, `on_probed`, `on_completed` | done | `feat(chat): drive the folder picker with keys, a typed path and completion` |
| 4.5-4.6 `StoreCmd::RecentFolders`, `StoreReply::RecentFolders`, `recent_folders` SQL | done | `feat(chat): read the folders earlier conversations were kept under` |
| 4.7-4.8 `runtime/folder.rs` `walk_git`, `probe_many`, `complete` | done | `feat(chat): find git roots and list directories for the folder picker` |
| 4.8 `Answer` enum, `Prober::{start_many, start_completion}`, `Effect::{ProbeFolders, CompleteFolder}`, `AppEvent::{FoldersProbed, FolderCompleted}` | done | `feat(chat): answer picker candidates and completions off the event loop` |

Verification: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (7162 lib tests) green. About 394 added non-comment, non-blank production lines.

Deviations and notes:
- `merge` keeps `Concrete` only (design D3 amendment), not `Concrete|Outside` as D10 still says; it takes already probed candidates `(Source, Result<Probed, Refusal>)`, so HOME, `/`, ancestors, outside, secret and deleted folders are dropped by the A1 verdict, never re-judged.
- `Source` order: `LastUsed`, `PaneRoot`, `Recent`, `LastShell`; a stable sort, dedupe by the probed real path, cap 12.
- The picker has no `Accepting(token)`: while a probe is pending only Esc acts, so a reply cannot be stale; `on_probed` outside that state is ignored. A stale completion is dropped by comparing the field text sent with the current one.
- `PickerOutcome::{Handled, Probe(path), Complete{text, dir, prefix}, Cancel}`. The typed path goes through `expand` and the typed-flow `typed()` mapping (`Outside` -> `not allowed`; `Home` and `AboveHome` are accepted when typed).
- `RecentFolders` is a read-only SELECT (`GROUP BY folder ORDER BY MAX(updated_at) DESC`); a failure or a panic answers an empty list; an unreachable store thread answers an empty list from `StoreHandle::send`. `migrate_to` and the schema version are untouched.
- `walk_git(home, start)` canonicalizes both, stops below HOME (a `.git` in HOME is never a root) and `/`. `probe_many(env, paths, git)` applies the walk only to `PaneRoot` candidates and drops those without a repository.
- `folder::Answer` is now an enum (`Probed`, `Many`, `Completed`) instead of a tuple; `bounded` is built on a new generic `within`. Probing the candidates and listing completions are bounded by the same 3 s `TIMEOUT` and answer an empty list when late.
- `AppEvent::{FoldersProbed, FolderCompleted}` and the `RecentFolders` reply are ignored by the app until PR B2 wires the picker.

### Batch 2b: PR A2 review fixes (W1-W5, S1-S2)

- W1/W2: `Purpose::Resume(token)`; `App.resume_folders` holds `(token, conversation, path)`; only the latest resume answer counts; `/cd` submit and any folder change drop the entry.
- W3: `Purpose::Cd(token)` and `App.cd_requests` (token + submitted arg); an answer is dropped unless it matches the latest request and the composer still holds that arg.
- W5: at `y` the folder is probed again (`Purpose::Confirm(token)`, through the off-loop Prober); a different path or a refusal gives `/cd refused: folder changed since it was checked`; the composer clears and the folder applies only on a matching answer. Busy is checked at `y` and again at the answer.
- W4: `ChatState::restore_folder` for a resume: system line `folder restored: <f>`, model notice `This conversation's folder is <f>.` once, no `End`, no `SetFolder`; memory is untouched when the resolved project equals the resumed session project, else it resolves again keeping the resumed session (`restore_memory_for_folder`).
- S1: `drop_folder_state` on pane removal. S2: comment in `chat_store.rs` that v3 may assume `folder` exists.
- Production size of A2 after the fixes: 561 added / 33 removed lines gross (`git diff --numstat`, tests and snapshots excluded; comments and blanks included, roughly 170 of them), 179 added in the fixes. Over the 400 soft limit, under the 600 hard limit.
- Tests: 7148 lib tests green. RED was seen at compile level (new variants/fields) for each batch; the behavioural assertions were written with the code in the same step.

### Batch 3b: PR B1 review fixes (W1-W5, S)

Merged `feat/chat-folder-a2` first (clean except the `Purpose::Cd(token)` test and this file).

- W1: `complete(roots, dir, prefix)` canonicalizes `dir` and lists nothing unless it is `Concrete` or `Home`; each entry is canonicalized and must pass `roots.judge_real`, so a link into `~/.ssh` is never suggested.
- W2: `Tab` ignores a held key and waits for the answer (`completing`, freed by any answer of this picker, stale text included). In the runtime, `Prober` keeps one `Flight` per kind (candidates, completions), held by the blocking work itself until it really ends; a request that finds it taken answers empty at once, so a hung filesystem cannot fill the blocking pool.
- W3: the roots load once per request; `each` bounds every candidate on its own (all in parallel), a stalled one is `folder not found` and `merge` drops it. `probe_many` is gone from production (tests keep a helper over `candidate`).
- W4: `Completion{names, truncated}`; a truncated list is shown but never extends the field.
- W5: names with control characters are dropped in `complete`.
- S: `RECENT_ASK` = 40 for `RecentFolders`, `merge` caps at 12; the typed text is trimmed before completion; `FolderPicker::new(id, ..)`, and `set_rows`, `on_probed`, `on_completed` drop answers whose id is not the picker's (`Purpose::Pick(id)`, `Answer::{Many, Completed}` and the events carry it).
- TDD note: W1 and W5 were seen failing against the old `complete` before the fix; the signature changes (ids, `Completion`) were RED at compile level; the stalled-candidate and single-flight tests were written together with the code.
- 7187 lib tests green.

### Batch 3c: PR B1 review round 2

- Busy signal: `Completion.busy` and `Answer::Many(.., busy)` / `AppEvent::FoldersProbed{busy}` say a request was turned away because an earlier one is still on the disk (B2 shows it; no UI here).
- `complete` checks at most `EXAMINED_MAX` (4 x `COMPLETIONS_MAX`) entries and marks the list truncated when it stops there; names with Unicode format characters (zero width, bidi override, BOM, soft hyphen) are dropped with the control ones; the doc says `AboveHome`/`Outside` folders list nothing.
- Test seams: `Prober.bound` (the request bound, `TIMEOUT` outside tests), `many_with` and `completion_with` take the judging closure. Tests: a slow candidate times out and keeps the flight until its thread ends; a second request is turned away busy; a slow or panicking listing/candidate frees the flight. Mutation checks: dropping the hold in `each` or in the listing fails them.
- Tests only: listing HOME with `.` leaves out `.ssh` and `.gnupg`; `.gnupg` and `.config/gh` list nothing inside.
- Left for B2: a flight or debounce for the single-folder `start` and a session-unique picker id allocator.
- 7194 lib tests green.

## Batch 4: PR B2 (tasks 5.1-5.7), branch `feat/chat-folder-b2`, base `feat/chat-folder-b1`

| Task | Status | Commit |
|---|---|---|
| 5.1-5.2 open flow: `ProbeFolder{Open}`, deferred steps, picker in `ChatState`, keys, paste, gathering | done | `feat(chat): settle the chat folder before the open steps and show the picker for a broad one` |
| 5.3-5.4 accept/Esc, resume picker, held keys in the folder ask | done | `feat(chat): ask for a folder when a resumed conversation has none and keep held keys from answering the folder ask` |
| 5.5-5.6 render (rows, empty, error, gathering, tiny panes, hostile names) | done | `feat(chat): draw the folder picker in the chat pane` (+ clippy fix commit) |
| 5.6 `/cd` Tab completion, hints for no match and still gathering | done | `feat(chat): complete the folder of a /cd line with Tab and say when nothing matches` |
| 5.7 README (Spanish) picker, keys, `/cd`, busy refusals, held keys, `chat.db` stores the path | done (picker and `/cd` part) | `docs: document the chat folder picker and /cd` |
| B1 round 2 busy signal | done | `feat(chat): tell the picker and the /cd line when the folder check was busy` |

`feat/chat-folder-b1` was merged twice (round 1 and round 2 review fixes, which also carried A2 fixes #414); no rebase.

Verification: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (7238 lib tests) green.

Deviations and notes:
- `Purpose` (after the A2 fixes) is `Cd(u64) | Confirm(u64) | Resume(u64) | Pick(u64) | Open`. The picker's Enter probe is `Pick(picker id)`; the open probe has no token (the `opening` set guards it).
- Picker ids come from the same session counter as the folder tokens (`App.folder_seq`), never per pane. Composer completions use id `0` (`COMPOSER`), which no picker has.
- Open flow: `App.opening` holds the chats whose steps wait for the folder. Keys of a chat that is `opening` without a picker (probe in flight, up to 3 s) are swallowed. A `None` origin opens the picker at once. A concrete result keeps the pane folder as it is (no canonical-path replacement: the tools canonicalize on their own); only the verdict matters.
- On accept at open, `set_folder` finds no prompt yet and sets `reprepare_prompt`, so the first `PreparePrompt` answer is re-requested once. It is correct and cheap, and avoided a new setter.
- Accepting the folder the chat already has changes nothing (no `folder changed` line). Accept re-checks busy; a busy chat shows `/cd refused: <reason>`.
- Esc in the picker adds `chat folder is <f>; use /cd <folder> to change it` for both the open and the resume case.
- Resume: `no stored folder`, a refused/broad stored folder open the picker (RecentFolders asked with `RECENT_ASK`), unless `folder_busy`. A concrete stored folder still restores silently.
- Held keys in the folder ask: a held key is never an answer; a held Esc or `n` still cancels. The Enter that submitted `/cd` counts as held for 500 ms and does not cancel (documented in the README); `y` held never confirms.
- Busy: `FoldersProbed{busy}` ends the gathering with an empty list and the hint `folder check is busy, try again`; a busy completion shows the same hint (picker) or a system line (composer). Zero matches: `no matching folder`. Tab while gathering: `still looking for folders` and no request.
- `Composer::window` (the sideways scrolling line) moved from `TagEditor::window`, which delegates; the picker field uses it. `folder_picker::{completion_target, completed}` are pure and shared by the picker and the `/cd` line.
- Tests: `App::update_settled` (cfg(test)) answers the open probe "concrete" and closes a picker, so the ~50 existing tests that open or resume a chat keep their meaning; `press` and a few `ui`/`runtime` helpers use it.
- TDD note: the accept/Esc/closing-pane code was written in the first work unit together with the open flow; its tests were added right after (they passed at once). The held-key, resume-picker, render, Tab, hint and busy behaviour were seen failing first.
- A3 and PR #410 were not on `origin/main` when B2 started; the README therefore has its own picker section (a note says its keys move to the "Referencia de teclas por vista" table once #410 lands). The `--dir`, `[workspace] dir` and `install-desktop --dir` docs of task 5.7 belong to A3.
- Production size: about 580 added non-comment, non-blank lines (picker.rs 315, render 126, rest spread), over the 400 soft limit.

### Batch 4b: B2 review fixes and the split into three stacked PRs

- W1: `Prefix` note, summary, tags and the memory switch answer `Choose the chat folder first` while the picker is open or the pane still waits for its folder (`refuse_over_folder`).
- W2: a pick on a busy chat keeps the picker open with the busy reason in the hint (it used to close the picker and keep the broad folder).
- W4: `update_settled` answers only the Open probe the update emitted and closes only pickers the update opened, dropping only their own gathering effects.
- S5: a chat opened from a chat in `$HOME` shows the picker and starts nothing (test). S7: pastes are swallowed while the pane waits for its folder; README documents it and the three hints.
- S6: the picker row reads `<source> · <path>` so a cut path keeps its source (`pane root`, `last used`, `recent`, `last shell`); `completed()` ignores names with control or format characters.
- The B2 branch is split without rewriting it: `feat/chat-folder-b2a` (open flow, resume picker, held keys, W1/W2/W4/S5/S7 paste), `feat/chat-folder-b2b` (picker render and `Composer::window`), `feat/chat-folder-b2c` (`/cd` Tab, hints and busy, README). The tree of b2c equals the tree of `feat/chat-folder-b2`.

## Remaining

A3 merge (3.1-3.8, PR #411), closing 6.1-6.2.
