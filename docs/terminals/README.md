# Terminal records

lazarobox runs in any terminal that sends standard keys, but a terminal counts
as **supported** only when this folder holds a record for it: a filled copy of
the checklist below, with a date, a version and a tester. No record means
"untested": it may work, and nothing here promises it.

A record is a snapshot. A new terminal version or a new lazarobox version can
change a result, so each record names both.

## Index of records

| Terminal | Status | Record |
| :-- | :-- | :-- |
| Kitty | record pending (template ready) | [kitty/](kitty/README.md) |
| Windows Terminal (WSL) | pending, not yet in `RECORDED_TERMINALS` | [windows-terminal/](windows-terminal/README.md) |
| WezTerm | deferred, no record yet | - |

Only a terminal listed in `RECORDED_TERMINALS` (`src/core/desktop.rs`) is
reported as "has a record" by `install-desktop`. Adding a terminal means a
folder here, a filled record, and that constant, in the same change.

## Record header

Copy this table to the top of the record and fill it in.

| Field | Value |
| :-- | :-- |
| Terminal | |
| Terminal version | |
| OS | |
| WSL (yes/no, distro) | |
| Multiplexer (none, tmux, zellij) | |
| Keyboard layout | |
| lazarobox version | |
| Date | |
| Tester | |

## Checklist

Result is `pass`, `fail`, `n.a.` or `pending`. Put what you saw in the note,
above all for every `fail`.

| Item | Result | Note |
| :-- | :-- | :-- |
| `Ctrl+Space` prefix arrives (status bar lists the keys) | pending | |
| `Ctrl+Space` twice sends NUL to the pane | pending | |
| `Ctrl+]` works as the prefix alias, and twice sends 0x1D | pending | |
| First-run prefix probe result (`pass`, `alias` or `missed`) | pending | |
| `Alt+Enter` and `Ctrl+J` insert a newline in chat | pending | |
| `Ctrl+S`, `Ctrl+O`, `Ctrl+Y`, `Ctrl+B`, `Ctrl+R`, `Ctrl+G` reach the app | pending | |
| `Alt+Up` / `Alt+Down` (orchestrator; only once it ships) | pending | |
| Copy (`y`): which notice shows, and does it paste elsewhere (OSC 52 / tool) | pending | |
| Mouse: click, wheel, native selection modifier | pending | |
| Bracketed paste and focus in/out | pending | |
| Truecolor and box-drawing glyphs | pending | |
| Glyph width of the tool mark (once model-marks ships) | pending | |
| `install-desktop` launcher, or the WSL start | pending | |
| `clip.exe` text is UTF-16LE and pastes correctly (WSL only) | pending | |

## Adding a record

1. Create `docs/terminals/<terminal>/` with a `README.md` that links the
   records, and one file per run named `<version>-<date>.md`.
2. Paste the header and the checklist, then fill every row. Do not leave a
   row blank: use `n.a.` with a reason.
3. Update the index above.
4. Add the terminal name to `RECORDED_TERMINALS` in the same change.
