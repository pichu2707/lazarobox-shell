# COPY mode in a shell pane: enter COPY mode (Ctrl+Space [), move, select
# whole lines with V and copy with y; the status line reports the copy.
# The mouse wheel is not shown: vhs ScrollUp scrolls xterm.js's own buffer
# (term.scrollLines), it sends no wheel event to lazarobox. The paste is not
# shown either: OSC 52 does not reach a clipboard that vhs can paste from.
# Needs the demo folder and launchers from ~/.cache/lazarobox-gif-demo/
# Render from the repository root: vhs docs/assets/tapes/copy-mode.tape
Output docs/assets/copy-mode.gif
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
Type "seq -f 'build step %g ok' 1 30"
Enter
Sleep 1s
Type "cat src/inventory.py"
Enter
Sleep 1500ms

# COPY mode: Ctrl+Space [
Ctrl+Space
Sleep 400ms
Type "["
Sleep 1s
Type@250ms "kkkkkkk"
Sleep 600ms
Type "V"
Sleep 600ms
Type@300ms "jj"
Sleep 1s
Type "y"
Sleep 3s
