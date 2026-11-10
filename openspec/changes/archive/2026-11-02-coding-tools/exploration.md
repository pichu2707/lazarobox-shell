# Exploration: coding-tools (write, run, search and git tools over the lazarobox MCP server)

Date 2026-10-21. Base: origin/main 8aeddf8 (v0.18.0 + README fix). Supersedes and absorbs openspec/changes/write-tools/exploration.md (2026-10-17) and the "test-runner SDD" that agent-orchestrator listed as separate.
Note: engram mem_search was not available to this executor; the orchestrator seams were read from the worktrees .claude/worktrees/agent-orchestrator*/openspec/changes/agent-orchestrator/{design,tasks}.md (S1b-2 state: S1c tasks 1c.* all unchecked, i.e. S1c has not started).

## 0. Executive summary
- The four families are feasible, but the work is about 4,400 naive lines (incl. tests) = ~10-11k real changed lines (x2.5 calibration) in ~24-28 PRs. Plan it as 5 milestones with a go/no-go after M2.
- Hard prerequisite: multi-argument tools (P0). Verified: BUILT_INS, one_string(), parse_call(), Call::argument(), MAX_ARG_BYTES=4096 are all single-string (src/core/ai/tools/mod.rs:123,151,205,99,195). Land it on main BEFORE orchestrator S1c-1 (not started), keeping the public API stable and adding a `Family` tag so Toolset::Code becomes a family filter.
- Biggest unknowns: (1) the approval UX for big diffs (full-pane review view with a coverage gate remains the recommendation), (2) the command runner's jail (the CLI jail forbids write+exec; tests need write+exec in the project, so a new `rwx` class and a toolchain profile are needed; macOS needs real-Mac verification), (3) approval fatigue vs the goal "build a project from chat" (owner rule: every write waits for y).
- File tools are Linux-only today (src/runtime/tools/open.rs:293 `other` module always returns Unsupported): read_file/list_dir do not work on macOS at all. Whole coding-tools stack is Linux-first unless a portable-open slice is added.

## 1. Current state (verified against code)
Registry and parsing (src/core/ai/tools/mod.rs)
- BUILT_INS: 4 tuples (name, ONE arg, description, Access) at :123: find_dir(query), list_dir(path), read_file(path), run_in_pane(command, Access::Write). one_string() builds the schema :151. registry() :165. parse_call() :205 requires one string, rejects extra keys, cap MAX_ARG_BYTES=4096 :195. parse_call_with() :241 routes external tools (cap MAX_EXTERNAL_ARGS 4 KiB :234, generic JSON, always Access::Write). Call enum :60; Call::name() :87; Call::argument() :99 feeds the transcript summary (host.rs:323).
- MCP codec: MAX_LINE_BYTES = 1 MiB (mcp.rs:40); duplicate JSON keys rejected only on the MCP raw-line path (mcp.rs:119,362). parse_call's doc (mod.rs:201) says any other entry point (Ollama) must do the same: verify during P0.
- Ollama builds tools from the same registry (adapters.rs:234-297, registry at :287); loop MAX_ROUNDS = 8 (runtime/chat/ollama.rs:36); MAX_CALLS = 16 per reply (core/ai/chat/stream.rs:277).
Host and approvals
- runtime/tools/host.rs: TurnScope :82 {events, locality, origin, approver, external}; execute() :274. It validates, builds the ask via ask_for (:286-298), registers the wait BEFORE announcing (:310), announces ToolCall{approval} (:320), and runs only after Decision::Sent (:360-418, run() at :406). The host does not know the approval mode.
- Free path: runtime/tools/mod.rs:68 run() = spawn_blocking + timeout(limits.time + GRACE) + label::wrap with a fresh nonce.
- core/ai/chat/approval.rs: ApprovalAsk {Run, Read, External} :61; ask_for :83 (Run always, Read on Cloud, External always; find/list never); Route {Prompt, AutoType, AutoRun} :168; MAX_AUTO_PER_TURN=5 :179; route() :285; Approvals holds ONE pending approval, a second request is refused with "another approval is pending" (:42,:465).
- app: on_tool_ask app/tool.rs:121 (ARM_DELAY 400 ms :28, armed_at :169); on_approval_key app/run.rs:112; approve :184 requires prompt_fits (:173); approval_prompt :419 builds {target, command, keys}.
- Overlay: core/ai/chat/approval_rows.rs MAX_COMMAND_ROWS = 6 (:15), full_rows :75, fits :120: a prompt is approvable only if ALL its text fits. External tool args are truncated at EXTERNAL_ARGS_SHOWN=1024 chars with a visible "(truncated: N of M chars hidden)" (approval.rs:106) and y is still allowed: a precedent, and proof that a user MCP filesystem server cannot be reviewed today.
- Full-pane views precedent inside a chat: list_open, memory_view_open, tag_editor_open (app/run.rs:93-96, app/chat.rs).
Read side
- runtime/tools/fs.rs: Limits (:44) read 64 KiB, list 200, depth 8, visited 20_000, time 2 s, find results 20; execute_with :168 matches Read/List/Find and returns NotReadTool for RunInPane/External (:186). open_checked :216.
- Paths (core/ai/tools/paths.rs): deny list under HOME (SECRET_DIRS :23, SECRET_FILES :38 incl .env*, .npmrc, .envrc, histories), resolve :328, resolve_exec :382 (Want::{Existing,Home,New}), canonical_new :404 (already resolves a not-yet-existing path), is_system_dir :196. NOTE: `.git` is NOT in the read deny list today (git config can hold credentials in remote URLs; redact() is the only guard).
- open.rs: Linux openat2(RESOLVE_BENEATH|NO_SYMLINKS) with O_NOFOLLOW walk fallback (:42,:139; mode hard-coded 0 at :142); fd_path via /proc/self/fd. Non-Linux: always Unsupported (:293).
- Results: label::wrap caps at OUTPUT_CAP 32 KiB and wraps as untrusted data with a per-call nonce (core/ai/tools/label.rs:4,19). Pane output capture is 8 KiB, head 100 / tail 100 lines (tools/capture.rs:25-29).
Run-in-pane (auto-execute change has LANDED)
- exec.rs: allow-list grammar for `run` mode: cd, pwd, ls, tree, mkdir only (SPECS :93, MAX_WORDS 8). Output comes from reading the pane screen (capture.rs), needs a shell pane, a known cwd (not on macOS) and an idle line. `cargo test` cannot be expressed. mkdir -p already runs directly in run mode, so create_dir in run mode is only a convenience (host-side, no pane needed).
Engines and wiring
- Server-wide wiring, no per-tool flags: claude --allowedTools=mcp__lazarobox (wiring.rs:130) with MCP_TOOL_TIMEOUT 900000 (:16,:136); codex tool_timeout_sec 900 (:152) ; gemini admin policy mcpName allow + timeout 900_000 (:230,:268). The engines themselves stay write-less: claude `--tools` empty (adapters.rs:376), codex `read-only` (:441), gemini deny-all policy. So only lazarobox writes. CLI turn total 15 min, idle 300 s (loopback.rs:70,84); pending approvals pause the timers (host.rs:304-309).
- origin: Tools::On{origin: Option<PathBuf>} = the cwd the chat was opened from (engine.rs:54-66). No origin falls back to "/" for relative reads (runtime/chat.rs:698).
Process and jail machinery (reusable)
- Jail: core/ai/jail.rs JailSpec {rx, ro, rw, ro_files, rw_files} :964; helper `lazarobox __jail-exec <rules> -- program argv` (SUBCOMMAND :27, JailArgs::render :1194); Landlock ABI >= 3 (MIN_ABI :17); runtime/jail.rs enforce :149 handles BindTcp only (outbound stays open, :163-168); rw rules explicitly EXCLUDE Execute (:190); Seatbelt: sbpl.rs RW class = file-read* file-write* with NO process-exec (:137), network-outbound open (TAIL :70), jail_spec_for refuses rx/rw overlap on macOS (jail.rs:1142-1159, "Seatbelt unions its rules"). SEATBELT_ENABLED=true (:44), but README says macOS is untested at runtime / unsupported and claude is never jailed on macOS.
- The jail is built for ONE CLI with an empty private cwd (CliLayout, jail.rs:920); there is no toolchain profile.
- Child-process precedent: runtime/mcp/client/spawn.rs (env_clear + BASE_ENV [PATH USER LANG LC_ALL TMPDIR] + HOME, token never passed, process_group(0), kill_on_drop, GroupKill in runtime/proc.rs:29).
Dependencies (Cargo.toml): tokio, ratatui, crossterm, portable-pty, libc, vt100, serde_json, toml, toml_edit, rusqlite (bundled), unicode-*, landlock =0.4.7 (linux). NO regex, ignore, globset, similar, gix, git2. tempfile is dev-only.
Specs: openspec/specs/ is empty (only .gitkeep); no prompts/ dir and no tools.md on this branch (the old exploration's tools.md lives on an unmerged branch): recheck at design time. README (Spanish) states the model cannot change files.

## 2. Check of the old write-tools exploration against today's code
Still valid: tool signatures, exact-string edit, create-only write_file (O_EXCL), coverage-gated review view, one-refusal-text oracle rule, tier A/B deny lists, caps (32 KiB write, 16 KiB per edit field, 1 MiB edited file), per-turn serialization gate, Linux-only primitives.
Drifted line refs: BUILT_INS mod.rs:95 -> :123; ask_for approval.rs:76 -> :83; route :117 -> :285; on_tool_ask app/tool.rs:110 -> :121; on_approval_key run.rs:108 -> :112; approve :180 -> :184; engine flags adapters.rs:225,288 -> :376 (claude --tools) and :441 (codex read-only); gemini policy wiring.rs:249 -> :242-268.
Changed facts:
1. auto-execute landed: Route has AutoRun and MAX_AUTO_PER_TURN=5; the W5 dependency is satisfied. A new Route::AutoApprove is still needed for create_dir.
2. resolve_exec/Want::New/canonical_new (paths.rs:382,404) already resolve non-existent targets: resolve_write can build on them instead of from scratch.
3. mkdir -p is already auto-runnable in run mode via run_in_pane (needs a shell pane and Linux cwd). create_dir stays useful for pane-less (worker) flows.
4. The "12 write prompts per turn" default conflicts with the new goal (build a project). Needs a product decision (Q1).
5. agent-orchestrator now fixes the seams (Toolset::Code, ChangeLedger, Evidence) and lists "write-tools SDD" (row 5) and "test-runner SDD" (row 7) as separate: both are now folded into coding-tools; the orchestrator design table needs an update.
6. Gap not covered before: delete/rename/move files; read_file has no offset/limit (64 KiB hard cap, fs.rs:66); .git readable.

## 3. Gaps per family
### A. Write/edit
- No write path exists: no open.rs write primitives, no resolve_write, no write executor, no diff/review code, no multi-arg tools.
- Approval overlay cannot show a file or diff (6 rows).
- Batched tool calls (claude) collide with the one-pending-approval rule and get denied "another approval is pending".
- No delete/rename. read_file cannot page past 64 KiB. macOS: no file primitives.
### B. Run commands/tests
- run_in_pane types into the user's pane; output is a screen scrape (8 KiB), requires an idle shell pane, and run mode only accepts cd/pwd/ls/tree/mkdir. There is no captured, non-pane runner, no timeouts/caps for child processes, no toolchain jail profile (the CLI jail forbids write+exec and is shaped around one CLI binary), no structured outcome (exit code, duration, truncation) for Evidence.
### C. Search
- find_dir finds DIRECTORIES only (fs.rs:423); list_dir is one level. There is no recursive file discovery (glob) and no content search (grep). No regex/glob crate.
### D. Git
- Nothing. External MCP servers (a user's git server) would always prompt with args truncated; no read-only git verbs, no env hardening, no commit flow.
### Cross-cutting
- One string argument per tool; Call::argument() summary assumes it.
- Per-family enable/disable: only a global [ai.tools] switch exists.

## 4. Prerequisite P0: multi-argument registry
Shape: replace the BUILT_INS tuple with a table of ToolDef {name, description, access, family, args: &[ArgDef{name, kind: Str|Int|Bool|StrList, required, cap}]}. Schema builder generates {"type":"object","properties":...,"required":[...],"additionalProperties":false}. parse_call validates per ArgDef (missing/wrong type/extra/too long), then a per-tool constructor builds the Call. ToolError keeps its closed set (add nothing user-visible unless needed). Call::argument() stays and returns a one-line summary (path for writes, never content).
`Family` tag: Read | Search | Write | Exec | Git | Pane. Orchestrator's registry_for(Toolset) becomes a family filter, not a name list: ReadOnly = {Read, Search}; Code = ReadOnly + {Write, Exec, GitRead}; never Pane, never External; Orchestrator = its own 4 tools.
Keep stable: registry(&[ExternalTool]), parse_call, parse_call_with, Call::name(), Call::argument(), MAX_ARG_BYTES semantics for the existing 4 tools (byte-identical tools/list for them; snapshot tests must stay green).
Blast radius (verified by reading the call sites): core/ai/tools/mod.rs (table, schema, parse, ~340 lines of tests assume one string), core/ai/tools/mcp.rs (tools/list, call() -> parse_call_with, tests :500-880), core/ai/chat/adapters.rs (ollama_round_body only consumes registry(); no change expected), runtime/tools/fs.rs execute_with (exhaustive match :181-186 gets new variants), runtime/tools/host.rs execute (matches :277,:345,:360-399), core/ai/chat/approval.rs ask_for (:83 exhaustive), app/tool.rs :131 and app/run.rs :189,:424 (match on ApprovalAsk), UI approval renderers and insta snapshots, runtime/mcp/session.rs (registry at :459).
Optional args in P0 (needed later): read_file{path, start_line?, max_lines?}; run_command timeout_secs (int); grep flags (bool/int).
Conflict with in-flight orchestrator chain: S1c-1 (not started; base = S1b-2) adds registry_for(Toolset) + Call::Orchestra in tools/mod.rs; S1c-2 touches host.rs TurnScope/execute; S1d touches runtime/chat.rs. A feature-branch-chain keeps the tracker off main until the end, so P0 on main will need to be merged into the tracker before S1c-1. Lowest-risk plan: land P0 on main now with the stable API above; tell the orchestrator owner to rebase S1c-1 onto it (additive: registry_for + one variant). Variants for the new families are added only in their own slices, so host.rs/fs.rs matches change late. DECISION NEEDED from the user (Q0).

## 5. Approaches
### 5.1 Review display for writes (the 6-row overlay problem)
| Option | Pros | Cons | Effort |
|---|---|---|---|
| A. Pager inside the overlay | small change | few rows, competes with transcript | Medium |
| **B. Full-pane review view, y gated by coverage (frontier of rows shown)** (recommended, as in the old exploration) | most rows; same pattern as list/memory/tag views; works with Prefix z; the gate proves the user saw everything | new render branch + state + keys; renderer and y must share one module; 25 pages for a 500-line file | Medium-high |
| C. Summary in overlay + `v` to view | cheap | y without seeing content breaks "shows full content" | Low |
| D. Global fullscreen overlay | biggest area | new InputMode | High |
| E. Open proposed file/diff in a pane with $PAGER/delta | reuses panes/pty | temp file outside scope, y cannot be gated by coverage, pane layout juggling | Medium |
| F. Apply then review via git diff and revert | easy | violates approve-before-write | n/a |
Keep B. Reuse for git commit (staged diff) and for run_command if the command is long. `fits()` semantics are unchanged for one-line asks (create_dir, run_command, git add).
### 5.2 Edit format
| Option | Pros | Cons |
|---|---|---|
| **E1 exact old/new, unique match** (recommended v1) | small/local models can emit it; exact diff; deterministic errors; no parser | exact whitespace; one site per call |
| E2 unified patch | compact for several hunks | invalid hunks from small models; fuzzy apply differs from what was approved; 200+ line parser |
| E3 line ranges | easy to emit | stale numbers; silent misapply after redacted/truncated reads |
| E4 `edits: [{old,new}]` array, applied atomically, one review | far fewer prompts when building a project | array parsing in P0, overlap rules, bigger review; recommend as v1.1 once the review view exists |
### 5.3 write_file overwrite
O1 create-only (O_EXCL), old decision: safe, simple. O2 overwrite allowed only when the existing file is within the read cap AND unchanged by redaction (so read==disk), review = diff (needs a diff algorithm: `similar` crate). Recommend O1 for v1; O2 is a follow-up if rewriting friction shows up. Ask the user (Q7).
### 5.4 Command runner
| Option | Pros | Cons |
|---|---|---|
| R1 host child, unjailed, allow-list + approval | simplest | full user privileges; a build script/test is arbitrary code; ~/.ssh etc readable |
| **R2 host child through `__jail-exec` with a toolchain profile** (recommended on Linux) | blast radius = project dir + toolchain; reuses jail helper and GroupKill; allow-list model | new rwx class (write+exec), new profile builder (CliLayout does not fit), outbound network stays open unless ConnectTcp is handled (ABI 4, kernel 6.7+), macOS needs real-Mac verification |
| R3 run in the user's pane (typed + Enter, screen capture) | zero new process code | 8 KiB screen scrape, needs idle shell pane/known cwd, run-mode grammar cannot express cargo/npm/pytest, user-visible state conflicts, unusable for workers |
| R4 external container (bwrap/nsjail/docker) | strongest | not installed by default; out of scope |
| R5 purpose-built verbs run_tests/run_build/run_lint mapped to detected project commands | tiny surface, easy approvals, great Evidence | less flexible; needs project detection |
Recommend R2, with the command shape `run_command{program, args[], cwd?, timeout_secs?}` (argv, never a shell string) and an allow-list table of (program, subcommand, flag shape) modeled on exec.rs SPECS (cargo test/check/build/clippy/fmt --check, npm|pnpm|yarn test|run <script>, pytest / python -m pytest, go test|vet|build, make?). IMPORTANT: the allow-list bounds the VERB, not the behavior (cargo test runs build.rs, proc macros and test code; npm test runs package.json scripts; make runs the Makefile). Therefore the real controls are (1) per-command user approval by default, (2) the jail, (3) tier-B warnings on writes to build files. R5 is a cheaper first slice if the allow-list proves politically hard.
Runner contract: stdin /dev/null; env_clear + base env + (CARGO_TERM_COLOR=never, NO_COLOR=1, CI=1); no session token; new process group + kill on timeout/cancel/drop (daemonizing grandchildren escape: documented limit, same as spawn.rs); wall-time default 120 s, max 600 s (MCP clients wait 900 s INCLUDING the approval wait); one pipe for stdout+stderr (ordering), ring-buffer capture: keep ~4 KiB head + ~24 KiB tail (failures are at the tail), then clean() + redact() + label::wrap (32 KiB cap); structured RunOutcome {exit, signal, timed_out, truncated, duration, digest} so Evidence{command, exit, digest} (S8) needs no re-parsing; one run at a time per chat, global cap for parallel workers; rlimits in the helper (FSIZE, CORE=0, CPU) as defense in depth; cwd must resolve under the scope.
Jail profile (Linux): rx /usr (and toolchain dirs resolved from PATH: ~/.cargo/bin, ~/.rustup, nvm/volta/pyenv roots), ro system dirs (LINUX_RO), rwx the project root (incl. target/, node_modules/, .venv), rw private TMPDIR and the package caches (~/.cargo/registry, ~/.cargo/git, ~/.npm, ~/.cache/pip), NOT ~/.cargo/credentials.toml, ~/.ssh, ~/.gnupg, ~/.aws, ~/.gitconfig. Landlock is allow-list, so secrets outside the grants are unreachable by default. Needs: `rwx` class in JailSpec/parse_args/render, Landlock rule that keeps Execute for rwx (runtime/jail.rs:190 excludes it today), SBPL RWX class (process-exec + file-write), exempting the macOS overlap check for this profile, optional ConnectTcp restriction for a "no network" mode (Seatbelt: omit network-outbound). Project-internal secrets (project/.env) ARE readable by the child; with network on, a malicious test can exfiltrate them: this is why network policy is a product question (Q3).
Availability policy when no jail (kernel < 6.2, macOS probe fails): refuse run_command, or allow unjailed with an always-prompt warning. Product question (Q2).
### 5.5 Search
| Option | Pros | Cons |
|---|---|---|
| **S1 in-process: add `regex` (+ small glob matcher or `globset`) and reuse the budgeted walker in fs.rs** (recommended) | enforces our deny list, redaction, nlink/regular-file rules, caps and budget per file; linear-time regex (no ReDoS); no external binary | no .gitignore semantics unless added; need a built-in skip list (target, node_modules, .git, dist, build, .venv, __pycache__) |
| S2 shell out to rg/grep | fast, familiar | bypasses our deny list unless re-implemented as globs; not installed everywhere; output not under our caps |
| S3 `ignore` crate full walker | .gitignore correctness | its own fs access bypasses open_beneath; use only `ignore::gitignore` as a matcher fed by our walker |
Recommend S1 + optionally S3-as-matcher later. Rules: glob returns names only (like list_dir: no ask, secret names omitted with the standard marker); grep returns file text snippets, so it is a Read: ask on Cloud locality (like read_file) with the pattern and root shown, free on Local, and pre-approved inside orchestrator ReadScope roots; per-file cap (1 MiB), skip binary/non-UTF-8, result caps (e.g. 100 matches, 300 chars per line, 20 KiB total), optional context lines 0-2, regex size_limit and pattern cap 512 bytes, redact every returned line, wrap as untrusted.
### 5.6 Git
| Option | Pros | Cons |
|---|---|---|
| **G1 shell out to `git` through the runner primitive, fixed verb/flag allow-list** (recommended) | matches the user's real git (config, ignore, worktrees, submodules); tiny code; reuses runner caps/timeouts | needs git installed; config-driven code execution must be neutralized |
| G2 gix (gitoxide) | no binary; pure Rust | very large dependency tree and compile time; status/diff ok, commit with hooks/signing/config parity is not; unified diff output needs extra work |
| G3 git2 (libgit2) | mature | C build (like rusqlite bundled) + openssl/ssh features to trim; still no hook parity |
G1 hardening (all mandatory): -c core.fsmonitor=false -c core.pager=cat -c diff.external= --no-ext-diff --no-textconv, GIT_CONFIG_NOSYSTEM=1, GIT_TERMINAL_PROMPT=0, GIT_OPTIONAL_LOCKS=0 for status, --no-pager, porcelain v2 -z parsing for status, bounded log (-n, fixed format), pathspec excludes for secret names (:(exclude).env* etc.) because `git diff` would otherwise print a modified tracked .env past the read deny-list, redact + wrap the output. commit/add: approval shows message + staged diffstat/diff in the review view; run hooks inside the jail or pass --no-verify (Q6); gpg signing/pinentry can hang a headless child (decide); never push/fetch/checkout/reset/clean/rebase in v1. Reads of `.git` via read_file should be denied or at least reviewed when git tools exist.

## 6. Security risks (ranked)
1. Run = arbitrary code execution as the user (build.rs, tests, npm scripts, Makefile). Controls: approval per command, jail, env scrub, no pane involvement. macOS/older kernels: no jail.
2. Deferred execution chain: model writes (user approves a diff) a Makefile/build.rs/package.json/CI file, then runs it. Tier-B warnings, review gate, jail.
3. Prompt injection driving approved actions; approval fatigue ("y" reflex across dozens of prompts); mitigations: coverage gate, ARM_DELAY (longer for writes, ~800 ms), no "approve all" in v1 (Q1).
4. Exfiltration: outbound network from the jailed child with project .env readable; grep/read results to cloud providers (existing ask semantics).
5. Secret leakage via new surfaces: git diff/log -p of tracked secrets, grep snippets, test output. Deny list + redact + pathspec excludes.
6. Git config-driven execution (fsmonitor, textconv, external diff, pager, aliases, hooks, ssh command).
7. TOCTOU/symlink/hardlink on writes (openat2 BENEATH+NO_SYMLINKS on the parent, O_EXCL, nlink==1, snapshot compare before apply).
8. Existence oracle: failed edits/greps returning distinct errors before the policy runs; keep the single REFUSED text ("path not available").
9. Resource exhaustion: unbounded output, forks, huge greps, ReDoS (linear regex), hung children (group kill), orphan daemonizers (documented).
10. Origin policy: origin == HOME, a parent of HOME, a system dir, outside HOME, or None (falls back to "/" today: runtime/chat.rs:698) must REFUSE writes/exec.
11. Invisible/bidi/zero-width characters in written content (refuse, never clean) and homoglyphs (not detected).
12. Concurrency: parallel workers sharing one chat approval slot.

## 7. Interactions with agent-orchestrator
- Seams (design.md D7 and the types block): `Toolset::{ReadOnly, Code /*S7*/, Orchestrator}`, WorkerResult.changes: Vec<Change> (S7 ChangeLedger), WorkerResult.evidence: Vec<Evidence> (S8, Evidence{command, exit, digest}). Workers get find_dir/list_dir/read_file only today; the filter is applied twice (offered list and in execute).
- coding-tools should expose: (a) Family tags so Code = ReadOnly+Write+Exec(+GitRead), never run_in_pane/external; (b) write executor returning a structured Change {path, kind, bytes, +/- lines} to feed ChangeLedger; (c) runner returning RunOutcome for Evidence; (d) path scope = TurnScope.origin (or a worker root), independent of ReadScope (reads pre-approved inside user-approved roots; writes ALWAYS prompt).
- Approval queue: orchestrator D4 shares the orchestrator's (pane, turn) for worker asks and S1 denies a second simultaneous ask; S3 builds the queue. Code workers (S7) require the S3 queue. coding-tools should ship only a minimal per-turn serialization gate (Mutex in TurnScope, taken before Approver::ask) and let S3 supersede it; coordinate who owns it.
- Sizes: orchestrator S7 ~500 and S8 ~400 plug into these seams; S8 is gated by coding-tools M3 (runner), S7 by M2 (writes + review).
- In-flight file overlap: S1c-1 tools/mod.rs, S1c-2 runtime/tools/host.rs (TurnScope, execute filter, ask label), S1d runtime/chat.rs. coding-tools touches the same files in P0 (tools/mod.rs), W-d/X-c (host.rs execute arms, TurnScope gate), W-c/X-c (approval.rs, app/tool.rs, app/run.rs). Sequence P0 first; schedule host.rs-touching slices after S1c-2 lands in the tracker, or rebase deliberately (Q0).
- Ollama "not recommended" for orchestrator and MAX_ROUNDS = 8: a local model gets at most 8 rounds per turn, which is too few for coding. Consider a higher cap when Code tools are on.
- Orchestrator design rows 5 and 7 (write-tools SDD, test-runner SDD) must be rewritten to point at coding-tools.

## 8. Proposed slicing (naive = incl. tests; real = x2.5 calibration; each PR aims <= 400 but expect size exceptions)
| ID | Slice | Naive | Real est | PRs |
|---|---|---|---|---|
| **M0** | prerequisites | | | |
| P0 | multi-arg registry, Family tag, optional-arg kinds, tests (behavior-preserving for the 4 tools) | 200 | ~500 | 1 (likely size:exception) |
| P1a | open.rs write primitives (Linux): openat2 with mode, O_EXCL create, mkdirat, renameat, unlinkat | 120 | ~300 | 1 |
| P1b | (optional, gated by Q4) portable open for macOS: Walk strategy + fcntl F_GETPATH; unlocks reads on macOS | 100 | ~250 | 1 |
| **M1** | search (read-only) | | | |
| S-a | glob (+ read_file start_line/max_lines) | 150 | ~380 | 1 |
| S-b | grep: regex dep, walker reuse, redaction, caps, Cloud ask (ApprovalAsk::Search) | 250 | ~650 | 1-2 |
| **M2** | writes | | | |
| W-a | resolve_write: origin rules, tier A/B lists, one-refusal rule | 200 | ~500 | 1 |
| W-b | review core (pure): ReviewLine, hunk builder, coverage frontier | 200 | ~500 | 1 |
| W-c | review app state/keys/ApprovalAsk::{CreateDir,Write,Edit}/UI, write ARM_DELAY | 300 | ~750 | 2 |
| W-d | create_dir + write_file executor, serialization gate, caps | 300 | ~750 | 2 |
| W-e | edit_file: pure matcher/EOL/BOM/hunk, then apply (snapshot, temp+rename) | 400 | ~1000 | 2 |
| W-f | Route::AutoApprove for create_dir in run, per-mode tests, model-facing docs | 150 | ~375 | 1 |
| **M3** | runner | | | |
| X-a | pure: argv allow-list, Call::RunCommand, ApprovalAsk::Exec, one-line overlay | 250 | ~625 | 1-2 |
| X-b | runtime capture executor: spawn, env scrub, group kill, ring buffer, timeouts, RunOutcome | 300 | ~750 | 2 |
| X-c | host/app wiring, modes, availability policy, tool descriptions | 200 | ~500 | 1 |
| X-d | Linux jail toolchain profile: rwx class, helper args, network switch, rlimits | 350 | ~875 | 2 |
| X-e | Seatbelt profile + real-Mac verification (CI probe + tester checklist) | 250 | ~625 | 1-2, hardware-gated |
| **M4** | git | | | |
| G-a | read-only status/diff/log via runner, hardening, porcelain parsing, pathspec excludes | 250 | ~625 | 1-2 |
| G-b | add/commit with message + staged-diff review | 250 | ~625 | 1-2 |
| **M5** | docs: spec deltas, README threat model/privacy, tools.md rewrite, design decisions | 150 | ~375 | spread |
Totals: ~4,400 naive, ~10-11k real, roughly 24-28 PRs. Suggested releases: M0+M1 (0.19.0, low risk), M2 (0.20.0, the writes), M3 (0.21.0), M4 (0.22.0). Go/no-go checkpoint after M2 on review UX and approval fatigue. Orchestrator dependency map: S7 needs M2, S8 needs M3 (X-a..X-d on Linux).
Ordering alternative: start M2 before M1 if the orchestrator S7 timeline is the priority.

## 9. Recommendation
Single SDD `coding-tools`, milestone-gated, Linux-first, order P0 -> M1 -> M2 -> M3 -> M4:
- P0 on main first with a stable public API and a Family tag.
- Search in-process (regex + reused walker); grep asks on Cloud.
- Writes: exact-string edit, create-only write_file, full-pane review view with a coverage gate; every write prompts in every mode; create_dir auto in run mode.
- Runner: captured host child in a toolchain jail (R2) with an argv allow-list and per-command approval; refuse or always-prompt-warn when no jail.
- Git: shell out with hardened flags; read verbs first; commit with a reviewed staged diff; no push.
- Per-family toggles in config, default OFF for Write/Exec/GitWrite so existing users see no behavior change.

## 10. OPEN PRODUCT QUESTIONS (proposed default in parentheses)
Blocking (ask first)
Q0. Land P0 on main now and have the orchestrator chain rebase S1c-1 onto it, or wait for S1c-2? (land now)
Q1. Approval fatigue: keep strict "every write waits for y" (owner rule) for a whole project (dozens of files), or add a batch review (model proposes N changes, one combined review), or a per-session "accept edits inside this directory" mode? Per-turn cap 12 prompts is too low for building a project: raise, or make configurable? (strict per-change in v1; batch review as a follow-up using the ChangeLedger; cap configurable)
Q2. Runner approval: prompt on every command? Allow auto-run in `run` mode for an allow-listed set, only when the jail is active? What when there is no jail: refuse, or run unjailed with an always-prompt warning? (prompt every command in v1; refuse without jail)
Q3. Network for child processes: off by default (breaks cargo fetch/npm install), on, or ask per run? Landlock needs ABI 4 (kernel 6.7+) to deny outbound TCP; Seatbelt can. (on, with a prominent notice; "offline" option later)
Q4. macOS: are coding tools in scope for v1? Today file tools do not work on macOS at all, README says macOS is unsupported, and the jail needs a real Mac to verify. (Linux-first; macOS after a portable-open slice and tester sign-off)
Q5. Scope root = the chat's origin directory only; refuse when origin is HOME, a parent, a system dir, outside HOME or missing. Monorepo parents or multiple roots? How does it relate to orchestrator ReadScope roots? (single root = origin)
Q6. Git: which verbs (status, diff, log, add, commit; also rm/mv/branch/checkout/stash?), never push/fetch? Commit hooks: run inside the jail or --no-verify? GPG signing? Should workers commit or only the user/orchestrator? (status/diff/log/add/commit; no push; hooks inside the jail; workers do not commit)
Q7. write_file overwrite: create-only, or overwrite with a diff review when the file is unredacted and under the read cap? (create-only)
Q8. Delete/rename/move files: include `delete_file`/`move_file` (approval, trash vs unlink), or leave them to git mv/rm? Without them refactors are blocked. (defer; git mv/rm in M4)
Business rules and limits
Q9. Sizes: write 32 KiB, edit 16 KiB per field, edited file 1 MiB, run output 32 KiB (4 KiB head + tail), default run timeout 120 s / max 600 s, grep 100 matches / 20 KiB. (yes)
Q10. Which toolchains in the first allow-list (cargo, npm/pnpm/yarn, pytest/python -m pytest, go, make)? User-extendable via config, or fixed? (cargo, npm, pytest, go; fixed in v1)
Q11. R5-style purpose-built `run_tests` verb first (smaller, safer) or the generic argv runner? (generic argv)
Q12. Grep on Cloud engines: prompt every time, or one approval per project scope (like the orchestrator ReadScope)? (prompt; scope-approval later)
Q13. Deny rules: keep the tier A/B lists from the old exploration; also deny writes to node_modules? deny reads of .git? (writes: deny .git, node_modules; reads: deny .git/config only)
Q14. Config surface: per-family toggles (default off for Write/Exec/GitWrite) in the config menu, or only the global Tools switch? (per-family, default off)
Q15. Ollama MAX_ROUNDS=8: raise when coding tools are on? (raise to 24 behind the toggle)
Q16. Multi-edit per call (edits array) in v1 or v1.1? CRLF and non-UTF-8 files (Latin-1 CSVs): refuse in v1? Unicode Cf policy (refuse bidi/zero-width, allow ZWJ/ZWNJ with a note)? Trailing newline added and reported? (v1.1; refuse; refuse bidi/ZW; add and say so)
Q17. Concurrency: one run at a time per chat and a global cap for workers? Serialize batched write prompts per turn? (yes, yes)
Q18. Release/doc impact: README/tools.md currently promise "cannot change files"; accept a visible changelog/threat-model update per release? (yes)

## 11. Risks
- Scope: ~10k real lines; M3 jail and M2 review UX are the two unbounded areas.
- macOS verification is hardware-gated and already lagging for the CLI jail.
- P0 touches the files the orchestrator chain is about to edit (tools/mod.rs, host.rs).
- Approval fatigue vs the product goal; owner rule may need a relaxation, which is a security-model change.
- The allow-list bounds verbs, not behavior: runner safety rests on approval + jail.
- Landlock cannot stop daemonizing grandchildren or restrict outbound network below ABI 4.
- Git config-driven code execution, and `git diff` bypassing the read deny list.
- Single approval slot vs batched calls/workers until the S3 queue exists.
- Duplicate-key handling on the Ollama path unverified.

## 12. Ready for Proposal
Yes, after a proposal question round on Q0-Q8 (the blocking ones). Proposal should freeze: scope (Linux-first), milestone gates, the Family/Toolset contract with agent-orchestrator, approval-fatigue stance (Q1), runner security model (Q2/Q3), and the git verb list (Q6).
