# Conversation history: a short chat (with a note saved to project memory),
# close the chat, reopen it and resume the saved conversation from the list.
# Needs ~/.cache/lazarobox-gif-demo/start-history.sh (isolated config and
# data, a git demo project) and an isolated engram server, started first:
#   ENGRAM_DATA_DIR=~/.cache/lazarobox-gif-demo/engram engram serve 7438
# The real engram store (port 7437, ~/.engram) is never touched.
# Render from the repository root: vhs docs/assets/tapes/history.tape
Output docs/assets/history.gif
Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 600
Set Padding 16
Set Framerate 12
Set TypingSpeed 35ms

Hide
Type "rm -rf ~/.cache/lazarobox-gif-demo/data/lazarobox && ~/.cache/lazarobox-gif-demo/start-history.sh"
Enter
Sleep 3s
Show

Sleep 1s
# a short chat: Ctrl+Space c, then n for a new conversation
Ctrl+Space
Sleep 400ms
Type "c"
Sleep 1500ms
Type "n"
Sleep 1500ms
Type "Suggest one short name for a method that removes stock from an inventory."
Sleep 500ms
Enter
Sleep 10s

# save the answer as a note in project memory: Ctrl+Space n, a title, Ctrl+S
Ctrl+Space
Sleep 400ms
Type "n"
Sleep 1500ms
Type "Name for removing stock"
Sleep 800ms
Ctrl+S
Sleep 2s

# close the chat pane: Ctrl+Space w q, then y
Ctrl+Space
Sleep 400ms
Type "w"
Sleep 400ms
Type "q"
Sleep 1200ms
Type "y"
Sleep 1500ms

# reopen the chat: the list shows the saved conversation; Enter resumes it
Ctrl+Space
Sleep 400ms
Type "c"
Sleep 3s
Enter
Sleep 4s
