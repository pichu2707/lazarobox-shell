# Demo of lazarobox's chat asking Search Console through the MCP server named
# gsc. The server is gsc-demo-server.py, next to this tape: a stand-in that
# returns INVENTED data for the fictional property sc-domain:neon-ramen.example,
# so no real Google account or result appears. Each tool call waits for `y`.
# Needs the demo folder and launchers from ~/.cache/lazarobox-gif-demo/
# (isolated XDG config with claude haiku, approval = "run", a [[ai.mcp.servers]]
# entry name = "gsc" running ~/.cache/lazarobox-gif-demo/gsc-demo-server.py,
# and a 0600 secrets.toml with [gsc] GSC_SKIP_OAUTH = "true").
# Render from the repository root: vhs docs/assets/tapes/gsc-chat.tape
Output docs/assets/gsc-chat.gif
Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 600
Set Padding 16
Set Framerate 12
Set TypingSpeed 35ms
Set WaitTimeout 120s

Hide
Type "cp docs/assets/tapes/gsc-demo-server.py ~/.cache/lazarobox-gif-demo/ && rm -rf ~/.cache/lazarobox-gif-demo/data/lazarobox && ~/.cache/lazarobox-gif-demo/start-gsc.sh"
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
Type "¿Cómo va el SEO de neon-ramen.example esta semana? Dame clics y las 5 consultas principales."
Sleep 500ms
Enter
# three read-only calls: list_properties, get_performance_overview and
# get_search_analytics; each one is approved with y
Wait+Screen /y call it/
Sleep 1500ms
Type "y"
Sleep 500ms
Wait+Screen /y call it/
Sleep 1500ms
Type "y"
Sleep 500ms
Wait+Screen /y call it/
Sleep 1500ms
Type "y"
Sleep 500ms
Wait+Screen /usage:/
Sleep 6s
