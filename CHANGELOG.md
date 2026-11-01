# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

0.8.0 is the first version published to crates.io. 0.7.0 is an unpublished
baseline.

## [Unreleased]

### Added

- Copy now sends OSC 52 and also runs a local tool when one exists (`wl-copy`,
  `xclip`, `xsel`, or `clip.exe` in WSL; none over SSH). The notice says what
  happened, short and action first so it fits an 80-column statusline:
  `Copied (<tool>)`, `<tool> failed; sent via OSC 52 (<reason>)`,
  `<tool> failed; not copied: too large for OSC 52 (<reason>)`, or
  `No paste? Install wl-copy or xclip (OSC 52 sent)`.
- `Ctrl+]` (and `Ctrl+5`) works as an alias of the `Ctrl+Space` prefix.
- First-run prefix check: if `Ctrl+Space` does not reach lazarobox, it says to
  use `Ctrl+]`. The result is kept per terminal; `--check-prefix` runs it
  again.
- `install-desktop` asks which terminal to use when run on a TTY without
  `--terminal`, marks terminals with a test record, and prints notes for WSL.
- `docs/terminals/` holds the support policy, the checklist and the record
  templates. A terminal is supported only with a filled record.

### Fixed

- Alt+Enter is no longer lost in zutty, which sends it as `CSI 27;3;13~` while
  xterm modifyOtherKeys is on (the input library cannot parse that form):
  startup asks the terminal to turn that mode off (`CSI > 4 ; 0 m`) and exit
  asks it to return to its initial value. zutty then sends ESC LF, which the
  chat now treats as a newline (Alt+Ctrl+J).

### Changed

- A single `Ctrl+]` or `Ctrl+5` no longer reaches the program in a pane: it is
  the prefix alias now and takes two presses (`Ctrl+] Ctrl+]` sends 0x1D). This
  affects vim tag jump and the telnet escape.
- `install-desktop` no longer says lazarobox is "tested in Kitty"; it says
  whether the terminal has a test record, and never refuses to install.
- README: Windows is described as WSL only, and the terminal record policy is
  linked.

## [1.0.1] - 2026-10-31

### Changed

- The batch review no longer reads `y locked` in the counter: it says
  `N remaining · ↓ to unlock y`, and the keys row `y after scrolling` (`y
  locked` in panes too narrow for it). Pressing `y` with lines unseen still
  does nothing, but for 2 seconds the counter says what to do first, `scroll ↓
  · N lines unseen · y needs all` (`scroll ↓ · N unseen` in narrow panes). On a
  batch that cannot be approved (too large or empty) `y` says `y can't approve
  this batch · press n`, and an empty batch no longer reads "too large to
  review".
- Write works on Linux and macOS (APFS and HFS+); on any other system an
  approved batch writes nothing and says so. On macOS, a filesystem without an
  atomic no-replace rename (exFAT, SMB, NFS) answers "this system or volume does
  not support a safe no-replace write" and whatever was done is undone, and
  replacing a file drops its extended attributes, ACLs, quarantine flag and
  Finder tags, and the new file takes its group from the parent folder (BSD
  semantics).

### Internal

- Releases can be started with one click: Actions -> Release -> Run workflow
  bumps the version, moves `[Unreleased]` under it, commits, tags and
  publishes, and `dry_run` rehearses it. Pushing a `vX.Y.Z` tag by hand still
  works. See `RELEASING.md`.

## [1.0.0] - 2026-10-30

First stable release. From here on, `config.toml`, the `chat.db` format, the
public CLI and the key bindings follow the stability promise described in the
README. It builds on 1.0.0-rc.1 with what testing the chat on local models
turned up: small Ollama and llama.cpp models now finish write tasks with fewer
stops (the batch review opens by itself, refusals name the chat's folder, a
tool call written as text or a step left unfinished gets one reminder), and
`y` on a model's command in run mode runs it.

### Changed

- With Ollama and llama.cpp, reasoning that a model leaks into its answer
  after a tool round, closed by a bare `</think>`, is now shown as reasoning
  instead of as part of the answer. In those rounds the answer appears whole
  at the end of the round.
- With Ollama and llama.cpp, a model that writes a tool call as JSON text
  instead of making it is asked once, in the same turn, to call the tool. The
  chat shows `the model wrote a tool call as text; it was asked to call the
  tool`. The text is never run, and the extra request counts toward the
  turn's limit.
- With Ollama and llama.cpp, a model whose answer ends by announcing its
  next action ("I'll create the test next") without a tool call is asked
  once, in the same turn, to do that step or to sum up. The chat shows `the
  model said what it would do next without a tool call; it was asked to do
  it`. A question, an offer, a courtesy or a summary does not count (#439).
- With Ollama and llama.cpp, an empty answer with tools on, or one whose
  last sentence starts with a sign-off such as "Ready for next steps." or
  "Listo para el commit.", gets the same single reminder; the chat shows `the
  model sent an empty answer; it was asked to go on` or `the model stopped
  before finishing; it was asked to go on` (#439).
- A turn that offers the write, exec or git write tools no longer tells the
  model it cannot change files, in the system prompt or in the `run_in_pane`
  description. The system prompt now names what its tools allow (write,
  test, commit), says it must do every step asked in the same turn, and must
  not claim a file, test run or commit that no tool result shows (#439).
- The result of an applied batch now ends with `Continue with any step the
  user asked for that is not finished yet.`, and the refusal of `run_command`
  and the git tools while changes are staged now says `propose them first
  with propose_batch, then call this tool again` (#439).
- A refused write (`write_file`, `edit_file`, `create_dir`, `propose_batch`)
  now names the chat's folder, so a model learns where its paths start:
  `path not available (paths are relative to the chat's folder ~/proj)`. The
  text is still the same for every reason. Only a missing parent folder of a
  path the policy allowed is named: `the folder src/ does not exist`.
- When a chat turn ends normally and the model left staged changes without
  calling `propose_batch` (common with small local models), the batch review
  opens by itself, with the same coverage check, arming delay and `y`/`n` as a
  proposed batch. A cancelled, timed-out or failed turn, or a chat that already
  waits on an approval, still leaves the orphan behind `Ctrl+G`.
- In approval mode `run`, `y` on a model's `run_in_pane` prompt now runs the
  command: lazarobox presses Enter, waits for it and returns its output to the
  model, like a safe command that runs on its own. The prompt says
  `y run it · n cancel · p next pane`. A command that cannot run (multi-line,
  unknown pane directory, a busy or half-typed pane, a non-UTF-8 locale) is
  typed without Enter as before, and the chat line says why. `ask`, `auto` and
  the `Ctrl+Space`, `e` confirm keep typing without Enter.

## [1.0.0-rc.1] - 2026-10-30

First release candidate for 1.0.0. It is published to crates.io, but cargo
does not install a pre-release unless asked for it (`cargo install
lazarobox-shell --version 1.0.0-rc.1`). Use it, report what turns up, and
1.0.0 follows from the last good candidate. From 1.0.0 on, `config.toml`, the
`chat.db` format, the public CLI and the key bindings follow the stability
promise described in the README.

### Added

- Chat folder: the chat now asks for its folder instead of silently using the
  folder it was launched from. When it opens (or resumes a conversation)
  with no usable folder, for example from a desktop launcher that starts in
  `$HOME`, a picker offers the last used project, the repository roots of the
  panes, recent chat folders and a typed path with `Tab` completion; `Esc`
  keeps the current folder. `/cd <folder>` changes the folder of an open
  chat: it is your command (it never reaches the model), asks `y` showing the
  old and the new folder, and waits while a write, an approval or a command
  is pending. The chat remembers its last folder and a resumed conversation
  opens in it. To send a message that really starts with `/cd`, type
  `//cd`.

- The codex row of the AI menu names how codex is installed after its version, for example `codex available · 0.153.4 · standalone` (also `npm` and `homebrew`; nothing for an unknown layout). It is a label for messages only: the jail rules still come from the real paths alone (#180).
- A menu save says where it wrote in the statusline (`saved to
  ~/.config/lazarobox/config.toml`), and the README has a section on where
  `config.toml` lives.
- `lazarobox-shell --version` (`-V`) and `--help` (`-h`) print and exit 0,
  before the terminal or the nesting guard are touched.
- Start folder: `lazarobox-shell --dir <folder>` and `[workspace] dir` in
  `config.toml` choose where the first shell starts (flag, then config, then
  the launch folder). An invalid folder does not stop the app: it starts in
  the launch folder and shows a notice inside lazarobox.
- `install-desktop --dir <folder>` writes `Path=` and `--dir=` in the
  launcher; running it again without `--dir` removes both.

### Changed

- When the jail refuses a codex layout, the full reason (`y` in the menu, and the chat) now names the layout and what to do, for example `codex (standalone) is installed in a layout the jail cannot isolate: … would be both run and written; reinstall it with the official installer, npm or Homebrew`. The short form on the menu row is unchanged.
- `codex --version` now runs through the jail, like the model list. Before, it ran outside it, and with the standalone install the binary sits in codex's own writable folder.
- An unknown option or command now prints one line on stderr and exits 2
  instead of silently starting the TUI.
- `chat.db` schema 3: the closed-list checks on `profile`, `role` and `status`
  are gone (the values are still validated when written), so a new profile,
  role or status can be added without rebuilding tables. Opening an older
  file rebuilds `conversations`, `messages` and `tags` once, in one
  transaction, keeping every row and id. A build that only knows schema 2
  refuses a schema 3 file and leaves it untouched. The migration drops
  messages or tags whose conversation no longer exists (they could never be
  shown) and vacuums the file afterwards. Resuming a conversation leaves out
  messages of a role this version does not know and says how many.

### Fixed

- The AI menu no longer shows `available` for codex or gemini when the jail will refuse to start them. The row reads `codex refused · …/bin is run and written`, cannot be chosen, and the chat explains why. claude keeps `available · no jail: …` where it really runs without a jail (#180).
- The nesting guard no longer treats the value of `--dir` as a subcommand.

### Docs

- README: the chat folder picker and `/cd`, and the key reference for both.
- README: new "Estabilidad y compatibilidad (1.0)" section (what is frozen:
  `config.toml`, the `chat.db` format with forward migration, the public CLI
  and the key bindings; what is not: `__*` subcommands, log text, UI layout),
  an upgrade note, a single key-bindings reference for every view (the
  `openspec/` link is gone), a security and threat-model index, and macOS
  stated as not officially supported in 1.0.
- `SECURITY.md`: supported versions and private vulnerability reporting.
- `RELEASING.md`: the post-1.0 SemVer rule and the pre-1.0 release checklist.
## [0.30.0] - 2026-10-29

### Added

- Ollama downloads: with the Ollama server running, its model level ends in
  `download from Ollama…`. It lists up to 20 of the most popular models of
  the Ollama library that fit this PC's memory (embedding and cloud-only
  models left out), then the exact size of each tag from its registry
  manifest, then a confirmation with the license title, a link to read it,
  the models folder and the free space. Nothing is pulled before `y`. The
  pull goes through the local Ollama server (`POST /api/pull` on
  `127.0.0.1`), with progress on the row and in the status bar, a cancel
  prompt that keeps what Ollama pulled, and one detection when it ends.
  When the library page cannot be read, the built-in list is shown instead.
- One download runs at a time across Hugging Face and Ollama: while one
  runs, the other entry row says `a download is already running`.

### Changed

- The loopback client may POST to `/api/pull`, for the Ollama download only;
  chat requests still refuse that path.

### Fixed

- A held key no longer repeats a decision: on prompts, confirmations and
  menus, a key held down acts once, and only a new press acts again.
  `Ctrl+H` works as Backspace in text fields.
- Tool approval prompts arm after 800 ms instead of 400 ms, so the first OS
  key repeat of a held key (about 600-760 ms) can never answer one.
- The feedback form's footers say what `Esc` does on every screen.

## [0.29.0] - 2026-10-28

### Added

- Chat COPY mode (`Ctrl+Space [` in a chat pane): `v` and `V` select, `y`
  copies through OSC 52 (75 000-byte limit, same notice as the shell) and
  returns to TERMINAL. The text is clean: no drawing prefixes, wrapped lines
  rejoined, no key hints. `Ctrl+Y` (last answer) and `Ctrl+B` (last code
  block) also work inside COPY mode.
- Run mode (`approval = "run"`):
  - `read_file` and `grep` inside the chat's folder run without asking. The
    path is confined to the root when the file is opened. A read outside the
    folder still asks, a sensitive file is still refused, and with a cloud
    engine every read asks when the chat's folder is `$HOME` or `/`.
  - Safe pane search commands (`cd`, `pwd`, `ls`, `tree`, `grep`, `rg`,
    `find`) run without asking while they stay inside the chat's folder.
    Leaving the folder, a broad root with a cloud engine or an oversized walk
    asks; reaching a sensitive file, a git config or a secret folder is
    refused. `rg` is checked with ripgrep's own walker (`ignore` crate).
- A skill for coding agents to report lazarobox defects to GitHub, with the
  user's consent, after searching existing issues.
- README: demo GIFs for panes, COPY mode, the AI menu, history and run mode.
- Git tools for the chat, behind two toggles that are off by default: `git`
  (`git_status`, `git_diff`, `git_log`) and `git_write` (`git_add`,
  `git_commit`). No other verb exists and no tool takes a free-form argument
  list. Linux only; git must be in a system place.
  - The read tools exclude secret file names by pathspec, redact the whole
    output and are refused while a write batch is staged or proposed. A
    refused path opens no prompt. A local engine never asks; a cloud engine
    asks only for `git_diff`.
  - `git_add` shows a one-line prompt with the paths and the file count. Paths
    that cannot be shown whole, or that contain `'`, are refused.
  - `git_commit` opens a full-pane review on every engine: the message, a
    `signing disabled for this commit` notice, the hooks that will run and the
    whole staged diff (`--text`). The commit is unsigned, has no trailer and
    runs its hooks inside the jail. It is refused on a secret in the index or
    a cut diff, and the index is checked byte for byte again after `y`. The
    commit has a 120 s timeout.
  - After the commit the committed tree is compared with the reviewed one. A
    difference (a hook changed files) is reported, never undone, with a
    `git reset --soft HEAD~1` hint. An edit of the message by a `commit-msg`
    hook is not detected.
- README: how the chat's folder is chosen (the focused pane's folder when you
  open the chat, fixed for its life; CLI engines run in a private folder) and
  a threat model for `run_command` and git (the allow-list bounds the verb,
  the network is on, `.env` can be exfiltrated, UNIX sockets stay reachable,
  daemonizing grandchildren, hooks that change a commit or are written by the
  model).

### Changed

- The `git` and `git_write` rows of the AI menu now offer real tools instead
  of only holding the switch.
- `mkdir` and `git` are no longer on the run-mode allow-list: they always
  ask.
- New dependency: `ignore` 0.4.

## [0.28.0] - 2026-10-28

### Added

- `llama-server` can be installed from lazarobox, the one documented exception
  to "lazarobox never installs anything". With llamacpp `not installed`, its
  model list offers `install llama-server…`: pick a build (CPU, Vulkan on
  Linux with a driver, Metal on macOS), confirm the download and the install
  with `y`. The archive comes from `github.com/ggml-org/llama.cpp`, is held
  to its published SHA-256 digest (and verified with `gh attestation verify`
  when `gh` is logged in; otherwise a warning is shown), and is unpacked
  under `$XDG_DATA_HOME/lazarobox/llama.cpp/`.
  A `llama-server` of your own on the `PATH` always wins. With an owned
  install the list offers `remove the llama.cpp install` (y/n), which stops a
  running managed server first, and opening llamacpp tells you once per run
  when a newer build exists (of the installed variant, Vulkan included; never
  with `[updates] check = false`, silent offline, and it never replaces
  another footer notice). The Hugging Face download row
  no longer needs `llama-server`.
- `run_command` asks before every command, in every approval mode, and the
  prompt now says what the user is approving: the timeout, that the network
  is on and a test can read and send out the project's `.env`, that the output
  goes to the model (and, on a cloud engine, to its provider), which project
  files the program runs (`build.rs`, `package.json` scripts, `conftest.py`,
  `go.mod`), and, for `npm`, `yarn` or `pnpm` `install` and `ci`, that the
  install scripts of the project and its dependencies run, with the project's
  own lifecycle scripts shown. The notice is part of what must fit on screen
  before `y` answers. Cloud engines can now run commands (Linux only).
- The Exec row of the AI menu reads `on · needs Linux sandbox` when the system
  has no Landlock jail.
- Arguments with shell characters (`; | & < > ( ) * ? # ~ { } !`) show quoted
  in the prompt.
- A cancelled run that touched git hooks or config is told in the next
  command prompt of that chat, and still reaches the model.
- Chat memory: the history sent to the model is now sized in tokens against
  the engine's context window (llama.cpp reports it through `/props`), and the
  status line shows `context N%` once the window is known.
- When a long chat overflows the window, the messages that drop out are folded
  into a rolling summary sent as an inert block, instead of being lost. The
  summary uses the `summarize` task only when you set it in
  `[ai.tasks.summarize]`, otherwise the chat's own engine and model (never the
  general one). `[ai.chat] compaction = "off"` turns it off (default `"auto"`).
- `[ai.memory] save_summaries = true` (off by default) also saves that summary
  to engram, as one note per conversation. The first time it would have saved
  one, a line in the chat explains what would be saved and how to enable it.
- llama.cpp gets the tool loop on an OpenAI-style wire profile. It stays off
  (`llamacpp` is not in the verified set) until it is checked against a real
  server.

### Fixed

- `npm`, `yarn` and `pnpm` commands (`test`, `run`, `install`, `ci`) are
  refused outside the folder that has the `package.json` (a manager would read
  a parent one that nothing showed).
- A run that fails after it started now reports the git hooks and config it
  changed instead of dropping them.

## [0.27.0] - 2026-10-27

### Added

- Hugging Face downloads open on a list you pick from with `j`/`k` and
  `Enter`, no typing needed: the most downloaded text-generation GGUF models
  that fit this PC's memory, each with its estimated size (about Q4_K_M, and
  at most 70% of the RAM). `Esc` opens the search, which shows everything
  and marks with `too big for this PC` the repos, and the files of a repo,
  that do not fit.

- Ctrl+O opens the link of the Hugging Face token field, and the prefilled
  issue of the Feedback form, in the browser (`xdg-open`; on WSL `wslview`,
  then `rundll32.exe url.dll,FileProtocolHandler`, since `explorer.exe` opens
  some links in the file manager; on macOS `open`). With no opener the link is
  copied, and
  when the opener fails the link is copied and the footer says so.
  With `Mouse` on, a left click on the token link line opens it too.

- With `[ai.tools] write = true`, chats get four tools to change files in the
  project: `create_dir`, `write_file`, `edit_file` and `propose_batch`.
  Nothing touches the disk while the model stages; `propose_batch` opens a
  full-pane review of the whole batch (new text, diffs, a warning on build
  files), and only `y` applies it, all or nothing:
  - `y` stays locked until every line has been drawn on screen and 800 ms have
    passed; `n` or `Esc` rejects the whole batch; `Ctrl+C` cancels the turn.
  - Writes stay beneath `$HOME`, inside the chat's folder, never through links,
    and never in `.git`, `node_modules` or secret files. Invalid UTF-8,
    control, bidi or zero-width characters and CRLF are refused, not cleaned.
  - Apply checks every target again and refuses the whole batch if one
    changed, writes through same-folder temp files and atomic renames, and
    undoes the files already written if a step fails (`partial` lists what
    could not be undone).
  - A batch left by a cancelled or timed-out turn stays as an orphan: the pane
    shows ` staged N ` or ` orphan N · Ctrl+G `, and `Ctrl+G` opens its review.
    Closing the pane or quitting discards the batches with a notice.
  - `read_file`, `glob` and `grep` see the staged changes, marked
    `(pending, not on disk)`. The model gets counts and paths, never the diff.
- The chat key table lists `Ctrl+G`.

### Changed

- On a cloud engine, `edit_file` asks like `read_file`, once per file per turn,
  because its answers reveal what a file contains.
- Write is Linux only: elsewhere an approved batch writes nothing and says so.
- Feedback: Ctrl+S now sends the issue with your own `gh`
  (`gh issue create`, the body on stdin, no shell), shaped like the issue
  forms and labelled `bug` or `idea`; the form shows `Sent:` and the issue's
  link, which Ctrl+O or a click opens. A `gh` failure keeps the form and shows
  its message. Without `gh`, or not logged in, Ctrl+S opens the prefilled page
  instead. Ctrl+O still opens the prefilled page. lazarobox stores no GitHub
  token.

## [0.26.0] - 2026-10-27

### Added

- The Hugging Face token field shows the link to create a read token
  (`https://huggingface.co/settings/tokens/new?tokenType=read`) on a line of
  its own, so terminals that detect URLs keep it clickable.
- A **Feedback** row at the end of the Settings tab opens a short form (a bug
  report or an idea, a title, a description) that attaches the lazarobox
  version, the OS and WSL, the shell, the terminal and the detected AI
  engines. `Ctrl+S` copies a link that opens the matching GitHub issue form
  filled in; lazarobox sends nothing itself. Secrets in the text are masked
  before any link.
- GitHub issue forms for bug reports and ideas; ideas go to the project's
  Goals board.

### Fixed

- The `downloaded <file>` notice draws control and format characters of the
  file name as `?`, like the rest of the Hugging Face screens.
- A repo id with `--`, or a part ending in `-` or `.`, is refused, as the Hub
  refuses it.
- A partial download longer than its file is removed and fetched again
  instead of failing on every resume.
- A relative `HF_TOKEN_PATH` is read from the working directory, as
  `huggingface_hub` reads it, instead of being ignored.
- Esc while a pasted Hugging Face token is being checked stops the check, so
  the token is not saved.
- `d` on the Hugging Face row disconnects a saved token even when its check
  fails (offline, or a token the Hub no longer accepts).

### Security

- Every copy of the Hugging Face token that curl is given (the token and its
  header line) is zeroed when it is dropped, as the pasted token already was.

## [0.25.1] - 2026-10-26

### Fixed

- On macOS, codex installed with its official standalone installer now runs in the jail (#180). That installer puts the binary under `~/.codex/packages/standalone/releases/<version>/bin`, inside codex's own state folder, so the jail refused to start with `… would be both run and written by the jail`.
  - `~/.codex` stays writable, but `~/.codex/packages` is now read and run only.
  - The state folder and the folders above `packages` cannot be renamed, removed or replaced from inside the jail, so a planted tree can never take the binary's place.
  - The exception applies only when the program itself is in a real `packages` folder with that exact spelling. Every other layout where the program could be written is still refused.
  - Inside the jail codex cannot update itself. Run it directly to update; lazarobox picks up the new version on its next start.

## [0.25.0] - 2026-10-26

### Added

- Download GGUF models from Hugging Face without leaving lazarobox (#282). The llamacpp model list ends in `download from Hugging Face…`:
  - search the Hub or paste `owner/repo`, then pick a quantization (split models count as one);
  - a confirmation shows the file, its size, the license with a link to read it, the target folder and the free space, and nothing downloads before `y`;
  - the download runs in the background with the system `curl` (HTTPS only), with progress in the list and the status bar, and it can be cancelled and resumed;
  - each file's size and sha256 are checked before it lands in the standard Hugging Face cache (`~/.cache/huggingface/hub`), where the model list finds it and the managed llama-server runs it.
- A `Hugging Face` row at the end of the AI tab connects an account for gated or private repos. The pasted token is checked with the Hub and saved where `hf auth login` saves it (`~/.cache/huggingface/token`, mode 0600), never in `config.toml` and never on a command line. `HF_TOKEN` still wins when set.

### Changed

- lazarobox may now download a model, but only from that Hugging Face flow and only after you confirm it. It still never pulls a model through a server.

## [0.24.0] - 2026-10-26

### Added

- The quit prompt names `llama-server (shared)` when the managed llama-server
  this lazarobox started is answering another lazarobox, since quitting cuts
  that reply; the idle stop waits for such a reply and counts the idle time from
  it. Tabs and panes of one lazarobox share its server and never count.
- Managed llama-server: one server per model, each on its own port (the
  configured one and the seven after it). A second lazarobox, or a
  `llama-server` you started in a terminal, serving the same model file is used
  as it is and never stopped by lazarobox; another model starts its own server
  on the next free port. Only a listening socket of your own user is used.
- The quit prompt names what still runs in the panes (jobs started with `&`,
  `nohup` or `disown`, or a program in the foreground):
  `Quit? still running: sleep 77, npm run dev (y/n)`, three names at most and
  fitted to the width. After quitting, the same jobs are written to stderr
  with their process group, also after `exit` in the last pane. lazarobox
  still never stops them: a pane behaves like any terminal.
- `[ai.servers.llamacpp] args` passes extra arguments to the managed
  llama-server, after the ones lazarobox sets: for example
  `args = ["-ngl", "99", "-c", "16384"]`. Each entry is one argument, with
  no shell. A list that sets `--host`, `--port`, `-m` or `--model` (also as
  `--port=8080`), or that is not a list of strings, is ignored whole with a
  startup notice.
- The managed llama-server stops after `[ai.servers.llamacpp] idle_minutes`
  without a turn (15 by default, 0 = never), freeing the GPU memory; the
  next llamacpp turn starts it again. A turn still answering is never cut:
  the idle time counts from its end. The Settings tab of the menu has a
  `llama-server idle` row (5 min, 15 min, 30 min, never) that applies at
  once, with no restart.

### Changed

- Managed llama-server: a port where something else already answers no longer
  fails the turn with `another program answers on 127.0.0.1:<port>`; the next
  free port is used, and the turn fails only when all eight are taken (`no free
  port on 127.0.0.1 from <port> to <port+7>`). With llamacpp managed, the model
  picker keeps listing the local models while another llama-server answers on
  the port.
- The managed llama-server checks its model file before each start. When
  it is missing nothing starts and the chat reads `llama-server not
  started: model file not found: <path>`, instead of the last line of a
  failed llama-server. It is not checked when the config loads, so a drive
  mounted later works on the next turn.

### Fixed

- Nothing lazarobox starts is left running once it ends, however it ends.
  A small watcher (`lazarobox-shell __watch`, in its own session; `ps` on
  Linux shows `/proc/self/exe __watch`) outlives it; every process group
  lazarobox starts (CLI chat turns, MCP servers, the managed llama-server,
  short probes) is listed with it and taken off once reaped. When lazarobox
  quits, crashes, is killed with SIGKILL or loses its terminal, the watcher
  sends SIGHUP and SIGTERM to the groups still listed, SIGKILL after 3 s, and
  exits. Before, a crash or SIGKILL left them all
  running, and on macOS the managed llama-server then held its port so every
  later start failed. A group is only signalled while its leader is still the
  listed process (same start time) or gone, so a reused pid is never hit.
  `engram serve` is never listed.
- Quitting is polite. It used to SIGKILL CLI chat turns, MCP servers and the
  managed llama-server at once. Now, as soon as the terminal is restored,
  everything lazarobox started gets SIGHUP and SIGTERM, and SIGKILL only if
  it still runs 2 s later, while the history and the memory are saved. A
  turn still streaming its answer keeps running until then instead of being
  cut at its next line. A group whose leader ends gets SIGKILL at once for
  what it left behind. If lazarobox is killed during the quit, the watcher
  repeats the same stop, where what a leader left behind gets the grace too.
- A managed llama-server that ends on its own, during its start or later,
  no longer leaves its children running: they get SIGKILL before the server
  is reaped.

## [0.23.0] - 2026-10-25

### Added

- `[ai.tools]` gains five family toggles, all `false` by default: `search`,
  `write`, `exec`, `git` and `git_write`. The AI tab of the menu shows them
  as rows (Search, Write, Exec, Git, Git write) with a caution line for
  Write, Exec and Git write; saving writes only those keys and keeps the
  rest of `config.toml`. A family is offered only while `[ai.tools] enabled`
  is on and the chat's engine is verified for tools. With every toggle off,
  the tools offered to the engine are the same as in 0.22.0.
- With `search = true`, chats get two tools:
  - `glob` lists the files of the project that match a pattern (`*`, `?`,
    `**`, `[...]`), sorted and capped at 100. It never asks for approval,
    like `list_dir`.
  - `grep` searches file contents with a regular expression, capped at 100
    matches or 20 KiB, 200 characters per line. On a cloud engine it asks
    first (`search <pattern> in <folder>`), in every approval mode; on a
    local engine it runs freely.
  - Both skip symlinks, `.git`, `node_modules`, `target` and other build
    folders, binary files and secret files (their names are left out and
    the result says so). `grep` matches against the redacted text, so a
    pattern cannot probe a secret. A folder the ask could not show exactly
    (a symlink, `~`, an absolute path, `..`, hidden characters) is refused.
  - `read_file` takes `start_line` and `max_lines` to read a window of a
    long file, and says where the next window starts.
- Ollama allows 24 tool rounds per turn when any of these families is on
  (8 otherwise).

### Changed

- The menu popup is 22 rows tall (was 16); the full popup needs a terminal
  of at least 21 rows, a smaller one shows the one-line `MENU` fallback.

### Security

- `read_file` refuses a git `config` or `config.worktree` file under any
  `.git` folder, which can hold remote URLs with credentials.

## [0.22.0] - 2026-10-23

### Added

- The model picker of a stopped llama-server lists the `.gguf` models found
  on the machine (the llama.cpp models folder, llama.cpp's cache and the
  Hugging Face cache), marked `lazarobox starts it`. Choosing one and saving
  writes `managed = true` and that `model` under `[ai.servers.llamacpp]`,
  keeping the other keys, and starts llama-server at once, with no restart.
  The statusline says when it is ready or why it did not start.

### Fixed

- macOS: the jail's check that no program folder is also writable ignores
  letter case too. A codex under `~/.codex/...` with `CODEX_HOME=~/.CODEX`
  (the same folder on the default file system) was accepted; now it is
  refused like `~/.codex`.
- With `[ai.servers.llamacpp] managed = true`, the model picker offers the
  configured model while llama-server is not running (marked
  `lazarobox starts it`), instead of the note that says to start it by hand.

## [0.21.0] - 2026-10-23

### Changed

- CI and release workflows: `actions/checkout` v4 to v7.0.1 (Node 24, no more
  Node 20 deprecation warning; it refuses fork checkouts under
  `pull_request_target` and `workflow_run`). Every action is pinned to a
  commit SHA with its version in a comment, and no checkout keeps the git
  token on the runner (`persist-credentials: false`). `rust-cache` v2.9.3,
  `crates-io-auth-action` v1.0.6.
- macOS: the startup check of the jail may take 10 s in total instead of 5 s,
  so a slow first `sandbox-exec` on a loaded Mac no longer turns codex and
  gemini off for the session.

### Fixed

- macOS: the jail's refusals ignore letter case, like the default file system
  does. A state dir or package root spelled `~/library/keychains`,
  `~/LIBRARY/Caches`, `~/.SSH` or `/system` was granted before; now it is
  refused like `~/Library/Keychains`. Linux still matches case.
- The outdated-CLI message names our flag also when the CLI rejects it with
  its value (`--model=<name>`, `--mcp-config=`, `--allowedTools=`,
  `--setting-sources=`): it says `--model`, never the value, instead of the
  generic `it rejected an option`.

## [0.20.0] - 2026-10-23

### Added

- `r` on the menu's Updates row checks crates.io now, ignoring the cache and
  the 24-hour limit. The row reads `checking…` until the answer comes. It does
  nothing with `[updates] check = false` or after an install in this run.

## [0.19.0] - 2026-10-23

### Added

- lazarobox can manage llama.cpp: with `managed = true` and `model = "<absolute
  path to a .gguf>"` under `[ai.servers.llamacpp]`, the first llamacpp chat turn starts
  `llama-server --host 127.0.0.1 --port 39281 -m <model>` and waits until it
  answers; later turns reuse it, and quitting lazarobox stops it (on Linux it
  also dies if lazarobox crashes). Nothing starts on launch or on detection.
  The port is 39281 unless `port` is set. If something already answers there,
  nothing starts and the chat says so. The server's output goes to
  `llama-server.log` in lazarobox's state folder, and a failed start shows its
  last line. Wrong values are ignored with a startup notice.
- A server engine can live on another port: `[ai.servers.<id>] port = N` in
  `config.toml` (for `ollama`, `lmstudio`, `llamacpp` and `vllm`). Detection
  and chat both use it, always on `127.0.0.1`; without it the usual port is
  used. A port that is not an integer from 1 to 65535 is ignored with a startup
  notice. The menu never writes the table and keeps it on save. It is read at
  startup.
- An installed server that is not running (its binary is on PATH, but nothing
  answers on its port) is now listed in the engine picker as `not running`.
  Choosing it shows where lazarobox looks and how to start it, for example
  `llamacpp not running on 127.0.0.1:39080 · start it, then press r:
  llama-server --port 39080 -m <model.gguf>`; Enter or `y` copies it.
  lazarobox does not start the server, unless it manages llama.cpp (below).

- Chat with LM Studio, llama.cpp (`llama-server`) and vLLM. They talk to their
  server's `/v1/chat/completions` on `127.0.0.1` and their usual port (1234,
  8080 and 8000), stream the answer over Server-Sent Events and send no
  credentials, like Ollama. The body holds the model, the messages and the
  stream flag only: no reasoning is shown and no tools are offered. llama.cpp
  is tested against a reply recorded from a real server; LM Studio and vLLM
  use the same format but are not yet verified against a real one. The menu no longer marks them
  `detected · not in chat yet`; only opencode still has no chat adapter.

### Changed

- The engine picker no longer lets you choose an engine that chat has no
  adapter for (today only opencode). Its row still reads `detected · not in
  chat yet`, dimmed, and Enter does nothing on it; before, you could assign it
  and chat then refused it.

### Fixed

- Closing the terminal tab that runs lazarobox no longer leaves the servers it
  started running (MCP servers, CLI chats and the managed llama-server).
  Restoring the dead terminal made lazarobox abort before its quit cleanup.

## [0.18.0] - 2026-10-20

### Added

- The macOS jail: codex and gemini now run on macOS inside a Seatbelt jail
  (`/usr/bin/sandbox-exec`), after a startup check that proves it restricts
  (one 5 s deadline for the whole check). If the check fails they are refused,
  never run without the jail. claude is never jailed on macOS, to keep its
  Keychain login. macOS 14 or later is supported; gemini is not yet verified on
  a real Mac. The jail never grants anything at or under `~/Library` (as
  requested or as resolved), except `~/Library/Application Support/fnm`, and
  the credential folders in it stay refused. It opens one mach lookup,
  `com.apple.SecurityServer`, which codex needs to verify TLS certificates. The
  lookup reaches securityd, which reads keychains outside the sandbox and can
  serve keychain items over that port subject to its own ACLs and prompts; the
  keychain files stay denied by the profile (accepted risk). It refuses a layout where a program sits inside
  a writable folder, and a jailed CLI can still reach loopback, the LAN and the
  metadata address (accepted risk). A step-by-step check for macOS testers is in
  `docs/macos-tester-checklist.md`.

### Changed

- The refusal on systems with neither Linux nor macOS now reads `the CLI jail
  needs Linux or macOS`.

### Internal

- The real codex test of the macOS jail runs in its own non-blocking CI job
  and judges only the server's 401 as success; an answer such as 403, 429,
  `unsupported_country` is an annotated skip.
- Integration tests run fake CLIs through the real macOS jail helper and
  `sandbox-exec` on the macOS CI runners: reads, writes and execs outside the
  granted folders, unix sockets and `bind` are denied, and `/dev/tcp`, the
  environment, odd paths (`=`, quotes, newlines), nested vendor scripts, node
  and an npm-style `#!/usr/bin/env node` shim work. A profile that fails to
  compile (status 65) is now shown as a jail error too.
- The macOS jail helper now builds the `/usr/bin/sandbox-exec` command from
  the rendered profile and replaces itself with it, leaving the environment
  untouched, and the startup probe checks the real profile instead of a
  constant one. A `sandbox-exec` failure (status 71 and its own first stderr
  line) is shown as a fixed jail error and its stderr is never echoed.
- The macOS jail profile renderer (`sbpl::render`): it turns the jail rules into
  a Seatbelt profile text and a list of `-D` values. Paths only ever travel as
  values, never inside the text. The fixed rules read `resolv.conf` at its
  macOS target and keep Homebrew `etc/` closed, like `var/`.
- The jail rules are now built per system (`jail_spec_for`): the Linux rules are
  unchanged, and the macOS rules name no Linux-only path and add the macOS
  system and Homebrew folders to the list that is never granted.

## [0.17.2] - 2026-10-19

### Internal

- The macOS jail groundwork: a startup probe that checks, with three controls,
  that `/usr/bin/sandbox-exec` really confines a program, and the fault types
  that map its result. The jail stays off (`SEATBELT_ENABLED` is `false`), so
  nothing changes for users: codex and gemini are still refused on macOS. CI
  now runs a raw `sandbox-exec` smoke test on both macOS runners.
- More macOS jail groundwork: claude is exempt from the jail on macOS (it
  keeps its Keychain login and stays text-only, as today), and outside Linux
  the jail helper is the resolved path of this program. Nothing changes for
  users yet.

### Changed

- The AI engines menu now marks a detected engine that has no chat adapter
  (lmstudio, llamacpp, vllm, opencode) as `detected · not in chat yet`
  instead of `available · <version>`, so it no longer reads as usable. The
  note comes from the adapter table, so it disappears when an adapter lands.

### Fixed

- The chat no longer fails with `EAI_AGAIN` (or hangs) on WSL2 and on systems
  where `/etc/resolv.conf`, `/etc/hosts`, `/etc/nsswitch.conf` or
  `/etc/localtime` link outside `/etc` (WSL's `/mnt/wsl/resolv.conf`,
  systemd-resolved). The Linux jail now grants the exact target of each link
  read-only, never its folder, for claude, codex and gemini (#267).
- Memory now offers to start engram on WSL2 with `networkingMode=mirrored`
  instead of saying `memory unavailable: engram did not answer`. There a
  connect to a closed loopback port outside WSL's ephemeral range is dropped
  instead of refused, so a timed-out connect now checks whether anything holds
  the port: a free port means engram is not running; a held one is still a
  server that did not answer, so a second server is never offered.

## [0.17.1] - 2026-10-18

### Changed

- On systems without the jail (macOS), codex and gemini now say what to do
  instead. The menu row and the model list read `needs Linux · use claude`;
  the chat and the engine guidance read `the CLI jail needs Linux; on this
  system use claude or Ollama for chat`. claude keeps the neutral text, since
  it runs without the jail there (#180).

### Fixed

- A claude (or codex, or gemini) older than lazarobox needs no longer ends a
  turn with a bare `error: unknown option '...'` (#180). When the last line
  the CLI wrote is that error, the chat now says the CLI is too old and how to
  update it. The option is named only when it is one of the flags lazarobox
  itself passes (`claude does not know the option --permission-prompts: this
  claude is older than lazarobox needs; update it (claude update) and try
  again`); for any other word the message does not repeat it. The secret
  redactor is unchanged. No flag is dropped: the text-only options stay on.
- **claude older than 2.1.259 no longer fails with `unknown option`** (#180).
  The text-only argv needs `--permission-prompts none`, added in claude
  2.1.259. The chat now refuses a turn with a detected claude below that
  version, with the message `claude <version> is too old for lazarobox (it
  needs 2.1.259 or newer); update it: ...; then refresh from the menu
  (Ctrl+Space m, then r)`, and the engine row in the menu
  reads `update needed: 2.1.259+`. A version that cannot be read does not
  block. The text-only flags are never dropped to fit an old claude.

## [0.17.0] - 2026-10-18

### Added

- **Your own MCP servers in the chat.** Declare them in the user config as
  `[[ai.mcp.servers]]` (`name`, `command`, `args`, `env`, optional `project`
  and `enabled`). Global entries apply everywhere; a `project` entry applies
  only to that git root, compared as a real (canonical) path, and an entry
  with `enabled = false` turns a global server off for that project. A
  repo-local file is never read.
  - A server starts only for a chat turn with tools on, and only for an
    engine that will get the tools: Ollama, or a CLI that runs jailed. Each
    start is announced (`mcp: started <name>`); a server that fails gives one
    notice and the others keep working. Servers stop when the chat closes, on
    a turn with tools off and on quit.
  - **Every call asks `y`, even in Chat approval mode `auto`.** The prompt
    shows the server, the tool and every argument key, and says how much of a
    long value is hidden. Arguments are capped at 4 KiB.
  - Results come back wrapped as untrusted data and capped. Tool names are
    qualified as `<server>__<tool>`; built-in tools are never shadowed and
    colliding names are reported.
  - `env` lists variable names only; values are refused. Servers run with a
    scrubbed environment, `HOME` as their working directory, no shell, and
    never receive the session token or `LD_*`/`DYLD_*` variables.
- Releases are published by a GitHub workflow when a `vX.Y.Z` tag is pushed:
  it checks the version and CHANGELOG, runs the tests, publishes to crates.io
  through Trusted Publishing and creates the GitHub Release.

### Fixed

- macOS: the chat's read tools (`read_file`, `list_dir`, `find_dir`) always
  failed, because the random delimiter that wraps a tool result was only
  generated on Linux. It is now read from `/dev/urandom` on every system.
- macOS: the `/private/tmp` and `/private/var/folders` scratch folders are
  now treated as untrusted like `/tmp`, process names are read with `ps`
  where there is no `/proc`, and the MCP shim handshake is always bounded.
- CI runs the tests on Linux and on macOS (arm64 and Intel), with a pinned
  toolchain and an MSRV check.

## [0.16.2] - 2026-10-18

### Fixed

- The update now installs exactly the version the **Updates** row announces
  (`cargo install lazarobox-shell --locked --version <version> --root <root>`).
  It used to install whatever was newest on crates.io, so when that was the
  version already installed cargo did nothing, exited 0 and lazarobox still
  said "Updated to X". Now cargo installs exactly that version or fails,
  instead of installing a different one. A version that is not plain
  `major.minor.patch` is never passed to cargo and the command to run by hand
  is shown instead. The command shown for copying is unchanged: it asks for
  the newest version.

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

[Unreleased]: https://github.com/pichu2707/lazarobox-shell/compare/v1.0.1...HEAD
[1.0.1]: https://github.com/pichu2707/lazarobox-shell/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/pichu2707/lazarobox-shell/compare/v1.0.0-rc.1...v1.0.0
[1.0.0-rc.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.30.0...v1.0.0-rc.1
[0.30.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.29.0...v0.30.0
[0.29.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.28.0...v0.29.0
[0.28.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.27.0...v0.28.0
[0.27.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.26.0...v0.27.0
[0.26.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.25.1...v0.26.0
[0.25.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.25.0...v0.25.1
[0.25.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.24.0...v0.25.0
[0.24.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.23.0...v0.24.0
[0.23.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.22.0...v0.23.0
[0.22.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.21.0...v0.22.0
[0.21.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.20.0...v0.21.0
[0.20.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.19.0...v0.20.0
[0.19.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.17.2...v0.18.0
[0.17.2]: https://github.com/pichu2707/lazarobox-shell/compare/v0.17.1...v0.17.2
[0.17.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.17.0...v0.17.1
[0.17.0]: https://github.com/pichu2707/lazarobox-shell/compare/v0.16.2...v0.17.0
[0.16.2]: https://github.com/pichu2707/lazarobox-shell/compare/v0.16.1...v0.16.2
[0.16.1]: https://github.com/pichu2707/lazarobox-shell/compare/v0.16.0...v0.16.1
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
