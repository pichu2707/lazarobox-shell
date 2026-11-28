# Tasks: The chat runs approved commands itself (chat-runs-commands, #845)

Sources: proposal.md, design.md (D1-D14), specs/{agent-tools,pane-execution,chat-pane,command-runner,configuration,config-menu,agent-orchestrator}. Owner resolutions of 2026-11-22 are binding and already applied to spec and design (see "Owner resolutions").

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated production lines (total) | ~2,650 (non-test .rs, no blanks/comments, as rustfmt lays them out; design naive ~1,180 x ~1.9-2.2) |
| 400-line budget risk | High overall; every slice below is planned at or under 400 |
| Chained PRs recommended | Yes |
| Suggested split | S1a > S1b > S1c > S2a > S2b > S2c > S3a > S3b > S3c > S4 |
| Delivery strategy | auto-chain |
| Chain strategy | stacked-to-main |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: High

### Slices

| Slice | Goal | Est. prod lines | Risk |
|-------|------|-----------------|------|
| S1a | `settle.rs`: `Limits`, waiting phase, `Key`/hidden ranges, `StillRunning`/`NoInput`, step order (pure) | 300 | Medium |
| S1b | `echo_off()` on `core/pty/{mod,portable,fake}.rs`, `capture(.., hidden)`, `report.rs` words | 190 | Low |
| S1c | App wiring: `Settle::run`, probe, `user_key`, waiting/cut-off/no-input lines, `end_run`; README pair | 260 | Medium |
| S2a | `may_run_on_y`, `RunPolicy`, `approve_run`, `RUN_KEYS`/`TYPE_KEYS`, `Route::Auto*` removed | 260 | Medium |
| S2b | `Auto` removed, parser maps to `ask`, load-time rewrite (D13), toggle lines, badge, menu; README pair | 230 | Medium |
| S2c | Continuation follow after the 30 s cut (`Limits::Continue`, resume note) | 200 | Medium |
| S3a | `exec.rs` head-keyed grammar, `exec/specs.rs`, tiers, `AutoCtx.local` (seven verbs keep today's grammar) | 360 | High |
| S3b | New non-git verbs (`cat head tail wc`, `--version`), `Role::Rev`, `run_check` targets | 290 | Medium |
| S3c | git entries, `needs_one_of`, git config gate (`run_check`, `tools/git.rs`), `GIT_CONFIG_RISK`; README pair | 330 | High |
| S4 | Guidance (`tools.md`, `run_in_pane` description, `RUN_HINT`), macOS note; README pair | 230 | Low |

Slices at 330-360 are tight: measure while applying and move the last work unit to a follow-up slice if one crosses 400. Deviations from the design: S1 split in three, S2 in three, S3 in three (design allowed two), S4 doubled to fit the 1.9x factor.

## Owner resolutions (2026-11-22) and edits made to spec and design

1. Cut-off at 30 s continues with partial output and a notice; a continuation follow resumes the chat with the final output (spec pane-execution Long-running; design D9 unchanged).
2. Plain `git diff` asks; `--stat/--name-only/--name-status` run unprompted on cloud (spec agent-tools table and bypass rows).
3. Local engines keep seven unprompted commands (spec agent-tools Local engines; design D3 already).
4. `auto` migration: rewrite once, one notice, failure notice with reason (spec configuration x5 edits, config-menu x2; design D13, S2 file row, Migration).
5. 1 s quiet, 5 min wait cap (spec pane-execution Waiting, new scenario; chat-pane line list; design open questions closed).
6. macOS `y` only types, documented, visible line (spec agent-tools Requirement and scenario, pane-execution, chat-pane list; design D14).

## Global rules (every slice)

- Worktree: `git worktree add ../lazarobox-shell-<slice> -b feat/chat-runs-<slice> main` (never under /tmp). Each PR is based on main after the previous one merges.
- MSRV 1.88: no `if let` guards in match arms.
- Strict TDD: RED test (public interface) first, then GREEN, then REFACTOR. No silent failure: every refusal and fallback gets a visible line.
- Each slice: one CHANGELOG entry under `[Unreleased]`; README.md and README.es.md change together when user-visible.
- Exit gate: `cargo test`; `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check`; `rm -rf target`; remove the worktree after merge.
- Live check tasks marked `[manual]` follow `skills/run-lazarobox/SKILL.md` (isolated home, own tmux session, no real accounts).

## S1a: Settle core (base: main)

- [x] 1a.1 RED `settle.rs` tests: `Limits::Follow` byte-identical to today (10 s, any key `UserTyped`; #819/#824/#841 suites untouched); `Limits::Run` step order (alt screen, typed, settled, waiting, `WAIT_CAP`, `CUT_OFF`).
- [x] 1a.2 RED waiting phase: enters after 1 s quiet with `echo_off == Some(true)`, foreground not shell, no alt screen; `take_waiting()` true once; suspends cut-off; leaves on output or echo on; `sleep 40` (echo on) never waits.
- [x] 1a.3 RED clocks: `CUT_OFF` 30 s from the later of write, end of wait, last Enter; `StillRunning` (or `Unconfirmed` if foreground/anchor unknown); `NoInput` at 5 min.
- [x] 1a.4 RED `user_key(Key{line,enter,interrupt})`: attended flag, open range grows with probes, Enter/Ctrl+C/Ctrl+D close it; hidden cursor fails closed.
- [x] 1a.5 GREEN `settle.rs` (`Limits`, `Key`, `End::{StillRunning,NoInput}`, phase, ranges, `Settle::run`). No CHANGELOG entry: no user-visible change. Also touched: `report.rs` (a placeholder arm for the new ends, reworded in S1b) and `app/auto_run.rs` (compile fixes; wired in S1c). Exit gate.

## S1b: Echo probe and capture (base: main after S1a)

- [x] 1b.1 RED `core/pty` tests: `FakePty::echo_off()` scripted; real pty (tempfile-free) reports `Some(false)` for a shell, `None` where unsupported.
- [x] 1b.2 RED `capture.rs`: hidden ranges drop rows before `shape`; one `[input typed in the pane: hidden]` row per block; echo-row collision returns `None`; head, tail and wrapped-echo cases.
- [x] 1b.3 RED `report.rs`: words for `StillRunning`, `NoInput`, `output not available: you typed in the pane`.
- [x] 1b.4 GREEN `core/pty/{mod,portable,fake}.rs` (`PtyHandle::echo_off` via `MasterPty::get_termios` on the master, `libc::ECHO`; the `FakePty.echo_off` field), `capture(.., hidden)`, `report.rs` (`RunReport.withheld`, the `StillRunning`/`NoInput` words and `output not available: you typed in the pane`). No CHANGELOG entry: no user-visible change. Not wired: `Probe.echo_off` is still filled in S1c (the probe closure is untouched) and `auto_run` passes `&[]` and `withheld: false` until then. Exit gate.

## S1c: Wire settle into model runs (base: main after S1b)

- [x] 1c.1 RED `app_tests/tool_run_y.rs`: fake `sudo` (echo off, password typed) ends `Settled`, run not `UserTyped`, password marker absent from `RunDone`, request, store, transcript, log; output after the password kept.
- [x] 1c.2 RED lines: `pane N: waiting for your input in the pane; type it there (the chat does not see it)` once; cut-off and no-input lines (design wording); typing at the shell still ends `UserTyped`; cancel while waiting clears the mark.
- [x] 1c.3 GREEN `runtime.rs` probe field, `app/auto_run.rs` (`Settle::run`, `user_key`, lines, `end_run`), `app.rs`, `app/tool.rs`, `app/run.rs` (`note_typed` key and cursor line), `app/typed_run.rs` arms.
- [ ] 1c.4 `[manual]` `sudo true` on the real binary: waiting line shows, password typed in the pane, chat resumes by itself, marker absent from stored chat.
- [x] 1c.3b (typed_run arms done; the key-time foreground probe is NOT wired: the app holds no pty handle, so the foreground stays up to 16 ms stale) Notes from the S1a review: `app/typed_run.rs` (~L165) must get real arms for `StillRunning` and `NoInput` instead of the `_ =>` full-screen fallback. `Settle::user_key` reads the foreground from the last tick (up to 16 ms stale): probe the foreground at key time if the wiring allows it.
- [x] 1c.5 Docs: README.md + README.es.md (waiting for input, password never sent, 30 s cut-off); CHANGELOG. Exit gate.

## S2a: `y` runs in both modes (base: main after S1c)

- [x] 2a.1 RED `approval.rs` `may_run_on_y` table: blocked conversation, worker ask, `y_runs == false` give typed-only with the reason; default policy allows; `POLICY_TYPED`/`POLICY_ASKS` lines do not count toward the pause.
- [x] 2a.2 RED `app_tests/tool_run_y.rs`: `y` in `ask` runs (one write, one `\r`); GitHub-blocked chats and workers still type; macOS (`proc_cwd = None`) `y` only types with `the pane's directory cannot be read on this system`; keys line `y run it` / `y type it (no Enter)`.
- [x] 2a.3 GREEN `approval.rs` (`RunPolicy`, `may_run_on_y`, key-hint choice, `Route::AutoType` removed), `app/run.rs` `approve_run`. CHANGELOG; README pair. Exit gate.

## S2b: `auto` removed and migrated (base: main after S2a)

- [x] 2b.1 RED `config.rs`: parser maps `"auto"` to `ask` with problem `"auto" was removed; using "ask"`, writes nothing; `"AUTO"`, `true` give `expected "ask" or "run"`.
- [x] 2b.2 RED load rewrite (tempfile): `auto` becomes `"ask"` only, comments and other bytes intact; one notice; second start silent; read-only file shows the failure notice with reason at each start; other problems present means no rewrite.
- [x] 2b.3 RED toggle lines (`approval: ask (...)`, `approval: run (...)`), no ` AUTO ` badge, menu cycles `ask`, `run` and never shows `auto`; hint `ask: y runs; run: safe reads alone, rest y; per chat`.
- [x] 2b.4 GREEN `core/ai/config.rs`, startup load via `core/config_edit.rs`, `app/tool.rs`, `core/ai/chat/mod.rs`, `ui/components/chat.rs`. CHANGELOG; README pair (`auto` removed, mapped once). Exit gate.
- [ ] 2b.5 `[manual]` isolated home with `approval = "auto"`: notice once, file now `ask`, second launch silent; read-only config shows the failure notice.

## S2c: Continuation after the cut-off (base: main after S2b)

- [x] 2c.1 RED `typed_resume.rs`: server cut at 30 s resumes the chat with partial output; when it later ends the chat resumes again with final output and `lazarobox: the command you approved in pane N finished.`; Ctrl+C in the pane, a new message or closing the pane ends it with a visible line; #819/#824/#841 unchanged.
- [x] 2c.2 GREEN `app/auto_run.rs`, `app/typed_run.rs` (`resume` slot with `Limits::Continue`, `held_resumes`/`flush_resumes`). CHANGELOG; README pair. Exit gate.
- [ ] 2c.3 `[manual]` `npm run dev` (or `sleep 45; echo done`): partial output and notice at 30 s, resume when it ends.

## S3a: Head-keyed grammar and tiers (base: main after S2c)

- [x] 3a.1 RED `exec/tests.rs`: all existing 400+ grammar tests unchanged; longest head wins; one segment only; every metacharacter falls back; local engine runs exactly `cd pwd ls tree grep rg find`, cloud tier machinery in place.
- [x] 3a.2 GREEN `exec.rs` (`head`, `tier`, one-pass parser), `exec/specs.rs` (seven verbs as `Tier::Base`), `AutoCtx.local` from `run_scope(..).cloud`. CHANGELOG. Exit gate.

## S3b: New read verbs (base: main after S3a)

- [x] 3b.1 RED bypass table rows: `cat`/`cat -`, `tail -f`, `cat x > y`, `cat $(..)`, `sudo cat`, `env cat`, `FOO=1 cat`, `cargo --version`, `node --version`; Extended entries run on cloud and ask on local (`cat README.md` local asks).
- [x] 3b.2 RED `run_check` targets: `cat .env`, `cat ../../etc/passwd`, `cat ~/.ssh/id_rsa` give `NOT_ALLOWED`/`OUTSIDE_ROOT`.
- [x] 3b.3 GREEN `exec/specs.rs` (`cat head tail wc`, `<p> --version` for the D6 list, `Value::Rev`), `runtime/run_check.rs`. CHANGELOG. Exit gate.

## S3c: git and the config gate (base: main after S3b)

- [ ] 3c.1 RED git rows: `git status`, `git --no-pager log -n 5`, `git --no-pager diff --stat|--name-only|--name-status` run; plain `git diff`, `log` without `-n`, `log -p`, `show`, `-c`, `-C`, `--git-dir`, `--output`, `--ext-diff`, `--no-index`, `git st` ask.
- [ ] 3c.2 RED gate (tempfile repo): plain config passes; `core.fsmonitor`, `include.path`, `diff.external` ask with `GIT_CONFIG_RISK` (not counted toward the pause); allow-list keys pass.
- [ ] 3c.3 GREEN `exec/specs.rs` git heads and `needs_one_of`, `runtime/run_check.rs`, `runtime/tools/git.rs` (hardened config listing), `approval.rs` reason. CHANGELOG; README pair (what runs unprompted, local vs cloud, plain `git diff` asks). Exit gate.
- [ ] 3c.4 `[manual]` `run` mode: `ls` runs on a local engine, `cat README.md` asks; on a cloud-class setup `git diff --stat` runs and `git diff` asks.

## S4: Guidance and docs (base: main after S3c)

- [ ] 4.1 RED tests: `run_in_pane` description exact and identical in both modes and engine kinds; `tools.md`, description and `RUN_HINT` contain none of `press Enter`, `type it`, `auto`, `you run`; `RUN_HINT` = `Ctrl+Space e  type in pane`; printed bash block runs nothing.
- [ ] 4.2 GREEN `prompts/tools.md`, `tools/mod.rs`, `chat/transcript.rs`.
- [ ] 4.3 Docs: README.md + README.es.md same sections and order (flow, modes, unprompted list, sudo, 30 s, macOS `y` only types); CHANGELOG. Exit gate.
- [ ] 4.4 `[manual]` end to end: ask the chat to install a small tool in an isolated home; one `y`, chat reports the result.

## Close-out

- [ ] 5.1 After S4 merges: close #845 with links to the merged PRs. Remove all `../lazarobox-shell-<slice>` worktrees (`scripts/clean-builds.sh`).
