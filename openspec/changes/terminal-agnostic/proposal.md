# Proposal: Terminal-agnostic lazarobox

## Intent

lazarobox is tested only in Kitty. In other terminals, and in the WSL console, the prefix or the clipboard can fail with no sign. Users then think lazarobox is broken. Make every terminal dependency either work or say clearly that it did not. Replace "tested in Kitty" with per-terminal records.

## Scope

### In Scope
- Clipboard: OSC 52 first, then a local tool when one exists (`wl-copy`, `xclip`, `xsel`, `clip.exe`). Every copy shows a notice naming the path used, or the failure.
- Prefix: `Ctrl+Space` stays the default. Second FIXED prefix alias `Ctrl+]` (0x1D), a 1.x commitment.
- First-run probe: the app asks the user to press `Ctrl+Space`. If another key or nothing arrives within a few seconds, it shows a notice offering `Ctrl+]`. The result is remembered, so the probe runs once.
- Launcher: one-step launch in WSL through the Linux launcher path. Notice text no longer says "tested in Kitty"; it says whether the terminal has a record.
- Records: `docs/terminals/<terminal>/<version>-<date>.md` plus one shared checklist (prefix, alias, `Alt+Enter`, `Ctrl+J`, `Ctrl+O/S/Y/B/R/G/D/U/C`, `Alt+Up/Down` when present, mouse, paste, focus, truecolor, clipboard). Records in this change: Kitty, WezTerm, Windows Terminal (WSL). The user runs them.
- README: supported terminals = terminals with a record; others "untested". WSL replaces "Windows: not supported".

### Out of Scope
- Windows `.exe` launcher (later).
- Configurable prefix (1.0 freeze).
- Environment heuristics for the prefix (`WT_SESSION`, WSL, `TERM_PROGRAM`).
- Mouse, focus or truecolor capability probing; 256-color fallback.
- macOS terminals.
- Rewriting existing `[manual, Kitty]` spec tags.

## Capabilities

### New Capabilities
- `terminal-compatibility`: first-run prefix probe, its notice and remembered result; support policy by records; shared checklist.
- `desktop-launcher`: `install-desktop` behaviour, WSL path, record-aware notice.

### Modified Capabilities
- `modal-input`: "Prefix key" gains the `Ctrl+]` alias (literal send rule decided in spec); Kitty manual scenario moves to the checklist.
- `copy-selection`: "Yank to the clipboard via OSC 52" becomes a fallback chain with a mandatory notice.

## Approach

- One clipboard dispatcher in the runtime, behind the existing `SetClipboard` effect. All callers (COPY, chat copy, feedback, update) gain fallback for free.
- The terminal never confirms OSC 52. So the dispatcher always sends OSC 52, then also runs a local tool when one fits the session. Over SSH it skips local tools.
- Notice text: "Copied (wl-copy)" (tool name varies) when a local tool ran; "Sent to the terminal (OSC 52); if it does not paste, install wl-copy or xclip" when only OSC 52 was sent; a failure notice when the tool fails.
- Alias is one more entry in the data-driven prefix table.
- Probe: a small first-run state with a timeout tick. The result is stored outside `config.toml` (design picks the file).
- Records are docs only.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/runtime.rs` | Modified | Clipboard dispatcher, probe timeout |
| new `src/core/probe.rs` (name in design) | New | Probe state, result storage |
| `src/core/copy.rs`, new `src/core/clipboard.rs` | Modified/New | Tool choice, notice text |
| `src/core/keys.rs`, `src/app.rs` | Modified | Prefix alias |
| `src/core/desktop.rs` | Modified | WSL path, notice |
| `docs/terminals/` | New | Checklist and records |
| `README.md` | Modified | l.60-64, l.93, l.146-187, l.322 |

## Production-line forecast

| Slice | Lines |
|---|---|
| S1 clipboard fallback + notice | 150-220 |
| S2a prefix alias `Ctrl+]` | 30-60 |
| S2b first-run probe + remembered result | 120-200 |
| S3 launcher WSL + notice | 40-100 |
| S4 checklist, three records, README | 0 (docs) |
| Total | 340-580 |

400-line risk: High. Chained PRs are recommended. The probe (S2b) is the main growth.

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Probe misreads a slow user as a missing key | Med | Generous timeout; notice is advice, never blocks; user can rerun it (design) |
| Probe result stale after changing terminal | Med | Design decides the key of the stored result (e.g. per terminal) |
| Local tool fills the remote clipboard over SSH | Med | Skip tools when `SSH_CONNECTION` is set |
| WSLg does not list `.desktop` entries in the Start menu | Med | Verify in design; else document the gap |
| `Ctrl+]` collides with telnet escape or a TUI key | Low | Literal send of the alias to the pane |
| P01/P02/T52 live on the tracker branch | Med | Remove them when that branch merges |

## Rollback Plan

Each slice is a separate PR. Revert in reverse order. Reverting S1 restores OSC 52 only. Reverting S2b removes the probe; the stored result is ignored. Reverting S2a removes the alias. Docs can stay.

## Dependencies

- `origin/feat/agent-orchestrator` for `Alt+Up/Down` and P01/P02/T52.

## Success Criteria

- [ ] Every copy shows a notice naming the path or the failure.
- [ ] On first run where `Ctrl+Space` fails, the user sees a notice offering `Ctrl+]`, and the probe does not run again.
- [ ] `Ctrl+]` enters PREFIX in every recorded terminal.
- [ ] WSL user launches lazarobox in one step.
- [ ] README lists only terminals with records.
- [ ] Kitty, WezTerm and Windows Terminal (WSL) have records.

## Decisions

Answered by the user on 2026-11-01:

1. Alias: `Ctrl+]` (0x1D). It is a 1.x commitment.
2. Detection: first-run probe only. The app asks for `Ctrl+Space`; if another key or nothing arrives within a few seconds, it shows a notice and offers `Ctrl+]`. The result is remembered. No environment heuristic.
3. Copy notice on every copy, honest: "Copied (wl-copy)" when a local tool ran; "Sent to the terminal (OSC 52); if it does not paste, install wl-copy or xclip" when only OSC 52 was sent. Local tools are skipped when `SSH_CONNECTION` is set.
4. Records in this change: Kitty, WezTerm, Windows Terminal (WSL). The user runs them.
