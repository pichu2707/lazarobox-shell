# Proposal: coding-tools (write, run, search and git tools)

Inputs: `exploration.md` (engram #3325), user decisions #3326, #3327, #3328 (Q5, Q8-Q18 confirmed 2026-10-22). Supersedes `openspec/changes/write-tools/exploration.md` and absorbs the "write-tools SDD" and "test-runner SDD" (rows 5 and 7 of the agent-orchestrator design).

## Intent

Let a user **build a project from chat** on Linux. Today the model can only find, list and read, and type into a pane. This change adds four tool families on the lazarobox MCP server and the built-in tools: **search**, **write**, **run** and **git**.

## User decisions that override the exploration

| Topic | Exploration recommended | User decided (wins) |
|---|---|---|
| Write approval | one review per write | **Batch review**: the model stages changes, which may span turns, then closes the batch with a "propose batch" tool. The review is all-or-nothing, with a coverage gate over the whole batch. |
| write_file | create-only (O_EXCL) | **Overwrite is allowed** and shown as a diff, but only when the file is under the read cap and unredacted. |
| Git verbs | status/diff/log/add/commit, git mv/rm in M4 | status, diff, log, add and commit. No delete or move of any kind in v1. |

## Scope

### In scope
- **P0**: a multi-argument tool registry (`ToolDef`/`ArgDef`) with a `Family` tag (Read, Search, Write, Exec, Git, Pane). It lands on `main` first with a stable public API, and the `tools/list` output for the 4 existing tools stays byte-identical.
- **Search**: `glob`, `grep` (in-process `regex` over the existing budgeted walker), and `read_file` paging.
- **Write**: `create_dir`, `write_file`, `edit_file` (exact old/new, unique match), a staging store, a propose-batch tool, a full-pane batch review view with a coverage gate, and the apply step.
- **Run**: `run_command{program, args[], cwd?, timeout_secs?}` (argv, never a shell string), captured as a host child process inside a Linux Landlock toolchain jail. It returns a structured `RunOutcome`.
- **Git**: `status`, `diff` and `log` through the runner with hardened flags. `add` and `commit` show the message and the staged diff in the review view, and hooks run inside the jail.
- Per-family toggles, off by default. Ollama `MAX_ROUNDS` goes to 24 when the toggles are on. README and threat-model updates ship with each release.

### Out of scope (non-goals)
- macOS. It comes later, after a portable-open slice and tester sign-off.
- Delete, move and rename tools.
- Git push, fetch, checkout, reset, clean and rebase.
- Commits by workers.
- An offline (no-network) mode for `run_command`.
- Multiple roots or monorepo parents.
- Toolchains a user can add through config, and `make` or `python -m`.
- A "run_tests"-style purpose-built verb.

## Business rules

- **Root**: there is one root, the chat origin. Writes and runs are refused when the origin is HOME, a parent of HOME, a system dir, or missing. Every refusal returns the same text ("path not available") so it cannot reveal whether a path exists.
- **Batch**:
  - Staged changes never touch the disk before the user approves them.
  - `read_file`, `grep` and `glob` see the staged version, marked "pending, not on disk".
  - `run_command`, `git add` and `git commit` are refused while unproposed staged changes exist ("propose them first"). Tests always run on disk.
  - A batch orphaned by a cancelled turn stays staged in the chat, with a pane indicator. The user can review and apply it, or discard it. Closing the pane discards it with a notice.
- **Run**: every command prompts. With no active jail (Landlock ABI below 3, or a non-Linux system), `run_command` is refused. Only one run at a time per chat. The allow-list is fixed: cargo, npm/pnpm/yarn, pytest, go.
- **Network**: network is ON for runs, and the user sees a visible notice that a test can exfiltrate the project's `.env`.
- **Grep**: grep prompts on Cloud engines and runs free on Local, like `read_file`.
- **Deny**:
  - Writes to `.git/` and `node_modules/` are refused.
  - Reads of `.git/config` are refused.
  - The existing secret lists still apply.
  - Content with invalid or bidi/zero-width characters is refused, and so are CRLF and non-UTF-8 files.
- **Caps**:

| Limit | Value |
|---|---|
| `write_file` content | 32 KiB |
| `edit_file`, per field | 16 KiB |
| File being edited | 1 MiB |
| Run output | 32 KiB (4 KiB head plus the tail) |
| Run timeout | 120 s default, 600 s max |
| Grep results | 100 matches or 20 KiB |

## Capabilities

### New Capabilities
- `code-search`: glob, grep, read_file paging, and the staged-view marker.
- `code-write`: staging, propose batch, batch review with the coverage gate, apply, overwrite-as-diff, write deny rules.
- `command-runner`: the argv allow-list, approval, the jail profile, the network notice, the caps, and `RunOutcome`.
- `git-tools`: the read verbs, add/commit review, hardening, and the verb boundary.

### Modified Capabilities
- `agent-tools`: the multi-argument registry, `Family`, the approval model, and the path/deny policy. Its base spec exists only in the unarchived `changes/agent-tools/`.
- `lazarobox-mcp-server`: the tools served, and the jail and process rights.
- `chat-pane`: the review view, the staged/orphan batch indicator, and close discarding a batch.
- `config-menu`, `configuration`, `config-persistence`: the per-family toggles.
- `chat-engines`: Ollama `MAX_ROUNDS` 24 when the toggles are on.

## Approach

1. **P0 first, on `main`.** Behaviour stays the same. Variants for the new families arrive only in their own slices.
2. **M1 search.** Low risk and read-only.
3. **M2 write.** Staging, the batch review view and apply. There is a **go/no-go checkpoint** on review UX after M2.
4. **M3 run.** Linux toolchain jail: a new `rwx` class (Landlock Execute kept for the project root), rx toolchain dirs, rw caches and tmp, and no access to `~/.ssh`, `~/.gnupg`, `~/.aws` or credentials. Clean environment, process-group kill, ring-buffer capture, redact and wrap.
5. **M4 git.** Shell out to `git` through the runner. Use `-c core.fsmonitor=false`, `--no-ext-diff`, `--no-textconv`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`, and pathspec excludes for secret names.
6. **M5 docs.** Spread across the releases.

## Threat model and risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| The allow-list **bounds the verb, not the behaviour**: `cargo test` runs build.rs, proc macros and tests, and `npm test` runs scripts. | High | Approval for every command, the jail, and a review of build-file writes. |
| Exfiltration with network on (the project `.env` is readable inside the jail) | Med | Visible notice. Offline mode later. Landlock cannot block outbound traffic below ABI 4. |
| A deferred execution chain: an approved write to Makefile, build.rs or package.json, then a run | Med | Batch review, warnings on build files, the jail. |
| Approval fatigue and the "y" reflex | Med | One review per batch, the coverage gate, a longer arm delay for writes. |
| A large batch makes a review that is too long, so the user skims it | Med | The coverage gate. Go/no-go after M2. |
| Git config-driven code execution, or `git diff` printing a tracked secret | Med | Hardened flags, pathspec excludes, redaction. |
| Staged state diverges from disk before apply (TOCTOU) | Med | Snapshot compare at apply; the whole batch is refused if anything drifted. |
| Daemonizing grandchildren escape the group kill | Low | Documented limit. |
| P0 collides with the in-flight orchestrator files (`tools/mod.rs`, `host.rs`) | Med | P0 lands first. Slices that touch `host.rs` are scheduled after S1c-2. |
| Scope: about 10-12k real lines | High | Milestone releases and the go/no-go checkpoint. |

## Rollback Plan

- Every family sits behind a toggle that is off by default. With the toggles off, behaviour is byte-identical to 0.18.0, and a characterization test checks this.
- P0 does not change behaviour and can be reverted as a single PR.
- Each PR is revertable. Each milestone ships as its own release (0.19 search, 0.20 write, 0.21 run, 0.22 git).
- Nothing reaches the disk before approval, and apply writes through temp+rename per file. Git remains the user's undo; there is no built-in undo.

## Slice outline

Each PR has a budget of 400 lines. In this repo, real PRs have run **2.4x to 3x** the naive estimate, so the "Real" column applies that factor and size exceptions are likely.

| Slice | Content | Naive | Real | PRs |
|---|---|---|---|---|
| P0 | ToolDef/ArgDef, Family, schema, per-arg parse | 200 | 500-600 | 1-2 |
| M1 | glob, read_file paging, grep | 400 | 1,000-1,200 | 3 |
| M2 | open.rs write primitives, resolve_write and deny rules, staging store, diff and coverage core, review view and keys, create/write/edit plus apply, orphan indicator | 1,700 | 4,100-5,100 | 10-12 |
| M3 | allow-list and ask, capture executor, toolchain jail, wiring and notices | 1,100 | 2,600-3,300 | 6-7 |
| M4 | git read verbs, add/commit review | 500 | 1,200-1,500 | 3-4 |
| M5 | toggles, MAX_ROUNDS, docs | 250 | 600-750 | 2 |

The total is about 4,150 naive lines, or 10-12k real, in about 25-30 PRs.

## Dependencies (agent-orchestrator)

- P0 merges on `main` before orchestrator S1c-1. Chain PRs #279, #280 and #281 rebase on it.
- `Toolset::Code` becomes a family filter: Read, Search, Write, Exec and Git-read. It never includes Pane or External.
- The write apply step returns `Change` records for the S7 ChangeLedger, and the runner returns a `RunOutcome` for the S8 Evidence. S7 needs M2, and S8 needs M3.
- The design rows 5 and 7 must be repointed to `coding-tools`.
- The S3 approval queue supersedes this change's per-turn serialization gate.

## Resolved questions (user decisions, 2026-10-22)

1. **Partial apply failure:** roll back. If apply fails partway through a batch, the files already written are restored to their pre-batch state, which keeps the batch all-or-nothing on disk as well as in review. If the rollback itself fails, the batch is reported as partial, listing exactly which paths changed.
2. **Batch size cap:** yes, about 50 files or 1 MiB of total staged content, whichever comes first. Above that, the tool to propose the batch refuses and asks the model to split it.
3. **Commit signing:** disabled for model commits (`-c commit.gpgsign=false`), with a visible notice in the review view. Commits are never left to hang on a pinentry prompt.
4. **`create_dir`:** it joins the batch. There is one approval rule for every write, and nothing is auto-approved.
5. **Global run cap:** 2 concurrent runs across all orchestrator workers, on top of one run per chat.
6. **Worker batches:** the user reviews and applies them through the S3 approval queue. The interaction details are left to the design.
7. **Quitting the app:** it discards staged batches the same way closing the pane does, with a notice.
8. **Git toggles:** there are two. `Git` covers status, diff and log, and `GitWrite` covers add and commit. Both are off by default.

## Success Criteria

- [ ] P0 is on `main` before S1c-1, and the `tools/list` output for the 4 existing tools is byte-identical.
- [ ] On Linux, starting from an empty origin directory, a model scaffolds a small Rust crate through batches, gets each batch approved in a single fully covered review, runs `cargo test` with a structured outcome, and commits after a reviewed staged diff.
- [ ] No write tool changes a file without an approved batch, and no write or run happens outside the root.
- [ ] `run_command` is refused without a jail and while unproposed staged changes exist.
- [ ] With all toggles off, behaviour is identical to 0.18.0.
