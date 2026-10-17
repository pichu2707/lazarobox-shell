# Design: Auto-execute (`run` approval mode)

## Technical Approach

`run` is a third `ApprovalMode`. It lives only in `ChatState`, and the existing pure `approval::route` stays the ONE place that decides. Routing now also gets the grammar verdict and four new context bits.

A runnable command goes through four steps:
1. A pure, allocation-free grammar returns a borrowed `SafeCommand`.
2. A **check round trip**: the runtime reads `/proc/<pid>/cwd` and runs the path policy on the blocking pool. Nothing touches the disk on the UI loop.
3. **One write**: the framed bytes plus `\r`, but only if the cwd still equals the checked cwd and the shell owns the terminal.
4. A pure **settle machine**, fed by a probe on the existing 16 ms tick, ends the call. The app reads the anchored screen span, and the host cleans, redacts and nonce-wraps it.

Every case that cannot be checked becomes the prompt (D17), and there `y` types without Enter. D17 is unchanged. D18 is extended by D19–D23 below.

Superseded text in `agent-tools/design.md`, updated in S5:
- line 9 ("never sends Enter") gains the `run` exception;
- the line 91 non-goal is removed;
- the D18 threat-model sentence "nothing is ever executed by lazarobox" becomes "nothing outside the D20 allow-list".

## Architecture Decisions

| # | Decision | Rejected | Rationale |
|---|---|---|---|
| D19 | **Mode.** `ApprovalMode { Ask, Auto, Run }` with `ALL`, `id`, `from_id` and `cycled(forward)` (the `NotifyMode` pattern). `flipped` is removed. `APPROVAL_PROBLEM` names all three values. `Prefix a` calls `cycled(true)`. The menu row cycles in both directions, and its height is unchanged. `ChatState::mode_badge() -> Option<ApprovalMode>` replaces `auto_badge`. Badges: ` AUTO `/`A` (below 6 columns) on `warning_orange`, ` RUN `/`R` (below 5 columns) on `error_red`, fg `bg_base`, bold. System line for `run`: `approval: run (cd, pwd, ls, tree, mkdir run at once and their output goes to the model; other commands ask)`. **Model can never switch it:** `Call`, `ChatRequest`, `StreamEvent` and `Effect` carry no mode, and `route` takes the mode as an argument read from `ChatState`. The `runtime/tools/audit.rs` tokenizer gains one rule: `ApprovalMode` and `set_approval_mode` MUST NOT appear under `src/runtime/`. | A separate key for `run`; the mode inside `ToolHost`/`ChatRequest` | R1/R2. A compile-and-audit guarantee costs about 10 lines. Keeping the mode out of the request keeps tools-off byte-identical. |
| D20 | **Grammar and policy.** New `src/core/ai/tools/exec.rs`. `parse(&str) -> Result<SafeCommand<'_>, Fallback>` is a single pass with no allocation. **Character classes:** an ASCII fast path classifies each byte through a `const CLASS: [u8; 128]`; only a byte `>= 0x80` triggers a UTF-8 decode of that one character (`str::chars` on the remaining slice), which is accepted only if it is a Latin letter: U+00C0 to U+024F except U+00D7 (×) and U+00F7 (÷). That covers `á é í ó ú ü ñ ç à è ò`, the Nordic and Central-European letters and Latin Extended-A/B, so `música` and `año-2026` pass. Everything else non-ASCII is refused, and `char::is_alphanumeric` is deliberately NOT the rule: it admits combining marks (U+0345, U+05B0, U+093E), right-to-left letters, roman numerals, circled digits, superscripts and the Hangul fillers that `validate` refuses as invisible. Refused: controls (C0, C1 and DEL, which the table also marks), Unicode format characters (`Cf`: bidi controls U+202A–U+202E and U+2066–U+2069, ZWSP U+200B, ZWNJ U+200C, ZWJ U+200D, U+FEFF), any whitespace other than the single ASCII space (NBSP, U+2028, U+2029, U+0085), other scripts, non-ASCII punctuation and combining marks (a decomposed accent is refused, so macOS NFD names fall back to the prompt). Words are kept as up to 8 `&str` slices in a fixed array, with no heap. **Unquoted words** use `[A-Za-z0-9._/@%+:,=~-]` plus those Latin letters. A word MUST NOT start with `=`, `+` or `%` (zsh EQUALS, dirstack, jobs). `~` is allowed only as `~` or `~/…` and only as the first character: `a=~/x` and `a:~/y` are refused because bash and zsh tilde-expand after `=` and `:`. Words are separated by exactly one ASCII space; a leading, trailing or doubled space is refused (so `validate(s) == s` holds). **Single- and double-quoted words** are a whole word `'…'` or `"…"` (codex quotes paths with `"`) whose contents are ONLY the unquoted-word class (ASCII alphanumerics, `. _ / @ % + : , = ~ -`, the Latin letters) plus the ASCII space, so they hold none of `" $ \` ; | & ( ) < > * ? [ ] { } # ! ^ \` or `'` (whichever the quote, so `$`, the backtick, `\` and `!` stay refused inside `"…"`): a quoted word is inert text even when the shell sits inside an open quote, `$(`, heredoc or `for … do` (C1): an open `"` plus our `"x"` simply closes and reopens around text that holds nothing active, the same way an open `'` does for `'x'`. They are followed by a space or the end of the text, the verb is never quoted, and they may not start with `~` (the shell would not expand it, but the resolver would) or `-` (it would hide a flag). Everything in R5 is refused, along with tabs, controls, a `"` anywhere but around a whole word, and a word that is `-` or `--`. **Flags** come from a `const` per-verb table: `cd`: none, at most 1 path. `pwd`: none, 0 paths. `ls`: bundle chars from `lahtr1`, at most 4 paths. `tree`: `-d` only (`-a` is refused: it would list names under secret folders), optionally `-L <1–2 digits>` as a separate word (`-L` is NOT required: a bare `tree` runs, bounded by the 8 KiB / 200-line cap and the 10 s timeout), at most 1 path. `mkdir`: `-p`, 1–4 paths. Long options are refused. Any word starting with `-` is treated as a flag. **Paths**: `paths::resolve_exec(roots, cwd, input, Want::{Existing, Home, New})` checks twice, lexically and on the real path (the same idea as `resolve`). `Existing` (`cd`, `ls`, `pwd`'s cwd) allows paths outside HOME unless they fall under a secret root; a link outside HOME that resolves into HOME gets the HOME rules, so it cannot list `~/.ssh`. `Home` (`tree`, cwd included) is `Existing` that MUST be inside HOME (S5b: the user approved `cd` and `ls` outside HOME, not `tree`). `New` canonicalizes the deepest existing ancestor and appends the rest. `mkdir` targets MUST be inside HOME. For every verb except `cd`, the cwd itself MUST pass, because `ls` and `tree` without arguments, `pwd` and relative `mkdir` act on it. `SecretRoots::load` returning `None` falls back. | A deny-list; a shlex crate; canonicalizing on the UI thread | R4–R6. A zero-copy borrow means checked bytes == written bytes. The grammar is a strict subset of `validate`, so a property test asserts `validate(s) == s` for every accepted `s` and no re-normalization is possible. |
| D21 | **Enter, dirty guard, check round trip.** (a) Route context: `AutoCtx` gains `dirty`, `executing`, `cwd_known` (`proc_cwd.is_some()`, false on macOS) and `paused`. (b) `Route::AutoRun(SafeCommand)` reserves a per-target `RunSlot{chat, turn, call, command: String, phase: Checking}` and counts toward `auto_typed`, with no refund on fallback (fail closed). It then emits `Effect::CheckRun{chat, target, turn, call, command}`. (c) The runtime reads `/proc/<pid>/cwd` (no disk access) and runs `run_check::check` on `spawn_blocking` with a 2 s timeout. That builds the same `load_roots` as `fs.rs`, re-parses the same bytes and calls `resolve_exec` per target. The answer is `AppEvent::Run(target, RunEvent::Checked{call, cwd, cd_target} \| Refused{call, reason})`, sent over a new `run_rx` inbox arm. (d) The app re-runs `route` (the same choke point). If it passes, the app calls `frame_bytes(target, safe.as_str())`, which is the post-`validate` half split out of `typed_bytes` (the same alt-screen and paste checks), appends `b'\r'` and emits `Effect::AutoRun{chat, target, bytes, number, turn, call, cwd}`. (e0) Non-ASCII command bytes run only when the pane's locale (`LC_ALL`, then `LC_CTYPE`, then `LANG` of the spawned shell's env) names UTF-8; otherwise the command takes the prompt route (a non-UTF-8 locale or readline `convert-meta` could reinterpret the bytes). (e) The runtime write step checks that `readlink == cwd` AND that the foreground is the shell. Only then does it call `take` and write once. A failure feeds back `RunEvent::NotRun{call, reason}`, and the app opens the prompt with that reason. (f) `user_dirty` is set by every key, paste or wheel write forwarded to a pane. Only the keys Enter and Ctrl+C clear it. **Ctrl+U does NOT clear it:** bash kills only to the left of the cursor, so `ls x` plus leftover `; rm -rf ~` would run. Ctrl+U still clears `line_pending`, as in D18. A bare Enter clears `dirty` only if the previous key forwarded to the pane was not Tab, BackTab or an arrow: after those a completion menu (zsh menu-select, the fish pager) may be open and Enter only accepts an entry, so the pane stays dirty until the next bare Enter or Ctrl+C (cost: one prompt after Up and Enter). | A lexical check against the 1 s `proc_cwd` poll (stale for up to 1 s); synchronous `canonicalize` in the Effect handler (can stall on a hung mount); `Effect::AutoType{enter: bool}` (one flag away from Enter on `auto`) | R6, R10, R11. The cwd is compared at the write, which closes the cwd TOCTOU. Only a symlink swap between `canonicalize` and the shell's `chdir` remains, and it needs a local attacker who can write a path component (documented). A distinct Effect variant keeps `\r` structurally impossible on the `y` and `auto` paths. |
| D22 | **Settle, capture, text channel.** The pure `src/core/ai/tools/settle.rs` (see Interfaces) is fed by `RunEvent::Probe{foreground, cwd}`. The runtime emits a probe on the existing `tick` arm for each pane where `App::run_probes()` yields a Running slot: one `tcgetpgrp` plus one `/proc` readlink per tick, with no new thread or timer. Pty output updates `last_output` through the app clock. At `Done`, `src/core/ai/tools/capture.rs` turns the anchor and the prompt line into at most 2 `LineSpan`s (head 100 lines, tail 100 lines). Only those spans are read with `read_lines`. The command-echo rows are skipped while the row is `wrapped`. No prompt decoration is trimmed: comparing against the lines above the anchor can delete real output (bullets, table separators, `|` graph rows) and the emulator records no OSC 133 marks, so a multi-line prompt's upper rows reach the model wrapped and redacted. The echo is checked first: the anchor row and its wrapped rows must contain the command (whitespace squeezed) or the output is not available (fish abbreviations fail closed). Blank edges are trimmed and the rows are joined through `copy::join`. The result is capped at 8 KiB (screen text, before `clean` and `redact`, which may change its length by a few bytes) as head plus tail on char boundaries, with the marker `[… N lines not shown …]`. **Channel:** `Waits` stores `oneshot::Sender<Answer>`, where `enum Answer { Decided(Decision), Ran(Box<RunReport>) }` and `From<Decision>`. `Decision` stays `Copy`. The runtime `take`s the sender, writes, then calls `park(key, sender)`, so the existing `end_turn`, `end_pane` and drop cleanup also covers running calls. `Effect::RunDone{chat, turn, call, report}` leads to `Waits::finish`. The host runs `clean`, `redact` and `label::wrap` off the UI thread. A `None` nonce means `output not available`. | A `Decision::Ran(usize)` with text in a side map; capture in the runtime (the emu lives in the app); a new timer thread; OSC 133 | R8, R9. The settle machine is testable with a fake clock. The capture is O(lines read) ≤ 200 rows. Parking reuses the proven cleanup. |
| D23 | **Refusal pause and wording.** `ChatState.run_fallbacks: u8` counts consecutive grammar or policy fallbacks in `run`. It is reset at turn start and after each run. At `MAX_RUN_FALLBACKS = 3`, `AutoCtx.paused` makes every later call in that turn prompt, with the reason `auto-run paused for this turn`. Limit fallbacks (focus, hidden, cap, dirty) do not count. Fallback reasons appear only in `run`, so `auto` snapshots are unchanged. | No pause; a pause that counts every prompt | A model that keeps probing the boundary (typical of injection) stops getting runs mixed in with the noise. It costs one `u8` and fits the per-turn cap model. |

## Data Flow

```
host: validate ─▶ ToolCall{Run} ─▶ App::on_tool_ask ─▶ route(ask, Tool, mode, ctx)   ◀── the ONE choke point
   ├ Prompt(why) ─▶ overlay (y types, no Enter)            ├ AutoType ─▶ D18 unchanged
   └ AutoRun(safe) ─▶ RunSlot{Checking}, auto_typed+1 ─▶ Effect::CheckRun
        runtime: readlink /proc cwd ─▶ spawn_blocking(load_roots + resolve_exec) ─▶ run_rx
   ◀─ RunEvent::Checked{cwd} | Refused{reason} ─▶ (Refused ⇒ prompt with reason)
   App: route again ─▶ frame_bytes + "\r" ─▶ Effect::AutoRun{cwd}
        runtime: readlink==cwd && fg==shell ? take ─▶ write ─▶ park : RunEvent::NotRun ⇒ prompt
   App: RunSlot{Running{settle, anchor}}   tick(16ms) ─▶ Probe{fg, cwd} ─▶ Settle::step
   Done ─▶ capture spans ─▶ Effect::RunDone{report} ─▶ Waits::finish ─▶ host: clean→redact→wrap ─▶ model
```

## Interfaces / Contracts

```rust
// core::ai::tools::exec
pub enum Verb { Cd, Pwd, Ls, Tree, Mkdir }
#[derive(Clone, Copy)] pub struct SafeCommand<'a> { text: &'a str, verb: Verb, words: [&'a str; 8], len: u8 }
pub struct Fallback { pub why: Why, pub at: Option<Range<usize>> } // the byte span of the offending token
pub fn parse(text: &str) -> Result<SafeCommand<'_>, Fallback>;
pub fn targets<'a>(c: &SafeCommand<'a>) -> impl Iterator<Item = (&'a str, Want)>; // cd: Existing (or HOME)

// core::ai::chat::approval
pub enum Route<'a> { Prompt(Option<Fallback>), AutoType, AutoRun(SafeCommand<'a>) }
pub struct AutoCtx { typed_this_turn, line_pending, target_focused, tab_shown,
                     dirty: bool, executing: bool, cwd_known: bool, paused: bool }

// core::ai::tools::settle (pure)
pub struct Anchor { pub line: usize, pub col: u16, pub generation: u32, pub cwd: PathBuf }
pub struct Probe { pub now: Instant, pub foreground: Option<bool>, pub cwd: Option<PathBuf>,
                   pub cursor_line: Option<usize>, pub generation: u32, pub alt_screen: bool }
pub enum End { Settled, TimedOut, FullScreen, UserTyped, PaneGone }
impl Settle { fn new(Anchor, Instant); fn output(&mut self, Instant); fn user_key(&mut self);
              fn step(&mut self, &Probe) -> Option<End>; fn lines_comparable(&self) -> bool }
```

`step` checks these rules in order:
1. `alt_screen` gives `FullScreen`, and `user_key` gives `UserTyped`.
2. 10 s since the write gives `TimedOut`.
3. Otherwise the run is `Settled` only when all of these hold:
   - `foreground == Some(true)`. `None` waits, which fails closed.
   - Quiet: at least 250 ms since `max(last_output, written)`.
   - Progress: `cursor_line > anchor.line`, OR `cwd != anchor.cwd`, OR the generation changed. A changed generation means the lines are not comparable and the output will not be available.

`RunReport { number, end, cwd: Option<(PathBuf, CwdSource::{Proc, Osc7})>, cd_target, output: Option<String> }`. The host words it:

Every row starts with the exact command that ran (`<cmd>: `), then the text below.

| Case | Result text for the model | Row (after `<cmd>: `) |
|---|---|---|
| Settled | `Ran in pane N. cwd: <p>. Exit status is not known.` + wrapped output or `(no output)` | `ran in pane N — cwd <p>` / `— K lines` |
| cd mismatch | `… The shell is in <actual>, not <target> (cd may be an alias or function).` | `ran in pane N — cwd <actual> (expected <target>)` |
| TimedOut | `Ran in pane N; still running after 10 s. Output so far:` + wrapped | `ran in pane N — still running` |
| Lost / no nonce | `Ran in pane N; its output is not available.` | `ran — output not available` |
| FullScreen / UserTyped | `Ran in pane N; it opened a full-screen program` / `the user typed in the pane`, `so no output was read.` | same, short |

A `cd` also writes the system line `pane N: <cmd> → <cwd>` (for example `pane 2: cd ~/dev/curso-rust → /home/u/dev/curso-rust`); other commands add no system line. The tool description (neutral, S5): *"Send one shell command to the user's terminal pane. Depending on the user's settings it is only typed (the user reviews it and presses Enter) or typed and run; the result says which and, when it ran, includes the visible output. Only simple commands (cd, pwd, ls, tree, mkdir with a few flags) can ever run directly; anything else needs the user's approval and is only typed."* `TYPED_TEXT` stays as it is. The prompt (S5) gains `why`: a line such as `not run directly: not in the safe list: "|"`, plus a window of the command centred on `at`, bounded to the box width.

**Cleanup.**
- When a turn finishes or is cancelled, the app drops the slots of that `(chat, turn)` and `Waits::end_turn` drops the parked sender.
- When the target pane closes, the app emits `RunDone(PaneGone)`.
- When the chat closes, `end_pane` runs and the slots naming that chat are cleared.
- `executing` is released in every one of these cases.

**macOS.** `cwd_known` is false and `new_nonce()` is `None`, so `run` always prompts with the reason `the pane's directory cannot be read on this system`. OSC 7 is never used for the policy, because any program can print it. It is used only as the labelled fallback for the result cwd.

## Performance and residual threats

**Performance.**
- `parse` is O(n) over at most 4096 bytes, uses a `const` table and allocates nothing.
- `route` is O(1) beyond the parse. It runs twice per run, at ask time and after the check, and both runs are the same function on the same bytes.
- On the UI loop, the only IO is `tcgetpgrp` plus a `/proc` readlink. These are kernel-only, take a few µs, and happen only while a run is in flight.
- The disk is touched only in `spawn_blocking`, bounded at 2 s, and a timeout falls back to the prompt.
- Capture reads at most 200 rows, once.
- Redaction and wrapping run in the host task.
- Hot-path concern: none for idle panes, because the probe is skipped when `run_probes()` is empty.

**Residual threats, documented in the README in S5.** These extend the D18 addendum:
- `cd` hooks (chpwd, direnv, `git status` in the prompt).
- The symlink swap TOCTOU (D21).
- `CDPATH` and `cdable_vars`: the shell may search `$CDPATH` for a bare relative `cd name`, or treat a variable as a directory. The resolver checks the name against the cwd, so a `CDPATH` entry can send `cd name` elsewhere. Accepted: rewriting the command with `./` would break checked bytes == written bytes (D20). Documented in the README in S5.
- PS2 continuation: the user left `for …; do` open, and the run joins the loop body. The `cd` cwd check reports it honestly.
- A prompt line that leaks into the output when the trim heuristic misses it. It is redacted.
- Outside HOME, `cd` and `ls` expose names and `ls -l` metadata (symlink targets included), never contents. Bind mounts are not detected.
- Continuation prompts (PS2), C1. Enter clears `dirty`, but the shell may still be inside an open quote, `$(`, heredoc or loop. Closed by two measures: quoted words are inert (D20), and `validate` refuses model text with an unbalanced quote or a trailing backslash, so `y` and `auto` cannot open one. Accepted residual: a user-typed trailing `\` or an open construct the user later closes can merge a run's inert words into the user's command. OSC 133 prompt detection is the future fix.
- Completion-menu Enter: mitigated by the `after_menu` rule of D21. A user-typed trailing `&&`, `||`, `|`, `do` or `then` lets a later run complete the user's own line: accepted until OSC 133. `validate` also refuses model text ending in `&&`, `||`, `|` or `\`, or holding an unescaped backtick, `$(` or `<<`.
- Emulator query replies (DA1, DSR) written to the pane do not set `dirty`: fixed content, not attacker-chosen, and marking them would break `run` for prompts that query the terminal.
- Locale (D21 e0): `names_utf8` reads lazarobox's own environment. An rc file that changes the locale or sets readline `convert-meta` after the shell starts is not seen.

## File Changes

| File | Action | Slice |
|---|---|---|
| `src/core/ai/config.rs`, `src/core/ai/menu.rs`, `src/core/menu*.rs`, `src/core/prefix.rs`, `src/app/tool.rs` (toggle), `src/core/ai/chat/mod.rs` (badge), `src/ui/components/chat.rs` | Modify | S1 |
| `src/core/ai/tools/exec.rs` (new), `src/core/ai/tools/paths.rs` (`resolve_exec`, `Want`), `src/core/ai/tools/mod.rs` (mod) | Create/Modify | S2 |
| `src/core/ai/chat/approval.rs`, `src/app/auto_run.rs` (new: slot, route glue, events), `src/app/run.rs` (`frame_bytes` split), `src/app.rs` (`PtyPane.run`, `user_dirty`, Effects, `AppEvent::Run`), `src/runtime.rs` (`CheckRun`/`AutoRun` arms, `run_rx`), `src/runtime/run_check.rs` (new), `src/runtime/auto_type.rs` (`run_if_idle`), `src/runtime/tools/fs.rs` (`load_roots` pub(crate)), `host.rs` (S3 interim: `Decision::Ran(N)` answered at once, "output not read back") | Modify/Create | S3 |
| `src/core/ai/tools/settle.rs` (new), `engine.rs` (`Answer`, `RunReport`; `Decision::Ran` removed), `approver.rs` (`park`, `finish`), `host.rs`, `runtime.rs` (probe on tick), `src/app/auto_run.rs` | Create/Modify | S4a |
| `src/core/ai/tools/capture.rs` (new), `src/app/auto_run.rs`, `host.rs` (clean, redact, wrap) | Create/Modify | S4b |
| `src/core/ai/tools/mod.rs:117`, wiring snapshots, approval prompt `why`, README/privacy, `agent-tools/design.md` | Modify | S5 |

## Testing Strategy (strict TDD, `cargo test`)

| Slice | Tests |
|---|---|
| S1 | config parse table (`run` ok, `RUN` → ask plus a problem); `cycled` round trip; menu row tests; prefix `a` cycles with the system lines; badge buffer tests (colours, narrow `R`/`A`) |
| S2 | `exec` tables: accept/refuse for every R4/R5 row in the spec (accented letters and digits accepted; U+202E, U+200B/C/D, U+FEFF, U+00A0, U+0085, a combining mark and ESC refused; `tree` accepted, `tree -a` refused; doubled space refused), the `at` span of the offending token, the property test `validate(s)==s`, zero-alloc (no `String` in the API); `resolve_exec` on tempdir fixtures: secret roots, a dotfiles symlink, outside HOME, `New` ancestor, HOME refusing `mkdir` outside |
| S3 | `route` matrix (each new ctx bit, pause at 3); app_tests `tool_run.rs`: AutoRun reserves the slot, Checked → `AutoRun` bytes end in `\r` exactly once, Refused/NotRun → prompt with reason, a batch to the same pane prompts, dirty set/cleared (Ctrl+U keeps it), `y` and `auto` never emit `\r`; runtime with `FakePty`: cwd mismatch or busy → NotRun, take happens before the write |
| S4a | `settle` tables with an injected `Instant` (silent mkdir, cd via the cwd signal, slow output, `None` foreground, timeout, alt screen, user key); `Waits` park/finish/end_turn; host wording; tick-probe emits only for Running slots |
| S4b | `capture` on `Pane` fixtures: wrapped echo, two-line p10k and starship prompts, 1,000 lines → head/tail plus marker, 8 KiB cut on a char boundary, generation change → None; host: redaction, forged closing tag inside the wrapper |
| S5 | wiring snapshot updates; a tools-off byte-identity test; prompt `why` snapshot |

## Migration / Rollout

No migration is needed. This is a feature-branch chain on `feat/agent-tools`, S1→S2→S3→S4a→S4b→S5. The non-test estimates are all under 400: about 150, 300, 340, 290, 160 and 80. Until S3, `run` routes like `auto`. S4b can be cut, which leaves "ran plus cwd". Older builds read `run` as a bad value and fall back to `ask`.

**Future options, not in this change:**
- OSC 133 for exact boundaries and the exit status.
- A background-job pattern for long commands: output to a file, a tail plus the path, and a concurrency cap.
- Tier-2 verbs.

## Open Questions

None. Resolved on 2026-10-17:
- Ctrl+U does NOT clear `dirty` (D21); the spec now matches.
- `tree` without `-L` runs directly, bounded by the output cap and the timeout (D20); the spec now matches.
- Latin letters (U+00C0 to U+024F without × and ÷) and ASCII digits are allowed in words (D20); other scripts, marks and symbols prompt; the spec now matches.
