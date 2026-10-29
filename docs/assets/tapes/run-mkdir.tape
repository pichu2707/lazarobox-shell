# Demo of lazarobox's chat in approval = "run" mode (mkdir asks, y only types it, you press Enter).
# Needs the demo folder and launcher from ~/.cache/lazarobox-gif-demo/
# (isolated XDG config with [ai.tools] approval = "run" and claude haiku).
# Render from the repository root: vhs docs/assets/tapes/run-mkdir.tape
Output docs/assets/run-mkdir.gif
Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 600
Set Padding 16
Set Framerate 12
Set TypingSpeed 35ms

Hide
Type "rm -rf ~/.cache/lazarobox-gif-demo/data/lazarobox && ~/.cache/lazarobox-gif-demo/start.sh"
Enter
Sleep 3s
Show

Sleep 1s
Ctrl+Space
Sleep 300ms
Type "c"
Sleep 1500ms
Type "n"
Sleep 1500ms
Type "Run `mkdir docs` in the pane."
Sleep 500ms
Enter
Sleep 15s
Type "y"
Sleep 4s
# Move the focus to the shell pane and run the typed command yourself.
Ctrl+Space
Sleep 300ms
Type "h"
Sleep 1500ms
Enter
Sleep 1500ms
Type "ls"
Sleep 500ms
Enter
Sleep 4s
