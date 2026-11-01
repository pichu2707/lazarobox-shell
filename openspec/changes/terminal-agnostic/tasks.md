# Tasks: Terminal-agnostic lazarobox

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated production lines | 470-660 total (tests, fixtures, snapshots and docs not counted) |
| Per slice (production) | S1 190-240, S2a 30-50, S2b 150-200, S3 100-170, S4 0 |
| 400-line budget risk | High for the total; Low for S2a, S3, S4; Medium for S1 and S2b; each slice under 400 |
| Chained PRs recommended | Yes |
| Suggested split | S1, S2a, S3 independent; S2b after S2a; S4 last |
| Delivery strategy | ask-on-risk, resolved: chained PRs |
| Chain strategy | feature-branch-chain (tracker `feat/terminal-agnostic`; PR #1 targets the tracker, later PRs target the previous PR branch) |

Decision needed before apply: No
Chained PRs recommended: Yes
400-line budget risk: High

Resolved by the user: chained PRs with feature-branch-chain. Only the tracker merges to main.

### Reconciliation applied by this phase (2026-11-01)

- Spec `copy-selection`: `SetClipboard` keeps the OSC 52 bytes; new `CopyLocal` effect carries the text for the local tool (follows design D1). Behaviour is unchanged. OSC 52-only notice now has two texts: local desktop ("install wl-copy or xclip") and SSH ("enable OSC 52 in your terminal"). Tool failure notice is `<tool> failed; sent via OSC 52 (<reason>)`.
- Spec `terminal-compatibility`: `Ctrl+]` during the probe records `alias`, no offer (D8). Other keys record `missed` with the offer.
- Design: Open Questions closed; Risks section added for WSLg (verified by web search 2026-11-01).

### Rules for every PR

- Strict TDD: RED (compiling stubs, exact assertions, test fails for the right reason), then GREEN, then REFACTOR. Tests are never trimmed.
- Gate = `ENGRAM_PORT=8123 cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
- One work-unit commit per task group (conventional commits, no AI attribution). The planning phase commits nothing.
- Re-measure with `git diff --shortstat` (production files only) before opening each PR; above 400 production lines, split.
- Doc comments on every new public item. README and docs text in the language of the target file (README is Spanish).
- Silent failure is never allowed: every error path in these slices shows a notice.

### Suggested Work Units

| Unit | Goal | Est. prod lines | Depends on |
|------|------|-----------------|-----------|
| S1 | Clipboard core, worker, `CopyLocal`, notices | 190-240 | - |
| S2a | `Ctrl+]` alias, `SendAliasLiteral`, chat-copy `is_prefix()` | 30-50 | - |
| S2b | Probe, store, `--check-prefix`, self-heal | 150-200 | S2a |
| S3 | WSL notes, record-aware desktop notice, terminal picker | 100-170 | - (merge gate: WSL live check) |
| S4 | Checklist, records, README, follow-ups | 0 | S1, S2a, S2b, S3 built |

## S1: Clipboard fallback chain (`src/core/clipboard.rs`, `src/runtime/clipboard.rs`, `src/app.rs`, `src/runtime.rs`)

Spec: copy-selection "Yank to the clipboard via OSC 52" (all scenarios tagged [auto]).

- [x] 1.1 RED: `core::clipboard::candidates(Session)` table tests: ssh gives `[]`; wsl gives `[ClipExe, WlCopy, Xclip, Xsel]`; wayland gives `WlCopy` first; display-only gives `Xclip, Xsel`; nothing gives `[]`.
- [x] 1.2 RED: `args(Tool)` (`xclip -selection clipboard`, `xsel --clipboard --input`, others none) and `payload(Tool, &str)` (UTF-8; `ClipExe` UTF-16LE with BOM, marked unverified until the Windows Terminal record) tests.
- [x] 1.3 RED: notice text tests: `Copying (wl-copy)`, `Copied (wl-copy)`, `wl-copy failed; sent via OSC 52 (<reason>)`, OSC 52-only local text, OSC 52-only SSH text. The failure text never starts with `Copied`.
- [x] 1.4 GREEN: implement `Tool`, `Session`, `candidates`, `args`, `payload`, notice fns in `src/core/clipboard.rs`.
- [x] 1.5 RED: app tests through `App::update`: with a tool, `y` emits `SetClipboard(osc52)` and `CopyLocal{seq,text}` and notice `Copying (wl-copy)`; without a tool only `SetClipboard` and the local OSC 52-only notice (existing ~25 assertions stay valid); SSH gives the SSH notice and no `CopyLocal`; too-large text with a tool emits `CopyLocal` only, without a tool keeps `Selection too large to copy` (D5).
- [x] 1.6 RED: `AppEvent::Clipboard{seq, result}` tests: Ok gives `Copied (wl-copy)`; Err gives the failure notice; a stale `seq` is dropped; an open menu footer is updated too (as `on_open_failed`).
- [x] 1.7 GREEN: `Effect::CopyLocal`, `AppEvent::Clipboard`, `App::with_clipboard(Option<Tool>, ssh)`, `copy_to_clipboard` change (`app.rs:~2222`). Chat copy, feedback and update callers get the notices with no edit (assert one test per caller).
- [x] 1.8 RED: runtime worker tests with fake tool scripts in a temp dir (as `runtime/opener.rs` tests): exit 0 gives Ok; exit 1 gives Err with reason; sleep beyond `CLIP_TIMEOUT` (2 s, make it injectable) gives `timed out` and the child is killed; stdout and stderr are null; two quick jobs run only the newest (latest copy wins).
- [x] 1.9 GREEN: `Clipboard::find(path)` (PATH lookup once at startup), one worker thread with std mpsc, stdin written on a helper thread, `try_wait` poll. Wire in `src/runtime.rs`: Inbox field, `select!` arm, `apply` arm for `CopyLocal`, startup detection of `Session` from env (`SSH_CONNECTION`, `WAYLAND_DISPLAY`, `DISPLAY`, WSL).
- [x] 1.10 REFACTOR: remove duplicated notice strings; keep `copy_to_clipboard` the single choke point.
- [x] 1.11 Gate. Re-measure production lines (target 190-240).
- [x] 1.12 Docs: doc comments on `Tool`, `Session`, worker invariants (latest wins, timeout, null stdio).
- [x] 1.13 [manual] Local desktop: `y` shows `Copied (<tool>)` and the text pastes in another app; with PATH stripped of tools, the OSC 52-only notice shows. Result goes in the Kitty record (task 4.6).

## S2a: Prefix alias `Ctrl+]` (`src/core/prefix.rs`, `src/app.rs`, `src/core/ai/chat/copy.rs`)

Spec: modal-input "Prefix key". Independent of S1.

- [x] 2a.1 RED: key tests: `Char(']')+CONTROL` and `Char('5')+CONTROL` (crossterm's legacy parse of 0x1D, D6) both enter PREFIX with no `WritePty`. Add a unit test on crossterm parsing of byte 0x1D if reachable.
- [x] 2a.2 RED: `Ctrl+]` then `Ctrl+]` emits `WritePty [0x1D]` and returns to TERMINAL; `Ctrl+Space` then `Ctrl+Space` still emits `[0x00]`; `Ctrl+]` then `Ctrl+Space` emits `[0x00]`; `Ctrl+Space` then `Ctrl+]` emits `[0x1D]` (second key's own byte, D7).
- [x] 2a.3 RED: chat pane focused: either prefix key repeated writes nothing, returns to TERMINAL and shows a notice. In a group, the alias cancels like the prefix.
- [x] 2a.4 RED: `Ctrl+]`, `w`, `v` splits right (alias drives the whole tree).
- [x] 2a.5 GREEN: alias chords in `core/prefix.rs`, `is_prefix()`, `PrefixAction::SendAliasLiteral`, hooks in `app.rs` `on_terminal_key` (`~2255`) and `on_pending_key` (`~2329`); `core/ai/chat/copy.rs:~126` uses `is_prefix()`.
- [x] 2a.6 REFACTOR: no hard-coded prefix branch left; the prefix table row text matches the spec row.
- [x] 2a.7 Gate. Target 30-50 production lines.
- [x] 2a.8 Docs: doc comment on the alias commitment (fixed, 1.x, not configurable).

## S2b: First-run probe (`src/core/probe.rs`, `src/runtime/probe_store.rs`, `src/core/start.rs`, `src/app.rs`, `src/runtime.rs`)

Spec: terminal-compatibility "First-run prefix probe" and "Remembered probe result". Needs S2a merged.

- [x] 2b.1 RED: `terminal_key` table: `TERM_PROGRAM` wins; else `Windows Terminal` when `WT_SESSION`; else `TERM`; else `unknown`.
- [x] 2b.2 RED: JSON parse/render of `terminal key -> "pass" | "alias" | "missed"`; unknown value, bad JSON and empty file give a typed error, not a panic.
- [x] 2b.3 RED: `Probe` transitions with the app clock: `Ctrl+Space` gives `pass`, PREFIX, no offer; `Ctrl+]` gives `alias`, PREFIX, no offer (D8); a plain space or a non-printable or modified key gives `missed`, offer notice, key goes to the pane; an unmodified printable key other than space keeps waiting (user decision); `PROBE_WINDOW` (10 s) opens at the first tick and ends the probe with nothing stored and no offer; each result emits one `Effect::SaveProbe`.
- [x] 2b.4 RED: no probe when a stored result exists for this key; probe runs for a different terminal key; a startup notice wins and the probe is deferred with nothing stored (D13); `Ctrl+]` still works after any stored result.
- [x] 2b.5 RED: `--check-prefix` parsed in `StartArgs`; it starts the probe even with a stored result and shows `Checking the prefix key: press Ctrl+Space`.
- [x] 2b.6 RED: self-heal: stored `missed` plus later `Ctrl+Space` gives one `SaveProbe` with `pass`; stored `pass` plus `Ctrl+Space` emits no `SaveProbe`.
- [x] 2b.7 RED: store integration tests in temp dirs: missing file gives "probe once"; write is temp + rename; read error and write error each give an `AppEvent` that shows a notice and the app still starts; state dir resolves `$XDG_STATE_HOME/lazarobox/` then `~/.local/state/lazarobox/`.
- [x] 2b.8 GREEN: `core/probe.rs`, `runtime/probe_store.rs` (bounded read via `runtime/bounded_read.rs`, reuse `state_dir`), `Effect::SaveProbe`, `ProbeTick` (1 s, armed only while waiting, like `memory_watch`), `ProbeStore*` events, `App::with_probe`, `on_key` hook (`~1980`), `--check-prefix`, `runtime.rs` wiring (`~299`, `~510`, `apply` arm).
- [ ] 2b.9 REFACTOR: share the notice builder with the clipboard notices only if it removes duplication.
- [ ] 2b.10 Gate. Re-measure (target 150-200). If above 400 production lines in the PR, split off `--check-prefix` and self-heal into 2b-ii.
- [x] 2b.11 Docs: doc comments on `Probe`, `Outcome`, the file format and `PROBE_WINDOW`.
- [x] 2b.12 [manual] In a terminal that swallows `Ctrl+Space` the offer appears once; restart shows no probe; `--check-prefix` reruns it. Result goes in that terminal's record (task 4.6). (simulated with a plain space, Kitty; see the Kitty record)

## S3: Desktop launcher WSL notes, record-aware notice and terminal picker (`src/core/desktop.rs`, `src/runtime/desktop.rs`)

Spec: desktop-launcher. Independent of S1 and S2. Merge gate: task 3.8.

- [x] 3.1 RED: record-aware notice tests: `kitty` in `RECORDED_TERMINALS` gives "has a record"; an unrecorded name gives `<name> has no test record yet; see docs/terminals/`; the text never contains `tested in Kitty` and never refuses launch.
- [x] 3.2 RED: `RECORDED_TERMINALS == ["kitty"]`, and a test checks every entry has a `docs/terminals/<name>/` folder (fails until S4 adds the Kitty folder; create an empty `docs/terminals/kitty/` placeholder with the record in S4, or land the folder first). WezTerm is added only in the follow-up that brings its record.
- [x] 3.3 RED: `plan_install` with `wsl = true` writes the same Linux entry, names its path, and adds the WSL notes: the entry may not appear in the Start menu, WSLg runs it as a Linux GUI app so a Linux GUI terminal must be installed in the distro, and how to launch it otherwise. `wsl = false` output is unchanged.
- [x] 3.4 RED: install twice leaves one entry and says so; unwritable target gives a non-zero exit with the reason (existing behaviour, assert it still holds).
- [x] 3.5 GREEN: `NON_KITTY_NOTICE` becomes the record-aware note, `RECORDED_TERMINALS`, WSL notes in `core/desktop.rs`; pass `wsl` into the plan in `runtime/desktop.rs`.
- [x] 3.5a RED: `picker_choices` table tests (spec "Interactive terminal picker", design D16): only installed terminals; `$TERMINAL` and the `x-terminal-emulator` target added and deduplicated against PATH entries; `tested` set from `RECORDED_TERMINALS`; default equals today's automatic choice; empty installed list gives no choices.
- [x] 3.5b RED: `parse_choice` table tests: empty gives the default; `2` gives the second entry; `0`, `9`, `abc`, whitespace-only-with-text give `Retry` with a notice; EOF gives `Cancel`.
- [x] 3.5c GREEN: implement `Choice`, `picker_choices`, `parse_choice` pure in `core/desktop.rs`.
- [x] 3.5d RED: runtime tests with injected reader and writer (no real TTY): Enter writes the default entry; a number picks that terminal; invalid input prints the notice and asks again; EOF writes nothing, says cancelled, exits non-zero; no prompt when `--terminal` is passed or when not a TTY (output equals today's); a fake terminal binary on PATH is never executed (marker file stays absent).
- [x] 3.5e GREEN: thin IO in `runtime/desktop.rs`: `is_terminal()` on stdin and stdout, skip with `--terminal`, PATH lookup only for candidates, print list with the tested mark, read lines.
- [x] 3.5f REFACTOR: reuse the existing automatic-choice function for the default, no second copy of the candidate list.
- [x] 3.6 Gate. Target 100-170 production lines. If the PR passes 400, split the picker into S3b after S3.
- [x] 3.7 Docs: doc comments on the WSL assumptions (see design Risks) and on the picker rules (TTY only, no probes).
- [ ] 3.8 [manual, merge gate] On a WSL machine with a Linux GUI terminal in the distro: run `install-desktop`, check whether the entry in `~/.local/share/applications` appears in the Windows Start menu, and launch from it in one step. Record the result in the Windows Terminal (WSL) record (task 4.7). If user entries are not listed, S3 keeps the "may not appear" note and a follow-up is opened (Windows Terminal fragment or `.exe`), outside this change.

## S4: Docs, records, README (no production lines)

Spec: terminal-compatibility "Support policy by records" and "Shared checklist and record format". Starts when S1, S2a, S2b and S3 are built.

- [x] 4.1 Create `docs/terminals/README.md`: shared checklist (prefix `Ctrl+Space`, alias `Ctrl+]`, `Alt+Enter`, `Ctrl+J`, `Ctrl+O`, `Ctrl+S`, `Ctrl+Y`, `Ctrl+B`, `Ctrl+R`, `Ctrl+G`, `Ctrl+D`, `Ctrl+U`, `Ctrl+C`, `Alt+Up/Down` when present, mouse, paste, focus, truecolor, clipboard) and the record template: header table (terminal, version, OS, lazarobox version, date, tester) then `item | pass/fail/n.a. | note`.
- [x] 4.2 README: remove "tested in Kitty" (line ~93, Spanish text) and replace with a record-aware sentence.
- [x] 4.3 README: platform list (lines ~60-64): replace "Windows: no soportado" with WSL as a supported path once its record exists, plus the WSLg Start menu caveat from task 3.8.
- [x] 4.4 README: add a "Supported terminals" section listing only folders under `docs/terminals/`, each linking its newest record; everything else "untested". Update the prefix table (lines ~146-187): row `Ctrl+Espacio` / `Ctrl+]` sends its own byte (NUL / 0x1D); line ~146 says the prefix keys are fixed. Update the copy section (lines ~322-335): fallback chain, the notices, SSH skips local tools, `--check-prefix`.
- [x] 4.4b README and CHANGELOG: state the behaviour change that a single `Ctrl+]` or `Ctrl+5` no longer reaches the program in a pane; it takes two presses (`Ctrl+] Ctrl+]` sends 0x1D). This affects vim tag jump and the telnet escape.
- [x] 4.4a README: in the `install-desktop` text (line ~93) describe the picker: it asks only on a TTY without `--terminal`, lists installed terminals, marks tested ones, Enter keeps the automatic choice, Ctrl+D cancels.
- [x] 4.5 Check: grep README and `src/` for `tested in Kitty` and `Windows: no soportado` returns nothing.
- [ ] 4.6 [manual] Kitty: fill `docs/terminals/kitty/<version>-<date>.md` from the template (user). Include the S1 clipboard check (1.13), the SSH yank scenario, and the paste scenarios from copy-selection. (all but the SSH yank and mouse click done; see issue #490)
- [ ] 4.7 [manual] Windows Terminal (WSL): fill `docs/terminals/windows-terminal/<version>-<date>.md` (user). Include the one-step launch result (3.8), the probe result, `clip.exe` UTF-16LE check (if wrong, change `payload` to UTF-8 in a fix commit), and the alias.
- [ ] 4.8 [deferred, non-blocking] WezTerm record: the user fills `docs/terminals/wezterm/<version>-<date>.md` on their other PC. The follow-up PR that brings the record also adds `wezterm` to `RECORDED_TERMINALS` and to the README list. The success criterion "three records" is met in that follow-up.
- [x] 4.9 Task 3.2 passes (the `kitty` folder exists).

## Follow-ups outside this change's PRs

- [ ] F.1 [blocked on tracker `origin/feat/agent-orchestrator` merging] Replace P01, P02 and T52 in that branch's `manual-tests.md` with the checklist rows, and replace agent-tools tasks 1b.11 and 2e-3.13 with checklist rows. The checklist must include `Alt+Up` and `Alt+Down` (bound on the tracker). Do not edit these files on this branch before the tracker merges.

## Implementation Order and Rollback

| Order | Slice | Rollback |
|-------|-------|----------|
| 1 | S1 | revert: back to OSC 52 only, no notice |
| 2 | S2a | revert: alias removed |
| 3 | S2b (after S2a) | revert: probe removed, stored file ignored |
| 4 | S3 (after the WSL live check) | revert: old notice returns (S4 README must change with it) |
| 5 | S4 | docs only; can stay |

S1, S2a and S3 can be built in parallel by one writer in any order; S2b strictly after S2a; S4 last.
