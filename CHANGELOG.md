# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions before 0.8.0 were never published to crates.io: they are retroactive milestones
reconstructed from the merge history of `main`.

## [Unreleased]

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
  Starting the app creates an empty, private database file at
  `$XDG_DATA_HOME/lazarobox/chat.db` (default `~/.local/share/lazarobox/`),
  for that future history; nothing is written to it yet.
- Only Ollama is supported. Other engines are detected but have no chat
  adapter yet.
- One fixed system prompt; profiles come later.
- The chat talks to `127.0.0.1` only and sends no credentials.

## [0.7.0] - 2026-10-12

### Added

- Per-task AI model selection in the config menu: a general "Set all tasks"
  engine plus one row for each of the six AI tasks.
- Two-level picker (engine, then model) with a `custom...` row to type or paste
  a model name.
- Curated Ollama and CLI model catalogs. Models that are not installed are
  marked `download required` and cannot be chosen; nothing is downloaded.
- `s` saves the choice to the `[ai]` and `[ai.tasks.*]` tables of
  `config.toml`; `Esc` discards it. `Enter` never saves.
- `r` re-detects the available engines from the menu.

### Fixed

- Custom model names reject format characters.
- The "Set all" row reflects the resolved values.

## [0.6.0] - 2026-10-11

### Added

- Detection of local AI engines: `ollama`, `lmstudio`, `llamacpp` and `vllm`
  (loopback servers on their usual ports) and the `claude`, `codex`, `gemini`
  and `opencode` CLIs found in `PATH`.
- AI section in the config menu, with the detected engine status.
- `[ai]` engine and model keys in `config.toml`.
- Detection is read-only, uses `127.0.0.1` only and sends no credentials; the
  only command run is `--version` on the CLIs found.

### Fixed

- Hardened parsing of engine responses and sanitized control characters in
  versions and model names.
- Version probes are bounded and their process group is cleaned up.

## [0.5.0] - 2026-10-11

### Added

- COPY mode selection: `v` selects characters, `V` selects whole lines.
- `y` copies the selection to the system clipboard through OSC 52 (selections
  over 75,000 bytes are not copied) and returns to the terminal.
- Selection highlight and a VISUAL / V-LINE marker in the statusline.

### Fixed

- A held COPY exit key no longer re-triggers after leaving COPY.
- Empty yanks are skipped and trailing blank rows are dropped from line yanks.
- The COPY cursor resets correctly on screen switch or resize.

## [0.4.0] - 2026-10-11

### Added

- Desktop launcher (`packaging/lazarobox.desktop`) and a per-user installer
  script (`scripts/install-desktop.sh`) that opens the workspace in Kitty from
  rofi or the application menu.

## [0.3.0] - 2026-10-11

### Added

- Config menu on `Ctrl+Space m` with a Settings section: statusline and tab bar
  position (top or bottom), previewed live.
- `Enter` saves to `config.toml` keeping the user's comments, line endings and
  key order; `Esc` reverts. Writes are atomic and refused when the file has
  errors.

## [0.2.0] - 2026-10-11

### Added

- Splits, geometric focus, sticky resize and zoom for panes.
- Tabs with a tab bar that appears when there is more than one tab.
- Which-key style prefix groups behind `Ctrl+Space`.
- New panes and tabs inherit the working directory of the focused pane, tracked
  through OSC 7 with a `/proc` fallback.

### Changed

- A pane closes when its shell exits; the last tab closing quits the app.

## [0.1.0] - 2026-10-06

### Added

- Full-screen terminal pane running your `$SHELL` in a PTY, with a VT100
  emulation layer and TrueColor.
- Modal input: TERMINAL, PREFIX (`Ctrl+Space`) and COPY modes, with a quit
  confirmation.
- COPY mode with scrollback navigation using vim-style motions.
- Cursor shape passthrough and a statusline with the current mode, directory
  and shell.
- Linux support only.

[Unreleased]: https://github.com/pichu2707/lazarobox-shell/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/pichu2707/lazarobox-shell/releases/tag/v0.1.0
