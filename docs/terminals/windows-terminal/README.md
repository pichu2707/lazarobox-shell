# Windows Terminal (WSL)

Record status: **pending**. The test run has not been done yet; nothing below
is a result. This terminal is not in `RECORDED_TERMINALS` until a filled record
exists. Copy the template into `<version>-<date>.md` and fill it in.

## Record template

| Field | Value |
| :-- | :-- |
| Terminal | Windows Terminal |
| Terminal version | |
| OS | Windows |
| WSL (yes/no, distro) | yes, |
| Multiplexer (none, tmux, zellij) | |
| Keyboard layout | |
| lazarobox version | |
| Date | |
| Tester | |

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
| WSL start, or `install-desktop` entry in the Windows Start menu (WSLg) in one step | pending | |
| `clip.exe` text is UTF-16LE and pastes correctly | pending | |
