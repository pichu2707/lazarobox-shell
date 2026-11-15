---
name: run-lazarobox
description: Launch and drive the real lazarobox TUI in an isolated tmux session for a manual or live check of a change. Trigger when asked to run, launch, start, drive or screenshot lazarobox, or to confirm a change on the real binary and not only in tests.
---

# Run lazarobox for a live check

Any AI agent follows this procedure to drive the real binary without touching
the user's setup. Tests prove the code; this proves the screen.

## Hard rules

1. NEVER read or write the user's real `~/.config/lazarobox` or
   `~/.local/share/lazarobox`. Always run with an isolated HOME (below).
2. NEVER signal a lazarobox process you did not start. Run
   `pgrep -a lazarobox` first and kill only your own tmux session.
3. NO real accounts, OAuth, API keys or cloud engines. Never configure the
   user's engines or keys, and never download models. A local `ollama` that is
   already installed may serve as an offline engine; a trivial stdio MCP server
   run with `python3 -I` may stand in for real MCP servers.
4. NEVER use `--force` to bypass the nesting guard; unset `LAZAROBOX` instead.
5. At the end, delete the isolated dir and kill the tmux session.
6. Report evidence: screen text, diffs, `sha256sum` of files touched.

## Facts that bite

- Binary: `target/debug/lazarobox-shell` (package name, not `lazarobox`).
  Build with `cargo build --locked` from your own worktree.
- Isolated home must NOT be under `/tmp`: the app's jail refuses `/tmp` roots.
  Use `<repo>/.claude/manual-<name>/home` (`.claude/` is git-ignored).
- The config dir must be mode `0700`; a group/other-writable one is refused.
- An inherited `LAZAROBOX=<pid>` makes the app refuse to start (it happens
  when you run inside lazarobox): launch with `env -u LAZAROBOX -u TMUX`.
- The inner zsh with an empty HOME shows the new-user prompt: press `q`.
- Prefix is Ctrl+Space (`C-Space`); then `m` opens the menu, `c` opens chat.
  In the menu, digits switch tabs (Connectors was `4`). Numbers drift: READ
  the screen before pressing, do not trust them.
- The menu popup is narrow (about 60 columns): a notice longer than that is
  cut with an ellipsis, and that is worth reporting. Startup notices flash in
  the status bar for about 1 s; capture right away.
- Never fail silently: if a key shows no notice, that is a bug worth reporting.

## Launch

```sh
N=my-check; R=$(git rev-parse --show-toplevel)
H=$R/.claude/manual-$N/home
mkdir -p "$H/.config/lazarobox" "$H/.local/share" "$H/.local/state" "$H/.cache"
chmod 700 "$H/.config/lazarobox"
cargo build --locked
pgrep -a lazarobox        # note what is NOT yours
tmux new-session -d -s lzr-$N -x 160 -y 48 -c "$H" \
  "env -u LAZAROBOX -u TMUX HOME=$H XDG_CONFIG_HOME=$H/.config \
   XDG_DATA_HOME=$H/.local/share XDG_STATE_HOME=$H/.local/state \
   XDG_CACHE_HOME=$H/.cache $R/target/debug/lazarobox-shell"
sleep 1; tmux capture-pane -t lzr-$N -p     # startup notices
tmux send-keys -t lzr-$N q                   # only if the zsh prompt shows
```

## Drive and capture

```sh
tmux send-keys -t lzr-$N C-Space m           # menu
sleep 0.5; tmux capture-pane -t lzr-$N -p    # read before the next key
tmux send-keys -t lzr-$N Enter               # keys: Enter, Escape, Down, text
```

Send one step, wait, capture, decide. Quote the captured text as evidence.

## Teardown

```sh
tmux kill-session -t lzr-$N
rm -rf "$R/.claude/manual-$N"
tmux ls 2>/dev/null; ls "$R/.claude" | grep manual-   # both must show nothing of yours
```

Also run `rm -rf target` if your worktree is disposable (see `AGENTS.md`).
