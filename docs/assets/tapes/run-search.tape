# Demo of lazarobox's chat in approval = "run" mode (rg runs on its own inside the chat's folder).
# Needs the demo folder and launcher from ~/.cache/lazarobox-gif-demo/
# (isolated XDG config with [ai.tools] approval = "run" and claude haiku).
# Render from the repository root: vhs docs/assets/tapes/run-search.tape
Output docs/assets/run-search.gif
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
Type "Run `rg -n TODO` in the pane and tell me how many TODOs there are."
Sleep 500ms
Enter
Sleep 30s
