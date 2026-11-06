# Tasks: Orchestrator Worker Flow, milestone 1 (W1a, W1b, W2a, W2b)

Scope: first milestone only, chosen by the owner on 2026-11-06. Inputs: `proposal.md`, `design.md`, the delta specs of this change, and the engram decision `sdd/orchestrator-worker-flow/milestone-1`.

Execution context: artifact store `hybrid`; delivery strategy `ask-on-risk`; chain strategy `stacked-to-main` (each PR merges to main in order; each PR targets `main` and is rebased on the previous one once that merges); Strict TDD active, runner `cargo test`. The 400-line budget counts PRODUCTION code only; tests, fixtures and snapshots never count.

Conventions for every task below:

- Order inside a PR is RED (failing test written and seen failing) -> GREEN (smallest production change) -> REFACTOR (tests stay green). Tests live in-module or in `*_tests.rs`, as the surrounding code does.
- One PR = one work unit = one or more work-unit commits (tests travel with the behaviour they verify). Conventional commits, no AI attribution.
- Every PR must pass `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check`.
- Notices are ASCII, short, neutral, system lines in the pane transcript, never sent to a model as history (spec: chat-pane, "Worker-flow notices are transcript notices").
- Rollback of any PR is one revert; ORCH stays off by default.

## Decisions applied (owner, 2026-11-06)

- D15 `/write` approved as designed (per chat, in memory, `y` dialog, user command only). Task group W2b.
- llamacpp joins `MCP_VERIFIED` only after a manual llama-server tool-calling check passes. Separate manual-gated task (PR10). If the check fails, acceptance ends in the visible notice and PR10 is not opened.
- Decision 10 (interim one-writer guard) is not needed: no write-capable kind is added before W10a.
- Owner decisions 2-6, 8, 9 and D16 `/exec` stay OPEN. They belong to W3-W10 and are not tasked here.

## Drift check against main 86135bc9 (v1.11.0)

Every path in the design's File Changes rows for these four slices exists, with these differences:

| Design reference | Current main | Effect |
|---|---|---|
| `chat/engine.rs` `MCP_VERIFIED` ~126 | `src/core/ai/chat/engine.rs:131` | Line only. |
| `runtime/orchestra.rs` `open_session` 1576-1581, 1623-1626 | `open_session` at 1568; the `None` returns at 1577 and 1581; hub failure `.ok()?` at 1627-1630 | Line only. Same logic. |
| `runtime/orchestra.rs:1511,1582` read `spec.families` | `launch` at 1486; `tools_for(..., MCP_VERIFIED)` at 1511; `families: spec.toolset.families()` at 1512; `open_session` reads `spec.toolset.families()` at 1583 | Both sites still read `Toolset`, not `spec.families` (the work of PR5/PR6). |
| `OpenSession` `stage_key` and `drive_grant` | `TurnScope` now has `drive_grant: None` (WSL drive access) in both `ChatRequest` and `TurnScope` | New field; the new `families` plumbing must leave `drive_grant: None` for workers untouched. |
| `runtime/chat.rs:900-911` | `Chats::open_session` at `runtime/chat.rs:892`; `wires_tools` at 662 carries the comment "The llamacpp arm is inert until `MCP_VERIFIED` lists it (PR3)" | PR10 must update that comment. |
| `app/orchestra.rs:850-866` `orch_refusal` | Exists at `app/orchestra.rs:850`; `tools_for` check at 860 | As designed. |
| `config.rs:407` `slots: [Assignment; 6]` | Not in this milestone (W8a) | Not verified here. |
| `prompts/worker.md` | Does not exist; correct place is `src/core/ai/chat/prompts/worker.md` (prompt dir has per-profile `analyst/` and `programmer/` folders plus shared `coding.md`, `tools.md`) | New file; design must be read as profile-independent, like `coding.md`. |
| "a grant module beside `app/github.rs`" | `src/app/github.rs` (single file, `impl App`), `ApprovalAsk::GithubGrant` at `approval.rs:181`, handled at `app/tool.rs:148,203` and `app/run.rs:217,671` | New `app/write.rs` mirrors it. |
| `Command` for `/write` | `chat/folder.rs:55` `enum Command`, `/github` at 68 and 92; routed in `app/chat.rs:520` | As designed. |
| Spec checks at `ChatRequest` | New `ChatRequest` field `drive_grant` | No effect on tasks. |

No file listed in the design is missing. New WSL drive features and config unknown-key reporting did not move any touch point of W1-W2.

---

## Slice W1a: facts disclosed (split into two PRs)

Spec references: agent-orchestrator "Silent worker outcomes are disclosed [W1]"; chat-pane "Worker-flow notices are transcript notices" (both scenarios); orchestrator-handoffs scenario "A code worker with an empty ledger [W1, W4]" (the W1 half: the notice only).
Design: Corrections row 1 (the real silent degrade is the worker's), D26 not needed.

### PR1: empty ledger and staged-never-proposed notices

Files: `src/core/ai/orchestra/changes.rs`, `src/core/ai/orchestra/run.rs`, `src/core/ai/orchestra/event.rs`, `src/runtime/orchestra.rs`, `src/app/orchestra.rs`; tests in `src/core/ai/orchestra/tests.rs` and `src/app/app_tests/`.

- [x] 1.1 RED: pure test in `orchestra/tests.rs`: a `code` worker ended `done` with an empty ledger yields a `Disclosure::EmptyLedger { id, task, engine_model }` (name TBD in code); `review`/`summarize`/`general` workers yield none. Spec scenario: "Empty ledger [W1, auto]".
- [x] 1.2 GREEN: add the pure check in `orchestra/run.rs` (or `changes.rs`) over the existing ledger; add the event variant in `orchestra/event.rs`.
- [x] 1.3 RED: pure test: a worker that staged two files and ended with no `propose_batch` yields `Disclosure::StagedNotProposed` carrying the file count. Spec scenario: "Staged and never proposed [W1, auto]".
- [x] 1.4 GREEN: read the pane-level orphan state from `runtime/orchestra.rs` at worker finish (the batch is the pane's `StageKey::Pane`, so the check asks whether the worker's staged changes are still unproposed) and emit the event.
- [x] 1.5 RED (app): `App::update` test with insta snapshot: each event becomes a transcript system line naming worker id, kind and `engine>model`, not stored as a chat message, repeated each time it occurs. Spec: chat-pane scenarios "Notice is a system line, not history" and "Notice repeats".
- [x] 1.6 GREEN: render in `app/orchestra.rs` through the existing system-line path used by orchestra notices.
- [x] 1.7 REFACTOR: single notice-text module/consts in `app/orchestra.rs`; confirm the next orchestrator turn history does not contain the line (reuse the "history does not hold it" assertion).
- Done when: `cargo test` green; manual step M1 passes; the notice text is ASCII and neutral.
- Depends on: none. Parallel: independent of PR2 in code, but both touch `runtime/orchestra.rs` finish path; land sequentially to avoid conflicts.

### PR2: refused-spawn and degraded-turn notices

Files: `src/core/ai/orchestra/run.rs` (Refusal text exposure), `src/runtime/orchestra.rs` (`open_session`, `launch`), `src/runtime/chat.rs` (debug_assert only), `src/core/ai/orchestra/event.rs`, `src/app/orchestra.rs`; tests in `orchestra/tests.rs`, `src/app/app_tests/`.

- [x] 2.1 RED: pure/runtime test: every `Run::spawn` refusal (read-only goal, max workers, run paused, etc.) produces a notice event carrying the SAME redacted, capped line that the coordinator receives. Spec: "Refused spawn is seen by the user [W1, auto]".
- [x] 2.2 GREEN: emit the event where the spawn tool result is built in `runtime/orchestra.rs`; reuse the existing redact-and-cap function so coordinator and user see one string.
- [x] 2.3 RED: runtime test with the fake runner: a worker turn for which `open_session` returns `None` (no tooling; `Tools::Off`; unknown locality; hub open failure) emits `Degraded { worker, reason }` with a distinct reason each, and the turn still runs as text. Spec: "Turn degraded to no tools [W1, auto]" (worker half).
- [x] 2.4 GREEN: split the `None` exits of `open_session` (lines ~1577-1581 and the hub `.ok()?` calls at ~1627-1630) so each returns a reason; `launch` sends the notice before the turn starts. Behaviour of the turn is unchanged.
- [x] 2.5 RED: test for the coordinator half: an orchestrator turn that starts with `Tools::Off` (tooling missing) says which turn ran without tools. Check first whether `orch_refusal` (`app/orchestra.rs:850`) already stops it; if so, assert it is a refusal and add no notice (Corrections row 3).
- [x] 2.6 GREEN: add the coordinator notice only if 2.5 shows a reachable path; otherwise add `debug_assert!` in `runtime/chat.rs` ORCH turn construction (design: "no notice there; a `debug_assert` for ORCH only").
- [x] 2.7 RED (app): insta snapshot of the three notice kinds; a notice is not history. GREEN: render in `app/orchestra.rs`.
- [x] 2.8 REFACTOR: one shared "notice about worker X" formatter for PR1 and PR2 (`worker n/m kind engine>model`).
- Done when: `cargo test` green; manual step M1 passes; the open question "Whether W1's disclosure identifies the original trigger" is answered in the PR description from M1.
- Depends on: PR1 (shared formatter and event enum).

---

## Slice W1b: undelegated notice and key (split into two PRs)

Spec references: agent-orchestrator "An undelegated coordinator reply is disclosed [W1]" (three scenarios); modal-input "Delegate key [W1]" (three scenarios); chat-pane notices.
Design: D27 (undelegated notice), D24 not needed. Owner precedent `NOTICE` ("`y` is typed", 2026-10-29): Enter on an empty composer, fresh press only.

### PR3: detect and park the undelegated notice

Files: `src/core/ai/orchestra/run.rs` (`Run::undelegated()`), `src/runtime/orchestra.rs` (`turn_ended` at 863, `finish_run` at 1324), `src/core/ai/orchestra/event.rs`, `src/app/orchestra.rs`, `src/core/ai/chat/mod.rs` (`park_continue` pattern at 716); tests in `orchestra/tests.rs`, `app_tests/`.

- [x] 3.1 RED: pure test `Run::undelegated()` is true only with no slots and not cancelled; false after any `spawn_agent` that created a worker; false when cancelled. Spec: "A turn with a worker gets none [W1, auto]".
- [x] 3.2 GREEN: implement `Run::undelegated()`.
- [x] 3.3 RED: runtime test: `turn_ended` on `Settle` and `finish_run` each send the event when `undelegated()`; not sent otherwise. Spec: "Text-only turn gets the notice" (the "no further turn starts by itself" half is asserted here too).
- [x] 3.4 GREEN: send from both sites.
- [x] 3.5 RED (app): the notice is parked while the phase is not idle and shown when idle (`park_continue` pattern, separate channels); dropped on a failed turn and on a resume waiting for a reply; shown after EACH such turn (legitimate answer included, neutral text, no accusation). Spec: "A legitimate answer gets the same neutral notice", "Notice repeats".
- [x] 3.6 GREEN: park/flush in `app/orchestra.rs` with the stated text, `no worker ran, nothing written` (PR3 ships no key text; the key text moves to PR4; ASCII per chat-pane spec).
- [x] 3.7 REFACTOR: shorter forms by width as `github_hint` does (`chat/guidance.rs`), with a pure width test per form.
- Done when: `cargo test` green; manual step M2 shows the line with a text-only reply; no key action exists yet (Enter does nothing new).
- Depends on: PR1/PR2 only for the shared notice formatter; otherwise independent.

### PR4: the delegate key

Files: `src/app/chat.rs`, `src/app/orchestra.rs`, `src/core/ai/chat/guidance.rs`; tests in `app_tests/`.

- Note: PR3 ships the notice without key text (`no worker ran, nothing written`); the `Enter delegate | Esc dismiss` text moves here and is added together with the keys (`undelegated_hint` in `guidance.rs`).
- [x] 4.1 RED (app): Enter on an EMPTY composer, fresh press only, with an undelegated notice pending in an orchestrating, idle pane starts an orchestrator turn whose message is a fixed text of lazarobox's own and keeps the user's goal (the `resume_goal` pattern). Spec: "Key starts delegation [W1, auto]".
- [x] 4.2 RED: Enter with text in the composer sends normally; Enter held (repeat) does not trigger; Enter while a turn runs, or with ORCH off, does nothing new. Esc dismisses the line.
- [x] 4.3 RED: with no undelegated request, the key does nothing and a notice says there is nothing to delegate. Spec: "Nothing to delegate says so [W1, auto]". Resolve precedence: one guidance line at a time (provider notice, failure alert, undelegated), so Enter has one meaning.
- [x] 4.4 RED: orchestrator text, worker text and tool results containing "delegate" start no turn. Spec: "A model cannot press it [W1, auto]".
- [x] 4.5 GREEN: handle the key in `app/chat.rs`, wired to a small method in `app/orchestra.rs`; fixed text constant; update `guidance.rs` rendering.
- [x] 4.6 REFACTOR: keep the fixed text next to the notice text constants; run `cargo test` for existing Enter-handling snapshots.
- Done when: `cargo test` green; manual step M2 passes end to end (Enter produces a worker spawn on Ollama or a notice).
- Depends on: PR3.

---

## Slice W2a: worker prompt, menu ceiling, refusals (split into two PRs: PR5+6 combined, PR7)

Spec references: agent-orchestrator "Menu ceiling for worker toolsets [W2]" (three scenarios), "A worker that must write is refused when Write is off [W2]" (code half), "Spawn refusals that create no worker [W2, W8]"; agent-tools "Toolsets by session role" scenarios "Ceiling limits a writer", "Ceiling never widens", "A forged call is refused visibly"; prompt-profiles "Worker prompts require tools and forbid fragments as a result [W2]" (two scenarios).
Design: D17, D18 (code half), D25 (partial), D29, Corrections rows 1 and 2.

Ordering note: PR5 is inert (ceiling defaults to everything on) so that no worker silently loses write tools before the refusal exists. PR6 turns the ceiling on and adds the refusal together. Since 2026-11-06 both ship in one PR; the order inside it is unchanged.

### PR5 + PR6 (one PR, owner decision 2026-11-06): ceiling types, then ceiling wired with the Write-off refusal

PR5 and PR6 ship as ONE PR. The ordering trap still holds inside it: the ceiling is never turned on without `Refusal::WriteOff` and its visible notice.

#### PR5: ceiling types

Files: `src/core/ai/orchestra/mod.rs` (`Toolset` at 77, `families()` at 93, `WorkerSpec`), `src/core/ai/orchestra/worker.rs`, `src/core/ai/orchestra/run.rs` (`Run.ceiling`, `resolve`), `src/runtime/orchestra.rs` (the two `spec.toolset.families()` reads at 1512 and 1583); tests in `orchestra/tests.rs`.

- [x] 5.1 RED: pure test `Toolset::capped(menu) -> FamilySet`: removes Write and Exec when the menu lacks them, nothing else; property-style check that for every toolset and every menu `capped(menu)` is a subset of `families()` (never widens); `review`, `summarize`, `general`, `chat` stay read-only with Write and Exec on. Spec: "The menu never widens [W2, auto]", "Ceiling limits a writer".
- [x] 5.2 GREEN: implement `capped`.
- [x] 5.3 RED: `WorkerSpec` carries `families: FamilySet` set in `resolve`; `writes()` is `families.contains(Write)`; D25 comparison: `Run::spawn` uses `writes()` where the code compared `task == Code` (sites around `run.rs:394,656,747`; `run.rs:362` moves after `resolve`). Existing tests unchanged in behaviour.
- [x] 5.4 GREEN: add the field and `writes()`; `Run.ceiling` is a `FamilySet` set at `start`, default all on; `launch` and `open_session` read `spec.families`.
- [x] 5.5 RED: with the default ceiling every existing worker test still produces the same tool list (regression pin). Spec: "Role toolsets [auto]" unchanged.
- [x] 5.6 REFACTOR: remove now-dead direct `toolset.families()` reads in the runtime; keep `READ_ONLY` text change out of this PR unless a test forces it.
- Done when: `cargo test` green; behaviour identical to main for every user path (diff is types and plumbing).
- Depends on: none (independent of W1).

#### PR6: menu ceiling wired, Write-off refusal

Files: `src/core/ai/chat/engine.rs` (`OrchSend`), `src/app/orchestra.rs` (menu toggles -> ceiling), `src/core/ai/orchestra/run.rs` (`Refusal::WriteOff`, `attach`), `src/runtime/orchestra.rs` (`attach`, forged-call refusal), `src/app/orchestra.rs` (notice); tests in `orchestra/tests.rs`, `app_tests/`, tool tests for forged calls.

- [x] 6.1 RED: pure test: with `ceiling` lacking Write, `Run::spawn(Code, ...)` returns `Refusal::WriteOff`, creates no worker, starts no process, does not pause the run, does not raise a failure alert, and a following `wait`/`spawn_agent` behaves normally. Spec: "Code worker refused with Write off", "A refusal before a worker does not pause the run [W2, auto]".
- [x] 6.2 GREEN: `Refusal::WriteOff` in `run.rs` (code kinds always; the `simple` branch is added in W8b).
- [x] 6.3 RED: the refusal text names the `Write` toggle, says the worker was not started, and gives the INTERIM step: turn Write on in the AI menu. It is returned to the coordinator as an error result AND shown in the pane (reuses the PR2 refused-spawn notice path). The `/write` wording replaces the interim step in PR9; record that as a known follow-up.
- [x] 6.4 RED (runtime): Write off -> a `code` spawn never reaches `launch`; with Write on the same spawn gets write tools; a toggle change applies from the NEXT spawn and does not change a running worker; Exec off -> `run_command` absent and a forged `run_command` call returns an error result naming the cause with no ask. Spec: "Exec off means no run_command", "Turning Write on lets the spawn through" (menu variant).
- [x] 6.5 GREEN: send the ceiling in `OrchSend` at start and on `attach`; `App` derives it from `self.ai.toggle(ToolToggle::Write/Exec)`.
- [x] 6.6 RED: coordinator toolset is unaffected by the ceiling; a model text asking to enable Write changes no toggle. Spec: "A model cannot turn Write on [W2, auto]".
- [x] 6.7 REFACTOR: confirm `READ_ONLY` text if reworded keeps its test updated (D25).
- Done when: `cargo test` green; manual step M3 (Write off) passes.
- Notes (as built): `Refusal::WriteOff` text is `steps::WRITE_OFF` (names `Write`, says the worker was not started, gives the AI menu step `Ctrl+Space m`); the `/write` wording replaces the step in the later W2b PR. Exec is in no worker toolset today (W6 adds `run_command`), so 6.4's Exec half is a guard test (a forged `run_command` is refused with no ask). `READ_ONLY` text was not reworded (that is W8), so 6.7 needed no change. A read-only goal is refused as `ReadOnly` before `WriteOff` (the role decides, not the menu). `drive_grant` stays `None` for workers.
- Depends on: PR5; PR2 (notice path).

### PR7: worker prompt

Files: `src/core/ai/chat/prompts/worker.md` (new), `src/core/ai/chat/prompts.rs`, `src/core/ai/chat/prompts/tests.rs`, `src/runtime/orchestra.rs` (`launch` at 1504); tests in `prompts/tests.rs`.

- [x] 7.1 RED: `prompts/tests.rs`: the `code` worker system prompt contains the worker instruction (make changes through write tools, end with `propose_batch`, a code fragment in the final text is not a result, it overrides the fenced-block style), is appended for write-capable workers only (`spec.writes()`), and does not contradict `tools.md`. Spec: "Worker prompt asks for tools [W2, auto]".
- [x] 7.2 RED: a chat turn on the `code` assignment gets byte-for-byte the prompt it had before (compare against the current built-in); a user override of `code.md` still applies to both. Spec: "Chat turn unchanged [W2, auto]".
- [x] 7.3 GREEN: add `worker.md` (lazarobox-owned, not overridable per D29) and append it in `launch` only for write-capable workers; review workers (`live.review.system(id)`) keep their lens prompt.
- [x] 7.4 REFACTOR: single `include_str!` constant; keep the loader free of profile branching.
- Done when: `cargo test` green; manual step M4 (prompt on a real model) is recorded in the PR.
- Depends on: PR5 (`writes()`).

---

## Slice W2b: `/write` from the chat (split into two PRs)

Spec references: modal-input "Write on from the chat is a user action [W2]" (both scenarios); agent-orchestrator "A worker that must write is refused when Write is off" ("Turning Write on lets the spawn through", "A model cannot turn Write on").
Design: D15 (approved), D17 (`OrchCeiling`). Template: `/github` (`app/github.rs`, `ApprovalAsk::GithubGrant`).

### PR8: the `/write` grant (command, dialog, lifetime)

Files: `src/core/ai/chat/folder.rs` (`Command::Write`, parse at 92), `src/app/chat.rs` (route at ~520), new `src/app/write.rs`, `src/core/ai/chat/approval.rs` (`ApprovalAsk::ToolGrant { toggle, folder }`), `src/app/tool.rs` (deny from a tool call, beside 148), `src/app/run.rs` (dialog rendering beside 671), `src/runtime.rs` if the ask needs a runtime arm; tests in `folder_tests.rs`, `app_tests/`.

- [x] 8.1 RED: `parse_command("/write")` -> `Command::Write`; `/write x` is refused with a usage line; `/Write` stays plain text (same shape as `folder_tests.rs:313-325`).
- [x] 8.2 RED (app): `/write` in an idle chat with tools on and Write off opens a `ToolGrant` dialog naming the toggle and folder; `y` grants for this chat only, in memory, never saved to config; `n`/Esc declines with a visible line. Spec: "User turns Write on [W2, auto]" (the chat shows it was turned on).
- [x] 8.3 RED: refusals are visible with a reason: tools off, engine offers no tools, Write already on, no folder, conversation list open, another prompt pending (queued like `/github`). Mirror `R_*` constants.
- [x] 8.4 RED: the grant ends, with a system line each, on folder change, tools turned off, and the saved Write toggle going on or off; a resumed conversation does not carry it. Mirror `end_grant_for_folder`, `end_all_grants` for the new toggle.
- [x] 8.5 RED: `on_tool_ask` denies `ToolGrant` raised from a tool call (a model cannot open the dialog); worker or orchestrator text containing `/write` changes nothing. Spec: "Text cannot trigger it [W2, auto]". Add a test that the variant is rejected in every approval mode, like `GithubGrant`.
- [x] 8.6 GREEN: implement in `app/write.rs`, reusing the `ChatState` grant flag pattern; no ceiling effect yet (that is PR9), so this PR is user-visible only as a dialog and a flag.
- [x] 8.7 REFACTOR: extract what `github.rs` and `write.rs` share only if the duplication is mechanical; otherwise leave it.
- Done when: `cargo test` green; manual step M5 shows the dialog and the flag, no effect on spawns yet.
- Depends on: none strictly (independent of W2a), but order W2a first as the owner set.
- Size risk: high end of re-forecast is above 400; split candidate if the dialog plumbing in `tool.rs` and `run.rs` is wider than expected. Fallback split: 8a (parse + dialog + denied-from-tool) and 8b (lifetime and endings).

### PR9: the grant reaches the next spawn

Files: `src/runtime.rs` and `src/runtime/orchestra.rs` (`Effect::OrchCeiling`), `src/core/ai/orchestra/run.rs` (`Run.ceiling` update), `src/app/write.rs`, `src/app/orchestra.rs` (notice and guidance), PR6's refusal text; tests in `orchestra/tests.rs`, `app_tests/`.

- [x] 9.1 RED: after `/write` + `y`, an `Effect::OrchCeiling` updates the live run; the next `code` spawn gets write tools; a running worker keeps its original spec (like `set_roster`). Spec: "Turning Write on lets the spawn through [W2, auto]" (the `/write` variant).
- [x] 9.2 RED: when the grant ends (PR8 endings) the ceiling is lowered again for the next spawn; `Toolset::capped` still never widens beyond the role.
- [x] 9.3 RED: the Write-off refusal text now names `/write` as the chat step (replaces the interim menu step of PR6) and the chat-pane requirement "names the `Write` toggle and the step to turn it on from the chat" is asserted on the final string.
- [x] 9.4 RED: ORCH off or a plain chat: `/write` still works and widens the next plain-chat turn through `offered_families` (migration note: "`/write` also widens a plain chat's next turn, by the user's own `y`"). Pin it with a test, since this is user-visible outside ORCH.
- [x] 9.5 GREEN: add the effect, update `Run.ceiling` at `attach` and on the effect; extend `offered_families` for Write.
- [x] 9.6 REFACTOR: unify the guidance line for both grants if cheap.
- Done when: `cargo test` green; manual step M5 passes end to end on Ollama.
- Depends on: PR6 and PR8.
- Notes (as built, PR8+PR9 together; production is over 400, so the two commits are the cut): `ApprovalAsk::ToolGrant { toggle, folder }` (+ `tool_grant_prompt`, `TOOL_GRANT_DENIED`, `WRITE_*` lines in `approval.rs`); `ChatState.write_granted` (reset by a resume); `app/write.rs` holds the command, the `y` dialog, the endings and `orch_ceiling(id)` (saved menu plus Write while the grant holds and tools are on; the only widening). `offered_families` and the three `OrchSend`/`OrchDecide` ceilings read `orch_ceiling`. `Effect::OrchCeiling { pane, ceiling }` is sent only for an orchestrating chat, on `y` and on every ending (folder change or restore, tools off, saved Write toggle change, resume); the runtime arm calls `RunManager::set_ceiling(pane, ceiling)`, which ignores a pane without a live run. Texts: `WRITE_OFF` and `WRITE_OFF_CORRECTION` name `/write` (in backticks, because `mask_paths` turns a bare `/write` into `<path>` in the coordinator-bound text); the ORCH-on line says `type /write`. Not done: 8.7 (the key handler of `/github` and `/write` is duplicated on purpose, the lines differ), 9.6 (no shared guidance line: the `/write` grant has no hint above the composer). The README documents `/write` next to the `/github` section.

---

## Task PR10 (MANUAL-GATED): llamacpp tool calling and `MCP_VERIFIED`

Gate: do NOT open this PR unless the owner reports the manual llama-server tool-calling check as PASSED. If it fails, no code is written; acceptance for llama.cpp ends in the visible notice from PR2 (turn degraded to no tools) and the owner records the result.

Files: `src/core/ai/chat/engine.rs:131` (`MCP_VERIFIED`), `src/runtime/chat.rs:661` (remove the "inert until `MCP_VERIFIED` lists it (PR3)" comment), tests that pin the list (`src/app.rs:5777` asserts `tool_engines == MCP_VERIFIED`; `engine.rs` test near line 483 pins llamacpp; `core/ai/picker.rs:290` uses the list).

- [ ] 10.1 MANUAL: run M6 (below) on a real llama-server and report pass or fail in the PR or the change.
- [ ] 10.2 RED (only after a pass): test that `tools_for("llamacpp", true, Some(origin), MCP_VERIFIED)` returns `Tools::On` and that a worker on llamacpp now gets a tool session (`open_session` takes the `Adapter::Llamacpp` branch at ~1615); the existing llamacpp pin test is updated, not deleted.
- [ ] 10.3 GREEN: add `"llamacpp"` to `MCP_VERIFIED`; update the comment in `runtime/chat.rs`.
- [ ] 10.4 REFACTOR: none expected. Check `picker.rs:290` presentation (llamacpp now listed as tool-capable) with its snapshot.
- Done when: manual M6 recorded; `cargo test` green; the acceptance note in the PR says "verified on <model>, <server version>".
- Depends on: PR2 (so a failure is visible) and PR7 (prompt) for a meaningful check. May run in parallel with PR8/PR9.

---

## Manual acceptance (owner, per slice)

Unit tests cannot prove local tool calling. Use one Ollama model that supports tools, and one llama-server (llama.cpp) build for M6. ORCH on in the pane (`Ctrl+O`), Orchestrator enabled in the AI menu.

- M1 (W1a, after PR1+PR2): On Ollama, orchestrate a change request across two files. Check, in this order: if the coordinator never spawns a worker, the W1b line appears (after PR3); if a `code` worker ends with nothing staged, the transcript notice names its id, kind and `ollama>model`; if you stage and cancel before proposing, the "staged and never proposed" line appears; provoke a refused spawn (read-only goal, e.g. "explain how X works" with a code worker) and see the same reason as the coordinator; start a worker on an engine not in the tool list (e.g. llama.cpp before PR10) and see the "ran without tools: <reason>" line. Record which notice, if any, matches the original fragments-instead-of-files symptom.
- M2 (W1b, after PR3+PR4): Ask a plain question in an orchestrating pane; the neutral line with the delegate key shows. Press Esc: it goes. Ask for a change that the coordinator answers in text; press Enter on the empty composer: an orchestrator turn starts and the coordinator spawns a worker. Typing text first and pressing Enter sends the text as usual.
- M3 (W2a, after PR6): Turn Write off in the AI menu. Ask for a change; the `code` spawn is refused, no process starts, the line names `Write` and the menu step, the run does not pause and no failure alert opens. Turn Write on and retry: the worker gets write tools. Confirm a `review` worker never gets write tools with both toggles on.
- M4 (W2a, after PR7): On Ollama with Write on, ask for a small two-file change. The worker makes its edits with the write tools and ends with `propose_batch`; the batch review opens. A fragment in the final text instead of tool calls is a failed acceptance; record the model name and what it did. Plain chat turn on the `code` assignment shows no behaviour change.
- M5 (W2b, after PR8+PR9): Write off, ORCH on. Trigger the Write-off refusal; type `/write`; the dialog names the toggle and the folder; `y`; the line says Write is on for this chat. The next spawn gets write tools. Change folder: the grant ends with a line. Turn tools off: it ends. Paste text containing `/write` through worker output: nothing happens. In a plain (non-ORCH) chat `/write` widens the next turn.
- M6 (PR10 gate, llama-server): Start `llama-server` with a tool-capable model and a template that supports tool calls (`--jinja`). In lazarobox assign it to the `code` row, Write on, ORCH on. Before PR10 the worker must show the "ran without tools" notice (this is the expected visible failure). Then, on a build with `llamacpp` in `MCP_VERIFIED` (local branch), spawn a `code` worker for a two-file change and verify: tool calls are made, `propose_batch` opens the batch review, the batch applies. Also run two workers in parallel to see D12 behaviour (queue or "no answer" halt). Pass means PR10 opens; fail means PR10 is not opened and acceptance is the notice.

## Later milestones (not tasked)

Milestone 2 (W10a, W10b, W3, W4) is tasked below. Milestone 3 is W5, W6, W7, W8a, W8b, W8c, W9a, W9b (design order). Open owner decisions 2, 3, 4, 6, 7, 8 and D16 `/exec` belong to it and are not tasked. After milestone 2, revisit sizes: if W1a/W1b identify the original trigger, later slices may shrink.

## Review Workload Forecast

The design estimated 620-990 production lines for these four slices as 4 PRs. Past deliveries ran 2.4x to 3x their estimates, so the figures below are the design estimate for each PR multiplied by 2.4 (low) and 3 (high). Production only; tests, fixtures and snapshots are excluded.

| PR | Slice | Scope | Design estimate | Re-forecast (production) | 400-line risk |
|---|---|---|---|---|---|
| PR1 | W1a | empty ledger and never-proposed notices | 75-115 | 180-345 | Low |
| PR2 | W1a | refused-spawn and degraded-turn notices | 75-115 | 180-345 | Low |
| PR3 | W1b | `Run::undelegated`, parked notice | 60-100 | 145-300 | Low |
| PR4 | W1b | delegate key | 60-100 | 145-300 | Low |
| PR5+6 | W2a | ceiling types, ceiling wired, Write-off refusal (combined 2026-11-06) | 140-230 | 335-690 | Medium (split again if production passes 400) |
| PR7 | W2a | worker prompt | 30-50 | 70-150 | Low |
| PR8+9 | W2b | `/write` grant and grant reaches the next spawn (combined 2026-11-06) | 180-280 | 430-840 | High (split again if production passes 400) |
| PR10 | manual-gated | llamacpp in `MCP_VERIFIED` | 3-10 | 10-30 | Low |

Totals: ~1,495-2,890 production lines across the milestone (design: 620-990; the same 2.4x-3x factor applied to the whole gives 1,490-2,970, so the per-PR split is consistent).

- Estimated production changed lines per PR: see the table. No PR is expected to exceed 400 except the high end of PR8.
- 400-line budget risk: High for the milestone as 4 slices (every slice would exceed 400 at the re-forecast: W1a 360-690, W1b 290-600, W2a 410-840, W2b 430-840); Low to Medium per PR once split into the 10 above.
- Chained PRs recommended: Yes
- Decision needed before apply: Yes
- Proposed PR boundaries (stacked-to-main, merge in this order): PR1 -> PR2 -> PR3 -> PR4 -> (PR5+PR6) -> PR7 -> (PR8+PR9), with PR10 held until the manual llama-server check passes (it may be opened any time after PR7). Each PR starts from main after the previous one merged, finishes with its tests green and its manual step recorded, and reverts alone. Cut points: W1a at "notices of facts" vs "notices of refusals and degrade"; W1b at "detect and park" vs "key"; W2a at "types, wired ceiling and refusal" (one PR) vs "prompt"; W2b at "grant" vs "grant reaches the spawn". If the combined PR8+9 passes 400 at apply time, split it back into 8a (parse, dialog, denied from tool), 8b (lifetime endings) and 9. Owner decision 2026-11-06: PR5+PR6 and PR8+PR9 are combined; PR7 and PR10 stay apart.

Decision needed (ask-on-risk): confirm the split of the four slices into the nine code PRs plus the gated PR10, rather than one PR per slice with `size:exception`. Dependencies that matter for the order: PR5 must precede PR6 so no worker silently loses write tools; PR6's interim refusal text (menu step) is replaced in PR9 by `/write`.

## Work-unit commit plan (per PR)

Each PR is one deliverable unit; inside it, commit tests with the behaviour they verify (never "tests" or "types" as their own commit). Suggested messages: `feat(orchestra): disclose workers that changed nothing or never proposed` (PR1), `feat(orchestra): show refused spawns and tool-less worker turns` (PR2), `feat(orchestra): say when a coordinator reply delegated nothing` (PR3), `feat(orchestra): Enter on an empty composer asks the coordinator to delegate` (PR4), `refactor(orchestra): carry a per-worker family set and a menu ceiling` (PR5), `feat(orchestra): refuse a code worker when Write is off` (PR6), `feat(orchestra): worker prompt that requires tools` (PR7), `feat(chat): /write grants Write for this chat` (PR8), `feat(orchestra): a /write grant reaches the next spawn` (PR9), `feat(chat): list llamacpp as tool-verified` (PR10).

---
---

# Milestone 2: W10a, W10b, W3, W4 (PR11 to PR14)

Scope: chosen by the owner on 2026-11-06, in this order (engram `sdd/orchestrator-worker-flow/milestone-2`). Milestone 1 above is done and unchanged; PR10 stays manual-gated and is not part of this milestone. Execution context as in milestone 1: artifact store `hybrid`, chain strategy `stacked-to-main`, Strict TDD with `cargo test`, the 400-line budget counts PRODUCTION code only. Every convention of milestone 1 applies (RED -> GREEN -> REFACTOR, work-unit commits, no AI attribution, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` green, ORCH off by default, one revert per PR).

## Decisions applied (owner, 2026-11-06)

- Decision 5 (D11): `max_parallel` is the only ceiling. One notice when two workers share a local engine. Task group W10b (PR12).
- Decision 9 (D23, D24): hard spawn and finish gates, plus the user's `Enter gates off` override. A model can never use the override. Task group W3 (PR13).
- Still OPEN, milestone 3, NOT tasked: decisions 2, 3, 4, 6, 7, 8 and D16 `/exec`. Decision 10 (interim one-writer guard) was closed in milestone 1.

## Lessons from milestone 1, folded into every PR below

- Size: real PRs landed near the design figure, so the forecast is the design estimate plus about 30%, not 2.4x-3x.
- CI: each PR pays about 17 minutes of macOS CI. One PR per slice is the default when the slice fits in 400 production lines; a split is made only at the commit boundary named in the PR.
- Bypass paths: every task that adds a guard lists the paths that could skip it (retry, relaunch, queue promote, resume, attach, a lazarobox-started worker) and has a RED for each. A path that is deliberately exempt gets a test that says so.
- Stale state: every new piece of state names what clears it (folder change, resume, ORCH toggle, run end, pane close) and has a RED per clearing event.
- Notices only name a key that exists in the same PR.
- macOS clippy runs with `-D warnings`: a helper or fixture used only by Linux-gated tests carries the same `cfg`. Check by reading the diff, not by running Linux only.
- Behavioural RED: each RED below says what fails today. A test that passes before its GREEN is rewritten, or labelled GUARD (passes by design, pins a path that must not change) and then shown to go red against a naive GREEN.
- Time: tests on the orchestra writing lab use real short deadlines (about 1500 ms), NEVER `#[tokio::test(start_paused = true)]` (engram `testing/orchestra-writing-lab-paused-time`; this overrides the "paused tokio clock" line of the design's Testing Strategy for the writing lab). Run `cargo test` for those under `timeout`.

## Drift check against branch feat/orch-write-grant (PR #613)

| Design reference | Current branch | Effect |
|---|---|---|
| `StageKey` in "`tools/stage.rs`" | Two files: the enum is `src/core/ai/tools/stage.rs:27-32` (only `Pane`; its doc says it "is meant to grow"); the store is `src/runtime/tools/stage.rs` (`Stages` at 50, `badges` at 92-111, `discard_key` at 179) | W10a edits both. Two irrefutable patterns stop compiling when the second variant lands, which is a useful compile guard: `runtime/tools/stage.rs:96` and `runtime/chat.rs:1119` (`let StageKey::Pane(pane) = id.key;`). |
| `WorkerKey` | `src/core/ai/orchestra/mod.rs:36`; `SessionKey::Worker(PaneId, WorkerKey)` at 57 already exists | `StageKey::Worker` reuses `WorkerKey`; `core::ai::tools` starts to import `core::ai::orchestra` (same crate, no cycle). |
| `RunGate` claims "by `PaneId`" (D8) | `host/gate.rs`: `Claimed = HashSet<StageKey>` (42), `acquire` 93, `is_busy` 88 | Claim today is per `StageKey`; W10a normalises it to the pane. |
| Exec sinks | `host/exec.rs`: `sinks: HashMap<StageKey, Unreported>` 85, `sink` 103, key at 121 and 304 | Key-generic already; only pinned. Workers cannot call `run_command` until W6, so D8 is unit-tested only. |
| Orphan and close paths | `host/orphan.rs`: `close_batch` 35, `unproposed` 55 (its doc comment already says "until W10a gives each writer its own"), `review_batch` 64; `runtime/chat.rs`: `close_batch` 1089, `review_batch` 1099, `apply_batch` 1118 | W10a/W10b. The `unproposed` doc comment must be rewritten in PR12. |
| Seal and ask | `host/files.rs`: `Sealed` 60-103 (`keep` flag), `seal` 135, `seal_in` 161; `core/ai/tools/stage.rs`: `propose` 299, `StageError::Proposed` 292 | Re-propose fix (W10a) lives here. |
| Writer key at launch | `runtime/orchestra.rs`: `launch` 1552, `sync` launch loop 1480-1491, `open_session` 1662, `stage_key` at 1693, `disclose` 1773 (`StageKey::Pane(pane)` at 1775) | The flip to `Worker` is PR12, not PR11 (see the ordering note). |
| W10b "`chat/review.rs`" | `core/ai/chat/review.rs`: `BatchReview::labelled` 235 already carries the worker label | Probably untouched; confirm at apply. The real work is in `runtime/orchestra/review.rs`: `State` 122 (`viewers` 129 and `correcting` 133 are `HashSet<WorkerId>`, they need the proposer), `restaged` 892, `diff_text` 908 (both read `StageKey::Pane(pane)` at 901 and 924), `start_review` 930. |
| W3 "`run.rs`, `steps.rs`" | `run.rs`: `Refusal` 106, `spawn` 405, `finish` 867, `next_turn` 892 (`Capped` 79/901); `steps.rs`: `check` 177, `grant` is PRIVATE (152) | Needs a `Steps::grant()` accessor. `start_review` (`runtime/orchestra/review.rs:943-955`) and `RunManager::spawn` (`runtime/orchestra.rs:1138-1149`) match `Refusal` exhaustively, so a new variant is a compile guard. `finish_run` matches `FinishError` at 1403-1410. `NextTurn::Capped` is rendered at `runtime/orchestra.rs:915`. |
| D24 Enter order | As built: `chat.rs:326-346` (`answer_orch_alert`, then `answer_delegate`); `orch_guidance` `app/orchestra.rs:863`; `delegate_ready` 534 requires `orch_guidance(id).is_none()`; `guidance_is_hint` 917; `ChatState.delegate` `chat/mod.rs:506`, cleared at 639, 801 and 1613 | The gate line goes INTO `orch_guidance` after the alert, so `delegate_ready`, the "nothing to delegate" branch and `chat_guidance` hide the undelegated line without a second check. |
| Effect template | `Effect::OrchCeiling` `runtime.rs:1840`, `app/write.rs:54` | `Effect::OrchGatesOff` copies its shape. |
| Prompts | `src/core/ai/chat/prompts/{analyst,programmer}/orchestrate_odd.md` (both profiles) | Both change; pin both in `prompts/tests.rs`. |
| W4 "In `Handoff::of`" | `core/ai/orchestra/ledger.rs:127`: `Handoff::of(facts, judgement, blocked)` is pure over `Facts` (168), which holds NO ledger; `Run::facts` `run.rs:685`; `Run::receipt` 645 builds `Facts` for `Unit::Receipt`; `fallbacks` 850; `Row::new` ledger.rs:210; store note `persist.rs:58-97` | `Facts` must gain the worker's ledger paths. `of` serves worker, step, receipt and fallback handoffs: the check must be explicit per unit. |
| W4 "shown in the result" | `Run::collect_wrapped` run.rs:597; runtime `collect` 1237, `handoff` 1262 (ack text), `Closed` event built in `finish_run` 1393 | The first `collect` happens BEFORE the handoff exists, so it cannot carry the check (see PR14 note). |

No file listed in the design is missing.

Ordering note (differs from the design's file split, on purpose): in the design W10a also flips the writers to `StageKey::Worker`. Review lenses and the correction worker read the proposer's batch through the PANE key today (`review.rs:901,924`, `views` at `orchestra.rs:1693`), so a flip without the proposer mapping would blind every review for one PR. PR11 therefore ships the key, the store, the claim and the re-propose fix with writers still on `Pane` (behaviour identical for every user path), and PR12 flips writers, viewers, corrections and disclosure together. This is the same "inert first" shape as PR5/PR6.

Spec note: the scenario "Before W10, one writer at a time" (agent-orchestrator) was never implemented, because owner decision 10 closed with no interim guard in milestone 1. PR12 records it as superseded in its description; no test.

---

## Slice W10a: a batch per key (one PR)

Spec references: code-write "Worker batches [W10]" (store half of "One proposal does not seal another's files"); agent-orchestrator "One batch per writing worker and parallel writers [W10]" ("Same file, drift refused"). Design: D7, D8, D9, D10 (apply-time half only), Corrections row on the denied ask.

### PR11: `StageKey::Worker`, per-pane operations, claim by pane, re-propose

Files: `src/core/ai/tools/stage.rs`, `src/runtime/tools/stage.rs`, `src/runtime/tools/host/{gate,orphan,files}.rs`, `src/runtime/chat.rs` (1089-1119); tests in `core/ai/tools/stage.rs`, `runtime/tools/stage.rs`, `host/gate_tests.rs`, `host/files_tests.rs`, `runtime/chat/tools_tests.rs`.

- [x] 11.1 RED: `StageKey::Worker(PaneId, WorkerKey)` exists; `key.pane()` returns the pane for both variants; two workers of one pane, and one worker id in two runs, are distinct keys. GREEN: add the variant and `pane()`. Because nothing builds a `Worker` key outside tests until PR12, put `#[cfg_attr(not(test), allow(dead_code))]` on the variant with a comment naming PR12 (macOS clippy `-D warnings`); PR12 removes it.
- [x] 11.2 RED (`runtime/tools/stage.rs`): `badges` for a pane holding batches under two keys (2 and 3 entries) shows ONE badge of 5; it is `Orphan(5)` when no turn of the pane is live, and a pane with only empty batches shows none. Fails today by compile error, and a naive `key.pane()` rewrite fails it too (`out.insert` would keep only the last key's count). GREEN: accumulate per pane.
- [x] 11.3 RED: `Stages::discard_pane(pane)` (used by `close_batch`) drops EVERY key of the pane and returns the summed entry count (`None` when 0); a sibling pane's batch survives. RED: the orphan review of a pane with two orphan keys opens the OLDEST batch (lowest generation) and the next call opens the next one. Decision made here, flagged to the owner in the PR text: the user reviews one worker batch at a time, oldest first. GREEN: `Stages::review_pane`, and `runtime/chat.rs` `close_batch`, `review_batch`, `apply_batch` use it / `.pane()`.
- [x] 11.4 RED (`gate_tests.rs`): two keys of ONE pane: the second `acquire` is refused at once with `CHAT_BUSY`; keys of two panes both get a slot; `is_busy(Worker key)` is true while the pane's `Pane` key holds the claim. Fails today (distinct keys never conflict). GREEN: normalise the claim to the pane inside `acquire` and `is_busy` (D8). Check every caller of `is_busy` (grep) so none keeps a raw key.
- [x] 11.5 GUARD (`exec_tests.rs`, passes by design): `sinks` keep one `Unreported` per key, so worker A's command is not refused `propose them first` because of worker B's unproposed batch, and is for its own. Pins the open point of the code-write spec ("scoped to its own batch") now that two keys can exist.
- [x] 11.6 RED (`files_tests.rs`): re-propose after a denied ask. Worker batch `Proposed`, its ask denied by the end of the coordinator's turn (`Sealed` dropped with `keep` set, nobody waiting): the owner of that batch calls `propose_batch` again and gets a fresh ask over the same files. Fails today with `StageError::Proposed`. RED (stays refused): a second `propose_batch` while the first ask is still open; another key's `Proposed` batch is never re-asked by a different worker. A re-propose runs the same drift and size checks as the first. GREEN: remember on the store entry that no ask is open (cleared by `announced`, set when the ask is dropped) and let `seal` re-propose only then. This also changes a plain chat turn (same function): add a chat-turn test so the change is deliberate.
- [x] 11.7 GUARD (`stage.rs`): two batches of different keys stage the same path; the first is applied; the second, approved, writes nothing, stays `Proposed` and reports drift. Spec "Same file, drift refused [W10, auto]". Passes by design (apply-time drift exists); pinned because two keys are now possible.
- [x] 11.8 REFACTOR: grep every `StageKey::Pane(` and say in the PR which stay (plain chat: `runtime.rs:6326`, `runtime/chat.rs:969`) and which become key-generic; rewrite the `unproposed` doc comment only in PR12.
- Done when: `cargo test` green, clippy and fmt green on Linux and read for macOS; a single `code` worker behaves as on main (manual M7).
- Production forecast: design 200-320 less about 30 for the flip, plus 30% = 220-380. Medium. If production passes 400 at apply time, the cut is two commits: (1) 11.1-11.5, 11.7, 11.8, (2) 11.6 re-propose; split there and ask the owner.
- As built (PR11, branch `feat/orch-stage-keys`, from `docs/owf-milestone-2-tasks`, not pushed): two commits, `feat(tools): key a staged batch by pane or by worker` (11.1-11.5, 11.7, 11.8) and `fix(tools): ask again when a model proposes a batch whose ask ended` (11.6). Production about 202 changed lines (146 added, 56 removed) against the 400 budget, so no split. `Stages::badges` sums per pane; `discard_pane` and `review_pane` (oldest generation first, an unanswered batch is shown again) back `ToolHost::close_batch(pane)` and `review_batch(pane, live)`, which now take a `PaneId`; `RunGate` claims `PaneId`; `Chats::apply_batch` reads `id.key.pane()`. Re-propose: `Batch::ask` / `ask_gone` with an `asking` flag, set by `Stages::seal`, cleared when `Sealed` drops with `keep` set (turn ended or call dropped). `Stages::discard_key` is now `#[cfg(test)]`. `StageKey::pane()` is the only way to a pane; `StageKey::Worker` carries `cfg_attr(not(test), allow(dead_code))`, PR12 removes it.
- `StageKey::Pane(` kept on purpose in PR11 (PR12 flips the writer ones): plain chat `runtime.rs:6326` and `runtime/chat.rs:969`; writers `runtime/orchestra.rs:1693` and `disclose` `:1775`; review reads `runtime/orchestra/review.rs:901,924`. PR12 owns the last three groups together.
- Known edge: a batch sealed by the user's orphan review (no ask of the model) also counts as "no ask open", so a model that proposes it meanwhile opens a second ask over the same id; the first answer wins and the other finds the batch gone. M7 (manual) is still owed to the owner.
- Depends on: none (milestone 1 merged or PR #613 as base).

---

## Slice W10b: writers, reviews and disclosure on the worker's key (one PR)

Spec references: code-write "One proposal does not seal another's files [W10, auto]"; agent-orchestrator "Two writers, two batches [W10, auto]", "Same file, drift refused [W10, auto]"; the owner's decision 5 (no spec scenario; the notice text is asserted here). Design: D7, D9, D11, D12, D22 (only the viewer/correction mapping, not internal workers).

### PR12: parallel writers

Files: `src/runtime/orchestra.rs` (`open_session` 1693, `disclose` 1775, `sync` 1480-1491 for the notice), `src/runtime/orchestra/review.rs` (`State` 122, `restaged` 892, `diff_text` 908, `start_review` 930), `src/runtime/tools/host/orphan.rs` (doc), `src/core/ai/orchestra/run.rs` (pure engine-sharing query), `src/app/orchestra.rs` (notice goes through the existing `OrchestraEvent::Notice`, 395); tests in `src/runtime/orchestra/tests.rs` (writing lab, Linux-gated helpers carry the same `cfg`), `orchestra/review_tests.rs`, `orchestra/tests.rs`, `src/app/app_tests/`.

- [ ] 12.1 RED (writing lab, real deadlines): two `code` workers of one pane each stage a different file; worker 1 calls `propose_batch`; the ask lists ONLY worker 1's files; worker 2 can still stage and propose; approving 1 applies only 1's files and its ledger alone flips to applied. Fails today: the shared pane batch seals both. GREEN: `open_session` builds `StageKey::Worker(pane, key)` for writers; drop the `cfg_attr` of 11.1.
- [ ] 12.2 RED: two proposals pending from two workers queue as labelled reviews, one at a time, in arrival order, each labelled with its worker (D9); answering the first does not touch the second.
- [ ] 12.3 RED: a lens or the refuter reads its PROPOSER's batch, not a sibling's: with worker 1 under review and worker 2 holding other staged files, the review prompt and `restaged` see worker 1's files only. The correction worker stages into the proposer's batch and may not propose. Fails after a naive 12.1 (a lens would read the empty pane batch). GREEN: `State.viewers` and `correcting` map reviewer -> proposer; `open_session` picks the proposer's key for them; `restaged` and `diff_text` take the proposer.
- [ ] 12.4 RED (bypass paths): a lens that is retried, relaunched or started from the queue keeps its proposer mapping; a correction retried after a failure is still no proposer and no second correction; a review of worker 1 started while worker 2 is mid-staging does not flag worker 2's files as late paths of worker 1.
- [ ] 12.5 RED: disclosure per writer. Worker 1 staged and never proposed while worker 2 proposed: the "staged, never proposed" notice names worker 1 only (today the pane flag makes it all-or-none; see the `orphan.rs:50-55` doc comment). GREEN: `disclose` asks `unproposed(Worker key)`; rewrite that doc comment.
- [ ] 12.6 RED (stale state): ending or cancelling the run with two staged writers leaves one orphan badge counting both; closing the pane drops both; a resumed run starts with no worker batches; the orphan review (11.3) opens them oldest first.
- [ ] 12.7 RED (decision 5): pure `Run` query "worker N shares a local engine with a running worker M" counts only workers whose turn is in flight (status Running and not parked) and only engines where `locality(engine) == Local` and `!forwards_off_machine(engine, model)` (an Ollama `*-cloud` model is not shared). Runtime: ONE `Notice` per run per engine, ASCII, text like `workers 1 and 2 share <engine>, a local server: it may queue their requests; set max_parallel = 1 in [ai.orchestrator] to run one at a time` (names an existing config key only). Not sent: different engines, a cloud engine, sequential workers, a third worker on an engine already told. Sent from the `sync` launch loop, so spawn, queue promote, retry and relaunch all reach it (RED for promote and retry). The once-per-engine memory lives on `Live`, so a new run notifies again. Fails today: no notice.
- [ ] 12.8 RED (app): the notice is a transcript system line, not stored as history (reuse the "history does not hold it" assertion), insta snapshot.
- [ ] 12.9 Docs: CHANGELOG `Unreleased` entry (parallel writers, one review each, the engine notice). Record in the PR text the superseded scenario "Before W10, one writer at a time".
- [ ] 12.0 Flip checklist (from the PR11 review). Each site below still assumes the writer key is `StageKey::Pane(pane)` and MUST move with the writer flip, or it reads an empty batch without any error: `src/runtime/orchestra.rs:1693` (`stage_key` in `open_session`), `src/runtime/orchestra.rs:1775` (`unproposed` in `disclose`), `src/runtime/orchestra/review.rs:901` (`staged_changes`), `src/runtime/orchestra/review.rs:924` (`staged_diff`) and `src/runtime/chat.rs:969` (the plain chat turn: check whether it stays `Pane`; a plain chat is not a worker, so it is expected to stay, but say so in the PR). Also check the `CHAT_BUSY` text ("a command is already running in this chat") when the holder of the pane claim is a sibling worker: the claim is by pane since PR11, so the message must still make sense to the worker that is refused.
- Done when: `cargo test` green (writing-lab tests under `timeout`); manual M8 passes, including the D12 observations.
- Production forecast: design 90-160 plus about 30 for the moved flip, plus 30% = 155-250. Low.
- Depends on: PR11.

---

## Slice W3: step gates and the user's override (one PR)

Spec references: agent-orchestrator "Gates on step state are visible and bounded [W3]" ("A gate names itself", "The turn cap ends a trapped run visibly", "Only the user overrides"); chat-pane notices. Design: D23, D24. Owner decision 9: hard gates. The gated transitions are exactly D23's two: SPAWN of a write-capable worker needs `steps.check(Step::Implement)`, FINISH is refused when `grant == Implement` and no write-capable worker was spawned. Lifting is all-or-nothing for the run: one key lifts both.

### PR13: gates, override key

Files: `src/core/ai/orchestra/steps.rs` (`grant()` accessor), `src/core/ai/orchestra/run.rs` (`Refusal::Gate`, `FinishError::Gate`, `spawn_internal`, `gates_off`, last-gate memory), `src/core/ai/orchestra/ledger.rs` (`FinishError`, 188), `src/core/ai/orchestra/event.rs` (`GateRefused`), `src/runtime/orchestra.rs` (`spawn` 1138, `finish_run` 1403, `next_turn` 915, `start` 709, `attach` 758, new `gates_off(pane)`), `src/runtime/orchestra/review.rs` (`start_review` 943-955), `src/runtime.rs` (new `Effect::OrchGatesOff`, beside 1840), `src/app/orchestra.rs` (`orch_guidance` 863, the new key method, `guidance_is_hint` 917), `src/app/chat.rs` (between 331 and 343), `src/core/ai/chat/guidance.rs` (the line and its width forms), `src/core/ai/chat/mod.rs` (`ChatState.gate_offer`, cleared at the sites of `drop_delegate`: 639, 801, 1613), `prompts/{analyst,programmer}/orchestrate_odd.md`, `prompts/tests.rs`; tests in `orchestra/tests.rs` (or `gate_tests.rs`), `steps_tests.rs`, `app_tests/`.

- [ ] 13.1 RED (pure): `Run::spawn(Task::Code)` with the early steps open returns `Refusal::Gate` naming the SPAWN gate and the first open step (`authorize` when none is closed); with steps closed through `classify` (`track` skipped for a small goal) it passes. `review`, `summarize`, `general` spawns are never gated. Precedence is pinned: `Paused/Ended`, then `ReadOnly` (a read-only goal says read-only, not "open step"), then `WorkerCap`, then `Gate`, then `WriteOff`. Gate on the ROLE (`writer`), so a Write-off spawn is still gated first. GREEN: add the variant, `Steps::grant()`, the check in `spawn`. Existing tests that spawn a `code` worker with no steps will go red: give the test module one helper that closes the steps (fixture lines are not production lines).
- [ ] 13.2 RED (pure): `Run::finish` with `grant == Implement` and no write-capable slot ever created returns `FinishError::Gate` (names the finish gate and `implement`); it passes after a `code` worker exists, even an ended one; `read_only` grant, and a run with no `authorize` at all and no workers, finish as today (a text-only answer is not gated). A spawn refused by `ReadOnly`, `WriteOff` or `WorkerCap` creates no slot, so it does not satisfy the gate; the refusal text must therefore name the gate and the user's override (see 13.6).
- [ ] 13.3 RED (exemption and bypass paths): the correction worker lazarobox starts (`start_review`, `Role::Correction`) is NOT gated: GUARD, shown to go red right after 13.1 GREEN, then GREEN with `Run::spawn_internal` used only there. Retry, relaunch, queue promote and `decide` of a worker that already exists never re-check the gate (steps only close). `grep slots.push` finds one site, `spawn`. A continuation turn (`attach`) keeps a lifted gate; a resumed or replaced run (`start`) does not inherit one (see 13.6).
- [ ] 13.4 RED (runtime): a gate refusal returns an error result to the coordinator that names the gate and the step it saw, and the SAME redacted, capped text reaches the pane as a `GateRefused` event; the run does not pause, no failure alert opens, no process starts; a later `spawn_agent` after the steps close works. A refused `finish` leaves the run `Open`, sends no `Closed` and no `Undelegated`. `start_review` and `RunManager::spawn` handle `Refusal::Gate` with explicit arms (no wildcard).
- [ ] 13.5 RED: turn cap. A run whose gate kept refusing until `max_turns` stops with a `Stopped` text that names the gate, the step and the cap, and says the override applies to the next run (spec "The turn cap ends a trapped run visibly"). A run that was never gated keeps today's text (GUARD). GREEN: the run remembers the last gate that refused.
- [ ] 13.6 RED (override state): `Run::lift_gates()` makes both gates pass for that run and returns which gates had refused. Runtime `gates_off(pane)`: a live run is lifted; with no live run the pane is remembered and consumed by the next `start` ONCE (a third run is gated again); a continuation `attach` keeps the lift; a replaced run (`start` over an old one) takes only the stored flag. The stored flag is cleared where the pane's other runtime state is (cancel and pane close). GREEN: `Effect::OrchGatesOff { pane }`, handled like `OrchCeiling` (returns at once).
- [ ] 13.7 RED (a model cannot use it): orchestrator text, `spawn_agent` prompts, worker text and tool results that say "gates off" or "Enter" lift nothing; no `OrchCall` variant reaches `lift_gates`, and the `OrchCall` dispatch has no wildcard arm. Spec "Only the user overrides [W3, auto]".
- [ ] 13.8 RED (app, guidance and key, D24 order as built: provider notice, failure alert, gate override, undelegated): `GateRefused` adds a system line naming gate and step and sets `ChatState.gate_offer`; `orch_guidance` shows `Enter gates off | <gate>: step <step>` in forms by width (pure test per form, ASCII, `github_hint` style), saying `this run` or `next run`; with an offer pending `delegate_ready` is false, the undelegated line is hidden and Enter does NOT delegate and the "nothing to delegate" notice is not shown; with the provider notice or the failure alert up, THEY answer Enter first. The line is a problem line, not a muted hint (`guidance_is_hint`).
- [ ] 13.9 RED (app, the key): Enter on an empty composer, fresh press, no modifiers, conversation view only, ORCH on: sends `Effect::OrchGatesOff` and writes one transcript line naming the gate lifted ("gates off: spawn gate lifted for this run"). Held Enter, Enter with text, Enter with a modal open, and Esc do nothing to the offer. Enter works while a turn runs (the lift targets the live run).
- [ ] 13.10 RED (stale offer): the offer is dropped by folder change, resume, ORCH toggle off, pane close, and when the NEXT run starts without Enter; a new refusal replaces it. One RED per event; mirror the `drop_delegate` sites.
- [ ] 13.11 RED (prompts): both `orchestrate_odd.md` say that a refused gate names the step to hand off first, that only the user can lift it, and not to retry the same spawn; `prompts/tests.rs` pins both profiles.
- [ ] 13.12 Docs: CHANGELOG `Unreleased` (gates, `Enter gates off`). The line names no key beyond Enter.
- Done when: `cargo test` green; manual M9 passes.
- Production forecast: design 180-300 plus 30% = 235-390. Medium. If it passes 400 at apply time, cut at two commits: (1) 13.1-13.5 (gates, no key), (2) 13.6-13.11 (override); the first is safe alone because a gated run with no override still ends at `max_turns`, said.
- Depends on: none in code (independent of W10); lands after PR12 by the owner's order.

---

## Slice W4: handoff check against the ledger (one PR)

Spec references: orchestrator-handoffs "App check of a handoff against the ledger [W4]" ("Claimed file not in the ledger", "Claim matches the ledger", "The orchestrator cannot clear a flag", "Nothing to check says so") and "A code worker with an empty ledger [W1, W4]" (the W4 half: the claimed file is flagged; the empty-ledger notice shipped in PR1). Design: D28. The `simple` kind does not exist yet (W8a); the check keys off a worker that has a ledger, `Task::Code` today.

### PR14: pure check, flag stored and shown

Files: `src/core/ai/orchestra/ledger.rs` (`Facts` 168, `Handoff` 111, `Handoff::of` 127, `Row::new` 210, `render_within`), `src/core/ai/orchestra/run.rs` (`facts` 685, `handoff` 629, `collect_wrapped` 597), `src/core/ai/orchestra/persist.rs` (`of_handoff` 58-97), `src/runtime/orchestra.rs` (`handoff` ack 1262-1277, `Closed` in `finish_run` 1393); tests in `ledger_tests.rs`, `orchestra/tests.rs`, `persist` tests, `app_tests/`.

- [ ] 14.1 RED (pure): `check_claims(done, ledger_paths)` returns Flagged / Matches / NotChecked. Cases: `wrote src/a.rs and src/b.rs` with a ledger of `src/a.rs` flags `src/b.rs` only; trailing commas, periods and backticks are stripped; `./src/a.rs` equals `src/a.rs`; URLs, versions (`v1.2`), `e.g.` and plain numbers are not paths; a path repeated is listed once; more than 8 flagged paths show `+n more`; no recognisable path gives `NotChecked("no file named in the handoff")`; a worker with no ledger gives `NotChecked("no change ledger")`; names that match give `Matches`. An absolute path token is compared against ledger paths by suffix (decide at RED and pin it). GREEN in `ledger.rs`.
- [ ] 14.2 RED: `Facts` carries the worker's ledger (paths and applied flags) set in `Run::facts`; `Handoff::of` stores a `check` for `Unit::Worker` ONLY. The step handoff, the receipt handoff and the lazarobox-built fallback (`done` is `none`, so `NotChecked("no handoff text")`) are covered by one test each: no false flag from a step or receipt `done`. A claim in a handoff of a `code` worker with an EMPTY ledger flags every claimed path (spec scenario).
- [ ] 14.3 RED (sticky): the orchestrator re-saves the handoff with a `done` that no longer names the path, and the flag remains; it clears only when the worker's ledger now holds the path (a retry staged it). The orchestrator's verdict (`on_course`) is stored and shown BESIDE the flag; a reason that says the flag is wrong changes nothing (spec "The orchestrator cannot clear a flag").
- [ ] 14.4 RED: write-capable worker, verdict `on_course`, ledger empty or with nothing applied: a second, neutral flag (`on_course, nothing applied yet`, never an accusation); `off_course` and `blocked` do not raise it; it is recomputed from the ledger when rendered, so a later apply clears it.
- [ ] 14.5 RED (shown, one test per place): (a) the `handoff` ack text carries `claimed, not in ledger: <paths>` or `not checked: <reason>` or `claims match the ledger`; (b) `collect` AFTER a handoff exists shows the check inside the untrusted-data wrapper, after the ledger; (c) the `close` report shows the flag under the worker row, and a ledger rendered at its bound still shows a short `Flags: n` (the cap never hides it); (d) the engram note of the handoff gains a `check:` line. Note for the owner: the spec says "in the `collect` result", but the first `collect` of a worker happens before its handoff exists, so only the handoff ack, a later `collect`, the `close` report and the stored note can carry it. Nothing here reads the worker's own final text.
- [ ] 14.6 REFACTOR: if more than a handful of tests build `Handoff` literals, add one `Handoff::test()` helper rather than touching each; keep it `cfg(test)` and used by non-gated tests only (macOS clippy).
- [ ] 14.7 Docs: CHANGELOG `Unreleased`.
- Done when: `cargo test` green; manual M10 passes.
- Production forecast: design 110-190 plus 30% = 145-250. Low.
- Depends on: none in code. Lands last by the owner's order; after PR12 a worker's ledger is per worker, which this check assumes.

---

## Manual acceptance, milestone 2 (owner, per slice)

Same setup as M1-M6: one Ollama model that supports tools, ORCH on (`Ctrl+O`), Orchestrator enabled, Write on (or `/write`).

- M7 (W10a, after PR11): Regression, one `code` worker. Ask for a small two-file change; the batch review opens labelled with the worker and applies as before. Then provoke the denied ask: leave the review unanswered until the coordinator's turn ends without `wait`, so the review closes; the worker's next `propose_batch` asks again (record whether this could be provoked, and what the transcript said). Cancel a run with a staged file: one orphan badge; open it, apply or discard.
- M8 (W10b, after PR12): Two parallel writers on ONE Ollama server, `max_parallel` 2. Ask for two independent changes in different files. Check: both workers run; each proposes; two reviews arrive one at a time, each labelled with its worker; approving the first applies only its files and leaves the second pending; a lens review of one worker never shows the other's files. The single local-engine notice appears once and names `max_parallel`; with `max_parallel = 1` it does not appear. D12, run it twice with `OLLAMA_NUM_PARALLEL=1` and `=2` (`ollama ps`/server log to confirm): with 1 the second request queues, and a wait past 120 s may end that worker "no answer" as a visible halt with retry; with 2 both stream. Record what happened, including a model swap if the two workers use different models. Same file: ask both to edit one file; approve the first; the second reports drift and writes nothing. Cancel one worker mid-staging: its orphan shows without touching the other's batch.
- M9 (W3, after PR13): Ask for a change and make the coordinator skip `authorize` (a model that spawns first, or a prompt that tells it to). The `code` spawn is refused with a line naming the spawn gate and the open step; the run does not pause and no failure alert opens. Let it reach the turn cap if you can: the stop line names the gate and the cap. Press Enter on the empty composer with `Enter gates off` showing: the transcript names the gate lifted and the spawn now starts. With the provider notice or a failure alert up, Enter answers THOSE first. Paste `Enter gates off` as chat text and as worker output: nothing lifts. Toggle ORCH off and on, change folder: the line is gone.
- M10 (W4, after PR14): Run a `code` worker that writes one of two files you named, or ask the coordinator to hand off claiming both. The `handoff` ack, a second `collect` and the `close` report show `claimed, not in ledger: <file>`; the verdict is shown beside it. Re-save the handoff without the file: the flag stays. A handoff naming no file says `not checked`, never `ok`.

## Review Workload Forecast (milestone 2)

Basis: design file-change figures for W10a, W10b, W3 and W4 plus about 30% (milestone 1 landed near its design figures), adjusted for moving the writer flip from W10a to W10b. Production lines only; tests, fixtures and snapshots are excluded.

| PR | Slice | Scope | Design estimate | Forecast (production) | 400-line risk |
|---|---|---|---|---|---|
| PR11 | W10a | `StageKey::Worker`, per-pane close/badge/review, claim by pane, re-propose after a denied ask | 200-320 (about 30 move to PR12) | 220-380 | Medium (split at the commit boundary if over 400) |
| PR12 | W10b | writers, lenses and corrections on the worker's key, per-worker disclosure, local-engine notice | 90-160 (plus about 30 moved) | 155-250 | Low |
| PR13 | W3 | spawn and finish gates, turn-cap text, `Enter gates off` | 180-300 | 235-390 | Medium (split at the commit boundary if over 400) |
| PR14 | W4 | claim check against the ledger, stored and shown | 110-190 | 145-250 | Low |

Totals: about 755-1,270 production lines across the milestone (design: 380-670 for these four slices as written).

Estimated changed lines (production): 755-1,270 in total; no single PR is forecast above 400, but the high ends of PR11 and PR13 sit within 20 lines of it.
400-line budget risk: Medium
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
Decision needed before apply: No

Reason for "No": the owner already chose the cut (one PR per slice, order W10a, W10b, W3, W4) and the chain (stacked-to-main), and nothing is forecast above 400. It becomes Yes only if PR11 or PR13 passes 400 at apply time; the cut points are named in each PR (PR11: re-propose commit; PR13: override commit).

Proposed PR boundaries (stacked-to-main, merge in this order): PR11 -> PR12 -> PR13 -> PR14. Each PR starts from main after the previous one merged (until PR #613 merges, PR11 branches from `feat/orch-write-grant`), finishes with its tests green and its manual step recorded, and reverts alone. Suggested branches: `feat/orch-stage-keys` (PR11), `feat/orch-parallel-writers` (PR12), `feat/orch-step-gates` (PR13), `feat/orch-handoff-check` (PR14). PR10 stays held on its manual gate and may open at any time.

Dependencies that matter: PR12 needs PR11 (the key and the pane operations); the writer flip and the proposer mapping must land together (PR12) or reviews go blind; the `cfg_attr` of 11.1 is removed in PR12; the gate line of PR13 shows `Enter gates off` only in the same PR as the key.

## Work-unit commit plan, milestone 2

Tests travel with the behaviour they verify. Suggested messages: `refactor(stage): key a staged batch by pane or worker` and `fix(stage): a worker can propose again after its ask was denied` (PR11); `feat(orchestra): each code worker stages and reviews in its own batch` and `feat(orchestra): say when two workers share a local engine` (PR12); `feat(orchestra): gate worker spawn and finish on the ODD steps` and `feat(orchestra): Enter lifts a gate for the user` (PR13); `feat(orchestra): check a handoff's claimed files against the ledger` (PR14); plus one `docs(openspec): tick PRn` commit each.
