# Exploration: auto-execute (a `run` approval mode: type + Enter + output back to the model)

User decision (2026-10-17): after the 2f.10 manual test (claude typed `cd` through the tools, no Enter), the owner wants a direct order such as "llévame a curso-rust" to run without `y` and without a manual Enter. Of the three options offered (auto is enough / auto-execute only for cd / full auto-execute now), they chose **full auto-execute now**, before 0.16.0. Design D18 had deferred "auto-execute (Enter plus output capture)" to a later SDD; this is that SDD. Order after it: Ollama tools 3a/3b, then release 0.16.0. Engram copy: `sdd/auto-execute/explore`.

## Current state

### Mode and routing
- `ApprovalMode {Ask, Auto}` in `src/core/ai/config.rs:130`, with `id`, `from_id` and `flipped`. `APPROVAL_PROBLEM` names only `ask` and `auto`.
- The pure router `route()` in `src/core/ai/chat/approval.rs:117` returns `Route {Prompt, AutoType}`.
  - `AutoCtx` has `typed_this_turn`, `line_pending`, `target_focused` and `tab_shown`.
  - `MAX_AUTO_PER_TURN` is 5.

### App side (`src/app/tool.rs`)
- The flow is `on_tool_ask`, then `auto_route`, then `auto_type`, then `typed_bytes`.
- `typed_bytes` (`src/app/run.rs:248`):
  - validates the command, checks the pane and refuses the alt screen;
  - uses `encode_paste` or raw bytes, and refuses `\n`/`\t` without bracketed paste;
  - never appends `\r`.
- `Effect::AutoType` sets `pty.line_pending = Some(call)`. Enter, Ctrl+C and Ctrl+U clear it in `note_user_key`.

### Runtime
- `src/runtime.rs:970` first claims the answer (`chats.take`).
- Then `auto_type::type_if_idle` checks `PtyHandle::shell_in_foreground()` (tcgetpgrp, `portable.rs:97`) and answers `Typed(N)`, `Busy(N)` or `Denied`.
- `Decision` (`engine.rs:129`) is `Copy` and holds only a `usize` and `&'static str`, so it cannot carry output text.

### Host (`src/runtime/tools/host.rs:200-290`)
- `Typed(N)` becomes `TYPED_TEXT` and the row "typed into pane N - no Enter".
- Results already pass through D14: `sanitize::clean`, a 32 KiB cap and the nonce wrapper. `memory::redact::redact` exists.

### Tools prompt
- There is no `tools.md` on this branch; it lives on the unmerged 2f-fix branch.
- Everything that says "never executes" is in three places:
  - the description at `src/core/ai/tools/mod.rs:117`;
  - `TYPED_TEXT`;
  - the wiring snapshots.

### Specs and design say the opposite today
- pane-commands: "Approval types, never executes", "No output capture".
- agent-tools:
  - "run_in_pane tool MUST return only sent or denied";
  - "Chat approval mode": "Executing the command ... is NOT part of any mode".
- design.md: the header (line 9), D17, D18, the threat-model addendum and the non-goal at line 91.

### Screen reading
- `Pane` (`src/core/pane.rs`) wraps vt100 as `pty.emu` in App state.
- Useful calls: `read_lines(LineSpan)` with stable line numbers (`first_line()` + `scrollback_len`), `line_generation()`, `live_cursor()`, `line_end`, `cwd()` (OSC 7 only).
- **OSC 133 is not supported**: `Responder::unhandled_osc` parses only OSC 7.
- The runtime polls the shell's real cwd from `/proc/<pid>/cwd` every 1 s (`runtime.rs:97`, `:1111`; Linux only).

### Other facts
- **Threading:** the foreground check is in the runtime (`handles`), the emu is in the App, and the same loop drives both (`Panes::apply`). Capture therefore needs a runtime→App event (like `poll_cwds`) or an Effect round trip.
- **Dependencies:** no shlex or shell-words crate. `vt100 0.16.2` only.
- **Compatibility:** `auto` has never shipped (0.16.0 is unreleased), so extending it breaks nothing.

## 1. Mode model
| Option | Pros | Cons |
|---|---|---|
| **M1: a third value `run`** (ask / auto / run). **Recommended.** | `auto` keeps its meaning (types, never runs). The escalation is explicit and per chat, with a distinct badge. The default stays `ask`. Fits the enum + `cycled` pattern (`NotifyMode`). | `Prefix a` becomes a cycle. Touches the menu row, `config_edit`, `APPROVAL_PROBLEM`, `flip_approval` and specs in 5 capabilities. |
| M2: a per-command decision inside `auto` | No new value. | Silently changes what `auto` means. The badge cannot say what will happen. The "types only" mode disappears. |

- **Config:** `[ai.tools] approval = "ask"|"auto"|"run"`, case-sensitive. A bad value falls back to `ask` with a notice.
- **Menu:** the same Approval row cycles through 3 values, so the popup height is unchanged.
- **Toggle:**
  - `Prefix a` cycles forward, and the system line names the new mode.
  - Alternative: a second leaf just for `run`, which avoids reaching it with one press.
- **Badge:** ` RUN ` in red, ` AUTO ` in a calmer colour. Below 6 columns, `R`/`A`.
- **Scope:** per chat, reset on pane open and conversation switch, never stored or sent (D18 rules).

## 2. What may run (the safety core)

### Approach
- **Option A, recommended:** an allow-list with a strict grammar.
- **Option B, rejected:** a deny-list. A shell has unbounded ways to spell a dangerous command, so a deny-list fails open.
- **Option C, rejected for v1:** run everything in `run`. A prompt injected through `read_file` would become arbitrary code execution.

### Parser
- A small whitelist tokenizer in core, about 120 lines, with no crate.
- Accepted words: `[A-Za-z0-9._/@%+:,~-]`, `=` after the first character of an argument, and optionally plain single-quoted strings.
- Everything else is refused:
  - the characters `; & | < > ( ) { } $ \` \ ! * ? [ ] #`;
  - newlines, double quotes and env assignments;
  - a first word starting with `=`/`~`;
  - a leading space, `\r` and `\t`.
- Flags (`-x`) pass only through a per-command allow-list.
- Globs are refused in v1: expanded file names can start with `-`, and zsh has brace expansion and EXTENDED_GLOB.

### Allow-listed commands can still be dangerous
- `find`: `-exec`, `-execdir`, `-ok`, `-delete`, `-fprint*`, `-fls`. Excluded; `find_dir` already covers it.
- `git` is not read-only on an untrusted repo:
  - `core.fsmonitor`, `diff.external` and textconv run code from `.git/config`;
  - pagers grab the foreground;
  - `-c`, `--output`, `--ext-diff` and `--exec-path` are dangerous.
  
  Excluded from v1. A later version could allow only the exact form `git --no-pager status|log|diff` with allow-listed flags.
- `rg`: `--pre` and `--hostname-bin` execute programs.
- `cat`, `head`, `tail`, `grep` and `rg` send file content to the model, bypassing the cloud `read_file` approval, redaction and the path policy. Excluded; `read_file` already covers them. `tail -f` and a bare `cat` would also block forever.
- `env`, `printenv` and `set` expose API keys. Pagers and editors are excluded. `tree -o` and `sort -o` write files.

### Lists
- **v1:**
  - `cd`, `pwd`;
  - `ls` with `-l -a -h -1 -t -r` (no `-R`);
  - `tree` with `-L -d -a` (no `-o`).
- **Tier 2 (open question):** `git --no-pager status|log`, `wc`, `file`, `stat`, `which`.

### Path policy on arguments
- Reuse `tools::paths` (`SECRET_DIRS`, `SECRET_FILES`, `CREDENTIAL_DIRS`, `check_path`, `resolve`, `SecretRoots`).
- Gotcha: relative arguments depend on the shell's real cwd, not the chat origin. `cd ~/.ssh` followed by a relative argument sidesteps a lexical check.
- Mitigations:
  - refuse `cd` into secret or credential dirs;
  - resolve arguments against the real cwd from `/proc/<pid>/cwd` at write time, and prompt when it cannot be read (macOS);
  - refuse `..`, and canonicalize to catch symlinks.
- Placement:
  - the grammar is pure, in core (e.g. `src/core/ai/tools/exec.rs`, next to `command.rs`);
  - the IO goes in the runtime step that already does the foreground check.

### Commands outside the list in `run`
- Recommended: fall back to the normal prompt, where `y` types the command without Enter, as in D17.
- Alternatives: type it as in `auto`, or refuse it with a reason.
- The router gains `Route::AutoRun`, and `Prompt` stays the fallback for every limit.

## 3. Sending Enter
- **One write:**
  - the `typed_bytes` output plus `b"\r"` after the bracketed-paste end marker, or after the raw single-line bytes;
  - a single `PtyHandle::write`, so it is atomic against user keys;
  - reuse `typed_bytes` through a new `Effect::AutoRun`, or an `enter` flag on `AutoType`, so the checks cannot drift.
- **`line_pending`:**
  - After Enter there is no pending line.
  - Mark the pane as "executing" in the same slot until capture ends. A second batched call to that pane then prompts.
- **New critical risk:**
  - D6 accepted "typed after a half-typed line" only because Enter was never sent. With Enter, a half-typed `rm -rf ` plus our `cd x` runs as `rm -rf cd x`.
  - Guard, with no screen reading: a per-pane `user_dirty` flag.
    - It is set by any key or paste forwarded to the pane, except Enter, Ctrl+C and Ctrl+U, which clear it. `note_user_key` already sees every key.
    - AutoRun requires a clean pane, otherwise it prompts.
    - History recall (Up) and Tab count as dirty.
  - Prefixing Ctrl+U is not recommended: it destroys the user's input, and bash only kills to the left of the cursor.
- **Foreground check:** already correct. A REPL, `ssh` or a nested shell gives `Busy`. Its blind spots remain: the `read` builtin, and a microsecond race.
- **Focus, hidden or zoomed tab, cap, pending approval and update pane:** unchanged, and they matter more now. Whether the per-turn cap is shared with typed commands is an open question.

## 4. Output capture
- **What comes back:**
  - Without shell integration the exit status is unknowable.
  - The result carries the bounded output text, whether it settled or timed out, and for `cd` the new cwd.
  - The cwd comes from `/proc/<pid>/cwd`, falling back to OSC 7. It is authoritative and alone answers "take me to curso-rust".
- **Completion heuristic:**
  - Builtins never leave the foreground, so done means: foreground is the shell, AND about 250 ms without output, AND (output was seen OR the cwd changed).
  - A hard timeout of about 10 s returns "still running, output so far" and releases the mark.
  - The alt screen during capture aborts with a reason.
- **Reading:**
  - At write time, record the anchor: the cursor's stable line plus `line_generation()`.
  - At completion, read `emu.read_lines(anchor+1 ..= cursor_line-1)`. If the generation changed (resize, eviction, screen flip), return "output not available".
  - Trim the blank edges. Multi-line prompts can leak decoration.
  - Then apply `clean()`, a cap (8 KiB or 200 lines, head + tail, with a "truncated N lines" marker), `redact()` and the nonce wrap.
- **Plumbing gotchas:**
  1. `Decision` is `Copy`, so it cannot carry text. The text needs its own channel (a Waits answer enum, or `Decision::Ran(usize)` + out-of-band text). This touches engine.rs, host.rs and approver/pending.
  2. The foreground state lives in the runtime and the emu in the App. Add a polled `AppEvent::Foreground(pane, bool)` for active captures (the `poll_cwds` pattern). The state machine is a pure core type (now, saw_output, foreground, cursor_line), testable in app_tests.
  3. The call stays pending until capture ends (timers paused, D13). Cancelling and ending the turn drop the capture.
- **OSC 133:** exact boundaries and the exit status, and cheap to parse (about 40 lines). It only works if the shell emits it (fish 4, kitty/wezterm scripts, p10k can; most setups do not). Use the heuristic in v1, OSC 133 later, and never inject rc snippets.
- **Order and fallback:**
  - Within the change: execute + settle + cwd first, screen text second.
  - Fallback cut if the date is at risk: "ran in pane N" + cwd only.
- **Privacy:**
  - Captured text goes to the engine. With the v1 list (names only, no content readers, path policy on arguments) the extra exposure is directory names, comparable to the free `list_dir`.
  - If content readers are ever allowed, cloud engines should keep a prompt for each run.
  - Extend the README privacy note and the `run` system line.

## 5. Prompt-injection threat model
- **Attack:** a file read with `read_file`, or a directory name from `ls`, tells the model "run X".
- **Worst case with the v1 list:** `cd`/`ls`/`pwd`/`tree` anywhere reachable. No network, no writes, no content read, no exfiltration beyond directory names to the provider.
- **Residual risks:**
  - (a) `cd` into an attacker-controlled directory triggers shell hooks: chpwd, zsh-autoenv, trusted direnv/mise, and prompt themes running `git status`, which can trigger fsmonitor. The path policy cannot catch this. An optional limit: `cd` only under `$HOME`, or only into directories learned through `find_dir`/`list_dir`.
  - (b) The cwd change silently changes the context of the user's next manual command. Mitigated by the result row, the RUN badge and a system line naming the new cwd.
  - (c) The dirty-line guard is a heuristic. Text restored by shell plugins goes undetected.
  - (d) Aliases or functions shadowing allow-listed names (a `cd` function such as zoxide) are the user's own.
  - (e) Instructions or escape sequences in file names shown by `ls`. D14 and `clean()` handle them.
  - (f) The foreground blind spots: `read`, and a program started between the check and the write.
  - (g) zsh, bash and fish differ (`!`, `=cmd`, `~user`, EXTENDED_GLOB). The grammar refuses those characters outright. Refusing by default is the control.

## 6. The tools prompt and tool description
- The `mod.rs:117` description and `TYPED_TEXT` become false in `run`.
- D18 requires the registry and requests to be identical across modes: the toggle acts mid-turn, and `tools/list` is fixed at session start.
- **Recommended:** a neutral description, with each result telling the truth. Example: "Send a shell command to the user's pane. Depending on the user's settings it is only typed (the user presses Enter) or typed and run; the result says which. Only simple read-only commands (cd, ls, pwd, tree) can be run directly."
  - Results read "typed … no Enter" or "ran in pane N: <cwd/output>".
  - The wiring snapshots change, and tools-off stays byte-identical.
- **Rejected:** a mode-aware description. It needs the mode in `ChatRequest`, which goes against D18.
- The tools prompt (`tools.md`, 2f-fix branch) needs the same neutral wording once merged.

## 7. Spec and design impact
- **New capability `pane-execution`:** the grammar, Enter, the executing mark, the dirty-line guard, capture and the result contract.
- **Deltas:**
  - agent-tools: Chat approval mode, the run_in_pane tool contract, and "Tool output is untrusted data" (now including pane output).
  - pane-commands: "Approval types, never executes" gets one exception; "No output capture" becomes "only in run mode".
  - chat-pane: the RUN badge.
  - configuration and config-persistence: the `run` value.
  - config-menu: the Approval row has 3 values.
  - modal-input: the binding cycles.
  - terminal-session: the alt screen during capture.
- **design.md:**
  - Supersede the D18 non-goal and the header sentence.
  - Add D19 (run mode and allow-list), D20 (Enter and the dirty-line guard) and D21 (capture).
  - Amend the threat-model addendum.
  - D17 is unchanged; D18 is extended.

## 8. Size and slices (non-test lines, each PR under 400)
1. **S1:** mode, config, menu, toggle, badge (`run` routes like `auto` for now). About 150-200.
2. **S2:** the allow-list grammar, the per-command policy and the lexical path rule. Pure core, about 250-300, plus table tests.
3. **S3:** the Enter path:
   - `Route::AutoRun`;
   - the dirty and executing marks;
   - the Effect plus the runtime write with `\r`;
   - the runtime cwd read for the path policy;
   - the result wording.
   
   About 300-380.
4. **S4a:** settle + cwd result + the text channel. **S4b:** screen text capture. Together about 350-450, hence the split.
5. **S5:** the tool description, snapshots, README and privacy, spec and design. About 60 lines of code plus docs.

Order: S1 → S2 → S3 → S4a → S4b → S5. S1 and S2 are safe to merge early: until S3, `run` behaves like `auto`.

## 9. Open product questions (proposed default)
1. The safe list: cd, pwd, ls, tree only? `git --no-pager status|log`? Never content readers? *(cd, pwd, ls, tree)*
2. Is `run` per chat, resetting like `auto`? *(yes)*
3. A command outside the list in `run`: prompt, type like `auto`, or refuse? *(prompt)*
4. Should `y` on that prompt ever press Enter? *(no)*
5. Does output go back to cloud engines too, or do they ask first? *(yes, bounded)*
6. `Prefix a` cycles 3 values, or `run` gets its own key? Badges? *(cycle; ` RUN ` red, ` AUTO ` amber)*
7. A per-turn cap shared with typed commands? Output cap? *(5 shared; 8 KiB / 200 lines)*
8. A confirmation when switching to `run`? *(system line only)*
9. Heuristic completion without exit status for 0.16.0, OSC 133 later? *(yes)*
10. `cd` limited to `$HOME`, refusing secret dirs? *(refuse secret dirs; `$HOME` open)*

## Recommendation
- M1, a third `run` mode.
- Option A: a whitelist grammar over cd/pwd/ls/tree, with flag allow-lists and the path policy checked against the real cwd.
- `\r` in the same write, guarded by `user_dirty` plus the existing limits.
- Capture by heuristic settle, with `cd` returning the authoritative cwd.
- A neutral tool description, with the truth in each result.
- Commands outside the list go to the prompt.

## Risks
- **Execution appended to a half-typed line.** New and severe; it needs the dirty guard.
- **Shell hooks triggered by `cd`.**
- **No exit status.**
- **Unknown cwd for relative paths on macOS.**
- **`Decision` is `Copy`.** That is a plumbing cost.
- **Spec, design and README promise "never executes".** They must change in the same release.
- **S4 goes over 400 lines unless split.**
- **Wiring snapshot churn.**
- **git, rg and find look read-only but are not.**
