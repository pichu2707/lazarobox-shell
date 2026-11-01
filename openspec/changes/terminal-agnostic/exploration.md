# Exploration: terminal-agnostic

## Goal

Make lazarobox work the same in any host terminal, including the WSL console. Replace the Kitty-only manual passes with per-terminal records.

## Current state

lazarobox is a TUI. It always runs inside a host terminal. Today it depends on that terminal in five ways, with no capability check:

| Dependency | Where | Note |
|---|---|---|
| `Ctrl+Space` prefix (NUL byte) | `src/core/keys.rs`, `src/runtime.rs:1997` (`to_app_event`) | Frozen in the 1.0 table: not configurable. |
| OSC 52 is the only clipboard path | `src/core/copy.rs:341-371`, `src/app/chat_copy.rs:190`, `src/runtime.rs:1567` | No `wl-copy` / `xclip` / `xsel` / `clip.exe` fallback. The terminal never confirms the copy. |
| SGR mouse (1000/1006), bracketed paste, focus events (?1004) | `src/runtime.rs:1880`, `2063-2078`, `src/core/mouse.rs:12-14` | Enabled without probing. |
| Truecolor | 25 uses of `Color::Rgb` | No 256-color fallback. |
| Desktop launcher | `src/core/desktop.rs:20-38`, `337-350` | Terminal list; `NON_KITTY_NOTICE` says "tested in Kitty". |

Other facts:

- No keyboard enhancement flags are pushed. There is no `TERM_PROGRAM` or Kitty detection, except the environment line of the feedback report (`src/core/menu/feedback.rs:269-289`, which also detects WSL).
- `src/core/opener.rs` already handles WSL (`wslview`, `rundll32.exe`).
- App-bound modified keys on main: `Ctrl+Space`, `Alt+Enter` (alternative `Ctrl+J`), `Ctrl+O/S/Y/B/R/G/D/U/C`. `Shift+Enter` is sent to the pane as `\r`.
- Windows Terminal: `Ctrl+Space` can arrive as a plain space on some keyboard layouts before Terminal 1.20 (microsoft/terminal#16641).
- README: platform list at l.60-64 ("Windows: not supported"); key reference and 1.0 table at l.146, l.158-187.
- All clipboard writes already funnel through one effect (`SetClipboard`) flushed by the runtime.

## Correction

`Alt+Up` and `Alt+Down` ARE bound, but only on the tracker branch `origin/feat/agent-orchestrator` (orchestrator worker selection, slice S3c), not on main. The terminal checklist must include them.

## Options considered

- A) Startup prefix check with a visible notice.
- B) Clipboard fallbacks with a visible notice.
- C) Per-terminal records `docs/terminals/<terminal>/<version>-<date>.md` with one shared checklist.
- D) WSL support: `clip.exe`, launcher path; optional additive prefix alias (default unchanged, respects the 1.0 freeze).

Recommendation: all four, in small slices.

## Premises agreed (authoritative)

Agreed with the user on 2026-11-01. They override the exploration where they differ.

1. Prefix: `Ctrl+Space` stays the default. Add a startup check with a visible notice when it does not arrive, plus a second FIXED prefix alias. This is additive, so the 1.0 table still holds (the prefix is not configurable).
2. Clipboard: OSC 52 first, plus an automatic fallback (`wl-copy` / `xclip` / `xsel` / `clip.exe`) when one exists. Always show a status notice saying how the copy was done, or that it failed.
3. Launch is one step. lazarobox always needs a host terminal, so the launcher hides it. WSL reuses the Linux launcher path (to verify in design: does WSLg expose Linux `.desktop` entries in the Windows Start menu?). A Windows `.exe` launcher is out of scope (later).
4. Support: the README lists as supported only terminals that have a record in `docs/terminals/<terminal>/<version>-<date>.md`. Others are "untested" and block nothing. The records replace the Kitty-only manual passes P01/P02/T52 (`openspec/changes/agent-orchestrator/manual-tests.md` on `origin/feat/agent-orchestrator`) and agent-tools tasks 1b.11 and 2e-3.13.
