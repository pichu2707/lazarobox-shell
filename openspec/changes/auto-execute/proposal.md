# Proposal: Auto-execute (a `run` approval mode: type, Enter, output back to the model)

## Intent

In the 2f.10 manual test the user said "llévame a curso-rust". Claude found the directory and typed `cd ~/…/curso-rust` into the pane, but the user still had to press `y` and then Enter. In `auto` the `y` goes away, but the Enter does not, and the model never learns whether the `cd` worked. The owner wants a direct order like that to just happen, before 0.16.0. Design D18 had deferred "auto-execute (Enter plus output capture)" to a separate SDD. This is that SDD.

The goal: a chat in `run` mode can run a small, strictly checked set of navigation commands in the user's pane, and the model gets a bounded, untrusted result back. Everything else keeps the D17/D18 behaviour.

## Decisions (user, 2026-10-17, settled; engram `sdd/auto-execute/explore`)

- **R1 Mode.**
  - `ApprovalMode` gets a third value, `run`, giving ask / auto / run.
  - The mode is per chat, as in D18. It resets on pane open and on conversation switch, and it is never stored per conversation or sent to the model.
  - The config default stays `ask`. `[ai.tools] approval = "run"` is a valid config value.
- **R2 Toggle.**
  - `Ctrl+Space a` cycles ask → auto → run → ask.
  - A system line names the new mode and says what it does.
  - The Approval menu row cycles the same three values, so the popup height does not change.
  - Entering `run` needs no extra confirmation.
- **R3 Badges.** ` RUN ` is red. ` AUTO ` changes from red to amber.
- **R4 Safe list.** In `run`, these commands are typed and followed by Enter. Each has a per-command flag allow-list:
  - `cd`, with no flags;
  - `pwd`, with no flags;
  - `ls`, with `-l -a -h -1 -t -r` (no `-R`);
  - `tree`, with `-L -d -a` (`-L` optional; no `-o`);
  - `mkdir`, with `-p` only.
- **R5 Grammar.**
  - A strict whitelist grammar with no shell expansion.
  - It refuses ``; & | < > ( ) { } $ ` \ ! * ? [ ] #``, newlines, double quotes, env assignments and globs.
- **R6 Path policy.**
  - Every argument is resolved against the shell's real cwd and checked with `tools::paths`.
  - `cd` and `mkdir` never enter secret or credential dirs.
  - `cd` is not limited to `$HOME`.
- **R7 Off-list commands.** In `run`, a command outside the safe list gets the normal approval prompt. There, `y` types it without Enter, exactly as D17 does today.
- **R8 Output.**
  - The output goes back to the model for every engine, cloud engines included.
  - It is bounded to about 8 KiB / 200 lines (head plus tail, with a truncation marker).
  - It is cleaned, redacted and nonce-wrapped (D14).
  - For `cd`, the result carries the authoritative new cwd.
- **R9 Completion.**
  - Done means a heuristic settle: the foreground is back to the shell, plus a short quiet time, plus a change in output or cwd.
  - A hard timeout of about 10 s applies.
  - There is no exit status. OSC 133 is a later follow-up.
- **R10 Dirty line.** If the user has a half-typed line in the target pane, the command does not run, and the user is prompted instead.
- **R11 Existing limits.** Each of these makes a run fall back to the prompt:
  - the target pane has the focus, its tab is hidden, or the tab is zoomed onto another pane;
  - the foreground check fails;
  - the target would be an update pane, which is never targeted.

  `MAX_AUTO_PER_TURN` = 5 is shared between typed and run commands.
- **R12 Description.**
  - The tool description is neutral and identical in every mode.
  - Each result says what actually happened.
  - With tools off, requests stay byte-identical.

## Scope

### In Scope
- The `run` value across config, menu, toggle, badge and system line.
- A pure allow-list grammar plus the per-command flag and path policy, in core.
- `Route::AutoRun`: one PTY write of the typed bytes plus `\r`, the per-pane dirty-line and "executing" marks, and the policy checked against the real cwd at write time.
- The settle state machine, the cwd result, and a text channel for results (because `Decision` is `Copy`).
- Capture of the pane's screen text after the anchor line, bounded and passed through D14.
- The neutral tool description and truthful result wording, updated snapshots, the README and privacy note, the specs, and design D19–D21.

### Out of Scope
- Content readers (`cat`, `head`, `tail`, `grep`, `rg`), `git`, `find`, `env`/`printenv`, pagers and editors.
- Running arbitrary commands, and `y` ever pressing Enter.
- OSC 133 and exit status.
- Write tools (`create_dir` and others). They are the separate `write-tools` change, which depends on S1–S3 here.
- Injecting shell rc snippets, and limiting `cd` to `$HOME` (R6 keeps it open).

## Capabilities

### New Capabilities
- `pane-execution`: covers
  - the grammar and the safe list with its flag allow-lists;
  - the argument path policy against the real cwd;
  - Enter in the same write;
  - the dirty-line guard and the executing mark;
  - settle and timeout;
  - screen capture;
  - the result contract: ran / timed out / not available, plus cwd and bounded text.

### Modified Capabilities
All deltas apply on top of the unarchived `agent-tools` change specs where the capability lives there.
- `agent-tools`:
  - Chat approval mode gets the third value.
  - The `run_in_pane` contract may now return "ran" plus output.
  - "Tool output is untrusted data" now covers pane output.
- `pane-commands`: "Approval types, never executes" gains the `run` exception. "No output capture" becomes "only in `run` mode".
- `chat-pane`: the ` RUN ` badge is red, ` AUTO ` turns amber, and the system lines change.
- `configuration`, `config-persistence`: `approval` accepts `run`. Values stay case-sensitive, and a bad value falls back to `ask`.
- `config-menu`: the Approval row cycles three values, and the height is unchanged.
- `modal-input`: the prefix `a` leaf cycles three modes.
- `terminal-session`: a run aborts if the pane enters the alternate screen during capture, and the write appends `\r` only in `run`.

### Superseded design text (agent-tools `design.md`)
- The D18 non-goal (line 91): auto-execute is now in scope here.
- The header sentence (line 9), "lazarobox never sends Enter": it gains the `run` exception.
- "Approval types, never executes" and "No output capture" (pane-commands spec, D18 threat-model addendum): amended.
- D17 itself is unchanged. D18 is extended by D19 (the mode and the allow-list), D20 (Enter and the dirty guard) and D21 (capture).

## Approach

1. **Routing.** `approval::route` stays pure. It returns `AutoRun` only when all of these hold:
   - the mode is `Run`;
   - the command passes the grammar and the policy;
   - the pane is not dirty or executing;
   - every R11 limit passes.

   Any other case returns `Prompt`, never a denial.
2. **Writing.**
   - `App::typed_bytes` is reused, so the checks cannot drift.
   - `Effect::AutoRun` writes the bytes plus `\r` in one write, after the runtime has checked the foreground and re-checked the path policy against `/proc/<pid>/cwd`.
   - If the cwd is unreadable, the call prompts.
3. **Capture.**
   - The app records an anchor (the stable cursor line plus `line_generation`).
   - A polled `AppEvent::Foreground` feeds a pure settle machine.
   - At settle, the app reads the lines between the anchor and the prompt. Then it cleans, caps, redacts and wraps them.
4. **Result.** The call stays pending until the capture ends. The answer travels on its own channel next to `Decision`. Cancelling the call or ending the turn drops the capture.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/config.rs`, `src/core/menu.rs`, `src/core/prefix.rs`, `src/ui/components/chat.rs` | Modified (mode, row, toggle, badges) |
| `src/core/ai/tools/exec.rs` | New (grammar and policy) |
| `src/core/ai/chat/approval.rs`, `src/app/tool.rs`, `src/app/run.rs` | Modified (AutoRun, marks, Effect) |
| `src/runtime.rs`, `src/runtime/auto_type.rs`, `src/runtime/tools/host.rs` | Modified (write with `\r`, cwd read, foreground poll, results) |
| `src/core/ai/chat/engine.rs`, approver/pending | Modified (result text channel) |
| `src/core/pane.rs` (read only), a new settle module in core | New / Modified |
| `src/core/ai/tools/mod.rs`, wiring snapshots, README | Modified (neutral description) |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Enter is appended to a half-typed user line (`rm -rf ` + `cd x`) | Med | A per-pane `user_dirty` flag, set by any forwarded key or paste except Enter, Ctrl+C and Ctrl+U. Up and Tab count as dirty. A dirty pane prompts. The focused-pane fallback also applies |
| `cd` into an attacker-chosen directory fires shell hooks (chpwd, direnv, prompt `git status` → fsmonitor) | Med | Accepted and documented. The path policy, the RUN badge, a system line naming the new cwd and the result row are the mitigations. `run` is never the default |
| Heuristic completion is wrong (slow output, silent `mkdir`, a multi-line prompt) | Med | The settle also accepts the prompt line moving. A 10 s hard timeout returns "still running, output so far". A changed generation returns "output not available". No exit status is ever claimed |
| macOS has no `/proc/<pid>/cwd` | High on macOS | Relative arguments cannot be checked there, so the call falls back to the prompt and never runs blind. The cwd result falls back to OSC 7 when it is present |
| `Decision` is `Copy` and cannot carry text | Certain | A separate answer channel (S4a), kept out of S3 so the Enter path lands first |
| Prompt injection asks the model to run something | Med | Allow-list only, with no expansion, no content readers, no network and no writes beyond `mkdir`. Output passes through D14 |
| Spec, design and README still promise "never executes" | High | All of them are updated in S5, before the 0.16.0 release |
| S4 exceeds 400 lines | Med | It is already split into S4a and S4b |

## Rollback Plan

- Each slice reverts on its own on the tracker `feat/agent-tools`.
- Until S3, `run` routes like `auto`. Reverting S3–S4b returns to "type, never Enter" with no data change.
- Older builds read `approval = "run"` as a bad value and fall back to `ask` with a notice.
- No persisted state or migration is involved.

## Dependencies

- 2f-fix is merged first: it carries the `tools.md` prompt that needs the neutral wording.
- `write-tools` follows this change and depends on S1–S3 (`create_dir` runs directly in `run`).
- Then come Ollama tools 3a/3b and the 0.16.0 release.

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
- [ ] In `run`, "llévame a curso-rust" ends with the pane in that directory, with no `y` and no Enter. The model's result names the new cwd.
- [ ] `ls -la` in `run` returns bounded, wrapped output. `ls -R`, `tree -o f`, `cd ~/.ssh`, `mkdir ~/.aws/x`, `ls *` and `cd x; rm y` never run.
- [ ] An off-list command prompts, and `y` types it without Enter.
- [ ] A dirty, focused, hidden, busy or update pane, or the 6th call in a turn, prompts.
- [ ] `ask` and `auto` behave exactly as before. With tools off, requests are byte-identical.

## Slices and Size Forecast (non-test lines, each PR under 400)

| Slice | Content | Size |
|---|---|---|
| S1 | Mode, config, menu, toggle, badges (`run` routes like `auto`) | 150–200 |
| S2 | Grammar, flag allow-lists, lexical path rule (pure core) | 250–300 |
| S3 | `Route::AutoRun`, dirty and executing marks, Effect, write with `\r`, real-cwd policy, result wording | 300–380 |
| S4a | Settle machine, foreground poll, cwd result, text channel | 180–230 |
| S4b | Screen capture, cap, redact, wrap | 170–220 |
| S5 | Description, snapshots, README and privacy, specs, design D19–D21 | ~60 code + docs |

The total is about 1,100–1,400 lines. It is a feature-branch chain onto `feat/agent-tools`, with delivery `ask-on-risk`. If the date is at risk, S4b can be cut, leaving "ran in pane N" plus the cwd only.

## Proposal question round

Resolved: the user settled every product question on 2026-10-17 (R1–R12). Open for design, not product:
- the exact settle signals for silent commands;
- the shape of the text channel.
