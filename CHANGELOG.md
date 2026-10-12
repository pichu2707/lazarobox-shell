# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions before 0.7.0 were never published: they are retroactive milestones
reconstructed from the merge history of `main`.

## [Unreleased]

### Added

- AI chat (in development on `feat/ai-chat`): talk to the AI engine and model
  chosen in the config menu from inside the workspace.

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

[Unreleased]: https://github.com/pichu2707/lazarobox-shell/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/pichu2707/lazarobox-shell/releases/tag/v0.1.0
