# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- AI chat (planned): talk to the AI engine and model chosen in the config menu
  from inside the workspace.

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

[Unreleased]: https://github.com/pichu2707/lazarobox-shell/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/pichu2707/lazarobox-shell/releases/tag/v0.7.0
