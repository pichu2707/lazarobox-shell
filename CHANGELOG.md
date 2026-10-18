# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

0.8.0 is the first version published to crates.io. 0.7.0 is an unpublished
baseline.

## [Unreleased]

### Fixed

- The update now installs exactly the version the **Updates** row announces
  (`cargo install lazarobox-shell --locked --version <version> --root <root>`).
  It used to install whatever was newest on crates.io, so when that was the
  version already installed cargo did nothing, exited 0 and lazarobox still
  said "Updated to X". Now the install is that version or a clear failure. A
  version that is not plain `major.minor.patch` is never passed to cargo and
  the command to run by hand is shown instead. The command shown for copying
  is unchanged: it asks for the newest version.

## [0.16.1] - 2026-10-18

### Fixed

- Enter on **Updates** did nothing visible when `~/.cargo` and `~/.cargo/bin`
  are group-writable (mode 0775, the default with umask 002 on Debian and
  Parrot). The update install refused that `cargo` with the strict rule meant
  for the AI CLIs and fell back to a footer that was cut off. The install now
  accepts a `cargo` whose folders are group-writable only when the folder is
  yours and your group is private to you (named like your user, with no other
  member). A shared group (`users`, `staff`) and root-owned folders that the
  group can write are refused, and so is anything others can write or anything
  under `/tmp`. The folders that hold the link to `cargo` (such as
  `~/.cargo/bin`) are checked too, and the check runs again when you press
  Enter. The AI CLI jail keeps its strict rule.

### Changed

- The update texts are in plain words. The menu note says which version is
  available and to go to **Updates**; when lazarobox cannot update itself it
  says so, shows the command to copy and the reason on the footer (installed
  another way, Rust is not installed, Rust folder is too open, or an unusual
  folder name). Without Rust, the note says to install it first (rustup.rs).
  Enter on **Updates** closes the menu, as Esc does, so focus lands on the
  update tab. The update tab starts by saying that it can take a few minutes
  and that lazarobox must stay open, and ends with
  `Updated to X.Y.Z. Restart lazarobox to use it.` A failure says what
  happened and gives the command to run by hand.
- README: short install (with the link to rustup) and update instructions.

## [0.16.0] - 2026-10-17

Agent tools: the chat can now ask lazarobox to look at the user's files and to
use the user's terminal panes. Tools are active on claude, codex, gemini and
Ollama, and each engine was verified manually before this release. They are on
by default, also for existing users who upgrade (see **Changed**).

### Added

- Run a command from the chat. When the last finished answer of a chat ends
  with a shell block (`bash`, `sh`, `shell` or `zsh`), a hint under the block
  names `Ctrl+Space`, then `e`. It opens a confirmation over the composer
  (`run in pane N (<cwd>)`, the exact command and the keys): `y` types the
  command into the target pane and never presses Enter, `n` or `Esc` declines,
  `p` moves the target to the next shell pane of the tab, and `Ctrl+C` cancels
  the answer and declines. The target is the shell pane of the current tab used
  last. Commands with control characters, escape sequences, invisible or
  spoofing characters, or non-ASCII whitespace are refused, never cleaned; a
  multi-line command needs bracketed paste; a pane running a full-screen app
  is refused. It works with every engine because it reads the block of the
  answer.
- Built-in agent tools: `find_dir`, `list_dir`, `read_file` and `run_in_pane`,
  with strict argument parsing (4096 bytes at most per argument). The read
  tools run on the blocking pool with a time limit and cancellation, wrap what
  they return in an untrusted-output block with a per-call nonce (32 KiB cap),
  and redact secrets before the model sees them. `read_file` opens below
  `$HOME` without following symlinks (`openat2`, with a per-component fallback)
  and refuses hard-linked and special files; `list_dir` and `find_dir` leave
  out names that look like secrets and say so.
- Path policy for the read tools. Anything outside `$HOME` is refused, and so
  are the secret roots: `~/.ssh`, `~/.gnupg`, the credential folders of other
  CLIs, `~/.password-store`, `~/.config/gh`, `~/.config/gcloud`, the keyrings,
  lazarobox's own config and data folders (they follow `XDG_CONFIG_HOME` and
  `XDG_DATA_HOME`), and at any depth `.env`, `.env.*`, `.envrc`, `.netrc`,
  `.git-credentials`, `.pgpass`, `.npmrc` and the shell and REPL histories. A
  path is judged as requested and as resolved, so a link to a secret folder
  does not get around it. Every call fails closed when `$HOME` or a secret
  root cannot be read. The list is best effort and is documented as such.
- MCP for the CLI engines. A hand-rolled MCP JSON-RPC core, a per-chat unix
  socket session behind a token, and a stdio shim that relays a chat's MCP
  lines to that socket. The shim is the only extra binary the jail grants,
  together with one socket, and it sends its token only to a private socket
  owned by the user. A tool session lives for one turn and ends at once when a
  turn falls back to text.
- Per-engine wiring, active and verified on each engine: claude (a per-turn MCP
  config, `--allowedTools=mcp__lazarobox`, an empty `--setting-sources`),
  codex (the server through `-c` overrides and its own environment) and
  gemini (a per-turn work folder with an admin policy). Each decoder lets
  through only the lazarobox tool calls, and a failed wiring falls back to a
  text-only turn. A turn with tools tells the model it has them, only on the
  attempt that is wired. The per-turn CLI files are refused to `read_file`.
- Tool rows in the transcript for each call and result, and an approval
  overlay answered per pane, turn and call. Reads from a cloud engine ask
  first with the resolved path, because what a tool returns goes to the
  provider; Ollama is local and reads without asking. A prompt the model opens
  ignores `y`, `n` and `Esc` for a short arming delay, and the timers of a
  turn pause while a call waits for the user. Turning tools off ends the tool
  session of every chat that has one.
- Approval modes `ask`, `auto` and `run`. `ask` (the default) needs a `y` for
  every command. In `auto` a model's `run_in_pane` is typed into the target
  pane without a prompt, but Enter is still never pressed. `Ctrl+Space`, then
  `a` cycles the mode of the focused chat, and a system line says which one is
  on. A chat in `auto` wears an amber ` AUTO ` badge and one in `run` a red
  ` RUN ` badge. `approval = "ask" | "auto" | "run"` in `[ai.tools]` sets the
  starting mode; anything else falls back to `ask` with a notice. The mode
  lasts for the conversation. Auto-typing needs the shell in the foreground of
  an unfocused, visible pane with no earlier unsent auto-typed line; otherwise
  it falls back to the normal prompt, and so it does after five commands in a
  turn.
- `run` mode (auto-execute). For a single command from a short safe list
  (`cd`, `pwd`, `ls`, `tree`, `mkdir`) lazarobox types the command, presses
  Enter, waits for it to settle and returns the output to the model. Words
  accept Latin letters (accented too), digits and `. _ / @ % + : , = ~ -`;
  paths may be unquoted or inside single or double quotes. `&&`, `;`, `|`, `$`,
  redirections, globs and long options are not accepted and use the normal
  prompt. At most 5 commands run in a turn, and three consecutive refusals by
  grammar or path pause `run` until the turn ends. The confirmation of a
  command that did not run directly shows a reason line with the piece that
  caused it, with invisible characters drawn as `?`. The model is told which
  commands run directly and how to ask for them.
- Output of a run goes back to the model as the screen text left after the
  command: 8 KiB and 200 rows at most (head and tail, with the cut marked),
  cleaned, redacted and wrapped like the output of the read tools. A command
  that does not end in 10 s returns `still running` with what is visible. A
  `cd` also reports the new directory in a system line. The result never
  includes the exit code, and a shell that rewrites the typed line (fish
  abbreviations) gives `output not available`.
- `[ai.tools] enabled` (default on) and `approval` in `config.toml`, with a
  **Tools** row and an **Approval** row in the AI menu. The menu writes only
  those two keys and never touches `[ai.mcp.*]`.
- Tools with Ollama. Because Ollama has no CLI that runs tools, lazarobox runs
  the tool loop itself over `/api/chat`: it sends the tools in the `tools`
  field, runs the model's calls through the same registry, approvals and modes
  as the CLIs, and sends the results in a new request of the same turn. At
  most 8 requests per turn; past that the turn ends with a system line and
  keeps the partial answer. A model that does not support tools (a 400 on the
  request with `tools`) is retried once without them, before any round. Tool
  messages live only inside their turn. The loop is active and verified with
  a real model.

### Changed

- Tools are on by default (`[ai.tools] enabled = true`), so an upgrade turns
  them on for existing users. Turn them off with the **Tools** row of the AI
  menu or with `[ai.tools] enabled = false`.
- When the AI section is shown, the config menu groups its rows into two
  tabs, **Settings** and **AI**, switched with `Tab` and `Shift+Tab`. `j` and
  `k` wrap inside the active tab and each tab remembers its row.

### Fixed

- The Gemini workspace trust: the folder is now trusted by environment
  variable and the policy rule carries a `toolName`, so gemini loads the
  lazarobox tools. The env, context, hooks and skills a trusted folder would
  load are switched off, and the inherited trust variable is scrubbed.
- A zoomed tab, an update pane, a half-typed line and a paste that does not
  run the line no longer let an auto-typed command land in the wrong place:
  these cases prompt or clear the unsent mark as appropriate.
- Approvals carry an id drawn from a process-wide counter, so a stale answer
  or withdraw is ignored, and a shell block is offered only from a finished,
  successful answer of the last turn.
- A dispatch that panics no longer leaves the terminal broken, and a client
  that half-closes the MCP connection still gets the replies of its calls in
  flight.

### Security

- Tool output and everything the model can read passes the same cleaning and
  redaction; secrets are removed even when they sit across the read cap or at
  the cut edge of a window.
- Commands are refused, not cleaned, when they contain invisible, bidi or
  spoofing characters, and the text of a model that opens a continuation
  (unclosed quote, `$(`, heredoc, trailing `\`) is refused in `run`.
- The MCP session token is never printed (not in `Debug`, not in the stderr
  tail of a failed CLI), `mcp.json` is wiped after a wired turn, and the
  shim token goes only to a private socket of ours.
- In `run` the pane output is sent to the model, also with cloud engines; in
  `ask` and `auto` the model receives none of it. `cd` and `ls` may go
  outside `$HOME` (secret folders stay refused, also through symlinks) and
  expose only names and `ls -l` metadata; `tree` and `mkdir` stay inside it.
- Residual risks of `auto` and `run` (typing after a half-typed line, `cd`
  hooks, `CDPATH`, the path-check window, PS2 continuation, prompt lines in
  the output) are listed in the README.

## [0.15.4] - 2026-10-17

### Added

- Updates row in the Settings section of the menu. It shows the running
  version (`Updates  0.15.3`); when the update check knows a newer one the
  name carries a `*` and the value reads `0.15.4 available`. Enter acts only
  when an update is available. If the running binary (canonicalized) is under
  `$CARGO_HOME/bin` (default `~/.cargo/bin`), it opens a new tab that runs
  `cargo install lazarobox-shell --locked` directly as the pane's program (no
  shell), with `cargo` resolved on the login `PATH` and run from `$HOME`. On
  exit 0 the tab closes, a notice says `lazarobox X.Y.Z installed: restart
  lazarobox to use it` and the row reads `X.Y.Z installed: restart`. On a
  failure the tab stays open with `update failed (exit N)` so the error can
  be read. Any other install runs nothing: the footer shows the command, and
  Enter or `y` copies it (OSC 52). `[updates] check` stays config-only.
- Spawning a pane with a program and its arguments, and an exit-status event
  (`PtyEvent::Status`) that is sent before `Exited` for panes that ask for it.

### Changed

- The Notify and Sound rows moved from the AI section to Settings (after
  Mouse). Their behaviour (h/l cycling, `t` test and its footer report,
  saving) and the config keys `[ai] notify` and `[ai] notify_sound` are
  unchanged.
- The update check no longer waits for 24 h when the cache was written by
  another lazarobox version and its latest version is not newer than the
  running one (for example right after an upgrade): the next start checks at
  once. The cache now records the checking version (`checked_by`; an old file
  without it counts as another version). A cache written by the running
  version keeps the once-a-day rule, as do a newer cached version and the
  one-hour retry after a failure.
- The update pane trusts its `cargo` like the AI CLIs are trusted. It prefers
  the `cargo` beside the running binary (`<cargo bin dir>/cargo`), else the
  first on the login `PATH`, kept as found for the rustup proxy; one under
  `/tmp`, or writable by group or others (or in such a folder), is refused and
  the command is shown instead. The pane's first line names what runs, a
  non-UTF-8 `PATH` is passed through unchanged, and the install gets
  `--root <parent of the cargo bin dir>` so it always replaces the running
  binary. A symlinked `~/.cargo` is compared by where it points. A child that
  ends without an exit status is a failure (`update failed (no exit status)`)
  and its pane stays open.
- The menu has one more row (Updates), so the smallest full popup is now 8
  rows high (it was 7); below that the one-line fallback is shown.
- The install command shown by the menu note, the statusline hint and the
  footer is `cargo install lazarobox-shell --locked` everywhere.

## [0.15.3] - 2026-10-16

### Added

- Desktop notification for finished chat turns, independent of the outer
  terminal. `[ai] notify` now takes `"auto"`, `"desktop"`, `"bell"` or
  `"off"` (the Notify row of the AI menu cycles them). `"auto"` always rings
  the bell and also sends a desktop notification when `DISPLAY` or
  `WAYLAND_DISPLAY` is set and `notify-send` is on the login `PATH`.
  `"desktop"` sends only the notification and rings the bell if it is
  unavailable or fails. `notify-send` runs without a shell, detached, with a
  2 s limit; its title is `lazarobox` and its text is a fixed line naming the
  pane, never any of the answer. The old booleans keep working (`true` is
  `"auto"`, `false` is `"off"`), and the default stays off.
- The desktop notification is hardened. The child gets an empty environment
  plus an allow-list (`PATH`, `HOME`, `LANG`, `LC_*`, the session bus and
  display variables), never secrets. A `notify-send` under `/tmp` or writable
  by group or others is skipped, like the CLI interpreters. The availability
  check runs off the UI thread. The desktop counts as unavailable over SSH or
  without a display and a session bus. At most one send is in flight, so a
  burst of finished turns is coalesced (`"auto"` still rings the bell for
  each).
- lazarobox plays its own sound with the notification in `"auto"` and
  `"desktop"` (`[ai] notify_sound`, default true, and a Sound row in the AI
  menu): `canberra-gtk-play -i message-new-instant`, else `pw-play` or
  `paplay` with the first of the freedesktop `message-new-instant.oga` and
  `complete.oga`. It follows the same rules as `notify-send` and does
  nothing when no player is found.
- `t` on the Notify or Sound row of the AI menu fires one test notification
  now, with the mode shown (saved or not) and ignoring the focus rule. The
  footer says what happened (`sent: bell + desktop + sound`) or why not
  (`desktop unavailable: no session bus`).
- How the notification machinery works, and its guard rails. The machine is
  probed once at startup, in the background, and again by the `t` test: the
  session (display, session bus, not over SSH), the trusted `notify-send` and
  the sound players. No send ever waits for a probe; if the result is not
  there yet, or the probe failed, the desktop and the sound are unavailable
  for that event and `desktop` mode rings the bell. A tool installed after
  startup is seen after `t` or a restart. Each probe takes a ticket and a
  slow or old probe never overwrites a newer plan; a probe that panics is
  caught (without tearing the terminal down) and recorded as failed, and `t`
  reports "the check failed" and probes again. Just before an exec the real path
  is re-checked (exists, not under `/tmp`, not group- or world-writable),
  and that is the path that runs, with the found name as `argv[0]`. The
  child gets an empty environment plus an allow-list (`PATH`, `HOME`, `USER`,
  `LOGNAME`, `LANG`, `LANGUAGE`, `LC_*`, the session bus, display, audio and
  XDG variables), never secrets. SSH only disables the desktop when no local
  display is set, so a stale `SSH_CONNECTION` in a local tmux does not.
  Limits: 2 s per program (a program still running is killed and counts as
  failed), 4 s per send, 12 s for the sound chain, 3 s for the test probe.
  At most one send, one sound and one test run at a time; a send dropped
  because another is running rings the bell in `desktop` mode, as does an
  unavailable, untrusted or failed one, even if the send task dies. The
  sound tries canberra, then `pw-play` and `paplay` with each existing sound
  file; only an exit 0 within the limit is success, anything else moves to
  the next candidate, and the test report names the player that worked or
  why all failed. The test report lives in its own footer field until the
  next key press and is sent after its in-flight flag is cleared. In
  `desktop` mode a missing `notify-send` rings the bell and the sound may
  play too: they overlap by design. A turn that ended by cancel never
  notifies; its fallback body would read "Turn ended in pane N". The
  runtime is held by a guard that bounds its shutdown to 1 s on every way
  out, panics included.

## [0.15.2] - 2026-10-16

### Added

- The AI picker now says why an engine is degraded, and lets you copy it. A
  codex model list that could not be read shows `model list unavailable:
  <reason>` wrapped over up to four rows instead of one clipped row. Move onto
  the note and press Enter or `y` to copy the whole, sanitized reason through
  OSC 52 (the usual copied notice appears). The note is never a model choice
  and never changes the saved model. On the engine list, `y` on an engine
  that runs without the jail copies its full `no jail` reason; Enter still
  picks the engine.
- A background check for a newer version, made with the system `curl` at most
  once a day. When one exists, a text mark shows in the statusline and a note
  in the menu. Nothing is sent but one `GET` to crates.io; with no `curl` it
  does nothing.
- Diagnostics for the AI CLIs: the engine list shows why a CLI runs without
  the jail (`available · no jail: <reason>`), a failed codex model listing
  says why, and a failed chat turn shows the CLI's last stderr line with
  credentials redacted. The suggested `chmod` command ends its options with
  `--`.

### Fixed

- The login-shell PATH lookup works from a launcher again. The shell was
  started with the launcher's bare PATH, so rc files that call `tr` or
  `readlink` (`brew shellenv`, nvm) failed and linuxbrew and nvm never reached
  the merged PATH. The probe shell now starts with the process PATH plus
  `/usr/local/bin:/usr/bin:/bin`. If `$SHELL` gives nothing, `/bin/bash -l -c`
  is tried once (not when `$SHELL` is bash already; it reads the login files
  but not `~/.bashrc`, so a PATH set only there is not seen). Both share one
  3 s deadline, reaping included, instead of 2.5 s for the first alone.
- The login-shell entries now come first in the merged PATH, then the process
  entries it lacks, so the launcher picks the program your terminal picks. A
  stale `/usr/local/bin/codex` no longer hides the one in linuxbrew.
- An empty PATH, or one with only relative entries, no longer reaches the
  current directory: relative and empty entries are dropped everywhere, and
  when nothing absolute is left the PATH is `/usr/local/bin:/usr/bin:/bin`.
- A failed chat turn's error line is sanitized before secrets are redacted,
  so a zero-width character cannot split a key out of the redaction, and an
  error code such as `context_length_exceeded` is no longer taken for a
  secret.

## [0.15.1] - 2026-10-16

### Fixed

- Started from a desktop launcher or rofi, lazarobox inherited a bare PATH, so
  `claude`, `gemini` and `codex` were not detected. lazarobox now reads the
  login-shell PATH once at startup, with a bounded 2.5 s probe. If the probe
  fails or times out, it falls back to the process PATH. The probe runs your
  interactive rc files. The merged PATH is used for detection, `--version`,
  chat turns, the codex model listing and the engram server.

## [0.15.0] - 2026-10-15

### Added

- The codex model list now comes from your account. At detection time
  lazarobox runs `codex debug models` once (4 s timeout, 2 MiB output cap,
  stdin closed, private working folder, scrubbed environment, next to the
  version probe) through the same jail as the codex chat. Like running codex,
  that command may refresh the catalog from OpenAI with your login and update
  codex's own cache (`models_cache.json`). Without the jail it is not run. The
  picker offers `default (CLI picks)` followed by the models the account
  lists, with their display names as labels and the slugs as saved values. An
  older codex without the subcommand, or any failure, shows only `default`.
  The listing is kept in memory and refreshed at most every 10 minutes; the
  menu's `r` forces a new one. A failed refresh keeps the cached list while it
  is under 10 minutes old.
- A codex model that is assigned or typed in `custom…` but is not in the
  account's list is marked `not in your account's list` in the picker, on the
  task row and in the footer. It is a warning only.
- The menu footer shows `unsaved changes · s to save · Esc discards` while the
  AI draft differs from the saved config.

## [0.14.0] - 2026-10-15

### Added

- Nesting guard, modelled on tmux's `$TMUX`: every shell pane child gets
  `LAZAROBOX=<pid of the instance>`. Starting `lazarobox-shell` with that
  variable set (and non-empty) prints `lazarobox-shell is already running in
  this terminal; use --force to nest anyway` to stderr and exits 1, before the
  terminal enters raw mode. `--force` (a top-level flag) skips the check.
  Subcommands, such as the internal `__jail-exec`, are never blocked.
- `lazarobox-shell install-desktop [--terminal <cmd>] [--uninstall]` installs
  the desktop launcher from the binary itself, so it works after
  `cargo install lazarobox-shell` and with any terminal. The terminal is
  `--terminal`, then `$TERMINAL`, then `xdg-terminal-exec`, then the first of
  kitty, ghostty, wezterm, alacritty, foot, gnome-terminal, konsole,
  xfce4-terminal and x-terminal-emulator in `PATH`, else `Terminal=true`. An
  unknown terminal is launched as `<cmd> -e <bin>`, and any terminal other
  than Kitty prints a compatibility notice.

### Changed

- `scripts/install-desktop.sh` is now a thin wrapper around the subcommand and
  accepts the same options. The launcher template moved from
  `packaging/lazarobox.desktop` into the binary.
- AI model picker: CLI models now show readable labels while the saved value
  stays the CLI value, so `config.toml` is unchanged. claude lists `default
  (CLI picks)`, `Opus 5.5`, `Sonnet 5.5`, `Haiku 5.5` and `Fable 5.1` (the
  CLI aliases `opus`, `sonnet`, `haiku` and `fable`). gemini lists `default
  (auto)`, `Flash 3.8`, `Flash 3.5` and `Flash Lite 3.5` as explicit ids
  (`gemini-3.8-flash`, `gemini-3.5-flash`, `gemini-3.5-flash-lite`)
  instead of the CLI aliases, which may point at retired 2.5 models. A saved
  value outside the list, such as `gemini-2.5-flash`, still loads and is shown
  as a custom value. Task rows and the chat statusline keep showing the saved
  value (`claude › opus`).

### Fixed

- AI chat with the `gemini` CLI no longer ends with `the engine stopped
  answering` when the model is slow. gemini prints its first line at once and
  can then stay silent for a minute or more while it retries a busy model
  (HTTP 503), which tripped the 90 s idle limit. A CLI turn now waits up to
  300 s between outputs (the 15 minute total limit is unchanged; servers keep
  their 60 s), and after 10 s of silence the pane shows `still waiting for the model…`
  instead of a silent spinner.

## [0.13.0] - 2026-10-14

### Added

- Chat memory: deleting a conversation from the list (`d`) can also delete its
  engram notes. With memory on, `ENGRAM_HTTP_TOKEN` set and at least one saved
  note, the prompt reads `Delete conversation? y · n · a also delete its N
  notes`: `y` keeps the notes, `a` deletes the conversation and then each note
  (`DELETE /observations/{id}?expected_project=P` with the bearer token), and
  any other key cancels. The result is one line (`deleted N notes from
  engram`, `deleted K of N notes from engram` or `could not delete notes:
  <reason>`). Without a token or with memory off, the notes stay and a line
  says why. Retention eviction never sends a delete.

### Changed

- Chat memory: resuming a conversation reuses the engram session (and project)
  its notes were written under, so new notes land in the same session and it is
  ended once. A resumed conversation that writes nothing ends no session.
- The conversation list carries the number of notes of each conversation.

## [0.12.0] - 2026-10-14

### Added

- AI chat with the `claude`, `codex` and `gemini` CLIs, using your own login
  (no API key is read, stored or asked for). The conversation is sent to the
  CLI's provider.
  - Text only: each CLI starts with fixed options that remove its tools, so it
    cannot run commands or edit files. gemini gets a deny-all tool policy file
    written into the turn's private temp folder, no extensions and no MCP
    servers (its `plan` mode alone can be left by the model).
  - Linux jail (Landlock) for every CLI turn: no access to `$HOME`, `~/.ssh`
    or other CLIs' folders, writes only in the CLI's own state folder and a
    private per-turn temp folder, and only the interpreter and the CLI can be
    executed. codex and gemini do not run without the jail
    (`<engine>: not available on this system yet`); claude does, and says so
    once in the pane.
  - Script CLIs (npm installs) start through their interpreter. A Homebrew node
    gets read-only access to exactly the library folders its own ELF lists and
    to its OpenSSL config and CA bundle.
  - A newer codex or gemini that adds an event kind, or a codex that drops a
    feature flag lazarobox disables, makes the chat fail with an error until
    lazarobox is updated.
  - Tested with claude 2.1.299, codex-cli 0.142.5 and gemini 0.63.0.
- Scrolling for every pane. The mouse wheel scrolls the pane under the pointer
  without moving the focus: three lines per notch in a shell pane's
  scrollback and in the AI chat transcript, and straight to the application
  when it asks for the mouse (`vim`, `htop`) or runs on the alternate screen
  (`less`, `man`, which get arrow keys). Typing in the pane or pasting returns
  it to the live screen.
- COPY mode on AI chat panes (`Ctrl+Space [`): `j`/`k`, `Ctrl+u`/`Ctrl+d`,
  `gg`, `G`, `Esc` or `q` to leave. The streaming answer keeps arriving while
  the view is scrolled.
- A `[+N]` badge on a scrolled pane, a position in the statusline
  (`TERMINAL ↑12/340`) and a key hint in COPY mode.
- The menu `Mouse` row is now a real `off`/`on` setting with a live preview;
  `Esc` reverts it and `s` saves it as the root key `mouse` of `config.toml`.

### Changed

- The jail no longer lets a CLI read `/usr/bin`, `/usr/sbin` or `/bin`: the
  dynamic loader could otherwise run any program there.
- A script CLI's interpreter in a group- or world-writable folder, or under
  `/tmp`, is refused inside the jail.
- The mouse is captured by default (`mouse = true`), but only the wheel is
  reported: hold `Shift` to select text with the terminal. Set `mouse = false`
  to turn the capture off. The terminal is released on quit, on exit of the
  last pane and on panic.

### Removed

- The `config: mouse is not supported yet` notice.

### Security

- Known limits, also in the README: the CLI state folders are writable
  (a configuration written there runs on your next unjailed run), network is
  not restricted, claude rewrites `~/.claude.json` in place under the jail,
  claude's system prompt is visible in `ps`, gemini keeps its sessions in
  `~/.gemini` outside `chat.db`, and `execve` limits are defense in depth.

## [0.11.0] - 2026-10-14

### Added

- Conversation history for the AI chat. Messages are saved to the private
  `chat.db` as you chat. An answer that stops early (an error or a cancel
  after some text arrived) is saved too, marked as partial. The model's
  reasoning is never stored.
- Conversation list: `Ctrl+Space c` now opens the list instead of an empty
  chat. It shows the conversations of the current project (the git
  repository), newest first, as `date · profile · first message`.
  - `a` shows or hides the conversations of all projects.
  - `n` or `Esc` starts a new conversation.
  - `Enter` resumes the selected one: the full history is loaded, new
    messages are appended to the same conversation, and its stored profile is
    used.
  - `d` then `y` deletes it (any other key cancels). A pane that has the
    deleted conversation open stops saving and says so.
- Tags: each conversation has a project tag plus up to 8 extra tags. Edit them
  with `t` on a list row or with `Ctrl+Space T` inside a conversation (one
  comma-separated line; the project tag comes first; `Enter` saves, `Esc`
  discards). A saved edit counts as activity, so the row moves to the top of
  the list.

### Changed

- When an answer fails after some text was received, the partial text is kept
  in the history with status `error`; the error line itself is not stored.
- Closing a pane (or a tab, or quitting) while an answer is streaming saves
  the text received so far as `cancelled`.

### Known limits

- Extra tags come back in alphabetical order after a conversation is reopened.
- Deleting the engram notes of a deleted conversation and resuming the engram
  memory session are not part of this release (they arrive with the
  `engram-memory` follow-up).
- `Ctrl+Space T` has not yet been checked by hand in Kitty.

## [0.10.0] - 2026-10-14

### Added

- Optional project memory with [engram](https://github.com/Gentleman-Programming/engram),
  shared with other agents that use it. It is off by default: turn it on in
  the AI menu (`Ctrl+Space m`) or with `[ai.memory]` in `config.toml`
  (`enabled`, `acknowledged`, `port`). A one-time privacy warning must be
  accepted first. It works in git projects only and needs an engram 3.x server.
- Memory block at the top of a chat: shown as `▸ memory (N notes)`, and
  `Ctrl+Space o` shows or hides it.
- Save a note from the chat with `Ctrl+Space n` (`Ctrl+S` saves, `Esc`
  closes). Recognizable secrets are redacted first, and the model's reasoning
  is never stored.
- Conversation summary with `Ctrl+Space s`: review the draft (`Ctrl+S`
  approves, `Esc` discards); an approved summary is sent to engram when the
  chat closes.
- When engram is not running, the chat offers to start `engram serve`, showing
  the absolute path of the binary. The server keeps running after you quit
  lazarobox.
- Statusline indicator `mem: ready`, `mem: checking` or `mem: unavailable`.
  Every wait on memory is bounded to 5 seconds, so a slow or missing server
  never blocks the chat.
- Chat notification: lazarobox rings the terminal bell when a chat answer
  finishes while you are not looking at it (another pane, tab or window is
  focused). Turn it on with `notify = true` in `[ai]` or with the new Notify
  row in the AI menu. It is off by default.

### Changed

- `chat.db` moves to schema version 2 to hold the memory data. An older
  lazarobox refuses a newer `chat.db` and leaves it untouched.
- The chat database is now opened lazily: a session that never uses the chat
  no longer creates `chat.db` or its directory.

### Known limitations

- Conversations are not saved yet; chat history arrives in a later release.
  Deleting notes from lazarobox and resuming conversations come with it.
- Only Ollama has a chat adapter, and only it receives memory. Other engines
  are detected but cannot chat.
- Memory requires engram 3.x.
- The chat talks to `127.0.0.1` only and sends no credentials.

### Notes

- Still pending manual verification in Kitty: the `engram serve` offer, the
  note keys, the memory line toggle, the summary keys and the notification
  bell.

## [0.9.0] - 2026-10-13

### Added

- AI profiles for the chat: `analyst` and `programmer`, set with `profile` in
  the `[ai]` section of `config.toml` (default `programmer`) or with the new
  Profile row in the AI menu (`Ctrl+Space m`). An invalid value is reported and
  the default is kept.
- Built-in system prompts: ten in total, one for each profile and task
  (`general`, `code`, `chat`, `review` and `summarize`). The chat uses the
  `chat` prompt of the chosen profile.
- Prompt overrides: put your own prompt in
  `~/.config/lazarobox/prompts/<profile>/<task>.md`. The task file wins, then
  `general.md`, then the built-in prompt. Files must be regular UTF-8 text of
  at most 16 KiB; one that cannot be used is skipped with a notice.
- Workspace context in the system prompt: a short block with the project, git
  branch, current directory relative to the repository root, OS, shell and
  date. It never includes absolute paths, environment variables, file contents
  or terminal output; your home directory is shown as `~`, and git is never
  run. Turn it off with `context = false` in `[ai]` or with the new Context
  row in the AI menu. It is on by default.
- If the context cannot be prepared within 2 seconds, the chat falls back to
  the built-in prompt and shows the notice `workspace context unavailable`.

### Fixed

- Closing a chat pane while its first message was waiting for the prompt no
  longer sends that message.

### Known limitations

- Conversations are not saved yet; chat history arrives in a later release.
  Starting the app still creates an empty, private database file at
  `$XDG_DATA_HOME/lazarobox/chat.db` (default `~/.local/share/lazarobox/`),
  for that future history; nothing is written to it yet.
- Only Ollama has a chat adapter. Other engines are detected but cannot chat.
- The chat talks to `127.0.0.1` only and sends no credentials.

## [0.8.0] - 2026-10-12

First release published to crates.io.

### Added

- **Experimental** AI chat pane: `Ctrl+Space c` opens a chat with a local
  model inside the workspace. It works only with Ollama for now.
- Streamed answers: the reply appears as the model writes it, with wrapped
  prose and fenced code blocks.
- Reasoning block for models that "think" (for example `qwen3`): the thinking
  is kept apart from the answer and `Ctrl+R` shows or hides it. A statusline
  indicator shows while a model is thinking, even if another pane is focused.
- Composer keys: `Enter` sends, `Alt+Enter` or `Ctrl+J` adds a newline,
  `Esc` or `Ctrl+C` cancels the running answer, `Ctrl+U` clears the input,
  `PageUp`/`PageDown` scroll the transcript, `Ctrl+Y` copies the last answer
  and `Ctrl+B` its last code block (through OSC 52).
- The chat uses the engine and model of the `chat` task (`[ai.tasks.chat]` in
  `config.toml` or the AI menu, `Ctrl+Space m`); without its own setting it
  falls back to the general engine. The pane explains what is missing when
  the engine is not set, not running or not supported.
- Closing a chat pane or tab (for example `Ctrl+Space w q`) cancels its running
  answer.

### Known limitations

- Conversations are not saved yet; chat history arrives in a later release.
- Only Ollama is supported. Other engines are detected but have no chat
  adapter yet.
- One fixed system prompt; profiles come later.
- The chat talks to `127.0.0.1` only and sends no credentials.

## [0.7.0] - 2026-10-12

0.7.0 is the baseline before the first crates.io release and was not published.

### Added

#### Terminal panes and tabs

- Full-screen terminal pane running your `$SHELL` in a PTY, with a VT100
  emulation layer and TrueColor. Linux support only.
- Modal input: TERMINAL, PREFIX (`Ctrl+Space`) and COPY modes, with a quit
  confirmation.
- Cursor shape passthrough and a statusline with the current mode, directory
  and shell.
- Splits, geometric focus, sticky resize and zoom for panes.
- Tabs with a tab bar that appears when there is more than one tab.
- Which-key style prefix groups behind `Ctrl+Space`.
- New panes and tabs inherit the working directory of the focused pane, tracked
  through OSC 7 with a `/proc` fallback.
- A pane closes when its shell exits; the last tab closing quits the app.

#### Configuration menu

- Config menu on `Ctrl+Space m` with a Settings section: statusline and tab bar
  position (top or bottom), previewed live.
- `s` saves to `config.toml` keeping the user's comments, line endings and
  key order; `Esc` reverts. `Enter` cycles a Settings value or opens the
  picker, it never saves. Writes are atomic and refused when the file has
  errors.

#### Copy selection

- COPY mode with scrollback navigation using vim-style motions.
- `v` selects characters, `V` selects whole lines.
- `y` copies the selection to the system clipboard through OSC 52 (selections
  over 75,000 bytes are not copied) and returns to the terminal.
- Selection highlight and a VISUAL / V-LINE marker in the statusline.
- A held COPY exit key no longer re-triggers after leaving COPY; empty yanks
  are skipped and the COPY cursor resets on screen switch or resize.

#### Desktop launcher

- Desktop launcher (`packaging/lazarobox.desktop`) and a per-user installer
  script (`scripts/install-desktop.sh`) that opens the workspace in Kitty from
  rofi or the application menu.

#### AI engine detection

- Detection of local AI engines: `ollama`, `lmstudio`, `llamacpp` and `vllm`
  (loopback servers on their usual ports) and the `claude`, `codex`, `gemini`
  and `opencode` CLIs found in `PATH`.
- AI section in the config menu, with the detected engine status, and `[ai]`
  engine and model keys in `config.toml`.
- Detection is read-only, uses `127.0.0.1` only and sends no credentials; the
  only command run is `--version` on the CLIs found. Engine responses are
  parsed defensively and version probes are bounded.
- `r` re-detects the available engines from the menu.

#### Per-task AI model selector

- Per-task AI model selection in the config menu: a general "Set all tasks"
  engine plus one row for each of the five AI tasks (general, code, chat,
  review, summarize).
- Two-level picker (engine, then model). The CLI engines end with a `custom...`
  row to type or paste a model name; custom names reject format characters.
- Curated Ollama and CLI model catalogs. For server engines, models that are
  not installed are marked `download required` and cannot be chosen; nothing is
  downloaded.
- `s` saves the choice to the `[ai]` and `[ai.tasks.*]` tables of
  `config.toml`; `Esc` discards it. `Enter` never saves.

[Unreleased]: https://github.com/pichu2707/lazarobox-shell/compare/v0.16.0...HEAD
[0.16.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.4...v0.16.0
[0.15.4]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.3...v0.15.4
[0.15.3]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.2...v0.15.3
[0.15.2]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.1...v0.15.2
[0.15.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.15.0...v0.15.1
[0.15.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/pichu2707/lazarobox-shell/releases/tag/v0.7.0
