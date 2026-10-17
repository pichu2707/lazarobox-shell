# Tasks: Auto-execute (`run` approval mode)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated non-test lines | S1 ~150, S2 ~300, S3 ~340, S4a ~290, S4b ~160, S5 ~80 (plus docs). Total ~1,320 |
| Estimated changed lines incl. tests and snaps | ~3,200 across 6 PRs |
| 400-line budget risk | Low (S1, S4b, S5); Medium (S2, S3, S4a). Fallback splits noted per slice |
| Chained PRs recommended | Yes |
| Delivery strategy | ask-on-risk (already resolved: chained PRs) |
| Chain strategy | feature-branch-chain, tracker `feat/agent-tools`; each slice is one PR targeting the tracker, opened only after the previous one is merged |

Decision needed before apply: No
Chained PRs recommended: Yes
400-line budget risk: Medium

Size rule: count added lines before the first `#[cfg(test)]` in each file; test files and `.snap` files are excluded. Re-measure before opening each PR. Above 400, use the noted fallback.

Rules for every slice:
- Strict TDD: a RED task (compiling stubs, exact assertions) precedes its GREEN task. Tests are never trimmed.
- Gate (last task of each slice): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- Snapshots (insta, no cargo-insta): `INSTA_UPDATE=new INSTA_FORCE_PASS=1 cargo test --lib`, review each `.snap.new` by hand, strip the `assertion_line:` line, rename to `.snap`.
- One work-unit commit per task group (conventional commits, no AI attribution). The planning phase commits nothing.
- Each slice leaves the tree releasable. `run` MUST NOT press Enter before S3's guards (dirty guard, cwd re-check, foreground check, one-write) exist.
- Tasks marked `[manual]` are for the user, in a real terminal.
- `ApprovalMode` and `set_approval_mode` MUST NOT appear under `src/runtime/` (D19 audit rule).

### Dependency Graph

```
2f-fix PR (tools.md prompt) merged ─────────────────────────────┐
                                                                 ▼
S1 (mode/config/menu/toggle/badge) ─▶ S2 (grammar + paths, pure) ─▶ S3 (CheckRun, Enter, guards)
        │                                                          │
        │                                                          ▼
        │                                              S4a (settle, Answer channel, cwd result)
        │                                                          │
        │                                                          ▼
        │                                              S4b (screen capture)  [cuttable]
        │                                                          │
        │                                                          ▼
        │                                              S5 (description, snapshots, README, deltas)
        ▼
write-tools W5 (depends on S1; runs directly in `run` only after S3)
```

- S1 → S2 → S3 → S4a → S4b → S5 is strictly sequential (feature-branch-chain). S2 is pure and needs only the S1 mode type for later wiring, but is chained for review order.
- S5 additionally blocks on the 2f-fix PR being merged (neutral wording in tools.md).
- S4b can be cut: the tree then ships "ran in pane N" plus cwd, and S5 proceeds without capture wording.
- Manual checks M1 and M2 run after S3 (M1) and after S1 (M2); M1 repeats after S4b.

## S1: Mode, config, menu, toggle, badge (~150 non-test lines)

Files: `src/core/ai/config.rs`, `src/core/ai/menu.rs`, `src/core/menu*.rs`, `src/core/prefix.rs`, `src/app/tool.rs`, `src/core/ai/chat/mod.rs`, `src/ui/components/chat.rs`. Behaviour: `run` routes exactly like `auto` until S3.

- [x] S1.1 RED: config parse table: `approval = "run"` parses to `Run`; `"RUN"`, `"Run"` and unknown values give `Ask` plus a problem naming all three values (`APPROVAL_PROBLEM`). Spec: configuration, config-persistence.
- [x] S1.2 RED: `ApprovalMode::{ALL, id, from_id, cycled(forward)}` round trip both directions (ask→auto→run→ask and back); `flipped` no longer exists.
- [x] S1.3 RED: Approval menu row cycles three values in both directions; popup height unchanged (buffer test). Spec: config-menu.
- [x] S1.4 RED: `Prefix a` cycles the focused chat with `cycled(true)`; one system line per mode; the `run` line is exactly `approval: run (cd, pwd, ls, tree, mkdir run at once and their output goes to the model; other commands ask)`; no confirmation to enter `run`; a pending approval survives the cycle; the mode resets on pane open and conversation switch. Spec: modal-input.
- [x] S1.5 RED: `ChatState::mode_badge() -> Option<ApprovalMode>`; badge buffer tests: ` AUTO ` on `warning_orange`, ` RUN ` on `error_red`, fg `bg_base`, bold; narrow forms `A` (below 6 columns) and `R` (below 5 columns). Spec: chat-pane.
- [x] S1.6 RED: `route` with mode `Run` returns the same result as `Auto` for every existing `auto` test (pins "run routes like auto until S3").
- [x] S1.7 RED: audit test: `ApprovalMode` and `set_approval_mode` do not appear under `src/runtime/` (extend the `runtime/tools/audit.rs` tokenizer rule).
- [x] S1.8 GREEN: add `Run`, `ALL`, `id`, `from_id`, `cycled`; remove `flipped`; update `APPROVAL_PROBLEM`; menu row; `Prefix a`; `mode_badge` replaces `auto_badge`; render badges; audit rule.
- [x] S1.9 REFACTOR: reuse the `NotifyMode` cycle pattern; delete dead `auto_badge` and `flipped` call sites.
- [x] S1.10 Snapshots: regenerate chat badge and menu snapshots by the manual insta procedure; review each `.snap.new`.
- [x] S1.11 Docs: doc comments on `ApprovalMode` (model can never switch it) and `mode_badge`.
- [x] S1.12 Gate. Measure non-test lines; expect ~150. Measured: 77 added non-test lines, gate green (4592 lib tests).
- [ ] S1.M [manual] M2: narrow-width badge check in a real terminal: shrink the chat pane and confirm ` RUN ` becomes `R` and ` AUTO ` becomes `A` without wrapping or overlapping the title.

## S2: Grammar and path policy, pure (~300 non-test lines)

Files: `src/core/ai/tools/exec.rs` (new), `src/core/ai/tools/paths.rs` (`resolve_exec`, `Want`), `src/core/ai/tools/mod.rs` (mod only). No behaviour change at runtime; nothing calls these yet. Spec: pane-execution (grammar, flags, path policy).

- [ ] S2.1 RED: `parse` accept tables per verb: `cd`, `cd ~`, `cd ~/dev/x`, `pwd`, `ls`, `ls -la x`, `ls -1thr a b`, `tree`, `tree -a`, `tree -d -L 2 src`, `mkdir -p a/b`, single-quoted words with spaces, accented letters and digits (`música`, `año-2026`).
- [ ] S2.2 RED: refuse table for every R5 row: `; & | < > ( ) { } $ \` \ ! * ? [ ] #`, newline, tab, `"`, env assignment, globs, `-` and `--` words, long options, `ls -R`, `tree -o f`, `mkdir` without `-p`-only flags or with 0 or more than 4 paths, over 8 words, a word starting with `=`, `+`, `%`, `~user`, leading/trailing/doubled space, single-quoted words starting with `~` or `-`, containing `'` or `\`.
- [ ] S2.3 RED: Unicode refusals: U+202E and the bidi range, U+200B/C/D, U+FEFF, U+00A0, U+2028/2029, U+0085, a combining mark (decomposed accent), ESC, C1 controls and DEL.
- [ ] S2.4 RED: `Fallback.at` is the byte span of the offending token and `Fallback.why` names the reason (used by the S5 prompt).
- [ ] S2.5 RED: property test: for every accepted `s`, `validate(s) == s`; `SafeCommand::as_str()` borrows the input (pointer equality) and the API has no `String` (zero-alloc).
- [ ] S2.6 RED: `targets` yields `(path, Want)`: `cd` Existing (or HOME for bare `cd`), `ls`/`tree` Existing, `mkdir` New; cwd-implied targets for `ls`/`tree`/`pwd`/relative `mkdir`.
- [ ] S2.7 RED: `resolve_exec` on tempdir fixtures: secret roots refused (`~/.ssh`, `~/.aws`), a dotfiles symlink into a secret root refused on the real path, outside-HOME allowed for `Existing`, `New` canonicalizes the deepest existing ancestor, `mkdir` outside HOME refused, cwd itself must pass for every verb except `cd`, `SecretRoots::load` returning `None` gives a fallback.
- [ ] S2.8 GREEN: implement `exec.rs` (`CLASS` table, single pass, fixed `[&str; 8]`, per-verb flag table) and `resolve_exec` / `Want` in `paths.rs`.
- [ ] S2.9 REFACTOR: share lexical normalization with `resolve`; no duplicated secret-root logic.
- [ ] S2.10 Docs: module docs listing the allow-list and the invariant `checked bytes == written bytes`.
- [ ] S2.11 Gate. Measure; expect ~300. Fallback split if over 400: S2a (`exec.rs`) and S2b (`resolve_exec`).

## S3: CheckRun round trip, Enter write, dirty guard, cwd re-check (~340 non-test lines)

Files: `src/core/ai/chat/approval.rs`, `src/app/auto_run.rs` (new), `src/app/run.rs` (`frame_bytes` split), `src/app.rs`, `src/runtime.rs`, `src/runtime/run_check.rs` (new), `src/runtime/auto_type.rs`, `src/runtime/tools/fs.rs` (`load_roots` pub(crate)), `src/runtime/tools/host.rs` (interim answer). Spec: pane-execution, terminal-session, agent-tools, pane-commands.

Interim (until S4a): the host answers `Decision::Ran(N)` at once with "output not read back". This is the only releasable wording before the answer channel lands.

- [ ] S3.1 RED: `route` matrix with `Route<'a> { Prompt(Option<Fallback>), AutoType, AutoRun(SafeCommand) }`: each `AutoCtx` bit (`dirty`, `executing`, `cwd_known`, `paused`, plus existing focus/hidden/cap) forces `Prompt`; `Auto` and `Ask` never produce `AutoRun`; grammar fallback gives `Prompt(Some(fallback))`.
- [ ] S3.2 RED: fallback pause: `run_fallbacks` counts consecutive grammar/policy fallbacks; at `MAX_RUN_FALLBACKS = 3` every later call in the turn prompts with `auto-run paused for this turn`; limit fallbacks do not count; reset at turn start and after each run.
- [ ] S3.3 RED: app test `tool_run.rs`: `AutoRun` reserves a per-target `RunSlot{phase: Checking}`, counts toward `auto_typed` with no refund on fallback, emits `Effect::CheckRun`; a second call to the same pane in a batch prompts.
- [ ] S3.4 RED: app test: `RunEvent::Checked` re-runs `route`; if it passes, emits `Effect::AutoRun` whose bytes end in `\r` exactly once (`frame_bytes` plus `\r`, same alt-screen and paste checks as `typed_bytes`); `Refused{reason}` and `NotRun{reason}` open the prompt with that reason.
- [ ] S3.5 RED: app test: `y` on a prompt and the `auto` path never emit `\r` (distinct Effect variant, structurally impossible); an off-list command in `run` prompts and `y` types without Enter.
- [ ] S3.6 RED: dirty guard: `user_dirty` set by every forwarded key, paste or wheel write; cleared only by Enter and Ctrl+C; Ctrl+U does NOT clear it but still clears `line_pending`; a dirty pane prompts.
- [ ] S3.7 RED: macOS path: `cwd_known == false` always prompts with `the pane's directory cannot be read on this system`.
- [ ] S3.8 RED: runtime `run_check::check` on tempdir fixtures: re-parses the same bytes, builds `load_roots`, calls `resolve_exec` per target; 2 s timeout gives `Refused`; the answer carries `cwd` and `cd_target`.
- [ ] S3.9 RED: runtime with `FakePty`: write step requires `readlink == cwd` AND foreground is the shell; mismatch or busy gives `NotRun` and nothing is written; `take` happens before the write; one write only.
- [ ] S3.10 RED: cleanup: turn finish/cancel drops slots of that `(chat, turn)`; pane close and chat close clear slots; `executing` is released in every case.
- [ ] S3.11 RED: host interim: `Decision::Ran(N)` is answered at once with a result that says the output was not read back.
- [ ] S3.12 GREEN: `AutoCtx` bits, `Route::AutoRun`, `RunSlot`, `Effect::CheckRun` / `Effect::AutoRun`, `AppEvent::Run` plus `run_rx` arm, `frame_bytes` split, `user_dirty`, `run_check.rs`, `run_if_idle`, pub(crate) `load_roots`, host interim answer.
- [ ] S3.13 REFACTOR: single choke point `route`; no mode in `Call`, `ChatRequest`, `StreamEvent` or `Effect`; re-run the S1.7 audit.
- [ ] S3.14 Docs: doc comments for the one-write invariant, the cwd TOCTOU residual and `Ctrl+U` rationale.
- [ ] S3.15 Gate. Measure; expect ~340. Fallback split if over 400: S3a (route, slot, CheckRun, run_check) and S3b (AutoRun write, runtime guards, dirty flag).
- [ ] S3.M1 [manual] M1: real-shell checks in bash, zsh and fish, with a chat in `run`: (a) "take me to <dir>" runs `cd` with no `y` and no Enter; (b) `ls` and `mkdir -p` run; (c) half-typed line: type `rm -rf ` in the pane, then ask for a `cd`; it MUST prompt and not run; (d) Ctrl+U on the half-typed line does not re-enable runs until Enter or Ctrl+C; (e) a REPL (python, node) in the foreground: a command prompts and nothing is typed into it; (f) `cd ~/.ssh`, `ls *`, `cd x; rm y` never run. Record results in the PR.

## S4a: Settle machine, answer channel, cwd result (~290 non-test lines)

Files: `src/core/ai/tools/settle.rs` (new), `src/core/ai/tools/engine.rs` (`Answer`, `RunReport`; `Decision::Ran` removed), `approver.rs` (`park`, `finish`), `src/runtime/tools/host.rs`, `src/runtime.rs` (probe on tick), `src/app/auto_run.rs`. Spec: pane-execution (settle, result contract), terminal-session.

- [ ] S4a.1 RED: `Settle::step` tables with an injected `Instant`: silent `mkdir` (progress via generation or cursor), `cd` settles through the cwd signal, slow output keeps waiting, quiet under 250 ms waits, `foreground == None` waits (fail closed), `foreground == Some(false)` waits, timeout at 10 s gives `TimedOut`, `alt_screen` gives `FullScreen`, `user_key` gives `UserTyped`, generation change gives settled with `lines_comparable() == false`.
- [ ] S4a.2 RED: rule order: alt screen and user key win over timeout; timeout wins over settle conditions.
- [ ] S4a.3 RED: `Waits` stores `oneshot::Sender<Answer>`; `park`, `finish`, `end_turn`, `end_pane` and drop cleanup also cover running calls; `Decision` stays `Copy` and `From<Decision> for Answer`.
- [ ] S4a.4 RED: tick probe: the runtime emits `RunEvent::Probe{foreground, cwd}` only for panes where `App::run_probes()` yields a Running slot; none when idle (no new thread or timer).
- [ ] S4a.5 RED: host wording table, every row starting with `<cmd>: `: Settled (`Ran in pane N. cwd: <p>. Exit status is not known.` plus `(no output)`), cd mismatch (`The shell is in <actual>, not <target> ...`), TimedOut (`still running after 10 s`), Lost (`its output is not available`), FullScreen, UserTyped; rows and the `pane N: <cmd> → <cwd>` system line for `cd` only.
- [ ] S4a.6 RED: pane close during a run emits `RunDone(PaneGone)`; `executing` released; `CwdSource::{Proc, Osc7}` labelled, and OSC 7 is never used for the policy.
- [ ] S4a.7 GREEN: implement `settle.rs`, `Answer`, `RunReport`, `park`/`finish`, tick probe, `Effect::RunDone`, host wording; remove `Decision::Ran` and the S3 interim answer.
- [ ] S4a.8 REFACTOR: confirm no `ApprovalMode` under `src/runtime/` (S1.7 audit); no duplicated cleanup paths.
- [ ] S4a.9 Docs: doc comments on `Settle` rules and `RunReport`.
- [ ] S4a.10 Gate. Measure; expect ~290. Fallback split if over 400: S4a-i (settle and tests) and S4a-ii (Answer channel and host).

## S4b: Screen capture (~160 non-test lines)

Files: `src/core/ai/tools/capture.rs` (new), `src/app/auto_run.rs`, `src/runtime/tools/host.rs` (clean, redact, wrap). Cuttable: if cut, the S4a result stays "ran plus cwd". Spec: pane-execution (capture, result contract), agent-tools (untrusted output).

- [ ] S4b.1 RED: `capture` on `Pane` fixtures: simple output between anchor and prompt; wrapped command echo rows skipped; two-line p10k and starship prompt decoration trimmed (up to 2 lines above the new prompt compared with the lines above the anchor); blank edges trimmed; rows joined via `copy::join`.
- [ ] S4b.2 RED: caps: 1,000 lines gives head 100 plus tail 100 with `[… N lines not shown …]`; 8 KiB cut on a char boundary; at most 2 `LineSpan`s read.
- [ ] S4b.3 RED: generation change gives `output: None` (`output not available`); `None` nonce gives `output not available` (macOS).
- [ ] S4b.4 RED: host: output passes `clean`, `redact`, `label::wrap` off the UI thread; a forged closing tag inside the output cannot close the wrapper; secrets are redacted (including a leaked prompt line).
- [ ] S4b.5 GREEN: implement `capture.rs`, app glue at `Done`, host cleaning pipeline.
- [ ] S4b.6 REFACTOR: reuse existing `clean`/`redact` helpers from the read tools; no second redaction table.
- [ ] S4b.7 Docs: doc comments on capture heuristics and the residual prompt-leak threat.
- [ ] S4b.8 Gate. Measure; expect ~160.
- [ ] S4b.M1 [manual] repeat M1 (a)-(c) with bash, zsh and fish, now checking the model receives bounded output for `ls -la` and `tree`, and a decorated two-line prompt does not leak into the output.

## S5: Description, snapshots, README and privacy, deltas to agent-tools (~80 non-test lines plus docs)

Blocked until the 2f-fix PR (tools.md prompt) is merged. Files: `src/core/ai/tools/mod.rs:117`, wiring snapshots, approval prompt `why`, `README.md`, `openspec/changes/agent-tools/design.md` and spec deltas. Spec: agent-tools, pane-commands.

- [ ] S5.1 RED: tools-off byte-identity test: with tools off, every engine request is byte-identical to the pre-change baseline.
- [ ] S5.2 RED: the `run_in_pane` tool description is neutral and identical in every mode (exact text from design); `TYPED_TEXT` unchanged; wiring snapshots show the new description for all wired engines.
- [ ] S5.3 RED: prompt `why` snapshot: a line such as `not run directly: not in the safe list: "|"` plus a command window centred on `Fallback.at`, bounded to the box width.
- [ ] S5.4 GREEN: update the description, prompt rendering with `why`, and regenerate snapshots by the manual insta procedure (review each `.snap.new`, strip `assertion_line:`).
- [ ] S5.5 Docs: README `run` mode, safe list, residual threats (cd hooks, symlink swap TOCTOU, PS2 continuation, prompt-line leak, `tree -a` names) and privacy note that pane output goes to the model in `run`, cloud engines included.
- [ ] S5.6 Docs: edit `agent-tools/design.md`: line 9 gains the `run` exception, remove the line 91 non-goal, D18 threat-model sentence becomes "nothing outside the D20 allow-list"; apply the spec deltas to the agent-tools capability.
- [ ] S5.7 Gate. Measure; expect ~80 code lines.
- [ ] S5.M1 [manual] final M1 pass in bash, zsh and fish (cd, ls, mkdir, half-typed line, Ctrl+U, REPL in the foreground) and M2 (narrow-width badge) on the merged tracker before the 0.16.0 release.

## Cross-change notes

- write-tools W5 depends on S1 (the mode type); its `create_dir` runs directly in `run` only after S3 is merged.
- Rollback: each slice reverts alone on the tracker. Reverting S3 to S4b returns to "type, never Enter" with no data change; older builds read `approval = "run"` as a bad value and fall back to `ask`.
