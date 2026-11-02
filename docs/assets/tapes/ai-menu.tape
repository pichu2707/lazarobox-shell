# The AI menu: open it (Ctrl+Space m), jump to the Models tab, browse the task
# rows, open a task's engine and model picker, then leave without saving.
# Nothing is chosen, saved, downloaded or installed.
# Needs the demo folder and launchers from ~/.cache/lazarobox-gif-demo/
# Render from the repository root: vhs docs/assets/tapes/ai-menu.tape
Output docs/assets/ai-menu.gif
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
# open the menu: Ctrl+Space m
Ctrl+Space
Sleep 400ms
Type "m"
Sleep 2s
# Models tab
Type "2"
Sleep 2s
# browse the rows
Type@500ms "jjjjjj"
Sleep 1s
Type@500ms "kkkkkk"
Sleep 1s
# chat task row: open the engine picker
Type@400ms "jjj"
Sleep 800ms
Enter
Sleep 2s
Type@500ms "jjj"
Sleep 800ms
Type@500ms "kkk"
Sleep 800ms
# open the models of the current engine
Enter
Sleep 2s
Type@500ms "jj"
Sleep 800ms
Type@500ms "kk"
Sleep 1s
# back out level by level and close without saving
Escape
Sleep 1s
Escape
Sleep 1s
Escape
Sleep 2s
