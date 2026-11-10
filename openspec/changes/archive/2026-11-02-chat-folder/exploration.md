# Exploration: chat-folder

Copy of engram `sdd/chat-folder/explore` (#3430). Read from the working tree on `feat/coding-tools-m5b`, not `origin/main`.

## Problem

When lazarobox is launched from rofi, a `.desktop` launcher or WSL, the chat origin is `$HOME` (or `/`). Cloud engines then refuse every command, and CLI engines run in `data/cli-cwd`, so `cd` does nothing. Related: issue #394 (`/cd`).

## Code map

| Area | Finding |
|------|---------|
| `src/app/chat.rs` `open_chat_pane` | origin = `focused_state().effective_cwd()` |
| `src/app.rs` `effective_cwd` | Pty: OSC 7, then `/proc` poll; Chat: `origin_cwd` |
| `src/app.rs` `inherited_cwd` | focused pane -> `launch_cwd` -> `HOME` |
| `src/runtime.rs:231` | `launch_cwd` = `env::current_dir` |
| `src/app/tool.rs` `offer_tools` | -> `engine.rs` `tools_for(Tools::On { origin })` |
| `src/app/auto_run.rs` `run_scope` | `None` -> `/`; cloud unless locality is Local |
| `src/core/ai/chat/approval.rs` | `BROAD_ROOT` |
| `src/runtime/tools/host/exec.rs` `root_of` | refuses relative, non-dir, `HOME`, secret roots |
| `src/runtime/tools/host/files.rs` `reach` / `concrete_root` | `HOME` or an ancestor => `Reach::Broad`, asks |
| `src/app/context.rs` `begin_memory` | engram project resolved from origin cwd; `None` => NotGit degrade |
| `notes.rs`, `summary.rs`, `offer.rs`, `compact.rs`, `history.rs` | also key on `origin_cwd` |
| history | stores origin per chat; resumed chats keep the stored origin |
| `src/runtime/chat/cli.rs` | `Dirs::create` makes `cli-cwd` |
| `src/main.rs` | no start-dir flag (only internal subcommands + `install-desktop`) |
| `src/core/desktop/lazarobox.desktop` | `Exec`/`Terminal` only, no `Path=` |
| `README.md` ~l.336 | documents the "cd first" workaround |

## Options considered

(a)-(e) in the engram artifact: start-dir flag / `Path=`, config key, folder picker at open, `/cd` command, and combinations. User chose a combination (see `sdd/chat-folder/product-decisions`, #3431).

## Learned

- `ChatState.origin_cwd` has no setter.
- No chat slash-command infrastructure exists; the composer needs new parsing.
- #394: user-only command, confirm showing old and new folder, blocked while writes are staged, approvals pending or commands running; same restrictions as at open time; transcript line plus a notice to the model; history semantics to decide.
