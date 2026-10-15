# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

0.8.0 is the first version published to crates.io. 0.7.0 is an unpublished
baseline.

## [Unreleased]

### Added

- Nesting guard, modelled on tmux's `$TMUX`: every shell pane child gets
  `LAZAROBOX=<pid of the instance>`. Starting `lazarobox-shell` with that
  variable set (and non-empty) prints `lazarobox-shell is already running in
  this terminal; use --force to nest anyway` to stderr and exits 1, before the
  terminal enters raw mode. `--force` (a top-level flag) skips the check.
  Subcommands, such as the internal `__jail-exec`, are never blocked.
- `lazarobox-shell install-desktop [--terminal <cmd>] [--uninstall]` installs
  the desktop launcher from the binary itself, so it works after
  `cargo install lazarobox-shell` and with any terminal. The terminal is
  `--terminal`, then `$TERMINAL`, then `xdg-terminal-exec`, then the first of
  kitty, ghostty, wezterm, alacritty, foot, gnome-terminal, konsole,
  xfce4-terminal and x-terminal-emulator in `PATH`, else `Terminal=true`. An
  unknown terminal is launched as `<cmd> -e <bin>`, and any terminal other
  than Kitty prints a compatibility notice.

### Changed

- `scripts/install-desktop.sh` is now a thin wrapper around the subcommand and
  accepts the same options. The launcher template moved from
  `packaging/lazarobox.desktop` into the binary.

### Fixed

- AI chat with the `gemini` CLI no longer ends with `the engine stopped
  answering` when the model is slow. gemini prints its first line at once and
  can then stay silent for a minute or more while it retries a busy model
  (HTTP 503), which tripped the 90 s idle limit. A CLI turn now waits up to
  300 s between outputs (the 15 minute total limit is unchanged; servers keep
  their 60 s), and after 10 s of silence the pane shows `still waiting for the model…`
  instead of a silent spinner.

## [0.13.0] - 2026-10-14

### Added

- Chat memory: deleting a conversation from the list (`d`) can also delete its
  engram notes. With memory on, `ENGRAM_HTTP_TOKEN` set and at least one saved
  note, the prompt reads `Delete conversation? y · n · a also delete its N
  notes`: `y` keeps the notes, `a` deletes the conversation and then each note
  (`DELETE /observations/{id}?expected_project=P` with the bearer token), and
  any other key cancels. The result is one line (`deleted N notes from
  engram`, `deleted K of N notes from engram` or `could not delete notes:
  <reason>`). Without a token or with memory off, the notes stay and a line
  says why. Retention eviction never sends a delete.

### Changed

- Chat memory: resuming a conversation reuses the engram session (and project)
  its notes were written under, so new notes land in the same session and it is
  ended once. A resumed conversation that writes nothing ends no session.
- The conversation list carries the number of notes of each conversation.

## [0.12.0] - 2026-10-14

### Added

- AI chat with the `claude`, `codex` and `gemini` CLIs, using your own login
  (no API key is read, stored or asked for). The conversation is sent to the
  CLI's provider.
  - Text only: each CLI starts with fixed options that remove its tools, so it
    cannot run commands or edit files. gemini gets a deny-all tool policy file
    written into the turn's private temp folder, no extensions and no MCP
    servers (its `plan` mode alone can be left by the model).
  - Linux jail (Landlock) for every CLI turn: no access to `$HOME`, `~/.ssh`
    or other CLIs' folders, writes only in the CLI's own state folder and a
    private per-turn temp folder, and only the interpreter and the CLI can be
    executed. codex and gemini do not run without the jail
    (`<engine>: not available on this system yet`); claude does, and says so
    once in the pane.
  - Script CLIs (npm installs) start through their interpreter. A Homebrew node
    gets read-only access to exactly the library folders its own ELF lists and
    to its OpenSSL config and CA bundle.
  - A newer codex or gemini that adds an event kind, or a codex that drops a
    feature flag lazarobox disables, makes the chat fail with an error until
    lazarobox is updated.
  - Tested with claude 2.1.299, codex-cli 0.142.5 and gemini 0.63.0.
- Scrolling for every pane. The mouse wheel scrolls the pane under the pointer
  without moving the focus: three lines per notch in a shell pane's
  scrollback and in the AI chat transcript, and straight to the application
  when it asks for the mouse (`vim`, `htop`) or runs on the alternate screen
  (`less`, `man`, which get arrow keys). Typing in the pane or pasting returns
  it to the live screen.
- COPY mode on AI chat panes (`Ctrl+Space [`): `j`/`k`, `Ctrl+u`/`Ctrl+d`,
  `gg`, `G`, `Esc` or `q` to leave. The streaming answer keeps arriving while
  the view is scrolled.
- A `[+N]` badge on a scrolled pane, a position in the statusline
  (`TERMINAL ↑12/340`) and a key hint in COPY mode.
- The menu `Mouse` row is now a real `off`/`on` setting with a live preview;
  `Esc` reverts it and `s` saves it as the root key `mouse` of `config.toml`.

### Changed

- The jail no longer lets a CLI read `/usr/bin`, `/usr/sbin` or `/bin`: the
  dynamic loader could otherwise run any program there.
- A script CLI's interpreter in a group- or world-writable folder, or under
  `/tmp`, is refused inside the jail.
- The mouse is captured by default (`mouse = true`), but only the wheel is
  reported: hold `Shift` to select text with the terminal. Set `mouse = false`
  to turn the capture off. The terminal is released on quit, on exit of the
  last pane and on panic.

### Removed

- The `config: mouse is not supported yet` notice.

### Security

- Known limits, also in the README: the CLI state folders are writable
  (a configuration written there runs on your next unjailed run), network is
  not restricted, claude rewrites `~/.claude.json` in place under the jail,
  claude's system prompt is visible in `ps`, gemini keeps its sessions in
  `~/.gemini` outside `chat.db`, and `execve` limits are defense in depth.

## [0.11.0] - 2026-10-14

### Added

- Conversation history for the AI chat. Messages are saved to the private
  `chat.db` as you chat. An answer that stops early (an error or a cancel
  after some text arrived) is saved too, marked as partial. The model's
  reasoning is never stored.
- Conversation list: `Ctrl+Space c` now opens the list instead of an empty
  chat. It shows the conversations of the current project (the git
  repository), newest first, as `date · profile · first message`.
  - `a` shows or hides the conversations of all projects.
  - `n` or `Esc` starts a new conversation.
  - `Enter` resumes the selected one: the full history is loaded, new
    messages are appended to the same conversation, and its stored profile is
    used.
  - `d` then `y` deletes it (any other key cancels). A pane that has the
    deleted conversation open stops saving and says so.
- Tags: each conversation has a project tag plus up to 8 extra tags. Edit them
  with `t` on a list row or with `Ctrl+Space T` inside a conversation (one
  comma-separated line; the project tag comes first; `Enter` saves, `Esc`
  discards). A saved edit counts as activity, so the row moves to the top of
  the list.

### Changed

- When an answer fails after some text was received, the partial text is kept
  in the history with status `error`; the error line itself is not stored.
- Closing a pane (or a tab, or quitting) while an answer is streaming saves
  the text received so far as `cancelled`.

### Known limits

- Extra tags come back in alphabetical order after a conversation is reopened.
- Deleting the engram notes of a deleted conversation and resuming the engram
  memory session are not part of this release (they arrive with the
  `engram-memory` follow-up).
- `Ctrl+Space T` has not yet been checked by hand in Kitty.

## [0.10.0] - 2026-10-14

### Added

- Optional project memory with [engram](https://github.com/Gentleman-Programming/engram),
  shared with other agents that use it. It is off by default: turn it on in
  the AI menu (`Ctrl+Space m`) or with `[ai.memory]` in `config.toml`
  (`enabled`, `acknowledged`, `port`). A one-time privacy warning must be
  accepted first. It works in git projects only and needs an engram 3.x server.
- Memory block at the top of a chat: shown as `▸ memory (N notes)`, and
  `Ctrl+Space o` shows or hides it.
- Save a note from the chat with `Ctrl+Space n` (`Ctrl+S` saves, `Esc`
  closes). Recognizable secrets are redacted first, and the model's reasoning
  is never stored.
- Conversation summary with `Ctrl+Space s`: review the draft (`Ctrl+S`
  approves, `Esc` discards); an approved summary is sent to engram when the
  chat closes.
- When engram is not running, the chat offers to start `engram serve`, showing
  the absolute path of the binary. The server keeps running after you quit
  lazarobox.
- Statusline indicator `mem: ready`, `mem: checking` or `mem: unavailable`.
  Every wait on memory is bounded to 5 seconds, so a slow or missing server
  never blocks the chat.
- Chat notification: lazarobox rings the terminal bell when a chat answer
  finishes while you are not looking at it (another pane, tab or window is
  focused). Turn it on with `notify = true` in `[ai]` or with the new Notify
  row in the AI menu. It is off by default.

### Changed

- `chat.db` moves to schema version 2 to hold the memory data. An older
  lazarobox refuses a newer `chat.db` and leaves it untouched.
- The chat database is now opened lazily: a session that never uses the chat
  no longer creates `chat.db` or its directory.

### Known limitations

- Conversations are not saved yet; chat history arrives in a later release.
  Deleting notes from lazarobox and resuming conversations come with it.
- Only Ollama has a chat adapter, and only it receives memory. Other engines
  are detected but cannot chat.
- Memory requires engram 3.x.
- The chat talks to `127.0.0.1` only and sends no credentials.

### Notes

- Still pending manual verification in Kitty: the `engram serve` offer, the
  note keys, the memory line toggle, the summary keys and the notification
  bell.

## [0.9.0] - 2026-10-13

### Added

- AI profiles for the chat: `analyst` and `programmer`, set with `profile` in
  the `[ai]` section of `config.toml` (default `programmer`) or with the new
  Profile row in the AI menu (`Ctrl+Space m`). An invalid value is reported and
  the default is kept.
- Built-in system prompts: ten in total, one for each profile and task
  (`general`, `code`, `chat`, `review` and `summarize`). The chat uses the
  `chat` prompt of the chosen profile.
- Prompt overrides: put your own prompt in
  `~/.config/lazarobox/prompts/<profile>/<task>.md`. The task file wins, then
  `general.md`, then the built-in prompt. Files must be regular UTF-8 text of
  at most 16 KiB; one that cannot be used is skipped with a notice.
- Workspace context in the system prompt: a short block with the project, git
  branch, current directory relative to the repository root, OS, shell and
  date. It never includes absolute paths, environment variables, file contents
  or terminal output; your home directory is shown as `~`, and git is never
  run. Turn it off with `context = false` in `[ai]` or with the new Context
  row in the AI menu. It is on by default.
- If the context cannot be prepared within 2 seconds, the chat falls back to
  the built-in prompt and shows the notice `workspace context unavailable`.

### Fixed

- Closing a chat pane while its first message was waiting for the prompt no
  longer sends that message.

### Known limitations

- Conversations are not saved yet; chat history arrives in a later release.
  Starting the app still creates an empty, private database file at
  `$XDG_DATA_HOME/lazarobox/chat.db` (default `~/.local/share/lazarobox/`),
  for that future history; nothing is written to it yet.
- Only Ollama has a chat adapter. Other engines are detected but cannot chat.
- The chat talks to `127.0.0.1` only and sends no credentials.

## [0.8.0] - 2026-10-12

First release published to crates.io.

### Added

- **Experimental** AI chat pane: `Ctrl+Space c` opens a chat with a local
  model inside the workspace. It works only with Ollama for now.
- Streamed answers: the reply appears as the model writes it, with wrapped
  prose and fenced code blocks.
- Reasoning block for models that "think" (for example `qwen3`): the thinking
  is kept apart from the answer and `Ctrl+R` shows or hides it. A statusline
  indicator shows while a model is thinking, even if another pane is focused.
- Composer keys: `Enter` sends, `Alt+Enter` or `Ctrl+J` adds a newline,
  `Esc` or `Ctrl+C` cancels the running answer, `Ctrl+U` clears the input,
  `PageUp`/`PageDown` scroll the transcript, `Ctrl+Y` copies the last answer
  and `Ctrl+B` its last code block (through OSC 52).
- The chat uses the engine and model of the `chat` task (`[ai.tasks.chat]` in
  `config.toml` or the AI menu, `Ctrl+Space m`); without its own setting it
  falls back to the general engine. The pane explains what is missing when
  the engine is not set, not running or not supported.
- Closing a chat pane or tab (for example `Ctrl+Space w q`) cancels its running
  answer.

### Known limitations

- Conversations are not saved yet; chat history arrives in a later release.
- Only Ollama is supported. Other engines are detected but have no chat
  adapter yet.
- One fixed system prompt; profiles come later.
- The chat talks to `127.0.0.1` only and sends no credentials.

## [0.7.0] - 2026-10-12

0.7.0 is the baseline before the first crates.io release and was not published.

### Added

#### Terminal panes and tabs

- Full-screen terminal pane running your `$SHELL` in a PTY, with a VT100
  emulation layer and TrueColor. Linux support only.
- Modal input: TERMINAL, PREFIX (`Ctrl+Space`) and COPY modes, with a quit
  confirmation.
- Cursor shape passthrough and a statusline with the current mode, directory
  and shell.
- Splits, geometric focus, sticky resize and zoom for panes.
- Tabs with a tab bar that appears when there is more than one tab.
- Which-key style prefix groups behind `Ctrl+Space`.
- New panes and tabs inherit the working directory of the focused pane, tracked
  through OSC 7 with a `/proc` fallback.
- A pane closes when its shell exits; the last tab closing quits the app.

#### Configuration menu

- Config menu on `Ctrl+Space m` with a Settings section: statusline and tab bar
  position (top or bottom), previewed live.
- `s` saves to `config.toml` keeping the user's comments, line endings and
  key order; `Esc` reverts. `Enter` cycles a Settings value or opens the
  picker, it never saves. Writes are atomic and refused when the file has
  errors.

#### Copy selection

- COPY mode with scrollback navigation using vim-style motions.
- `v` selects characters, `V` selects whole lines.
- `y` copies the selection to the system clipboard through OSC 52 (selections
  over 75,000 bytes are not copied) and returns to the terminal.
- Selection highlight and a VISUAL / V-LINE marker in the statusline.
- A held COPY exit key no longer re-triggers after leaving COPY; empty yanks
  are skipped and the COPY cursor resets on screen switch or resize.

#### Desktop launcher

- Desktop launcher (`packaging/lazarobox.desktop`) and a per-user installer
  script (`scripts/install-desktop.sh`) that opens the workspace in Kitty from
  rofi or the application menu.

#### AI engine detection

- Detection of local AI engines: `ollama`, `lmstudio`, `llamacpp` and `vllm`
  (loopback servers on their usual ports) and the `claude`, `codex`, `gemini`
  and `opencode` CLIs found in `PATH`.
- AI section in the config menu, with the detected engine status, and `[ai]`
  engine and model keys in `config.toml`.
- Detection is read-only, uses `127.0.0.1` only and sends no credentials; the
  only command run is `--version` on the CLIs found. Engine responses are
  parsed defensively and version probes are bounded.
- `r` re-detects the available engines from the menu.

#### Per-task AI model selector

- Per-task AI model selection in the config menu: a general "Set all tasks"
  engine plus one row for each of the five AI tasks (general, code, chat,
  review, summarize).
- Two-level picker (engine, then model). The CLI engines end with a `custom...`
  row to type or paste a model name; custom names reject format characters.
- Curated Ollama and CLI model catalogs. For server engines, models that are
  not installed are marked `download required` and cannot be chosen; nothing is
  downloaded.
- `s` saves the choice to the `[ai]` and `[ai.tasks.*]` tables of
  `config.toml`; `Esc` discards it. `Enter` never saves.

[Unreleased]: https://github.com/pichu2707/lazarobox-shell/compare/v0.13.0...HEAD
[0.13.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/pichu2707/lazarobox-shell/releases/tag/v0.7.0
