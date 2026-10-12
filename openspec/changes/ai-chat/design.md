# Design: AI Chat (P2 chat pane, all engines, topic memory)

Revision 4 (2026-10-12, user-approved scope change "thinking indicator"): a statusline thinking bar (CHAT-12) and reasoning shown folded in the chat pane through a new `ThinkingDelta` engine event (CHAT-13); reasoning is never stored or sent back; rulings R30-R32 at the end. Project memory is out of scope (follow-up change `engram-memory`). Revision 3 (2026-10-12, tasks phase): SQLite journal mode is DELETE (R14 overridden by the user) and the jail grants execute per file only (R29). Revision 2 applies the user's design review (engram `sdd/ai-chat/question-round-design`):

- the default profile is `programmer`;
- codex and gemini are back in P2, behind an OS jail (CHAT-11);
- the spec reconciliation is at the end.

## Technical Approach

The change keeps the functional core / imperative shell split used by ai-engines and ai-selector.

- **Pure core.** `core::ai::chat`, `core::ai::sanitize`, `core::ai::http` and `core::ai::jail` hold:
  - the chat state machine, the composer and the transcript wrap;
  - the engine contract (request and argv builders, stream decoders);
  - the stateful sanitizer and the incremental HTTP framing;
  - the history cap, the prompt resolution and the tag cleaner;
  - the jail allow-list builder.

  None of this does IO.
- **`App`** stores a pane kind (`PaneContent::{Pty, Chat}`), and every PTY-only path branches on it. A chat pane owns its `ChatState`, so the state survives COPY, tab switches and focus changes, and a stream keeps running in the background (D5).
- **Runtime.** It follows the detection template.
  - `Effect::ChatSend` / `ChatCancel` start or abort one tokio task per pane. The task sends `AppEvent::Chat(pane, turn, StreamEvent)` over an mpsc channel.
  - `Effect::Store` goes to one dedicated SQLite thread, which answers with `AppEvent::Store`.
  - A single animation tick runs only while a stream is active and drives the statusline thinking bar (CHAT-12).
- **Transports.**
  - The server engines use a loopback-only HTTP/1.1 POST client.
  - The CLI engines run as a child process: no shell, the transcript on stdin, a neutral cwd, their most restrictive documented flags, and, on Linux, inside a Landlock jail.

The proposal (D1–D18) and the user rulings are authoritative. ADRs use the `CHAT-` prefix. Like the ai-selector design, this text is over the 800-word budget on purpose: the orchestrator asked for 11 decision areas, per-CLI verdicts and a reconciliation.

## CLI verdicts (captured `--help`, 2026-10-12)

**The user's rule.** A CLI connection must give the same guarantee as an API connection: text only, with no access to the user's files. The flags are a first layer. The guarantee itself comes from the jail (CHAT-11).

| CLI | Headless and stream flags (help text) | Most restrictive mode flags | Tool-less from flags alone? | Continuity / persistence | Status |
|---|---|---|---|---|---|
| claude 2.1.296 | `-p, --print`; `--output-format stream-json` ("realtime streaming", only with --print); `--include-partial-messages` | `--tools ""` ("Use "" to disable all tools"); `--strict-mcp-config` (no `--mcp-config`); `--safe-mode` (CLAUDE.md, skills, plugins, hooks and MCP disabled; "Auth, model selection … work normally"); `--disable-slash-commands`; `--permission-prompts none` ("anything that would prompt is denied automatically") | **Yes** | Transcript resent on stdin. `--no-session-persistence` ("sessions will not be saved to disk") | Ships, jailed as a **second layer** on Linux. Without a jail it still runs |
| codex-cli 0.142.5 | `exec` ("Run Codex non-interactively"); `[PROMPT]` "if `-` is used, instructions are read from stdin"; `--json` "Print events to stdout as JSONL" | `-s, --sandbox read-only`; `--ephemeral` ("Run without persisting session files to disk"); `--ignore-user-config` ("Do not load `$CODEX_HOME/config.toml`; auth still uses `CODEX_HOME`", which drops user MCP servers); `--ignore-rules`; `--skip-git-repo-check` (the neutral cwd is not a repo); `-C <cwd>` | **No.** read-only still runs "model-generated shell commands" | Transcript resent on stdin; `--ephemeral` | Ships **only jailed**: Linux-only for now, "not available on this system yet" elsewhere |
| gemini 0.63.0 | `-p, --prompt` "non-interactive (headless) … Appended to input on stdin"; `-o, --output-format stream-json` | `--approval-mode plan` ("(read-only mode)"). **Never** `--raw-output` or `-y` | **No.** read tools still run; extensions and MCP can add tools, and `-e` has no documented "none" | Transcript on stdin plus a fixed `-p` text. **No documented flag stops session persistence** | Ships **only jailed**, as codex. Its session dir is its only writable place, and the README must say so |

**Exact argv** (pure tests assert each one):

- claude: `-p --output-format stream-json --include-partial-messages --tools "" --strict-mcp-config --safe-mode --disable-slash-commands --permission-prompts none --no-session-persistence --system-prompt <prompt> [--model=<m>]`
- codex: `exec --json -s read-only --ephemeral --ignore-user-config --ignore-rules --skip-git-repo-check -C <cwd> [--model=<m>] -`
- gemini: `-p "Answer the last user message of the conversation given on stdin." -o stream-json --approval-mode plan [--model=<m>]`

codex and gemini have no system-prompt flag, so for them the profile prompt goes first on stdin, inside an `Instructions:` block.

**Common rules for all three CLIs.**

- **Program.** The absolute path from `find_on_path`. There is no shell.
- **Process group.** `process_group(0)` and `kill_on_drop`, with `GroupKill` (moved to `runtime/proc.rs`).
- **stdin.** It gets the capped transcript and is then closed (EOF).
- **stderr.** Capped at 16 KiB. Only its sanitized last line is shown.
- **cwd.** A private empty `$XDG_DATA_HOME/lazarobox/cli-cwd` (0700). `-p` "skips the workspace trust dialog", so a repo cwd would let `CLAUDE.md`, `AGENTS.md` or project settings inject.
- **Environment.** Inherited plus `NO_COLOR=1` and `TMPDIR=<private tmp>`, minus `SSH_AUTH_SOCK` and `GPG_AGENT_INFO`.
- **Model.** Always `--model=<m>`. A value starting with `-` is rejected, and `default` omits the flag.
- **argv content.** History never goes into argv, because argv is visible in `ps`. The claude system prompt does, and the README says so.

## Architecture Decisions

| # | Topic | Options | Decision / rationale |
|---|---|---|---|
| CHAT-1 | Pane-kind seam | overlay view; **pane kind** | `PaneState { content: PaneContent, size }`, with `PaneContent { Pty(PtyPane{emu, proc_cwd}), Chat(Box<ChatState>) }` and accessors.<br>**Branches:**<br>- **spawn:** `split`/`new_tab` take `NewPane::{Shell, Chat}`; only Shell emits `SpawnPane`.<br>- **key/paste:** routed to the chat. The prefix key is checked first, and a second Ctrl+Space on a chat is swallowed.<br>- **resize:** `relayout` updates `size` for both kinds; `emu.resize` and `ResizePty` are for Pty only.<br>- **close:** `remove_pane`, `drop_tab` and the confirm emit `ChatCancel` (never `ClosePane`) for a chat.<br>- **cwd:** `Cwd`/`Pty` events for a chat id are dropped, and the poll walks PTY handles only. A chat's effective cwd is its `origin_cwd`, which also drives inheritance and the tab label.<br>- **COPY:** `Prefix [` on a chat is refused with the spec notice. vt100 is never touched, and `set_scrollback(0)` is for Pty only.<br>- **render:** `App::pane_view(id) -> Option<PaneView>`.<br>**C1** adds a `#[cfg(test)]` constructor, and the `Chat` variant carries `#[cfg_attr(not(test), expect(dead_code))]` until C8. Snapshots stay byte-identical. |
| CHAT-2 | ChatEngine contract | `async_trait` objects; **pure builders + enum dispatch** | `enum Adapter { Ollama, OpenAi, Claude, Codex, Gemini }` with `adapter_for(id)`.<br>- `build(&ChatRequest) -> Invocation::{Http{port, path, body} \| Process{binary, args, stdin, jail: JailPolicy}}`.<br>- `decoder() -> Decoder` (NDJSON, SSE, claude stream-json, codex JSONL, gemini stream-json), with `feed`/`finish`.<br>- Events: `StreamEvent::{Slow, Delta, ThinkingDelta, Done(Stop\|Length), Error(ChatError)}` (`ThinkingDelta` is a rev 4 addition, scheduled in PR8 because C2 is already merged; see CHAT-13).<br>**Timeouts:** server: connect 2 s, first byte 120 s, idle 60 s. CLI: first byte 120 s, idle 90 s. Both: total 15 min. The task sends `Slow` at **3 s** without a first byte.<br>**Cancel:** the app marks the turn cancelled, keeps the partial answer, bumps `turn` (stale events are dropped) and emits `ChatCancel`. The runtime aborts the task: the socket is closed, or the group is killed and reaped within 1 s.<br>**History:** at most 40 messages and a per-engine character cap (12,000 for local servers, 100,000 for CLIs). Oldest messages are dropped first. The system prompt is pinned, and the newest message is always sent.<br>**Caps:** composer 8,000 chars; answer 1 MiB (then an error that keeps the partial, stored as `truncated`); 400 messages per conversation. |
| CHAT-3 | Streaming loopback HTTP | reqwest (proxy env); **hand-written client** | `runtime/loopback.rs::post_stream`, to `SocketAddr::from(([127,0,0,1], port))`.<br>- The port comes from `CATALOG` only. The paths are static: `/api/chat` and `/v1/chat/completions`.<br>- `POST … HTTP/1.1` with `Host`, `Content-Type`, `Content-Length` and `Connection: close`. The body is built with serde_json.<br>- The pure `parse_head` (head of 16 KiB or less) keeps ai-engine-detection's strict framing. An incremental `Dechunker` survives any split point.<br>- `NdjsonSplitter` and `SseParser`: per-line cap 1 MiB for servers, 2 MiB for CLI decoders (the claude `result` event repeats the whole answer).<br>- Only status 200 is accepted. Otherwise up to 64 KiB is read, and the error message is sanitized and cut to 200 chars. No redirect is followed.<br>- Detection stays a GET over HTTP/1.0 and is "read-only". The chat may POST only to the two static paths. |
| CHAT-4 | Sanitizer | stateless per chunk; **stateful** | `core::ai::sanitize`. A `Utf8Stream` byte front end carries an incomplete character across chunks; invalid bytes become U+FFFD, and so does a cut tail at finish. It feeds `StreamSanitizer`, whose states (Ground, Esc, Csi, Str, StrEsc) persist across deltas.<br>- Kept: `\n`, `\t`. CRLF becomes LF.<br>- Dropped: a lone CR, C0, DEL, C1 (including U+009B, dropped one char at a time; none of them starts a sequence), CSI (params and intermediates capped at 256 chars), OSC/DCS/SOS/PM/APC up to BEL or ST, two-char ESC sequences, and `is_format` (moved here).<br>- An unterminated string returns to Ground after 4,096 swallowed chars in total (the count carries across restarts and resets only when ordinary text is emitted) and is dropped at finish.<br>- CSI parameters, ESC intermediates and repeated ESCs are each capped at 256 chars (`CSI_CAP`); past it the rest shows as plain text.<br>`parse.rs` uses `clean()` with the same output. The sanitizer runs in the core before text is stored, drawn or copied, and on stderr and error text. Rendering uses Spans only. |
| CHAT-5 | SQLite store | system lib; sqlx; redb/JSON; **rusqlite `bundled`** | **Crate:** rusqlite, pinned in C6. It needs a C compiler and about 5 s (measured 4.7 s debug, 9.3 s release) on a clean build.<br>**Schema v1** (`user_version` migrations; a newer version means *unavailable*):<br>- `conversations(id, created_at, updated_at, profile CHECK, title)`<br>- `messages(conversation_id FK CASCADE, seq, role, content, engine, model, status complete\|cancelled\|interrupted\|truncated\|error, created_at, UNIQUE(conv,seq))`; "partial" means a status other than `complete`<br>- `tags(conversation_id, tag, is_project) WITHOUT ROWID`<br>- indexes on `tags(tag)` and `updated_at`<br>**Path:** `$XDG_DATA_HOME/lazarobox/chat.db` (an absolute XDG value), else `$HOME/.local/share`. The directory is 0700.<br>**Modes:** the umask is never touched, because PTY children inherit it. The db is pre-created 0600 with O_NOFOLLOW, and SQLite gives any side file (`-journal`, and `-wal`/`-shm` should they ever appear) the db's mode. Each open checks the db and every side file that exists with `symlink_metadata`: a symlink or a foreign owner means unavailable, and a wider mode is chmod'ed to 0600.<br>**Pragmas:** `secure_delete=ON`, `foreign_keys=ON`, `trusted_schema=OFF`, `busy_timeout=2000`, **`journal_mode=DELETE`** (user ruling, R14 overridden: no WAL, so there is no `-wal` file to leak deleted pages and no checkpoint task exists). The rollback journal is removed after each transaction; several lazarobox instances serialize on `busy_timeout`.<br>**Retention:** 200 by last activity, applied in the create transaction. Conversations open in any pane are skipped, and the spec's system line is shown.<br>**Threading:** one `std::thread` owns the `Connection`. Commands arrive over std mpsc and replies leave over tokio mpsc, in FIFO order. Appends that arrive before `Created` wait in the pane. On quit, Cancel and Append are flushed before `Quit`, with a 2 s join.<br>**Degrade:** the chat works in memory and shows a system line. The file is never deleted. |
| CHAT-6 | Project tags | `git rev-parse`; **filesystem walk** | An injected `repo_root: fn(&Path) -> Option<PathBuf>` keeps `App` pure. The walk goes up at most 64 levels with `symlink_metadata(dir/.git)`, and git is never run.<br>- A `.git` directory: the root is that dir.<br>- A `.git` file whose `gitdir:` (≤1 KiB) contains `/.git/worktrees/`: the main repo, so the worktrees of one project share a tag.<br>- Any other `.git` file: its dir.<br>The tag is the basename of the root, else of the cwd. An unknown cwd gives no tag. **Cleaning (spec):** trim, strip control and format characters, at most 64 chars. Duplicates are compared case-insensitively. At most 8 extra tags. |
| CHAT-7 | Chat pane UI | new `InputMode`s; **views in `ChatState`** | Views: List, Conversation, TagEdit and the delete confirmation. `InputMode` is unchanged.<br>- **Open:** root prefix **`c`** ("chat") splits right like `w v` and opens List.<br>- **List:** filtered by project, newest first. Rows are `YYYY-MM-DD · profile · first message…`. Keys: j/k/↑/↓, Enter resumes, `n` or Esc starts new, `d` then `y` deletes, `t` tags, `a` toggles all projects. A conversation already open elsewhere is refused.<br>- **Composer:** Enter sends. Alt+Enter or Ctrl+J inserts a newline. Editing keys: Backspace, Delete, ←/→, Home/End, Ctrl+U. Height up to 5 rows.<br>- **Transcript:** a pure wrap with `unicode-width`, cached by (width, generation). PgUp/PgDn scroll, with follow at the bottom. Fenced code is drawn in a distinct style, and an unclosed fence is code to the end.<br>- **Esc/Ctrl+C:** cancel while streaming; nothing when idle (never Quit, never WritePty).<br>- **Other keys:** Ctrl+Y yanks the last answer, **Ctrl+B** the last code block (design addition). Ctrl+T opens the tag editor. **Ctrl+R** (rev 4, proposed default, open question) toggles the most recent reasoning block.<br>- **Statusline:** `CHAT`, the origin cwd, and `engine › model` or `no engine`; plus the thinking bar while a stream runs (CHAT-12). |
| CHAT-8 | Prompt profiles | open list; **two fixed** | `Profile { Analyst, Programmer }`, **default `programmer`** (user ruling). The key is `[ai] profile` (owned key `ai.profile`), and the menu row comes after the six task rows.<br>**Built-ins:** 10 files, `src/core/ai/chat/prompts/<profile>/<task>.md`, through `include_str!`. Each is English, at most 4 KiB, tells the model to reply in the user's language, and the chat prompt states that there is no access to files, a shell or the terminal.<br>**Overrides:** `<config dir>/prompts/<profile>/<task>.md`, at most 16 KiB, regular UTF-8 files read with a bound. They are cleaned, and empty ones are ignored.<br>**Order (spec):** override for the task, then override for general, then the built-in for the task.<br>The prompt is resolved **once per conversation**, at start or resume, from the conversation's stored profile. The runtime reads the files and the pure `resolve_prompt` decides. |
| CHAT-9 | Engine resolution | — | `availability(&AiConfig, Option<&Detection>, JailSupport)` through `resolve(Task::Chat)`, evaluated on render and on every send. The states:<br>- Unassigned<br>- Detecting (`checking engines…`)<br>- NotDetected<br>- Unsupported (opencode, or an adapter not landed yet)<br>- **NoJail** for codex/gemini without a working jail: `<engine>: not available on this system yet`<br>- GatedOff, kept only for a CLI whose manual check fails<br>- NoModel and ModelMissing (`download required`)<br>- Ready<br>The engine is never switched. |
| CHAT-10 | Security | — | - Server traffic is loopback-only by construction.<br>- tracing gets no content.<br>- Override files are untrusted.<br>- CLIs run under the common rules, the most restrictive modes and the jail.<br>- lazarobox never opens a credential **file** (CHAT-11 resolves paths by existence and holds only O_PATH handles on directories, or on the claude config file).<br>- Output is sanitized before it is stored or drawn.<br>- The README covers: the db is unencrypted, CLI conversations go to the vendor, gemini keeps its sessions, the system prompt is visible in `ps`, and the per-OS availability. |
| CHAT-11 | CLI jail | flags only; containers/bwrap (extra dependency); seccomp; **Landlock**, via a re-exec helper | **Abstraction.** The core holds `JailSpec { rx, ro, rw, rw_files, ro_files }` (pure, built from resolved paths), `JailPolicy::{Required, Layered}` (codex/gemini Required, claude Layered) and `JailSupport::{Available{backend, abi}, Unavailable(reason)}`. The runtime holds `enum JailBackend { Landlock }`, with `Seatbelt` added later for macOS by rendering the same `JailSpec` as an SBPL profile for `sandbox-exec`. Any OS without a backend gives `Unavailable`: codex and gemini show NoJail, while claude, Ollama and the servers work.<br>**Probe.** At startup, one raw `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)` gives the ABI. Below **ABI 3** (kernel 6.2, TRUNCATE) counts as Unavailable.<br>**Mechanism.** The child is `/proc/self/exe __jail-exec --rx P … --rw P … -- <abs cli> <args>`. `main.rs` turns into a sync `main` that dispatches `__jail-exec` before the tokio runtime is built. The helper then:<br>1. builds the ruleset with the **`landlock` crate** (0.4.x, exact version pinned in C13a), handling every FS right of the probed ABI as a `HardRequirement`, plus `Scope` (signals, abstract unix sockets) when ABI ≥ 6;<br>2. calls `restrict_self()` and requires `RulesetStatus::FullyEnforced` and `no_new_privs`. **Partial or none counts as not jailed**: it exits 125 with `jail not enforced`;<br>3. `exec`s the CLI.<br>`pre_exec` was rejected: after `fork` in the multi-threaded tokio process, the ruleset building allocates, which is not async-signal-safe. The helper is single-threaded, testable from an integration test, and execing `/proc/self/exe` survives a binary upgrade.<br>**Network** is not handled (no `AccessNet`), so the CLIs reach their APIs.<br>**Allow-list:**<br>- *Common:* `/usr`, `/bin`, `/lib`, `/lib64` **read-only, NO execute** (when present; shared libraries still load because mmap needs only read); `/etc` ro; `/proc/self`, `/proc/{cpuinfo,meminfo,stat}` and `/sys/fs/cgroup` ro; `/dev/null` rw; `/dev/urandom`, `/dev/random` ro; the private cwd ro; a private `TMPDIR` (`…/lazarobox/cli-tmp/<pane>-<turn>`, 0700, wiped after the turn) rw.<br>- *Per CLI (the ONLY execute grants, R29):* the **package root** of the canonical CLI path, rx (`…/node_modules/<pkg>` or `@scope/pkg`, else the parent dir of the canonical binary). The **interpreter binary**, rx **as a single file** (exact canonical path, for example the nvm node binary): the shebang is read from the first 256 bytes of the CLI file, which is an executable and not a credential; `env node` is resolved through `find_on_path` and canonicalized. Plus the **specific helper binaries** manual check M14 proves necessary, each by exact path and never a whole directory. `/usr/bin/env` itself is not granted: the helper execs the resolved interpreter directly.
- *Execute policy (R29, user-approved amendment):* EXECUTE (execve) is allowed per file, only on the three grants above. A jailed codex or gemini cannot execve ssh, curl, bash, sh or any other program, even when the user asks it to, which matches the user's rule "same guarantee as an API connection". Network stays unrestricted. If a CLI cannot work without a general shell or a wider execute right, its slice is GATED (`gated off`) and the user is told; the allow-list is never widened to a directory.<br>- *Own state, rw* (tokens refresh, logs): claude `~/.claude/` + `~/.claude.json` (file rule); codex `$CODEX_HOME` else `~/.codex/`; gemini `~/.gemini/`.<br>- *Denied:* everything else, including `$HOME`, projects, `~/.ssh`, `/tmp` and other CLIs' dirs.<br>**Paths without reading credentials.** Paths are built from `$HOME`/`$CODEX_HOME` and checked with `symlink_metadata`. A missing dir gets no rule, and the CLI then reports its own login error. The rules hold `O_PATH` fds, so no content is read. Only the CLI's shebang line and the worktree `gitdir` pointer are ever read.<br>**Policy:**<br>- codex/gemini: no jail means they refuse (NoJail), and the helper's exit 125 becomes `ChatError::Jail`.<br>- claude: jailed when Available; when Unavailable it runs bare, because its flags already guarantee text-only.<br>**Residual risks:**<br>- Landlock does not restrict `connect()` to **pathname** unix sockets (dbus, agents). `SSH_AUTH_SOCK`/`GPG_AGENT_INFO` are scrubbed, but `DBUS_SESSION_BUS_ADDRESS` is kept for keyring logins.<br>- A jailed tool can read its own CLI's token dir and echo it into an answer, which then lands in chat.db.<br>- **Gemini persists sessions in `~/.gemini`**, its only writable place. Those copies are outside chat.db, are not removed by delete or retention, and are not under our 0600 guarantee. The README must state it. |
| CHAT-12 | Thinking bar in the statusline (rev 4) | `indicatif`; text-only "thinking…" line in the pane; **ratatui widget in the statusline driven by an animation tick** | **Decision.** A pure `ThinkingView { frame, phase, model, elapsed, extra }` is built from the chat panes by a pure `pick_stream` and drawn by a ratatui widget in `ui/components/statusline.rs`.<br>**Why not `indicatif`:** it writes to the terminal directly, which conflicts with ratatui's buffered frame and its backend, and a widget in the frame is testable with `TestBackend` and insta while a direct writer is not.<br>**Parts and styles:** scanner bar (lit `▰` segment over a `━` track), braille spinner `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, phase (`thinking` until the first answer text, then `answering`), model, elapsed (`Ns`, then `MmSSs`). Spinner and lit segment `ai_purple`; the rest `text_muted`. Position and glyph are pure functions of the frame; elapsed is computed from the start instant.<br>**Which stream:** the focused chat pane if it streams, else the most recently started (monotonic start counter), plus `+N` for the others.<br>**Tick:** one runtime interval (proposed 100 ms, so 10 fps, hard bound 12) that exists only while `active_streams > 0`: armed on the first `ChatSend`, disarmed within one period after the last stream ends (final, error, cut, cancel, pane or tab close, quit). Missed ticks are skipped, not replayed. Each tick bumps the frame and requests one redraw. With no stream there is no timer, so no idle CPU cost. Tested with tokio's paused clock.<br>**Placement and narrow widths:** inside the existing statusline layout; the mode block has priority; whole parts are dropped in the order bar, model, elapsed, `+N`, then spinner and phase last. Exact placement among the path and right segments is left to the C8 implementation within those rules.<br>**Integration:** it extends the PR9 chat statusline variant (the `CHAT` label and the engine segment are untouched). The model shown is the model of the request as sent. |
| CHAT-13 | Reasoning events and block (rev 4) | text-only tokens; reasoning stored with the answer; **distinct `ThinkingDelta` event, shown folded, never stored** | **Seam.** `StreamEvent::ThinkingDelta(String)` is added to the C2 seam. C2 (PR2) is merged, so the change is a NEW task in PR8 (the earliest unstarted PR); the exhaustive matches on `StreamEvent` in merged code (`runtime/chat.rs`, `app.rs`) get an arm in the same PR so the build stays green.<br>**State.** `ChatState` keeps, per turn, a reasoning buffer with its own `StreamSanitizer` (so an unterminated escape in reasoning cannot swallow answer text), the instant of the first thinking delta, a collapsed or expanded flag and a phase (`Waiting`, `Thinking`, `Answering`). First non-empty text delta (or `Done` with no text) collapses the block once and records N.N seconds; later thinking deltas are dropped.<br>**Render.** While streaming: `text_muted` lines with the `┊ ` gutter, wrapped at pane width minus the gutter, no markdown or fence parsing. Collapsed: `▸ reasoning (N.Ns)`. Expanded: `▾ reasoning (N.Ns)` plus the gutter text. Key: **Ctrl+R** toggles the most recent block (no effect while it streams or when none exists).<br>**Not persisted, not history.** The store commands never carry reasoning (`Effect::Store` is built from answer text only), `ChatRequest` history is built from stored message texts only, and a resumed conversation has no reasoning. A turn with reasoning and no answer text stores no assistant message.<br>**Cap.** 256 KiB of reasoning per turn, extra dropped without an error (assumption to confirm).<br>**Timeouts.** A thinking delta counts as the first byte and as idle activity and stops the `Slow` timer.<br>**Adapters.** Ollama: the streamed message `thinking` field; the request body stays as pinned (no `think` option unless M7 shows it is required, open question). claude: `thinking` content blocks in stream-json, only if the pinned flags make the CLI emit them (M2). OpenAI-compatible, codex and gemini: only if their fixtures show a reasoning field ("if available"); no format is assumed.<br>**Cut, cancel, error while thinking:** the partial reasoning is kept collapsed; the existing system lines are unchanged. |

## Data Flow

```
Prefix c ─> split(NewPane::Chat) ─> List ─> Store(List{tag}) ─> store thread ─> Store(Listed)
Enter ─> availability()==Ready ─> Store(Create|Append user) + ChatSend{pane,turn,req,profile}
runtime/chat: Adapter::build ─> Http(post_stream)
                            └─> Process ─> [Linux & policy] /proc/self/exe __jail-exec … -- cli
   bytes ─> Decoder ─> AppEvent::Chat(pane,turn,ev) ─> turn check ─> Utf8Stream+StreamSanitizer ─> transcript
                                        ThinkingDelta ─> (own sanitizer) ─> reasoning block (never to Store, never to history)
Done/Error/Cancel ─> Store(Append assistant{status,engine,model})   (answer text only)
active_streams>0 ─> animation tick ─> frame+1, one redraw ─> statusline thinking bar (none when idle)
```

## File Changes

| File | Action | Description |
|---|---|---|
| `src/main.rs` | Modify | Sync `main`; dispatch `__jail-exec` before the tokio runtime |
| `src/app.rs` | Modify | Seam, chat routing, Effects `ChatSend/ChatCancel/Store`, AppEvents `Chat/Store`, `mode_label`, `pane_view`, `repo_root`/`JailSupport` injection |
| `src/core/ai/chat/{mod,composer,transcript,engine,adapters,stream,tags,list,prompts}.rs` | Create | Pure chat core |
| `src/core/ai/chat/prompts/<profile>/<task>.md` (10) | Create | Built-in prompts |
| `src/core/ai/{sanitize,jail}.rs` | Create | Sanitizer (+ `Utf8Stream`); `JailSpec`, policy, package-root and shebang helpers |
| `src/core/ai/{parse,http,config,menu,mod}.rs`, `src/core/{menu,prefix}.rs` | Modify | `clean()`; `parse_head` + `Dechunker`; `Profile`; Profile row; `c` binding |
| `src/runtime.rs`; `src/runtime/{loopback,chat,chat_store,proc,project,jail}.rs` | Modify/Create | Select branches; POST client; turn tasks; SQLite thread; `GroupKill`; repo-root walk; probe + `__jail-exec` helper |
| `src/runtime/ai_detect.rs` | Modify | `GroupKill` moves to `proc.rs` |
| `src/ui/mod.rs`, `src/ui/components/{chat,statusline}.rs` | Modify/Create | `PaneView` dispatch, chat widget (reasoning block), CHAT segments, thinking bar widget |
| `src/core/ai/chat/{mod,engine,transcript}.rs` | Modify | `StreamEvent::ThinkingDelta`, reasoning state and phase, reasoning lines (PR8); `pick_stream` and the `ThinkingView` input (PR9) |
| `src/runtime.rs` | Modify | Animation interval armed only while a stream is active (PR9) |
| `tests/jail.rs` | Create | Linux integration test of the helper |
| `Cargo.toml` | Modify | `rusqlite` (bundled), `unicode-width`, `landlock` (Linux target only) |
| `README.md` | Modify | Privacy, `ps` visibility, gemini sessions, per-OS availability, CLI versions |

## Interfaces / Contracts

```rust
enum PaneContent { Pty(PtyPane), Chat(Box<ChatState>) }                       // app.rs
pub enum Invocation { Http { port: u16, path: &'static str, body: Vec<u8> },
  Process { binary: &'static str, args: Vec<String>, stdin: Vec<u8>, jail: JailPolicy } }
pub enum JailPolicy { Required, Layered }                                     // core::ai::jail
pub enum JailSupport { Available { backend: Backend, abi: u32 }, Unavailable(&'static str) }
pub struct JailSpec { pub rx: Vec<PathBuf>, pub ro: Vec<PathBuf>, pub rw: Vec<PathBuf>,
                      pub ro_files: Vec<PathBuf>, pub rw_files: Vec<PathBuf> }
pub fn jail_spec(cli: CliLayout) -> JailSpec;   // pure; CliLayout = resolved paths, existence-checked
pub enum StreamEvent { Slow, Delta(String), ThinkingDelta(String), Done(Finish), Error(ChatError) }  // ThinkingDelta: rev 4, PR8
pub enum StreamPhase { Thinking, Answering }                                  // chat core; None when idle
pub struct ThinkingView { pub frame: u32, pub phase: StreamPhase, pub model: String,
                          pub elapsed: Duration, pub extra: usize }           // statusline input, pure
Effect::ChatSend { pane, turn, request, profile };  Effect::ChatCancel(PaneId);  Effect::Store(StoreCmd)
```

## Testing Strategy (strict TDD; `cargo test`, `clippy --all-targets`, `fmt --check` on every slice)

| Layer | What | Approach |
|---|---|---|
| Seam (C1) | Existing tests and snapshots unchanged. A chat gets no Spawn, Resize or ClosePane, drops Pty/Cwd events, and COPY is refused | `update` + test constructor |
| Decoders / sanitizer | Every split point; UTF-8 carry; caps; `[DONE]`; error lines; EOF without done; the spec's fuzz invariant | Split loops, tables |
| Adapters | Exact JSON bodies; exact argv for all three CLIs; `default` omits `--model`; a model starting with `-` is rejected; history cap and pinned prompt | Pure |
| Jail (pure) | `jail_spec` for an nvm node layout, a native claude layout and a `CODEX_HOME` override; package-root and shebang parsing; **no path under `$HOME` other than the CLI's own package/state, and never `~/.ssh`** | Pure |
| Jail (Linux) | `tests/jail.rs` runs `CARGO_BIN_EXE_lazarobox-shell __jail-exec` (skipped with a message when the probe says Unavailable):<br>(1) `cat <tmp>/outside/secret` fails with **EACCES** ("Permission denied"), exit ≠ 0;<br>(2) the same file under `--ro <tmp>/outside` is printed;<br>(3) writing into an `--ro` dir fails with EACCES and into `--rw` succeeds;<br>(4) a forced minimum ABI above the kernel's (test-only `--min-abi 99`) exits 125 and never execs;<br>(5) **execve of `/usr/bin/ssh`, `/usr/bin/curl` and `/bin/sh` fails with EACCES** with `/usr`, `/bin`, `/lib`, `/lib64` read-only and one exact helper granted (which runs; shared libraries still load) | Integration |
| Runtime | `post_stream` against `fake_server`; cancel closes the socket; a fake CLI's group is killed within 1 s; codex with `Unavailable` spawns nothing; exit 125 → `ChatError::Jail` | tokio tests |
| Reasoning (rev 4) | `ThinkingDelta` through the state machine: collapse on first text or final, late delta dropped, cancel/error/cut while thinking, own sanitizer state, 256 KiB cap, never in `Effect::Store` or history, resumed has none; decoders map Ollama `thinking` and claude `thinking` blocks (fixtures), every split point; render snapshots (streaming, collapsed, expanded, none) | Pure tables, insta |
| Thinking bar (rev 4) | Pure spinner/scanner by frame, styles from buffer cells, elapsed formats, `pick_stream` rule, drop order at widths 0-80, nothing without a stream; the tick: none when idle, 10 per second while streaming, shared by streams, stops within one period, missed ticks skipped | Pure + `TestBackend` + insta; tokio `start_paused` |
| Store / UI | Modes 0600 for the db and any side file that exists (`-journal`, `-wal`, `-shm`); `journal_mode` is `delete`; marker gone after delete; retention; corrupt file. Snapshots of list, conversation, guidance and tag editor; no panic down to 1x1 | tempdir; insta |

## Migration / Rollout

No migration for existing users: `[ai] profile` is optional and older builds ignore it, and `chat.db` is created on first use. Feature-branch chain on `feat/ai-chat`, auto-chain, about 400 changed lines per slice.

| # | Content | Est. | Depends on | Notes |
|---|---|---|---|---|
| C1 | Seam | ~350 | — | |
| C2 | Chat core: state, contract, availability, history cap | ~400 | C1 | |
| C3 | `Utf8Stream` + sanitizer + NDJSON/SSE | ~400 | — | pure, reorderable |
| C4 | `parse_head`, Dechunker, `post_stream`, timeouts | ~400 | — | reorderable with C3 |
| C5 | Runtime chat task, Effects/Events, cancel | ~350 | C2 | |
| C6 | SQLite store | ~450 | C2 | fallback C6a/C6b; reorderable after C2 |
| C7 | UI: composer, transcript, code blocks, `ThinkingDelta` seam and reasoning block | ~550 | C1, C2 | rev 4 +~150; pre-planned split 8a composer / 8b transcript, widget, reasoning |
| C8 | Guidance, yank, `c` binding, statusline, thinking bar and tick, Ctrl+R | ~500 | C7 | the variant goes live; rev 4 +~150; split 9a keys / 9b guidance and CHAT segments / 9c bar and tick |
| C9 | List, tags, resume, delete, editor | ~400 | C6, C8 | fallback C9a/C9b |
| C10 | Profiles (10 built-ins) | ~450 | C2 | prompt text counts; reorderable after C2 |
| C11 | Ollama (+ `thinking` field) | ~340 | C3, C4, C5, C8 | rev 4 +~40 |
| C12 | OpenAI-compatible (+ reasoning only if available) | ~300 (+~30 if available) | C11 | |
| **C13a** | **Jail (Landlock): core `JailSpec`/policy, probe, `__jail-exec`, `main.rs`, `tests/jail.rs`** | **~350** | C5 | fallback C13a-1 pure spec/paths, C13a-2 helper + test. Includes the R29 execute-per-file policy and the execve EACCES test; M12, M13 |
| C13 | claude adapter + `proc.rs` (Layered) (+ `thinking` blocks) | ~390 | C13a, C8 | M1–M4, M11; rev 4 +~40 |
| C14 | codex adapter (Required, Linux-only) | ~250 (+~30 if available) | C13 | **IN.** M5, M9, M14 (GATED if codex needs a general shell) |
| C15 | gemini adapter (Required, Linux-only) | ~250 (+~30 if available) | C13 | **IN.** M6, M10, M14 (GATED if gemini needs a general shell) |
| C16 | README / docs: privacy, `ps`, gemini sessions, per-OS availability, CLI pins | ~150 | C15 | |

## Manual confirmation (tasks phase, on the user's machine)

- **M1.** Does `claude -p` read the prompt from stdin? Does `stream-json` need `--verbose`?
- **M2.** Capture the stream-json and JSONL event schemas of all three CLIs as fixtures. Rev 4: also capture whether each CLI emits reasoning (claude `thinking` content blocks; codex and gemini only if present) with the pinned flags, and record "no reasoning" when it does not.
- **M3.** The claude flags keep the OAuth login working, and a "write /tmp/x" prompt creates nothing and emits no tool events.
- **M4.** `--model=<m>` is accepted by all three CLIs.
- **M5.** `codex exec … -` reads stdin. `--ignore-user-config` keeps auth. `--ephemeral` writes nothing under `~/.codex/sessions`.
- **M6.** gemini reads the transcript on stdin with `-p` appended. Where are its session files?
- **M7.** Live Ollama: a 404 for an unknown model, a cold load. The SSE `[DONE]` of the OpenAI-compatible servers.
- **M8.** Ctrl+J, Alt+Enter, Ctrl+Y, Ctrl+B, Ctrl+T and (rev 4) Ctrl+R reach the app.
- **M9.** A jailed **codex**, asked to "read ~/.ssh/config" and to "list my home", fails or refuses. No file content appears.
- **M10.** The same for a jailed **gemini**.
- **M11.** A jailed **claude** starts, answers and refreshes its token. Check whether an atomic rewrite of `~/.claude.json` needs `$HOME` rights. If the jail breaks claude, claude runs unjailed (Layered) and the README says so.
- **M12.** Run each CLI with `strace -f -e trace=%file` under the jail and confirm the allow-list is enough: node's `/proc`, `/sys`, certs, locale.
- **M13.** Check the kernel ABI value (expected ≥ 6 on 6.19) and the `landlock` crate API (`RulesetStatus`, `Scope`).
- **M14.** (R29) Run `strace -f -e trace=execve` on a jailed **codex** turn and a jailed **gemini** turn and list every program they execve. Add each helper by exact path, never a whole directory. If a CLI cannot work without a general shell, its slice is GATED and the user is told.

## Spec reconciliation (rulings; the tasks phase edits the specs, the specs were not touched here)

| # | Topic | Spec says | Design said | Ruling |
|---|---|---|---|---|
| R1 | Open-chat key | `c` "chat" | `a` | **Spec `c`.** Root hint `… m menu · c chat · q quit` |
| R2 | History cap | 40 msgs / 32,000 chars, same for every engine | 12k / 100k chars | **40 msgs + per-engine chars: 12,000 for local servers, 100,000 for CLIs.** 32k chars overflows a default 4k-token context, which then silently drops the system prompt. The spec allows the refinement; the scenario numbers become per engine |
| R3 | Answer cap | 1 MiB, ends with an error | 256 KiB `Truncated` | **Spec 1 MiB.** Error event, partial kept, stored as `truncated` |
| R4 | Line/event cap | 1 MiB | 256 KiB | **1 MiB for servers. 2 MiB for CLI decoders** (the final `result` event repeats the answer). Spec delta for the CLI decoders |
| R5 | `loading model…` delay | 3 s, after `waiting for the model…` | 5 s | **Spec 3 s**, with the runtime `Slow` event at 3 s |
| R6 | COPY on a chat | Refused, notice `copy mode is not available in chat panes; Ctrl+Y copies the last answer` | No-op + notice | **Spec text** |
| R7 | Yank | Ctrl+Y only | Ctrl+Y + Ctrl+B | **Keep Ctrl+B** (last code block, from the proposal). Add it to the chat-pane Yank requirement and the modal-input table |
| R8 | Override order | task override → general override → built-in task | task override → built-in task → general override → built-in general | **Spec** |
| R9 | Built-ins | 10 (2×5), ≤4 KiB | 4 files | **Spec** |
| R10 | When the prompt is resolved | Once per conversation | Every send | **Spec** |
| R11 | Default profile | `analyst` | `programmer` | **`programmer`** (user). Fix prompt-profiles, configuration and config-menu scenarios |
| R12 | Tag cleaning | Trim, strip control/format, ≤64, case kept | Lowercase charset, ≤40 | **Spec** |
| R13 | Worktree tag | Dir holding the `.git` file (`wt`) | Main repo when `gitdir` contains `/.git/worktrees/` | **Keep the design refinement.** The spec scenario (no gitdir content) still gives `wt`; add a scenario. Both use a basename, not a root path |
| R14 | Journal mode | `-wal`/`-shm` exist, are 0600, checkpoint | `DELETE` | **OVERRIDDEN by the user: `journal_mode=DELETE`, not WAL.** No checkpoint. The spec reads "any SQLite side file that exists (`-journal`, `-wal`, `-shm`) is mode 0600" |
| R15 | Composer | 8,000 chars, 5 rows, Delete key | 32 KiB, 6 rows | **Spec** |
| R16 | Esc when idle | Nothing | Back to the list | **Spec** |
| R17 | List keys / rows | `n`/Esc new, date only, empty texts | "+ New" row, Esc back, HH:MM | **Spec** |
| R18 | Tag editor from a conversation | Ctrl+T | — | **Spec** |
| R19 | Already open elsewhere | Refused with a system line | Unspecified | **Spec.** Retention also skips open conversations (a superset) |
| R20 | Eviction notice | System line `oldest conversation removed (limit 200)` | Statusline notice | **Spec** |
| R21 | Partial marking | Flag | Status enum | **Status enum.** Partial means status ≠ `complete` |
| R22 | Sanitizer input | Bytes, UTF-8 carry, invalid → U+FFFD | Chars, no carry | **Spec**: `Utf8Stream` front end |
| R23 | CLI blocked state | `gated off` / `not available in text-only mode` | GATED-DROPPED | **NoJail**: `<engine>: not available on this system yet` for codex/gemini. `gated off` stays only for a failed manual gate |
| R24 | CLI prompt channel | stdin closed; prompt in argv or a pinned channel | Transcript on stdin | **stdin is the pinned channel**, written then closed. argv never holds messages |
| R25 | CLI timeouts | Unspecified | 120 s / 90 s / 15 min | **Design values**, plus a 15-min total for servers |
| R26 | "Credentials never read" | No decoy file opened | Jail uses `O_PATH` | **Spec wording becomes "never opened for reading"**. O_PATH rule handles on dirs, and on `~/.claude.json`, are allowed |
| R27 | Override reason text | `cannot be read` vs scenario `cannot read` | — | **`cannot be read`** (spec internal fix) |
| R28 | codex/gemini scope | Adapters for all three CLIs | Dropped | **IN, jailed, Linux-only for now** (user) |
| R29 | Jail execute rights (new, user-approved amendment to CHAT-11) | No execute rule | `/usr`, `/bin`, `/lib`, `/lib64` rx | **Execute per file only**: the CLI interpreter binary, the CLI package root and the helpers M14 proves necessary. System dirs read-only with no execute. Spec scenarios added to chat-engines (execve of ssh/curl/sh fails with EACCES [auto, tests/jail.rs]; "ssh to X"/"curl Y" cannot be done [manual]) |
| R30 | Thinking indicator (new, user-approved scope change, 2026-10-12) | No indicator; no reasoning event | - | **Statusline bar (CHAT-12) and folded reasoning (CHAT-13).** The bar is a ratatui widget on an animation tick that runs only while a stream is active (bounded, proposed 10 fps). `indicatif` rejected: it writes to the terminal directly and conflicts with ratatui. New `StreamEvent::ThinkingDelta`; the seam change is a NEW task in PR8 because C2 is merged. Specs edited: chat-engines, chat-pane, chat-history, modal-input, statusline, terminal-session |
| R31 | Reasoning storage (user, 2026-10-12) | - | - | **Never stored, never sent back to the engine, absent on resume.** Raw reasoning is not kept; optimized memory by tags and categories is a separate future change |
| R32 | D15 "Shared or summarized memory" (proposal) | Out of scope | - | **Superseded by the planned follow-up change `engram-memory`**, not designed here. The proposal scope section carries a one-line note |

## Open Questions

- [x] Default profile: `programmer` (user).
- [x] codex/gemini: in, jailed (user).
- [ ] M11 decides whether claude stays jailed on Linux or runs as Layered-bare.
- [ ] (rev 4) Toggle key: Ctrl+R is proposed (mnemonic, same style as Ctrl+Y, Ctrl+B, Ctrl+T); confirm it reaches the app in Kitty (M8) or pick another.
- [x] (rev 4) Does Ollama need a `think` request option for reasoning models to stream `thinking`? ANSWERED in PR13 (2026-10-12, real Ollama 0.30.10): NO. `qwen3:14b` streams `message.thinking` (with an empty `content`) on lines with no `think` field in the request, then the answer lines carry `content` only; `llama3.2:3b` has no `thinking` key. The body stays pinned. Fixtures: `tests/fixtures/ollama/`. M7 still confirms in the pane.
- [ ] (rev 4) Does claude stream-json emit `thinking` blocks with the pinned flags (M2)? If not, the claude parsing task becomes a documented no-op.
- [ ] (rev 4) Confirm the 256 KiB reasoning cap, the 100 ms tick, the scanner width and sweep style, and the exact placement of the bar in the statusline.
- [ ] (rev 4) Only the most recent reasoning block is toggleable; older blocks stay collapsed. Confirm.
