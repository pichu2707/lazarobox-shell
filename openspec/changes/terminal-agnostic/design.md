# Design: Terminal-agnostic lazarobox

## Technical Approach

Follow the existing split: pure decisions in `src/core/*`, thin IO in `src/runtime/*`, the app reacts to `AppEvent`s and emits `Effect`s. Every new piece copies a pattern that already ships:

| New piece | Pattern copied |
|---|---|
| Clipboard tool choice | `core/opener.rs` `candidates()` + `runtime/opener.rs` `Opener::find` (PATH lookup once at startup) |
| Clipboard result back to the app | `open_failed` channel (`runtime.rs:325`, `:1057`) -> `AppEvent` |
| Probe timeout tick | `memory_watch` = `anim::Animation::every` + `sync(app.flag())` (`runtime.rs:945-949`) |
| Probe result file | `update::cache_path` (`runtime/update.rs:44`) and `engram::serve::state_dir` (`runtime/engram/serve.rs:49`) |
| Alias | one more `Binding` in `PREFIX_TREE` (`core/prefix.rs:190`) |

## Architecture Decisions

| # | Topic | Options | Decision and why |
|---|---|---|---|
| D1 | Effect shape | (a) `SetClipboard(String)`, runtime builds OSC 52; (b) keep `SetClipboard(Vec<u8>)` (OSC 52 bytes) and add `Effect::CopyLocal { seq, text }` | **(b)**. One choke point already exists: `App::copy_to_clipboard` (`app.rs:2222`); all callers (yank `app.rs:2906`, chat copy `app/chat_copy.rs:211`, chat `app/chat.rs:430`, menu `app.rs:2205`) use it. Additive: an `App` built without a tool emits exactly what it emits today, so ~25 existing assertions stay valid. The too-large check stays pure in the app. |
| D2 | Who knows the tool | runtime only; app too | App gets the tool **name** at startup (`App::with_clipboard(Option<Tool>)`, like `with_opener(bool)` `app.rs:1086`) to write the notice. Runtime holds the resolved path. |
| D3 | Running the tool | inline in `apply`; per-copy task; one worker | **One worker thread**, std mpsc; it drains to the newest job before each run (latest copy wins, no race between two `xclip`s). `apply` only sends: the loop never waits. |
| D4 | Bound | wait for exit; timeout | Spawn with stdin piped, **stdout and stderr null** (`xclip`/`xsel`/`wl-copy` fork a server that would hold a captured pipe open). Write stdin on a helper thread. Poll `try_wait` up to `CLIP_TIMEOUT = 2 s`, then kill and report `timed out`. |
| D5 | OSC 52 too large (> `OSC52_MAX_PAYLOAD`, `core/copy.rs:342`) | keep refusing; tool only | With a tool: emit `CopyLocal` only, the notice comes from the tool. Without a tool: unchanged (`Selection too large to copy`). |
| D6 | Alias byte | crossterm legacy parse | 0x1D arrives as `Char('5')+CONTROL` (crossterm maps 0x1C..=0x1F to Ctrl+'4'..'7', see `core/keys.rs:102-108`). Alias = two chords: `Char(']')+CONTROL` and `Char('5')+CONTROL`. Physical Ctrl+5 sends the same byte in xterm-style terminals, so it is the same key. |
| D7 | Alias pressed twice | send NUL; send 0x1D | The **second key's own byte**: alias then alias sends 0x1D (keeps telnet escape usable), Ctrl+Space twice still sends NUL (`app.rs:2333`). New `PrefixAction::SendAliasLiteral`. In groups the alias cancels, like `PREFIX_KEY`. |
| D8 | Probe key reaction | consume the key; react normally | **React normally** (spec): the probe only observes. `Ctrl+Space` -> pass + PREFIX; alias (`Ctrl+]` or `Ctrl+5`) -> `alias` + PREFIX, no offer; a plain space, or any non-printable or modified key -> `missed`, offer notice, key goes to the pane; an unmodified printable key other than the space bar (user decision) is typing and does not end the probe; 10 s with no deciding key -> the probe ends, nothing stored, no offer, it runs again next start. |
| D9 | Probe window | 3 s; 10 s; 30 s | **10 s** (`PROBE_WINDOW`): "a few seconds" for the user, still tolerant of a slow reader. Tick every 1 s, armed only while waiting. The window opens at the first tick, not when the app is built, so a slow start does not eat it. |
| D10 | Stored result | `config.toml`; sqlite chat store; JSON file | **`$XDG_STATE_HOME/lazarobox/terminals.json`** (fallback `~/.local/state/lazarobox/`, reuse `state_dir`). Map `terminal key -> "pass" | "alias" | "missed"`. State, not config, not chat data. Read once at startup with `runtime/bounded_read.rs`; written by a synchronous `Effect::SaveProbe` (tiny file, temp + rename), like `SaveConfig` (`runtime.rs:1561`). Read or write error -> `AppEvent` -> notice. |
| D11 | Terminal key | one global result; per terminal | **Per terminal** (spec scenario "another terminal"): `TERM_PROGRAM`, else `Windows Terminal` when `WT_SESSION`, else `TERM`, else `unknown`. Reuse the rule in `core/menu/feedback.rs:278-280`. This only names the stored row; it never changes prefix behaviour, so it is not a prefix heuristic. |
| D12 | Rerun | menu item; CLI flag; delete the file | **`lazarobox-shell --check-prefix`**: one bool in `core/start.rs` `StartArgs` (`:14`). Notice `Checking the prefix key: press Ctrl+Space`. Plus self-heal: a stored `missed` becomes `pass` the first time `Ctrl+Space` arrives later (one `SaveProbe`). |
| D13 | Notice clash | override; defer | A startup notice (`Startup.notice`, `runtime.rs:812`) wins; the probe is deferred to the next start and nothing is stored. |
| D15 | Notice while the tool runs | say `Copied` at once; wait | At once: `Copying (wl-copy)`. On `AppEvent::Clipboard`: `Copied (wl-copy)` or `wl-copy failed (<reason>); sent to the terminal (OSC 52)`. An open menu footer is updated too (as `on_open_failed`, `app.rs:2182`). A result with an old `seq` is dropped. |
| D14 | WSL launcher | `.desktop` only; WT fragment; `.exe` | Keep the Linux `.desktop` path (user premise). On WSL, `plan_install` adds WSL notes; no new artefact in this change. See Verified facts. |
| D16 | Terminal picker in `install-desktop` | auto only; flag only; interactive prompt | **Interactive prompt, TTY only.** Pure core: `picker_choices(installed, env, records) -> Vec<Choice>` (installed known terminals from PATH, plus `$TERMINAL` and the resolved `x-terminal-emulator` target, deduplicated, `tested` = has a record, default = today's automatic choice) and `parse_choice(input, &choices) -> Pick | Retry(notice) | Cancel` (empty = default, number = that entry, other = retry, EOF = cancel). Thin runtime: checks `is_terminal()` on stdin and stdout, skips the prompt when `--terminal` is set, reads lines, prints the list and notices. Candidates are found by PATH lookup only; terminals are never executed (some open a window). `RECORDED_TERMINALS` starts as `["kitty"]`; WezTerm is added in the follow-up that brings its record. |

## Data Flow

    copy_to_clipboard(text)                         [app, pure]
      ├─ SetClipboard(osc52)  ──> host_out (as today, runtime.rs:1567)
      └─ CopyLocal{seq,text}  ──> clip worker ──> wl-copy|xclip|xsel|clip.exe
                                     └─ (seq, Ok|Err) ─> Inbox.clipboard
                                         └─> AppEvent::Clipboard ─> notice (stale seq ignored)

    startup: terminals.json ─> App::with_probe(key, stored) ─> Probe::Waiting(t0)
      key | ProbeTick(now) ─> core::probe ─> Outcome ─> notice + Effect::SaveProbe

## File Changes

| File | Action | Description |
|---|---|---|
| `src/core/clipboard.rs` | Create | `Tool`, `Session {wayland, display, ssh, wsl}`, `candidates()`, `args()`, `payload()`, notice texts |
| `src/runtime/clipboard.rs` | Create | `Clipboard::find(path)`, worker thread, timeout |
| `src/core/probe.rs` | Create | `Probe` state, `Outcome`, `terminal_key()`, JSON parse/render, notice texts |
| `src/runtime/probe_store.rs` | Create | read/write `terminals.json` |
| `src/app.rs` | Modify | `Effect::CopyLocal`, `Effect::SaveProbe`, `AppEvent::Clipboard`, `ProbeTick`, `ProbeStore*`; `copy_to_clipboard`; `on_key` hook after `:1980`; `on_terminal_key` `:2255`; `on_pending_key` `:2329` |
| `src/runtime.rs` | Modify | wiring near `:299` and `:510`, Inbox field, `select!` arm, `apply` arms |
| `src/core/prefix.rs` | Modify | alias chords, `is_prefix()`, `SendAliasLiteral` |
| `src/core/ai/chat/copy.rs` | Modify | `:126` uses `is_prefix()` |
| `src/core/start.rs` | Modify | `--check-prefix` |
| `src/core/desktop.rs` | Modify | `NON_KITTY_NOTICE` -> record-aware note; `RECORDED_TERMINALS`; WSL notes |
| `src/runtime/desktop.rs` | Modify | pass `wsl` into the plan |
| `docs/terminals/README.md`, `docs/terminals/<t>/<version>-<date>.md` | Create | checklist, template, three records |
| `README.md` | Modify | l.60-64, l.93, l.146-187, l.322 |

## Interfaces / Contracts

```rust
// core/clipboard.rs
pub enum Tool { WlCopy, Xclip, Xsel, ClipExe }
pub fn candidates(s: Session) -> &'static [Tool];
// ssh -> []; wsl -> [ClipExe, WlCopy, Xclip, Xsel];
// else wayland -> WlCopy first; display -> Xclip, Xsel.
// args: xclip -selection clipboard | xsel --clipboard --input | others none
// payload: UTF-8 bytes; ClipExe -> UTF-16LE with BOM (UNVERIFIED, see below)
```

Desktop note: `RECORDED_TERMINALS = ["kitty"]` (WezTerm is added with its record). Unrecorded terminal: `<name> has no test record yet; see docs/terminals/`. A test checks each entry has a `docs/terminals/<name>/` folder.

Record format: header table (terminal, version, OS, lazarobox version, date, tester), then one row per checklist item: `item | pass/fail/n.a. | note`. README "Supported terminals" lists only folders under `docs/terminals/`, each linking its newest record; everything else is "untested".

## Verified Facts vs Assumptions

| Claim | Status |
|---|---|
| All copies go through `copy_to_clipboard` (`app.rs:2222`) | Verified (grep) |
| 0x1D parses as `Char('5')+CONTROL` | Verified from code comment `core/keys.rs:102`; re-check with a unit test on crossterm parsing if reachable |
| `xclip`/`xsel` fork and keep inherited pipes open | Assumption (common knowledge, not checked here); D4 is safe either way |
| `clip.exe` mangles non-ASCII UTF-8 and accepts UTF-16LE with BOM | **Unverified** (no web access in this phase). Validate in the Windows Terminal record; if wrong, send UTF-8 |
| WSLg lists `~/.local/share/applications` entries in the Start menu | **Unverified**. Believed: WSLg scans XDG application dirs, skips `Terminal=true` entries, and runs `Exec` as a Linux GUI app, so a **Linux GUI terminal is required** (Windows Terminal is not used) |

## Testing Strategy

| Layer | What | Approach |
|---|---|---|
| Unit | `candidates`, `payload`, notices, probe transitions, `terminal_key`, JSON, alias chords, desktop notes | pure fns, app clock (`with_clock`) |
| Integration | worker with fake tools (exit 0, exit 1, sleep), store read/write/error | temp dirs, fake scripts as in `runtime/opener.rs` tests |
| App | `CopyLocal` + `AppEvent::Clipboard` notices, stale seq, probe through `App::update` | existing app test harness |
| Manual | per-terminal checklist | `docs/terminals/` records |

## Migration / Rollout

No migration. A missing `terminals.json` means "probe once". Chained PRs, each revertible:

| Slice | Content | Prod lines |
|---|---|---|
| S1 | clipboard core + worker + notices | 190-240 |
| S2a | alias chords, `SendAliasLiteral`, chat-copy | 30-50 |
| S2b | probe, store, `--check-prefix`, self-heal | 150-200 |
| S3 | WSL notes, record-aware notice, terminal picker | 100-170 |
| S4 | docs, records, README | 0 |

Total 470-660: chained PRs recommended. S2b depends on S2a. S1, S2a, S3 are independent. S4 last (records need S1-S3 built).

## Risks (web search, verified 2026-11-01)

- WSLg builds Windows Start menu shortcuts from Linux `.desktop` files (confirmed for `/usr/share/applications`; `~/.local/share/applications` unverified).
- The entry runs a Linux GUI terminal, so one-step launch on WSL needs a Linux GUI terminal installed in the distro.
- S3 merge gate: live check on a WSL machine. If user entries are not listed, a follow-up is needed (Windows Terminal fragment or `.exe`), outside this change.

## Open Questions (closed by the orchestrator, 2026-11-01)

- [x] Spec wording follows D1 (`SetClipboard` keeps OSC 52 bytes; `CopyLocal` carries the text). Spec updated.
- [x] `Ctrl+]` during the probe records `alias`, no offer (D8). Other keys record `missed` with the offer. Spec updated.
- [x] OSC 52-only notice varies per session: local desktop with no tool says "install wl-copy or xclip"; over SSH says "enable OSC 52 in your terminal". Spec updated. On WSL with no tool found, the local-desktop text applies (`clip.exe` is a candidate, so this is rare).
- [ ] WSLg Start menu behaviour: live check on a WSL machine before S3 merges (see Risks).
