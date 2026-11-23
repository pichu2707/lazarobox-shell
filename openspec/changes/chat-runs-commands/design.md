# Design: The chat runs approved commands itself (chat-runs-commands, #845)

## Technical Approach

This follows exploration approach A. The machinery already exists: `run_approved` (`src/app/auto_run.rs`) runs any command on `y` in `run` mode, and the settle machine (`src/core/ai/tools/settle.rs`) follows it. The change adds policy and settle states. It adds no new execution path.

1. **Settle** gains a waiting-for-input phase, a 30 s cut-off with a partial capture, and input redaction. These are pure rules in core, fed by one new probe fact: echo off.
2. **`y` runs in both modes**, behind one pure policy hook, `may_run_on_y`. `auto` is removed.
3. **The `run` grammar** (`exec::parse`) becomes an allow-list keyed by the leading words of the command (`git status`, `cat`, `rg --version`), with a tier that keeps local engines on today's seven verbs.
4. **Guidance**: the model calls the tool and never prints a command for the user.

Invariants kept:
- Checked bytes == written bytes. `parse` never rewrites.
- A run is exactly one `Effect::AutoRun` (the command plus `\r`).
- The runtime never learns the approval mode (`runtime/tools/audit.rs`).
- Every run happens in a visible pane.

## Architecture Decisions

| # | Decision | Choice | Rejected | Rationale |
|---|---|---|---|---|
| D1 | Classifier home | Extend `core/ai/tools/exec.rs`. `Spec.name` becomes `head: &[&str]` and the longest matching head wins. New fields: `tier`, `needs_one_of`, and `Value::Rev`. The spec tables move to `exec/specs.rs`. | A second classifier module next to `exec` | One parser keeps the one-pass, no-alloc, fixed-point guarantees and the existing 400+ grammar tests. A head such as `["git","status"]` handles subcommands with no second grammar. |
| D2 | Segments | Exactly one segment. Every metacharacter (`; & \| < > $ \` ( ) { } \\ ! * ? [ ] #`) stays a fallback. | Per-segment checks of `&&` and `\|` chains | One segment gives one echo row for `capture`, one settle, and no `cmd \| sh` or `x && evil` class at all. The proposal's "per segment" resolves to "one segment". |
| D3 | Tiers | `Tier::Base` covers today's `cd pwd ls tree grep rg find` on every engine. `Tier::Extended` covers the new entries and runs unprompted on cloud engines only. A local engine asks for an Extended command, and `y` runs it. | One list for all engines | This is the owner's decision ("local models get no unprompted run beyond today's read grammar"). `AutoCtx` gains `local: bool`, taken from `run_scope(..).cloud`. |
| D4 | git | Only `git status`, `git --no-pager log` (with `-n N` required) and `git --no-pager diff` (with a summary flag required: `--stat`, `--name-only` or `--name-status`) run unprompted. Plain `diff`, `log -p` and `show` ask. Global options other than `--no-pager` (`-c -C --git-dir --work-tree --exec-path --config-env`) are not in the list. | Allow `git diff` content, or require hardening `-c` flags in the text | Content diffs can print tracked secrets, which the `git_diff` tool filters by name. A pager in a pty waits for keys, so `--no-pager` is required. L32 (a repository can make git run a program) is closed by D5. |
| D5 | git config gate | `runtime/run_check.rs` runs `git config --local --worktree --no-includes --name-only --list` through the hardened spawn of `runtime/tools/git.rs` (`GIT_CONFIG_GLOBAL=/dev/null`, `NOSYSTEM`). The command passes only if every key is on an inert allow-list: `core.{repositoryformatversion,filemode,bare,logallrefupdates,ignorecase,precomposeunicode,symlinks}`, `remote.*.{url,fetch,pushurl}`, `branch.*.{remote,merge,rebase}`, `user.{name,email}`, `extensions.objectformat`. Anything else asks with `GIT_CONFIG_RISK`, which does not count toward the pause. | A deny-list (`core.fsmonitor`, `core.pager`, `diff.external`, `*.textconv`, ...) | An allow-list, as in the rest of the grammar. The user's global config is the user's own and stays trusted. |
| D6 | `--version` | Exactly `<p> --version` for `git rg fd bat jq gh docker make tmux curl`. | `node`, `python3`, `cargo`, `rustc`, `go`, `npm` | These are often shims. `rust-toolchain.toml` `path =`, asdf `path:` and similar files can make a shim run a binary from the project. |
| D7 | Waiting for input | A **phase** of the settle machine, not an `End`. It starts when the foreground is not the shell, `echo_off == Some(true)`, the pane has been quiet for 1 s (`WAIT_QUIET`) and no alt screen is shown. It suspends the cut-off. It leaves on output or when echo comes back on. One system line is shown on entry. | An `End::Waiting`; the quiet heuristic alone | An `End` would hand the turn back before the command ends. The owner wants the chat to resume by itself. Echo off (termios on the pty master) is the kernel's own password signal. Quiet alone cannot tell a password prompt from a dev server. |
| D8 | Keys during a model run | With `Limits::Run`, a key sent while the last probe saw a program in the foreground marks the run `attended` and records the key for redaction (D10). The run does not end. A key while the shell owns the foreground, or while the foreground is unknown, still ends the run as `UserTyped`. | Today's rule (any key is `UserTyped`) | Typing a password must not end the follow. A key at the shell means the user's own line mixes into the capture. |
| D9 | Long-running | `CUT_OFF` is a fixed 30 s, measured from the later of the write, the end of the last waiting phase and the last Enter typed. At the cut-off the run ends as `End::StillRunning` with `capture(.., prompt_below: false)`, or `Unconfirmed` when the foreground or the anchor is unknown. A run waiting longer than `WAIT_CAP` (5 min) ends as `End::NoInput`. In S2, the slot then becomes a **continuation follow**: an `#819`-style `resume` slot with `Limits::Continue` (no timer). When it settles, the chat resumes with the final output. A Ctrl+C typed in the pane, a new message (`stop_following`) or closing the pane ends it, each with a visible line. | Stop following at 30 s | Installs that take two minutes still report their result, and servers do not block the turn. This reuses `held_resumes` and `flush_resumes`. |
| D10 | Password safety | See "Redaction rule" below. | Detecting prompt text (`password:`), or dropping the whole capture | Prompt text varies by locale and program. Row-level hiding keeps the useful output. |
| D11 | `y` hook | A pure `may_run_on_y(ask, blocked, policy) -> Result<(), &'static str>` in `approval.rs`. It returns `Err` for a blocked conversation (GitHub text or newer rows: typed-only, as today), for a worker ask, and for `policy.y_runs == false`. `approve_run` calls it in place of `mode == Run`, then `y_run_blocker` runs as today (`MULTI_LINE`, `NO_CWD`, `RUNNING`, `HALF_TYPED`, `NON_UTF8`). | Changing `y_run_blocker` | It keeps one door. `auto_run` keeps its own block check as defence in depth. |
| D13 | `auto` migration | On load, `approval = "auto"` reads as `ask`. When the file has no other problem, the app rewrites that one value to `"ask"` once through `core/config_edit.rs` (`apply_edit`, atomic save) and shows one notice: `config: ai.tools.approval: "auto" was removed; the file now says "ask"`. On failure it shows `config: ai.tools.approval: "auto" was removed; using "ask" (the file was not changed: <reason>)` and keeps mode `ask`. The pure parser never writes; the app-level load step does. After a successful rewrite the notice does not repeat. | Silent rewrite; notice at every start | Owner decision 2026-11-22. A visible notice plus a one-time rewrite removes the permanent nag without a silent change. |
| D14 | macOS `y` | `y_run_blocker` keeps `NO_CWD` (no `proc_cwd`): `y` only types there, with the visible reason `the pane's directory cannot be read on this system`. Documented in both READMEs (S4). | Run without the write-step cwd comparison | Owner decision 2026-11-22: keep today's limitation, make it visible. |
| D12 | Settle compatibility | `Settle::new` keeps today's rules as `Limits::Follow` (10 s `TIMEOUT`, any key is `UserTyped`). `Settle::run` uses `Limits::Run`, for model runs from `auto_run`. `RunSlot::following` (#819) stays on `Follow`. | One rule set for both | The #819, #824 and #841 tests stay byte-identical. A typed follow keeps its documented "send a message to go on". |

### Redaction rule (D10, exact)

1. **Recording.** For each user key forwarded to the target pane during a `Run` or `Continue` settle, `Settle::user_key(Key { line, enter, interrupt })` records the cursor's absolute line at that moment. An open range `[first, last]` grows with each key. Each later probe also extends `last` to the probe's cursor line, which covers echoed input that wraps. An Enter key, Ctrl+C or Ctrl+D closes the range at the cursor line of that key, so output printed below it stays visible.
2. **Capture.** `capture` receives `hidden: &[RangeInclusive<usize>]`. Every row whose absolute line is in a range is dropped before `shape`. Each contiguous hidden block becomes one row: `[input typed in the pane: hidden]`. A range that touches the echo rows makes `capture` return `None` ("output not available").
3. **Fail closed.** A key recorded while the cursor was hidden (`line == None`) withholds the whole output: the report says `output not available: you typed in the pane`.
4. **Why this is enough.** Input typed with echo off never reaches the screen. Input with echo on (`pwfeedback` asterisks, type-ahead before the prompt, a `[Y/n]` answer) is drawn on the rows the cursor sat on, and those rows are hidden. Hiding happens in the app, before the text leaves for `clean`, `redact` or the runtime.
5. **Residual.** A program that prints the secret back on a later row of its own output cannot be told apart from normal output. A later `read_pane` call still asks the user each time.

### Settle step order (`Limits::Run`)

1. alt screen ends as `FullScreen`
2. `typed` ends as `UserTyped`
3. settled ends as `Settled` (rule unchanged)
4. waiting phase check
5. `WAIT_CAP` ends as `NoInput`
6. `CUT_OFF` ends as `StillRunning` or `Unconfirmed`

`take_waiting()` returns `true` once, on entry to the waiting phase. The app then pushes `pane N: waiting for your input in the pane; type it there (it is never sent to the chat)`.

Lines:
- Cut-off: `pane N: <exact command> is still running after 30 s; the chat goes on with the output so far`
- `NoInput`: `pane N: no input after 5 min; the chat goes on (the command still waits in the pane)`

### Bypass table (all must route to `Prompt`)

| Command | Reason |
|---|---|
| `cat x > y`, `wc -l <f`, `cat x # c`, `cat x*` | shell metacharacter |
| `cat $(rm -rf ~)`, ``cat `id` ``, `\cat x` | shell metacharacter |
| `ls; rm x`, `ls && rm x`, `cat x \| sh` | shell metacharacter |
| `find . -exec rm {} ;`, `find . -delete`, `tail -f log` | flag not allowed |
| `git -c core.pager=sh status`, `git -C .. status`, `git --git-dir=/x status` | flag not allowed |
| `git st` (alias), `git show HEAD:.env`, `git log` | not on the run list / `-n` required |
| `git --no-pager log -n 5 --output=x`, `git --no-pager diff --ext-diff`, `--no-index` | flag not allowed |
| `git --no-pager diff` (content) | summary flag required |
| `git status` in a repo with `core.fsmonitor` | `GIT_CONFIG_RISK` (D5) |
| `cat` / `cat -` | missing argument / bare dash |
| `sudo cat x`, `env cat x`, `command cat x`, `FOO=1 cat x`, `'cat' x` | not on list / assignment / quoted |
| `cargo --version`, `node --version` | not on the run list (D6) |
| `cat .env`, `cat ../../etc/passwd`, `cat ~/.ssh/id_rsa` | `NOT_ALLOWED` / `OUTSIDE_ROOT` (run_check) |
| `cat README.md` on a local engine | Extended tier asks (D3) |

## Data Flow

```
model run_in_pane ─▶ route(ask, mode, AutoCtx{local,..}, policy)
   ├─ Base/Extended match & free ─▶ CheckRun ─▶ run_check (paths, git gate) ─▶ AutoRun
   └─ Prompt ─▶ y ─▶ may_run_on_y ─▶ y_run_blocker ─▶ run_approved ─▶ AutoRun
                         └─ Err ─▶ type_run (typed only, reason line)
AutoRun ─▶ Settle::run ◀─ Probe{fg, cwd, echo_off, cursor, alt} each tick
              │ user_key(line) ─▶ attended + hidden ranges
              ├─ Settled/StillRunning/NoInput ─▶ capture(hidden) ─▶ RunDone
              └─ StillRunning (S2) ─▶ resume slot (Continue) ─▶ Settled ─▶ ResumeNote
```

## File Changes (slices; production lines only, tests excluded)

| Slice | Files | ~Lines |
|---|---|---|
| **S1** settle: waiting, cut-off, redaction (`pane-execution`). It lands first: today's `run`-mode `y` already runs `sudo`. | `core/ai/tools/settle.rs` (`Limits`, phase, `Key`, ranges, `StillRunning`/`NoInput`); `capture.rs` (`hidden`); `report.rs` (words for the new ends); `core/pty/{mod,portable,fake}.rs` (`echo_off()` via `MasterPty::get_termios`, or `tcgetattr` on the master fd, `None` elsewhere); `runtime.rs` (`probes`); `app/auto_run.rs` (`Settle::run`, probe, `user_key`, waiting line, `end_run`); `app.rs`, `app/tool.rs`, `app/run.rs` (`note_typed` passes the key and cursor line); `app/typed_run.rs` (new match arms) | 370 |
| **S2** `y` runs in ask, `auto` removed, continuation (`agent-tools`, `command-runner`, `configuration`, `config-menu`, `chat-pane`) | `core/ai/chat/approval.rs` (`may_run_on_y`, `RunPolicy`, key-hint choice, `Route::AutoType` and `Auto` arms removed); `core/ai/config.rs` (`Auto` removed; the parser maps `"auto"` to `ask` with problem `ai.tools.approval: "auto" was removed; using "ask"`; `APPROVAL_PROBLEM` names `"ask"` or `"run"`) plus the load-time rewrite of D13 (`core/config_edit.rs`, startup load in `app.rs`/`runtime.rs`, notice with or without the failure reason); `app/run.rs` (`approve_run`); `app/auto_run.rs` and `app/typed_run.rs` (continuation follow and its resume note `lazarobox: the command you approved in pane N finished.`); `app/tool.rs` (toggle lines); `core/ai/chat/mod.rs` and `ui/components/chat.rs` (badge). The menu follows `ApprovalMode::ALL` with no code change. | 300 |
| **S3** classifier allow-list (`agent-tools`) | `core/ai/tools/exec.rs` and the new `exec/specs.rs` (heads, tiers, `cat head tail wc`, git, `--version`, `Role::Rev`, targets); `core/ai/chat/approval.rs` (`AutoCtx.local`, tier check, `GIT_CONFIG_RISK` not counted toward the pause); `app/auto_run.rs` (fills `local`); `runtime/run_check.rs` (targets of the new verbs, git gate); `runtime/tools/git.rs` (config listing). Splits into S3a (no git) and S3b (git and gate) if it passes 400. | 390 |
| **S4** guidance and docs (`chat-pane`, `agent-tools`) | `core/ai/chat/prompts/tools.md`; `core/ai/tools/mod.rs` (`run_in_pane` description: call the tool and never print a command; `y` runs it; `sudo` passwords are typed by the user; partial output after 30 s, do not start it again); `chat/transcript.rs` (`RUN_HINT` = `Ctrl+Space e  type in pane`, because `e` types and never presses Enter); `README.md` and `README.es.md` together | 120 |

S1 is merged before S2, so a released build never runs `sudo` on `y` in `ask` under the 10 s timeout.

## Interfaces / Contracts

```rust
pub enum Limits { Follow, Run, Continue }
pub struct Key { pub line: Option<usize>, pub enter: bool, pub interrupt: bool }
pub enum End { Settled, TimedOut, Unconfirmed, StillRunning, NoInput, FullScreen, UserTyped, PaneGone }
// Probe gains: pub echo_off: Option<bool>
pub fn capture(pane: &mut Pane, anchor: &Anchor, command: &str, prompt_below: bool,
               hidden: &[RangeInclusive<usize>]) -> Option<String>;
#[derive(Clone, Copy)] pub struct RunPolicy { pub y_runs: bool, pub unprompted: bool } // Default: both true
pub fn may_run_on_y(ask: &ApprovalAsk, blocked: Option<BlockWhy>, policy: RunPolicy)
    -> Result<(), &'static str>;
```

Policy hooks (#657 → #658 → #659): `App.run_policy: RunPolicy` is a default constant in this change. `route_run` checks `policy.unprompted` first; `approve_run` checks `policy.y_runs`. The refusal lines are `POLICY_TYPED` and `POLICY_ASKS`, and neither counts toward the pause. #657 wires the source (its policy file). No config key ships now.

## Testing Strategy (Strict TDD)

| Layer | What | How |
|---|---|---|
| Unit | Every settle rule and its order; waiting enter and leave; cut-off clock resets; `Follow` unchanged; range recording; capture hiding (head, tail, echo collision, hidden cursor); the whole bypass table; tiers; `may_run_on_y` table; `"auto"` migration | Pure tests next to each module, in the `exec/tests.rs` style |
| Integration | git gate with a `tempfile` repo (`core.fsmonitor`, `include.path`, plain config); `echo_off` through `FakePty` | `runtime/run_check.rs` tests, never `/tmp` roots for the full suite |
| App | `y` in ask runs; a fake `sudo` (echo off, then a password typed) resumes, and the password is absent from `RunDone`; a server gives the partial output, then the continuation; GitHub-blocked chats and worker asks still type; #819, #824 and #841 suites unchanged | `app/app_tests/tool_run_y.rs`, `typed_resume.rs` |
| Live | `sudo true` and `npm run dev` on the real binary | `skills/run-lazarobox` isolated home |

## Migration / Rollout

The only data change is D13: a config holding `approval = "auto"` is rewritten to `"ask"` once on load, with one visible notice (never silent). If the rewrite fails the notice carries the reason, the mode is `ask`, and the notice shows again at the next start. Reverting S2 does not restore `auto` in files already rewritten.

Each slice is one revertable PR, stacked to main:
- Reverting S2 restores typed-only `y` in `ask`.
- S1 and S3 are safe on their own.

## Open Questions

All resolved by the owner on 2026-11-22:
- [x] macOS: `y` keeps only typing (D14), documented with a visible line.
- [x] Plain `git diff` asks in `run`; `--stat`, `--name-only` and `--name-status` run unprompted on cloud engines (D4).
- [x] `WAIT_QUIET` = 1 s, `WAIT_CAP` = 5 min (D7, D9).
- [x] The continuation follow resumes the chat a second time, with the final output, when a cut-off command ends (D9).
