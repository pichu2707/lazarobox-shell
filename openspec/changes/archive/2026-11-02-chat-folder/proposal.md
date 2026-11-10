# Proposal: Chat folder from anywhere

## Intent

Launched from rofi or any desktop launcher, lazarobox starts in `$HOME`; the chat inherits it. With a cloud engine, `run_command` and git refuse a `$HOME` root, run-mode commands stop running unasked (`the chat's folder is your home or /`) and reads ask; CLI engines run in their own `cli-cwd`, so asking the model to `cd` does nothing. Today's fix is "cd in a shell pane first" (README ~l.331-336). Goal: open lazarobox from anywhere and have chats and tools work in the right project, without ever picking one silently. Includes #394 (`/cd`).

## Scope

### In Scope
- **Picker at open** when the folder is `$HOME`, `/` or unknown (rofi, macOS without OSC 7, ssh/tmux): last used project pre-selected (one Enter), pane git roots, recent chat.db folders, last non-home shell cwd of the tab, typed path with completion. Esc keeps today's behaviour.
- **`/cd <path>`** (user-only): always asks y showing old and new folder; same root rules as at open; refused with a reason while writes are staged/proposed, an approval is pending or a command runs. First composer slash-command.
- **After a change**: transcript line, model notice not to reuse old paths; context block and engram project follow.
- **Persistence**: chat.db stores the last folder; resume opens it. Chat-from-chat inherits (picker if broad).
- **Start dir**: `--dir`, `install-desktop --dir` (`Path=`), `[workspace] dir`; precedence `--dir` > config > launch cwd. Spanish README replaces the workaround.

### Out of Scope
- A model-callable folder tool; WSL Start menu (#268); relaxing `$HOME` protection (only explicit choice, cloud still asks).

## Capabilities

### New Capabilities
- `chat-folder`: picker, `/cd`, root rules, change semantics, inheritance.
- `start-dir`: `--dir`, `Path=`, precedence.

### Modified Capabilities
- `chat-pane`: composer slash commands; open may show the picker.
- `chat-history`: last folder stored; resume uses it.
- `chat-memory`: project follows the folder.
- `configuration`: `[workspace] dir`.
- `terminal-session`: launch cwd honours start dir.

## Approach

One `ChatState` origin setter used by picker, `/cd` and resume; tools, auto-run, context and memory re-read it. Validation REUSES `host/exec.rs` `root_of` and `host/files.rs` `concrete_root`, no second policy. Picker candidates are pure core logic; runtime only probes folders.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/core/ai/chat/mod.rs` | Modified | origin setter |
| `src/app/chat.rs`, composer | Modified | picker at open, `/cd` |
| `src/app/tool.rs`, `engine.rs`, `auto_run.rs` | Modified | follow new root |
| `src/runtime/tools/host/{exec,files}.rs` | Reused | root policy |
| `src/app/context.rs` | Modified | memory project |
| history / chat.db | Modified | last folder |
| `src/runtime.rs`, `src/main.rs` | Modified | launch cwd, `--dir` |
| `src/core/desktop/lazarobox.desktop`, config, `README.md` | Modified | `Path=`, key, docs |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Root changes mid-turn | Med | `/cd` blocked while busy |
| Model reuses stale paths | High | notice; paths re-checked by policy |
| Privacy widening on cloud | Med | `$HOME` explicit only; cloud asks |
| Duplicated root policy | Med | one validator |
| Cwd unknown (macOS, ssh) | High | treat as unknown -> picker |

## Rollback Plan

`git revert` per PR. Without the picker, Esc-equivalent behaviour is today's. The chat.db column is additive and ignored by older builds; `Path=` goes away by re-running `install-desktop` without `--dir`.

## Dependencies

- Coding-tools chain (#392/#393/#395/#396) merged first.

## Delivery Forecast

ask-on-risk. Likely >400 production lines (tests excluded): PR A `/cd` + setter + start dir + docs; PR B picker.

## Success Criteria

- [ ] From rofi in `$HOME`, one Enter opens the chat in the last project and a cloud engine runs a command there.
- [ ] Esc keeps today's behaviour; no folder is chosen silently.
- [ ] `/cd` asks, is refused while busy, and logs the change.
- [ ] Resume opens the last folder; `--dir`/`Path=`/config start there.
