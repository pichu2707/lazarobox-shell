# Tasks: coding-tools (search, write, run and git tools)

Inputs: `proposal.md` (#3330), `specs/` (11 capability files, #3332), `design.md` (#3333, with "Resolved by the user (2026-10-22)"), decisions #3331 and #3334, and `agent-orchestrator/tasks.md` (S1 chain of 12 PRs).

Mode: STRICT TDD (`cargo test`). Every task is RED (failing test) -> GREEN (smallest code) -> REFACTOR. One task = one work-unit commit (target 150-250 real lines, never above 300), because the 600-line stop only works when measured per commit. Estimates are REAL lines (tests included; tests are about 60-70% of a diff). Conventional commits, no AI attribution.

## Review Workload Forecast

| Field | Value |
|---|---|
| Total estimated changed lines | ~11,950 real (excluding `.snap` files and `openspec/`) |
| PR count | 30 (2 direct to main, 28 chained on the tracker) |
| Chained PRs recommended | **Yes** |
| 400-line budget risk | **High** (about 20 of 30 PRs estimate above 400; real data runs 2.4x-3x naive) |
| Decision needed before apply | **Yes** (see "Decisions needed") |
| Delivery strategy | ask-on-risk, chain strategy feature-branch-chain |
| Tracker branch | `feat/coding-tools` |
| Hard stop | 600 lines per PR, measured per commit |

### Decisions needed before apply

1. **Size exceptions.** Likely `size:exception: approved` PRs: W-e4 (review view, ~600 plus ~12 snapshots), and possibly T (~450, churns ~10 menu test files and ~24 snapshots), P0a (~450), W-e2 (~450), X-c3 (~450), G-d (~450). Ask the user at apply time for each one that crosses 500 real lines. Do not pre-approve.
2. **Release vs. feature-branch-chain.** Under feature-branch-chain only the tracker merges to main, but the design ships milestone releases (0.19 P0+T+M1, 0.20 M2, 0.21 M3, 0.22 M4). Proposal: merge the tracker to main at each milestone boundary (after M1-c, after M5-a, after X-d2, after M5-b) and rebase the remaining chain on main. The user must confirm this, or choose one big tracker merge.
3. **Docs commits on the tracker.** `feat/coding-tools` already holds three `docs(openspec)` commits (fd54f63, ee4fe29, 8ba5e34) that P0 (cut from main) does not have. Proposal: they travel with the first tracker merge (after M1-c). Confirm.
4. **M1-c may touch `host.rs`.** The design says M1 avoids `host.rs`, but `Call::Glob/Grep` need a dispatch arm. M1-c must be implemented through a seam outside `host.rs` if possible; if a one-arm edit in `host.rs` is unavoidable, it must be tiny and is a rebase point against S1c-2. Verify at apply time (task M1c.0 is a spike).

### PR list and bases (feature-branch-chain)

Tracker = `feat/coding-tools`. Rule: PR1 of the chain targets the tracker; each later child PR targets the immediate previous PR branch. P0 is the exception the user decided: it targets `main` directly. After P0b merges, the tracker is rebased on `main`.

| # | PR | Est. real | Base (target) | Status |
|---|---|---|---|---|
| 1 | P0a registry + golden | ~450 | `main` | Ready |
| 2 | P0b FamilySet plumbing at LEGACY | ~350 | `main` (branch off P0a until P0a merges, then retarget) | Ready |
| 3 | T toggles + `max_rounds` | ~450 | `feat/coding-tools` (tracker, rebased on main) | Ready |
| 4 | M1-a glob core | ~400 | T branch | Ready |
| 5 | M1-b grep + paging + `.git/config` deny | ~450 | M1-a branch | Ready |
| 6 | M1-c search wiring + approval | ~350 | M1-b branch | Ready (see decision 4) |
| 7 | W-a content rules + `resolve_write` | ~350 | M1-c branch | Ready |
| 8 | W-b write primitives (`open.rs`) | ~350 | W-a branch | Ready |
| 9 | W-c stage state machine | ~400 | W-b branch | Ready |
| 10 | W-d edit + diff | ~400 | W-c branch | Ready |
| 11 | W-e1 ReviewDoc + layout + coverage core | ~400 | W-d branch | Ready |
| 12 | W-e2 apply (journal, rollback, `WriteOps`) | ~450 | W-e1 branch | Ready |
| 13 | W-e3 `Stages` store + `StagedView` | ~350 | W-e2 branch | Ready |
| 14 | W-e4 review view (app + ui) | ~600 | W-e3 branch | Ready, likely size:exception |
| 15 | X-a allow-list | ~350 | W-e4 branch | Ready |
| 16 | X-b `Capture` + `RunOutcome` | ~350 | X-a branch | Ready |
| 17 | X-c1 toolchain `JailSpec` (pure) | ~450 | X-b branch | Ready |
| 18 | X-c2 helper `--rwx/--net-bind/--rlimits` | ~400 | X-c1 branch | Ready |
| 19 | X-c3 runner (spawn core, `GroupKill`, env scrub) | ~450 | X-c2 branch | Ready |
| 20 | G-a git argv + porcelain parse (pure) | ~350 | X-c3 branch | Ready |
| 21 | G-b git runtime (git jail, preflight) | ~350 | G-a branch | Ready |
| 22 | W-f host staging tools | ~450 | G-b branch | BLOCKED by S1c-2 on main |
| 23 | W-g runtime chat: discard on close, orphan effects | ~400 | W-f branch | BLOCKED by S1c-2 |
| 24 | W-h badge, Ctrl+G, quit notice | ~450 | W-g branch | BLOCKED by S1c-2 |
| 25 | M5-a docs for search + write (0.19/0.20) | ~300 | W-h branch | Ready after #24 |
| 26 | X-d1 host `run_command`, `RunGate`, refusals | ~450 | M5-a branch | BLOCKED by S1c-2 |
| 27 | X-d2 Exec ask, notice, app wiring | ~400 | X-d1 branch | BLOCKED by S1c-2 |
| 28 | G-c host git read verbs + GitDiff ask | ~300 | X-d2 branch | BLOCKED by S1c-2 |
| 29 | G-d git add/commit + CommitReview | ~450 | G-c branch | BLOCKED by S1c-2 |
| 30 | M5-b docs for run + git, threat model, CHANGELOG | ~300 | G-d branch | Ready after #29 |

Likely `size:exception`: W-e4 (high), then T, P0a, W-e2, X-c3, G-d, X-c1, W-f, W-h, X-d1 (medium; decide with real numbers). Every other PR is expected between 300 and 450 and should be split further if a work-unit commit crosses 300.

Blocked-slice rule: the BLOCKED rows cannot start until agent-orchestrator S1c-2 (PR 5 of its chain) is on `main` together with the S1 tracker (user decision, S1c-2 is not merged early). They sit at the end of the chain so everything above can proceed now. All 7 BLOCKED PRs edit `runtime/tools/host.rs` (execute path), which S1c-2 changes (`TurnScope`, execute filter, scope check, ask label).

### Rebase points against agent-orchestrator

| Point | Files | Who rebases | Action |
|---|---|---|---|
| P0a/P0b land first | `core/ai/tools/mod.rs`, `host.rs:293` | agent-orchestrator S1c-1 (PR #279-281 chain) | P0 on main with a stable public API. S1c-1 maps `registry_for(Toolset)` to `registry_with(.., toolset.families())`. |
| P0b vs S1c-1 | `runtime/mcp/session.rs` (generic `Hub<K: HubKey>`) | S1c-1 rebases onto P0b | P0b only adds a `FamilySet` argument at LEGACY; keep the diff in session.rs to the minimum. |
| P0b vs S1d-x | `runtime/chat.rs:716`, `chat/adapters.rs:287` (`Chats::with_orchestra`) | S1d-3 rebases | Same, minimum diff. |
| T vs S1a/S1e | `core/menu/*`, `app/*` (ORCH row, Ctrl+O) | whoever lands second | T adds 5 rows: re-count the menu row tests and snapshots after the S1 tracker merges. |
| W-f..h, X-d, G-c/d | `host.rs`, `runtime/chat.rs` | coding-tools chain | Rebase the whole remaining chain on main once S1c-2 and S1 tracker are merged. |

## Phase P0 (direct to main)

### P0a: registry and golden (PR 1, ~450, base `main`)
Spec: agent-tools "Tool registry", "Multi-argument arguments", "Argument length", "Off equals 0.18.0".

- [x] P0a.1 RED/GREEN-on-old-code (characterization): capture the 0.18.0 `tools/list` bytes and the parse-error texts of the 4 tools as golden fixtures; the test passes on the UNREFACTORED code and is committed FIRST in its own commit. Files: `tests/` or `core/ai/tools/tests`, fixture file. ~90.
- [x] P0a.2 RED: `ArgDef` table tests: fixed parse order (unknown -> not an object -> per-arg missing/wrong type -> extra key -> too long), int clamping (`timeout_secs` 900 -> 600), gated arg accepted only when its family is on. GREEN: `registry.rs` with `ToolDef`, `ArgDef`, `ArgKind`, `Family` (incl. `GitWrite`). ~200. Dep: P0a.1.
- [x] P0a.3 RED: golden still green via the new table for the 4 existing tools (byte-identical). GREEN: migrate `tools/mod.rs` tuples to `ToolDef`, keep `ToolSpec`, `registry()`, `parse_call()`, `parse_call_with()`, `Call::name()`, `Call::argument()` signatures. REFACTOR. ~120. Dep: P0a.2.
- [x] P0a.4 RED: `Call::ReadFile{path, lines: Option<Lines>}` parses with `lines: None`. GREEN: change `host.rs:293` to read the new field (no behaviour change). ~40. Dep: P0a.3.
- [x] P0a.5 RED: raw duplicate-JSON-key check test (open question 3, Ollama `stream.rs:328`); GREEN: add the check. Include only if it fits in the PR; otherwise move to P0b. ~40.

### P0b: FamilySet plumbing at LEGACY (PR 2, ~350, base `main`)
Spec: agent-tools "Tool families", "Off equals 0.18.0"; lazarobox-mcp-server "Tools served".

- [x] P0b.1 RED: `FamilySet` bitset (Copy), `LEGACY` = {Read, Pane}, `registry_with(external, set)`, `parse_call_in(name, args, ext, set)`; a built-in outside the set returns `UnknownTool` with the same text as an unknown name. GREEN in `registry.rs`. `registry()`/`parse_call_with()` delegate at LEGACY. ~130. Dep: P0a.
- [x] P0b.2 RED: `ChatRequest.families` default LEGACY; Ollama stream, MCP server and `adapters.rs:287` gate parse by the set (a hand-called unoffered tool is refused). GREEN: plumbing in `mcp.rs`, `chat/stream.rs`, `chat/adapters.rs`. ~130. Dep: P0b.1.
- [x] P0b.3 RED: `runtime/mcp/session.rs` and `runtime/chat.rs:716` pass the set; the golden `tools/list` stays green with all toggles off. GREEN + REFACTOR. Minimum diff in these two files (rebase points). ~90. Dep: P0b.2.

## Phase T (toggles, first tracker PR)

### T: toggle plumbing (PR 3, ~450, base tracker)
Spec: configuration "Family toggle keys"; config-persistence "Family toggle keys are owned"; config-menu "Family toggle rows"; chat-engines "Ollama round limit with coding tools".

- [ ] T.1 RED: `[ai.tools]` booleans (search, write, exec, git, git_write) default false, older builds ignore them; unknown keys preserved. GREEN: `core/ai/config.rs`. ~100.
- [ ] T.2 RED: `config_edit.rs` writes/keeps only the owned keys without touching others. GREEN. ~100. Dep: T.1.
- [ ] T.3 RED: `FamilySet` from config; app passes it into `ChatRequest.families`; all off equals LEGACY. GREEN: `core/ai/config.rs` + app. ~90. Dep: T.1, P0b.
- [ ] T.4 RED: `max_rounds(set)` is 24 when Search/Write/Exec/Git/GitWrite is on, else 8. GREEN: `runtime/chat/ollama.rs`. ~50.
- [ ] T.5 RED: 5 menu rows (names, order, toggle keys); update the ~10 menu test files and ~24 insta snapshots (budget this). GREEN: `core/menu/*`. ~110. Dep: T.3. The Exec row also shows the "needs Linux sandbox" reason when no jail (D18) if the jail check is already available; otherwise add it in X-d2.
- [ ] T.6 RED: with an EMPTY set and no external tools, `ollama_round_body` (`chat/adapters.rs` ~316) omits the `tools` key instead of sending `"tools": []`. GREEN: skip the key when the registry is empty. ~20. This came from the P0b review: an EMPTY set cannot happen until the toggles exist.

## Phase M1: search (0.19)

### M1-a: glob core (PR 4, ~400)
Spec: code-search "glob", "Family and toggle".
- [ ] M1a.1 RED: glob -> anchored regex table (`*`, `**`, `?`, classes, anchoring, bad patterns). GREEN: `tools/glob.rs`, `Cargo.toml` (+regex). ~200.
- [ ] M1a.2 RED: `Call::Glob` args parse (ArgDef, caps, family gate Search). GREEN: `registry.rs`/`mod.rs`. ~100. Dep: M1a.1, P0b.
- [ ] M1a.3 RED: result ordering, 100-match cap, truncation text. GREEN. ~100.

### M1-b: grep, paging, `.git/config` deny (PR 5, ~450)
Spec: code-search "grep", "read_file paging"; code-write "Deny rules" (read side).
- [ ] M1b.1 RED: `read_file{start_line, max_lines}` gated under Search; schema byte-identical with Search off (golden). GREEN: `fs.rs` paging. ~150. Dep: M1a.
- [ ] M1b.2 RED: reads of a file named `config` or `config.worktree` under `.git` are refused with `path not available`. GREEN: `fs.rs`/`paths.rs`. ~80.
- [ ] M1b.3 RED: grep over the budgeted walker (100 matches or 20 KiB, secret lists skipped, binary skipped, redacted). GREEN: `runtime/tools/search.rs`. ~220.

### M1-c: wiring and approval (PR 6, ~350)
Spec: code-search "Family and toggle", "Staged view"(marker only, content comes in W-e3); agent-tools "Approval model".
- [ ] M1c.0 SPIKE (no code): find the dispatch seam for `Call::Glob/Grep` that avoids `host.rs`; if impossible, record the one-arm edit as a rebase point (decision 4).
- [ ] M1c.1 RED: `ApprovalAsk::Search`: prompts on Cloud, free on Local (same rule as `read_file`), `Prompt` in every mode (no AutoRun/AutoType). GREEN: `chat/approval.rs`. ~120.
- [ ] M1c.2 RED: app renders the ask, executes glob/grep, returns results. GREEN: `app/tool.rs`, `app/run.rs`. ~150. Dep: M1c.1, M1b.
- [ ] M1c.3 RED: end-to-end with Search off equals 0.18.0 (golden); Search on adds `glob`, `grep`. REFACTOR. ~80.

Release point 0.19: P0 + T + M1 (see decision 2).

## Phase M2: write (0.20)

### W-a: content rules and `resolve_write` (PR 7, ~350)
Spec: code-write "Deny rules", "Root policy", "Caps".
- [ ] Wa.1 RED: content refusal table (C0 except `\t`/`\n`, DEL, C1, `\r`, the fixed Unicode list including ZWJ); existing file must be UTF-8 with no CR/NUL; no newline added. GREEN: `tools/content.rs`. ~170.
- [ ] Wa.2 RED: `resolve_write`: root is HOME / ancestor of HOME / system dir / missing / outside HOME refused; `.git`/`node_modules` components and SECRET lists refused; one text `path not available`; tier-B warnings (Makefile, build.rs, Cargo.toml, `.cargo/config*`, package.json, pyproject.toml, setup.py, conftest.py, go.mod, `*.sh`, `.github/workflows/*`). GREEN: `tools/paths.rs`. ~180.

### W-b: write primitives (PR 8, ~350)
Spec: code-write "Apply" (primitives), "Nothing writes without approval".
- [ ] Wb.1 RED (tempfile, Linux): open parent with openat2 `RESOLVE_BENEATH|NO_SYMLINKS`; reject symlink swap, nlink>1, non-regular. GREEN: `runtime/tools/open.rs`. ~180.
- [ ] Wb.2 RED: `mkdirat`, `O_EXCL` temp + fsync + `renameat2(NOREPLACE)` for create, `renameat` replace keeping base mode. GREEN. ~170.

### W-c: stage state machine (PR 9, ~400)
Spec: code-write "create_dir and write_file staging", "Propose batch and size cap", "Orphan, close and quit".
- [ ] Wc.1 RED: `Staging -> Proposed(gen) -> Applied|Discarded`; staging refused while Proposed; stale gen refused; Proposed-orphan vs Staging-orphan. GREEN: `tools/stage.rs` with `StageKey::Pane`, `BatchId`. ~200.
- [ ] Wc.2 RED: caps at stage time (50 entries, 1 MiB) with "batch is full: propose it, then stage the rest", and re-check at propose; `write_file` 32 KiB. GREEN. ~120.
- [ ] Wc.3 RED: `write_file` on an existing file only when <= 64 KiB and `redact(clean(text)) == text`, else "use edit_file". GREEN. ~80.

### W-d: edit and diff (PR 10, ~400)
Spec: code-write "edit_file staging", "Batch review and coverage gate".
- [ ] Wd.1 RED: `edit_file`: `old` non-empty and unique in staged-or-disk text, fields <= 16 KiB, file <= 1 MiB. GREEN: `tools/edit.rs`. ~170.
- [ ] Wd.2 RED: diff with `similar` (Myers, 500 ms deadline, 3 context lines), create shows all added, `no newline at end of file`. GREEN: `tools/diff.rs`, `Cargo.toml`. ~230.

### W-e1: ReviewDoc and coverage core (PR 11, ~400)
Spec: code-write "Batch review and coverage gate"; chat-pane "Batch review view".
- [ ] We1.1 RED: `ReviewDoc` (headers, warnings, hunk lines), invisible chars in context lines shown as `<U+XXXX>`. GREEN: `core/ai/chat/review.rs`. ~170.
- [ ] We1.2 RED: single `layout(doc, width)`; a logical line is seen only when all its wrapped rows were shown; coverage after resize; `y` enabled only when all seen AND 800 ms elapsed. GREEN (pure, injectable clock). ~230.

### W-e2: apply (PR 12, ~450)
Spec: code-write "Apply" (preflight, drift, rollback, partial).
- [ ] We2.1 RED: `WriteOps` trait + fault injector; preflight drift refuses the whole batch (`Drifted`). GREEN: `runtime/tools/apply.rs`. ~200. Dep: Wb.
- [ ] We2.2 RED: journaled ordered apply (dirs by depth, then files), fault at step k rolls back in reverse. GREEN. ~150.
- [ ] We2.3 RED: failing undo returns `Partial{changed}`; success returns `Vec<Change>` for S7. GREEN. ~100.

### W-e3: Stages store and StagedView (PR 13, ~350)
Spec: code-search "Staged view"; code-write "Orphan, close and quit".
- [ ] We3.1 RED: runtime `Stages` store keyed by `StageKey` (memory only, one batch per key), seal/apply/discard by `BatchId`. GREEN: `runtime/tools/stage.rs`. ~170. Dep: Wc, We2.
- [ ] We3.2 RED: `StagedView` snapshot overlay for `read_file`, `glob`, `grep`, `list_dir`, with the `(pending, not on disk)` marker. GREEN. ~180. Dep: M1.

### W-e4: review view (PR 14, ~600, likely size:exception)
Spec: chat-pane "Batch review view"; code-write "Batch review and coverage gate".
- [ ] We4.1 RED: `ApprovalAsk::Batch(Arc<BatchReview>)` routes `Prompt` in every mode; `n`/Esc rejects, `y` locked until coverage + 800 ms. GREEN: `chat/approval.rs`, `app/review.rs`. ~220. Dep: We1.
- [ ] We4.2 RED (TestBackend + insta, fake clock): full-pane view with warnings, gate counter. GREEN: `ui/components/review.rs`. ~250 plus snapshots.
- [ ] We4.3 RED: the other asks keep `ARM_DELAY` 400 ms. REFACTOR. ~130.
- Cut line if it passes 500: put We4.3 and the snapshots in a follow-up commit and ask for the exception before opening the PR.

### W-f: host staging tools (PR 22, ~450, BLOCKED by S1c-2)
Spec: code-write "create_dir and write_file staging", "edit_file staging", "Propose batch and size cap", "Nothing writes without approval"; lazarobox-mcp-server "Coding tools run in the app".
- [ ] Wf.1 RED: `Call::{CreateDir, WriteFile, EditFile, ProposeBatch}` parse (family Write gate). ~100.
- [ ] Wf.2 RED: `host.rs` stages without ask or disk touch; `ToolEnv` carries `StagedView`; `propose_batch` seals and asks. ~220.
- [ ] Wf.3 RED: all-toggles-off golden stays green; a Write tool call with Write off is `UnknownTool`. ~130. Host re-checks the family as defense in depth.

### W-g: runtime chat lifecycle (PR 23, ~400, BLOCKED by S1c-2)
Spec: code-write "Orphan, close and quit", "Apply"; chat-pane "Closing discards a batch".
- [ ] Wg.1 RED: closing a pane calls `Stages.discard(key)` with a notice. ~130.
- [ ] Wg.2 RED: turn cancelled or CLI timeout leaves Proposed-orphan; unproposed batch at turn end is Staging-orphan; `Effect::BatchReview/Apply/Discard` routed via Chats. ~170.
- [ ] Wg.3 RED: the model learns of a user-side apply/discard through a system line on its next turn. ~100.

### W-h: badge, Ctrl+G, quit notice (PR 24, ~450, BLOCKED by S1c-2)
Spec: chat-pane "Staged batch indicator"; code-write "Orphan, close and quit".
- [ ] Wh.1 RED: pane badge for staged/orphan batches (`ui/components/chat.rs`, insta). ~200.
- [ ] Wh.2 RED: Ctrl+G opens the orphan review; check against the chat key table for conflicts (with Ctrl+O from S1e-1a). ~120.
- [ ] Wh.3 RED: quit discards batches with a notice (`app/notes.rs`). ~130.

Go/no-go checkpoint on review UX after W-h (design rollout).

### Deferred (outside this chain)
- Worker batches (code-write "Worker batches", D20): `StageKey::Worker{..}`, FIFO ask label `worker n/m kind engine>model`, unproposed worker batches discarded and reported in `WorkerResult`. Depends on agent-orchestrator S3 (multi-worker FIFO) and S7. Not planned here; add it as a task in S7. `StageKey` is designed to grow.

## Phase M3: run (0.21)

### X-a: allow-list (PR 15, ~350)
Spec: command-runner "Argv input and allow-list", "Refusals before start".
- [ ] Xa.1 RED: table tests per program/verb: cargo, npm/pnpm/yarn, pytest, go; denied flags (`--config`, `-Z`, `--manifest-path`, `--target-dir`, `-g`, `-modfile`, ...). GREEN: `tools/allow.rs`. ~220.
- [ ] Xa.2 RED: absolute path or `..` in any arg refused; `cwd` outside root refused; `run_command` args parse (`timeout_secs` clamp 120 default/600 max). GREEN. ~130.

### X-b: Capture and RunOutcome (PR 16, ~350)
Spec: command-runner "Output capture and RunOutcome".
- [ ] Xb.1 RED: 4 KiB head + 28 KiB tail ring with `[... N bytes omitted ...]`. GREEN: `tools/run.rs`. ~170.
- [ ] Xb.2 RED: `RunOutcome` (exit, signal, timed_out, cancelled, truncated, total_bytes, duration, digest <= 2 KiB tail); lossy UTF-8, `clean`, `redact`, `label::wrap`. GREEN. ~180.

### X-c1: toolchain JailSpec (pure) (PR 17, ~450)
Spec: command-runner "Jail profile".
- [ ] Xc1.1 RED: `core/ai/toolchain.rs` builds rx/rwx/rw/never classes from a fake FS and canonical program path (cargo, rustup, nvm/volta/fnm, GOROOT, pytest venv rule). ~250.
- [ ] Xc1.2 RED: `JailSpec` gains `rwx` in `core/ai/jail.rs`; invariant R29 amended: Execute only in `rx` or the single `rwx` root. ~120.
- [ ] Xc1.3 RED: env scrub table (passed, fixed, never-passed vars; PATH relative/under-root entries removed). ~80.

### X-c2: helper flags (PR 18, ~400)
- [ ] Xc2.1 RED (skip when ABI < 3): `--rwx` lets exec work in the root; `~/.ssh` unreadable. GREEN: `runtime/jail.rs`. ~200.
- [ ] Xc2.2 RED: `--net-bind open` (listen allowed), `--rlimits toolchain` (CORE 0, FSIZE 4 GiB, CPU 1200 s) set before exec. GREEN. ~200.
- Check the pathname UNIX socket residual risk against the kernel docs here and record it (accepted risk; seccomp is follow-up).

### X-c3: runner (PR 19, ~450)
Spec: command-runner "Process rules".
- [ ] Xc3.1 RED (`/bin/sh` scripts below the allow-list): spawn core on `std::io::pipe`, shared stdout/stderr, stdin `/dev/null`, `process_group(0)` + `GroupKill`. GREEN: `runtime/tools/runner.rs`. ~220.
- [ ] Xc3.2 RED: timeout, cancel-by-drop, group kill of grandchildren, normal exit `kill_and_reap` then 500 ms drain. ~140.
- [ ] Xc3.3 RED: env scrub and private TMPDIR removed after run, ring truncation integration. ~90.

### X-d1: host `run_command` (PR 26, ~450, BLOCKED by S1c-2)
Spec: command-runner "Refusals before start", "Process rules"; agent-tools "Approval model".
- [ ] Xd1.1 RED: refused with no jail ("commands need the Linux sandbox (Landlock ABI 3+)"); refused while the key has a staged/proposed batch ("propose them first"). ~150.
- [ ] Xd1.2 RED: `RunGate`: 1 per `StageKey` ("a command is already running in this chat") and global `Semaphore(2)` with a 60 s wait cap, taken after `y`, before spawn. ~170.
- [ ] Xd1.3 RED: `host.rs` wiring, Exec family gate, golden stays green. ~130.

### X-d2: Exec ask and notice (PR 27, ~400, BLOCKED by S1c-2)
- [ ] Xd2.1 RED: `ApprovalAsk::Exec` prompts every command in every mode; the exfiltration notice about `.env`; tier-B warnings shown. ~200.
- [ ] Xd2.2 RED: app/menu row shows the "no jail" reason; global-wait shown on the row. ~200 (menu snapshots).

## Phase M4: git (0.22)

### G-a: git argv and parse (pure) (PR 20, ~350)
Spec: git-tools "Verb boundary", "Hardened execution", "Read verbs".
- [ ] Ga.1 RED: argv templates for status/diff/log/add/commit with the fixed `-c` hardening, `:(exclude,glob)` pathspecs from SECRET_FILES, `:(literal)` for add. GREEN: `tools/git.rs`. ~200.
- [ ] Ga.2 RED: porcelain v2 `-z` parse, log `%x1f` parse, `max_count` <= 50. ~150.

### G-b: git runtime (PR 21, ~350)
- [ ] Gb.1 RED: git `JailSpec` (run spec without caches, ro `~/.gitconfig` and XDG git files, rw on `--absolute-git-dir` and `--git-common-dir` via a jailed read-only `rev-parse` preflight; linked worktrees). GREEN: `runtime/tools/git.rs`. ~200. Dep: X-c3.
- [ ] Gb.2 RED (temp repo, skipped without git): a planted `core.fsmonitor`/`diff.external` must NOT create its marker file; env `GIT_CONFIG_NOSYSTEM`, `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`. ~150.

### G-c: host git read verbs (PR 28, ~300, BLOCKED by S1c-2)
- [ ] Gc.1 RED: `git_status`/`git_log` no ask; `git_diff` prompts on Cloud, free on Local; refused while a batch is staged. ~150.
- [ ] Gc.2 RED: `ApprovalAsk::GitDiff`, host wiring, Git family gate. ~150.

### G-d: add and commit (PR 29, ~450, BLOCKED by S1c-2)
Spec: git-tools "add", "commit", "Two toggles".
- [ ] Gd.1 RED: `git_add` refuses secret names; one-line overlay (paths + diffstat) or the review view when it does not fit; GitWrite gate. ~170.
- [ ] Gd.2 RED: `git_commit`: `CommitReview` (message, full `--cached` diff, notice "signing disabled for this commit"), `-c commit.gpgsign=false`, no AI trailer, refused when the index holds a secret path, hooks run in the jail. ~200.
- [ ] Gd.3 RED: end-to-end in a temp repo: the commit is refused on a staged `.env`. ~80.

## Phase M5: docs

- [ ] M5a (PR 25, ~300) README sections for search, write, toggles, the review keys (Ctrl+G) and a threat-model section for staging/apply. Docs only; verify with `cargo test` (doc tests) and a link check. CHANGELOG entries for 0.19 and 0.20.
- [ ] M5b (PR 30, ~300) README for run and git, the threat model (allow-list bounds the verb, network on, `.env` exfiltration, UNIX socket residual risk, daemonizing grandchildren), CHANGELOG for 0.21 and 0.22.

## Closing tasks (after the chain is merged)

- [ ] Z.1 Archive `agent-tools` FIRST (its base spec lives only in the unarchived `changes/agent-tools/`), then archive `coding-tools`. Order is mandatory (user decision).
- [ ] Z.2 Repoint rows 5 and 7 of the agent-orchestrator design and S7/S8/S10 dependencies in its `tasks.md` from "write-tools SDD"/"test-runner SDD" to `coding-tools` (S7 needs M2, S8 needs M3). Edit in the orchestrator's own change, not here.
- [ ] Z.3 Confirm the `Evidence.digest` meaning (cleaned tail text) with the orchestrator owner.

## Dependency summary

```
P0a -> P0b -> (tracker rebase on main) -> T -> M1-a -> M1-b -> M1-c
 -> W-a -> W-b -> W-c -> W-d -> W-e1 -> W-e2 -> W-e3 -> W-e4
 -> X-a -> X-b -> X-c1 -> X-c2 -> X-c3 -> G-a -> G-b
 -> [S1c-2 + S1 tracker on main] -> W-f -> W-g -> W-h -> M5-a
 -> X-d1 -> X-d2 -> G-c -> G-d -> M5-b -> Z.1..Z.3
```

The chain is linear (feature-branch-chain). Pure cores are independent by file, so reorder inside the unblocked block is safe if one PR stalls (for example W-a..e1 and X-a..c1 share no files), but then re-base the following branches.

## Risks

- Real sizes: 20 of 30 PRs are at or above 400; per-commit measurement is the only reliable stop.
- 7 PRs are blocked on S1c-2 reaching main with the S1 tracker; if that slips, M2/M3/M4 cannot ship any user-visible behaviour (they stay dark).
- `host.rs`, `runtime/chat.rs`, `session.rs`, menu files are shared with the orchestrator chain: expect repeated rebases.
- A 30-branch linear chain amplifies a rebase on any early change (review fixes in P0 or T ripple through all).

## Decisions resolved by the user (2026-10-22)

1. **Size exceptions:** decided per PR at apply time, using real numbers. Nothing is pre-approved.
2. **Release cadence:** the tracker `feat/coding-tools` merges to `main` at each milestone boundary (M1 → 0.19, and so on). It is not one big merge at the end.
3. **Openspec docs commits:** they travel with the first tracker merge, the same as the agent-orchestrator planning commit.
4. **M1-c and `host.rs`:** spike M1c.0 is accepted. If a one-arm `host.rs` edit is unavoidable, it becomes a rebase point against S1c-2.
