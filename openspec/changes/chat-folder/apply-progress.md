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

## Remaining

PR A2 (2.1-2.10), A3 (3.1-3.8), B1 (4.1-4.8), B2 (5.1-5.7), closing 6.1-6.2.
