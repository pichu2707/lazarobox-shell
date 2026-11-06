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

- [x] 10.1 MANUAL: run M6 (below) on a real llama-server and report pass or fail in the PR or the change.
- [x] 10.2 RED (only after a pass): test that `tools_for("llamacpp", true, Some(origin), MCP_VERIFIED)` returns `Tools::On` and that a worker on llamacpp now gets a tool session (`open_session` takes the `Adapter::Llamacpp` branch at ~1615); the existing llamacpp pin test is updated, not deleted.
- [x] 10.3 GREEN: add `"llamacpp"` to `MCP_VERIFIED`; update the comment in `runtime/chat.rs`.
- [x] 10.4 REFACTOR: none expected. Check `picker.rs:290` presentation (llamacpp now listed as tool-capable) with its snapshot.
- Done when: manual M6 recorded; `cargo test` green; the acceptance note in the PR says "verified on <model>, <server version>".
- Depends on: PR2 (so a failure is visible) and PR7 (prompt) for a meaningful check. May run in parallel with PR8/PR9.
- As-built (2026-11-07): verified on Qwen2.5-7B-Instruct Q4_K_M, llama-server b11935 (`--jinja --parallel 1`). M6 passed: notice before, real tool calls after, `propose_batch` review applied, two parallel workers applied, `php -l` ok, D12 (wait under 120 s queued; over 120 s halts with "no answer from the engine", `r` retries). Qwen3.8-27B was not run. Qwen2.5-Coder-7B answers tool calls as text (model/template). Closes T3.0-T3.3 of `llamacpp-tools`. The `Degrade::ToolsOff` orchestra test now uses lmstudio instead of llamacpp.

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

W10a, W10b, W3, W4, W5, W6, W7, W8a, W8b, W8c, W9a, W9b (design order: W10a, W10b, W3, W4, W5, W6, W7, W8a-c, W9a-b). Open owner decisions 2-6, 8, 9 and D16 `/exec` belong to them. After this milestone, revisit sizes: if W1a/W1b identify the original trigger, later slices may shrink.

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
