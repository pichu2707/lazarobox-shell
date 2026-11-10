# Overview of lazarobox: split panes, move the focus, run a command in each
# pane, then ask the AI chat one short question.
# Needs the demo folder and launchers from ~/.cache/lazarobox-gif-demo/
# (isolated XDG config with claude haiku as the chat engine).
# Render from the repository root: vhs docs/assets/tapes/overview.tape
Output docs/assets/overview.gif
Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 600
Set Padding 16
Set Framerate 12
Set TypingSpeed 35ms

Hide
Type "rm -rf ~/.cache/lazarobox-gif-demo/data/lazarobox && ~/.cache/lazarobox-gif-demo/start-main.sh"
Enter
Sleep 3s
Show

Sleep 1s
Type "ls src"
Enter
Sleep 1500ms

# split below: Ctrl+Space w h
Ctrl+Space
Sleep 400ms
Type "w"
Sleep 400ms
Type "h"
Sleep 1500ms
Type "wc -l src/*.py"
Enter
Sleep 1500ms

# split right: Ctrl+Space w v
Ctrl+Space
Sleep 400ms
Type "w"
Sleep 400ms
Type "v"
Sleep 1500ms
Type "git --version"
Enter
Sleep 1500ms

# move the focus up to the first pane: Ctrl+Space k
Ctrl+Space
Sleep 400ms
Type "k"
Sleep 1s
Type "head -3 README.md"
Enter
Sleep 1500ms

# open the AI chat beside it: Ctrl+Space c, then n for a new conversation
Ctrl+Space
Sleep 400ms
Type "c"
Sleep 1500ms
Type "n"
Sleep 1500ms
Type "In one short sentence: what is a TUI?"
Sleep 500ms
Enter
Sleep 12s
