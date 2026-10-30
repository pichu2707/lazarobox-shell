# Exploration: write-tools (create_dir, write_file, edit_file over the lazarobox MCP server)

> Superseded by `coding-tools` (archived 2026-10-29 as exploration only; no spec merge).

**Context.** On 2026-10-17 the owner asked that the model create directories, create files with code, and edit files, so that it can help programmers and analysts (for example, changing KPI data). They chose structured MCP write tools over shell redirection.
- Writes ALWAYS show the full content or diff and wait for `y`, in every mode.
- The one exception: `create_dir` runs directly in `run` mode (the sibling change auto-execute).
- Scope: under the chat's directory only. Never secrets, `.env`, credentials or `.git`.
- Order: 2f-fix → auto-execute → write-tools → Ollama 3a/3b → 0.16.0.
- Engram copy: `sdd/write-tools/explore`.

## Current state
### Who writes
The engines cannot write. claude runs with `--tools ""`, codex with `-s read-only` plus the Landlock jail, and gemini with a deny-all policy (`src/core/ai/chat/adapters.rs:225,288`). So lazarobox (the unjailed app) writes, through the tool host.

### Registry
- `src/core/ai/tools/mod.rs:95`: `BUILT_INS` has 4 entries, each taking ONE string argument.
- `one_string()`, `parse_call()` and `Call::argument()` all assume a single argument of at most `MAX_ARG_BYTES = 4096`.
- `Access::Write` is only a classification today.

### Host
- `src/runtime/tools/host.rs` `execute()` validates the call, then calls the pure `ask_for(call, locality)` (`approval.rs:76`).
- It registers the wait BEFORE announcing `ToolCall{approval: Some(Box<ApprovalAsk>)}`, and runs the tool only on `Decision::Sent`.
- The host does not know the mode. The app applies it through `approval::route()` (`approval.rs:117`).

### App
- `on_tool_ask` (`src/app/tool.rs:110`) sets `armed_at = clock + ARM_DELAY` (400 ms).
- `on_approval_key` (`src/app/run.rs:108`) ignores input until the prompt is armed.
- `approve()` (`:180`) requires `fits()`.

### Overlay limit
- `approval_rows.rs` sets `MAX_COMMAND_ROWS = 6`, and a prompt is approvable only when all of its text fits.
- So a file or a diff practically never fits.
- Precedents for full-pane views inside a chat: `list_open`, `memory_view_open`, `tag_editor_open` (`run.rs:91`).

### Read side
- Everything is rooted at HOME: `SecretRoots`, `resolve` and `judge_real` in `paths.rs`.
- `open_beneath(home, rel, dir)` in `open.rs` uses `openat2(RESOLVE_BENEATH|NO_SYMLINKS)`, falling back to an `O_NOFOLLOW` walk, and then the fd path is judged again. It is Linux only.
- `read_file` refuses nlink>1 and non-regular files, redacts the content, and truncates it at 64 KiB.
- All refusals share one text: `REFUSED = "path not available"`.

### Precedents and dependencies
- `src/runtime/config_write.rs:77` `write_atomic`: a temp file opened with `create_new`, `sync_all`, the original permissions copied, then a rename. It refuses read-only files (`:50`).
- There is no diff code, no `similar` crate, and `tempfile` is a dev-dependency only.

### Wiring
Wiring is server-wide, so no flag change is needed:
- claude: `--allowedTools=mcp__lazarobox` (`wiring.rs:126`);
- gemini: an admin policy with `mcpName="lazarobox"` (`wiring.rs:249`);
- codex: a server override with a 900 s timeout.

### Text to supersede
- `tools.md` (still on agent-tools-2ffix, not merged): "You still cannot change files or run anything directly".
- `agent-tools/spec.md:5`: "State changes happen only in the user's own pane".
- The term "Write tool = run_in_pane + user MCP tools".
- design.md line 9: "only path to WritePty".

## 1. Tool schemas
- **Signatures:** `create_dir{path}`, `write_file{path, content}`, `edit_file{path, old, new}`. All are required strings, with `additionalProperties:false`.
- **Registry change:**
  - Generalize the table first: a list of `(arg, cap)` pairs per row.
  - New `Call::{CreateDir, WriteFile, EditFile}`.
  - `Call::argument()` returns the PATH, so rows never show content.
  - `fs::execute_with` keeps refusing these calls, and a separate write executor handles them.
- **Caps:**
  - `write_file.content`: 32 KiB.
  - `edit_file.old` and `new`: 16 KiB each.
  - The file being edited: 1 MiB.
  - All are far below the 1 MiB MCP line limit.

### `edit_file` strategy
| Option | Pros | Cons |
|---|---|---|
| **E1: exact string replace, unique match** (RECOMMENDED) | Works with small and local models. The diff is exact. Errors are deterministic. No parser needed. | Needs the exact whitespace. A multi-site change takes several calls. |
| E2: unified diff | Compact for several hunks | Small models produce invalid hunks. A fuzzy apply means what is approved differs from what is applied. Needs a parser of 200+ lines. |
| E3: line range | Easy to emit | Line numbers go stale. Silent misapply after a redacted or truncated read. |

No `replace_all` in v1. `old == ""` and `old == new` are refused.

### Text rules (refuse, never clean, as in D4)
- **Encoding:** UTF-8 only; NUL means not text.
- **Control characters:** only `\n` and `\t` are allowed. ESC, the rest of C0, DEL, C1 and a lone `\r` are refused.
- **Invisible characters:** bidi (Trojan Source), zero-width characters and U+FEFF are refused in model content, so that what is shown is what is written.
- **Line endings:** `edit_file` matches against an LF view and restores the file's endings (CRLF supported). Mixed endings are refused.
- **BOM:** an existing one is kept.
- **Trailing newline:** `write_file` adds `\n` if it is missing, and says so in the review and the result.

### `write_file` NEVER overwrites (`O_CREAT|O_EXCL`)
- **Why:**
  1. `read_file` redacts and truncates at 64 KiB, so rewriting a file from what was read would destroy redacted values and the cut-off tail.
  2. Reviewing a full rewrite hides the real change; a diff can be verified.
  3. `O_EXCL` removes the TOCTOU problem.
- **Existing file:** "file exists; use edit_file", returned only after the policy has passed (see section 2).

## 2. Path policy
- **Resolver:** `resolve_write(roots, origin, input)` in `paths.rs`, reusing `expand_tilde`, `normalize` and `display_target`. The result must be `canonical_origin/<names>`.
- **Opening:**
  - Open the PARENT with `open_beneath(origin, parent_rel, dir=true)`. The kernel then enforces BENEATH and NO_SYMLINKS relative to the origin, which is stronger than reads.
  - Check `fd_path` and `judge_real` again after opening.
- **write_file:** `openat(parent, name, O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW, 0o666)`. On an error, delete only what we created.
- **create_dir:** `mkdirat` one component at a time.
- **edit_file:**
  1. Open the target with `O_NOFOLLOW`, then `fstat` it: regular file, nlink==1, writable by the owner, within the size cap.
  2. Create a temp file with `O_EXCL` in the same directory.
  3. Write it, `fsync`, `fchmod` to the original mode, then `renameat`.
  4. Best-effort `fsync` of the parent directory.
- **New code in `open.rs`:** an `openat2` that takes a `mode` (`OpenHow.mode` is hardcoded to 0 at `:140`), plus `mkdirat`, `renameat` and `unlinkat`. About 80 lines.
- **Edge cases:**
  - Symlinks, as a component or as the target, are refused.
  - TOCTOU: the open is the check, and the fd path is judged again. The window in an edit between the compare and the rename is accepted and documented.
  - Hardlinks: nlink>1 is refused.
  - Platform: Linux only, like reads. Name comparisons are ASCII case-insensitive.

### Tier A deny list (same `REFUSED` text)
- Everything already excluded: `is_excluded_name`, secret roots, lazarobox config and data (XDG), `cli-tmp`.
- `.git`, `.hg` and `.svn` at any depth.
- rc files at any depth: `.bashrc .bash_profile .bash_login .bash_logout .profile .zshrc .zshenv .zprofile .zlogin .zlogout .inputrc config.fish`.
- `.ssh`, `.gnupg`, `authorized_keys`.
- `node_modules` (recommended).

### Tier B (allowed, with a warning in the review header and the result)
- Files that run later:
  - CI: `.github/`, `.gitlab-ci.yml`, `.circleci/`.
  - Build: `Makefile`, `justfile`, `Dockerfile*`, `build.rs`, `package.json`, `.cargo/config.toml`, `setup.py`.
  - Editor and hooks: `.vscode/tasks.json`, `.husky/`, `.pre-commit-config.yaml`.
  - Scripts and tests: `*.sh`, `conftest.py`, mise files.
- Any content starting with `#!`.
- This is a tripwire, not a boundary.

### Origin
- Origin == `$HOME`, a parent of it, or a system directory: writes are REFUSED. Otherwise the whole home would be in scope, `~/.config/nvim/init.lua` included.
- Origin outside HOME: refused, as reads are.
- No origin: "the chat has no project directory". No fallback.

### One refusal text
- Policy, outside-origin and secret refusals all return `path not available`.
- Distinct messages come only after the policy has passed. Otherwise `write_file .env` ("exists") versus `.envx` ("created") would be an existence oracle.

## 3. Approval UX
| Option | Pros | Cons | Effort |
|---|---|---|---|
| A. Pager inside the overlay | Small change | Few rows, competes with the transcript | Medium |
| **B. Full-pane REVIEW view, `y` gated by coverage** (RECOMMENDED) | Most rows. Same pattern as the memory and list views. Works with `Prefix z`. | New render branch, state and keys. The renderer and `y` must share one module. | Medium-high (~380) |
| C. Summary plus `v` to view | Cheap | `y` without seeing the content breaks "sees the FULL content" | Low |
| D. Global fullscreen overlay | Largest area | New `InputMode`, which D1 avoided | High |

### Design of B (pure core, `core/ai/chat/review.rs`)
- **Model:**
  - `ReviewLine = Head | Ctx | Add | Del | Note`.
  - `ApprovalAsk` gains `CreateDir{path}`, `Write{path, review}` and `Edit{path, review}`.
  - The host builds the review, so the app does no IO.
  - `ReviewState{scroll, frontier}` lives on `ChatState`.
- **Coverage gate:**
  - `frontier` is the first row never shown. After each frame or scroll: if `scroll <= frontier`, then `frontier = max(frontier, scroll + height)`.
  - `G` shows the end but does not skip the middle.
  - `y` works only if `frontier >= total_rows`. A single page unlocks at once, after the arm delay.
  - `height` comes from the same function the renderer uses.
- **Keys and footer:**
  - Scrolling: `j/k/Down/Up/PgDn/Space/PgUp/g/G/Home/End`.
  - Gated: `y`.
  - Declining: `n`, Esc, Ctrl+C.
  - Everything else is swallowed.
  - Footer: `y write it · n cancel · j/k scroll · 37%`. A pane that is too small shows a notice and a `Prefix z` hint.
- **Typing in flight:**
  1. ARM_DELAY, longer for writes (about 800 ms). Coverage counts only pages shown after the prompt is armed.
  2. The coverage gate: a stray key cannot satisfy it for content longer than one page.
- **Cost:** a 500-line file is about 25 pages; the 32 KiB cap bounds this. No weaker mode in v1.

### Diff without a new crate
- The change is exactly "replace the unique span S with T".
- The hunk: 3 lines of context, then S as `Del`, then T as `Add`, under `@@ -L,n +L,m @@`.
- First trim whole lines common to the start and end of S and T, because models pad `old` to make it unique.
- Colours: `-` red, `+` green, context muted, notes orange.
- `similar` is only worth adding if a large block has to be diffed line by line inside.
- The `write_file` review: `new file <path> - N lines, M bytes`, every line as `Add`, with a line-number gutter.

## 4. Execution
- **Where:** in the host after `Decision::Sent`, on a blocking thread.
- **No timeout during the apply:** a timeout halfway through a write would leave an unknown state. Writes run to completion, bounded by the caps, and cancelling only applies before they start.
- **Two phases:**
  1. Before asking: validate, resolve, open, take a snapshot (bytes plus dev/ino), build the review, and require exactly one match for `edit_file`.
  2. After `Sent`: resolve and open again, compare the bytes with the snapshot, then write. If the file changed: `the file changed since the preview; read it again`, and nothing is written.
- **Details:**
  - A read-only target is refused.
  - Owner, xattrs and ACLs are not preserved (documented).
  - `create_dir` behaves like `mkdir -p` inside the origin, with a depth cap of 16. An existing directory is fine; an existing file is an error.
  - No chmod, so nothing created is ever executable.
- **Results** (never any content, wrapped by `label::wrap`):
  - `created <path> (N bytes, M lines)`
  - `edited <path>: 1 replacement (+a -b lines)`
  - `created directory <path>`
  - otherwise one error from a closed set.

## 5. Modes (D18 and auto-execute)
- **Host:** mode-agnostic. `ask_for` asks for all three tools.
- **Routing:**
  - New route `Route::AutoApprove`, used only for `(CreateDir, Origin::Tool, Run)`. It emits `Effect::ToolDecision{Sent}` with no overlay.
  - `write_file` and `edit_file` prompt in every mode, as an explicit arm with a test per mode.
  - `create_dir` prompts in `ask` and `auto` (one line, so it fits the current overlay).
- **Per-turn limits:**
  - 12 write or edit prompts, counting failed edits.
  - 10 direct `create_dir` in `run`, then the prompt.
- **Batched calls:** `Approvals` allows a single pending approval, and claude batches calls. Recommended: a per-turn `tokio::sync::Mutex<()>` gate in `TurnScope`, taken before `Approver::ask` and held until the apply ends (about 20 lines).

## 6. Locality and privacy
- Writes send nothing out. `edit_file` needs `old` from `read_file`, which is already approved on cloud engines.
- **Oracle:** a failed `edit_file` (0 or more than 1 match) returns without any prompt. An injected model could probe a file bit by bit.
- **Mitigations:**
  - the same error for 0 and for more than 1 match;
  - failed edits count toward the cap;
  - no context is ever returned.
- **Residual:** a 1-bit channel bounded by the cap.

## 7. Prompt-injection threat model
- **Main control:** a human `y` after seeing the whole change. The only direct call is `create_dir` in `run`, which has no content, stays inside the origin and is capped.
- **Residual risks and their mitigations:**
  - Approval fatigue: the per-turn cap and the sequential gate. No "approve all".
  - A malicious change hidden far down: the coverage gate.
  - Homoglyphs: not detected. Bidi and zero-width characters are refused.
  - Deferred execution through CI, Makefile, build.rs, package.json, hooks or tasks.json: tier B warnings, tier A refusals, and nothing created executable.
  - Any source file can be run later, so the human review is the real control.

## 8. Wiring per engine
- `tools/list` goes from 4 to 7 tools, built from the same registry for MCP and the future Ollama loop.
- No flag changes.
- Manual check per engine:
  - 30 KiB of `content` through the shim;
  - a long wait while the user scrolls the review.
- Rewrite `tools.md`:
  - project directory only;
  - the user reviews every change;
  - no overwriting;
  - `edit_file` needs an exact, unique `old`;
  - read first;
  - one change at a time;
  - no secrets.
- Tool descriptions stay neutral and identical across modes. With tools off, requests stay byte-identical.

## 9. Spec and design impact
- **New capability `file-writing`:**
  - tools and schemas;
  - path policy and tiers;
  - the review view and coverage gate;
  - always asking, with the `create_dir`/`run` exception;
  - atomicity and no overwriting;
  - results and caps.
- **Deltas:**
  - agent-tools: registry 4 → 7, the "Write tool" term, the Purpose, approval scenarios, and clarify `spec.md:82` as read tools only.
  - chat-pane: the review view.
  - modal-input: scrolling and the `y` gate.
  - lazarobox-mcp-server: the tool list and argument sizes.
- **design.md:**
  - a note on line 9;
  - D17 gains the three prompts;
  - D18 `route` gains `AutoApprove`;
  - new D22 (write path policy), D23 (review view) and D24 (write execution).
- README privacy and threat model are updated in the same release.
- **Dependency:** `create_dir` in `run` depends on auto-execute S1.

## 10. Size and slices (non-test lines, each under 400)
| Slice | Content | Lines |
|---|---|---|
| W0 | Generalized registry (multiple args, caps, inert variants, snapshot churn); can fold into W1 | ~200 |
| W1 | Write path policy, `open.rs` primitives, host executor, `create_dir` with the one-line overlay | ~330 |
| W2 | Review view: `review.rs`, state, keys, UI, `y` gate, write arm delay | ~380 |
| W3 | `write_file`: validation, trailing newline, create-only, caps, per-turn cap, gate | ~300 |
| W4a / W4b | `edit_file`: pure core (matcher, EOL/BOM, hunk, ~200) / apply (snapshot, temp+rename, permissions, oracle, ~200) | ~400 |
| W5 | `AutoApprove` for `create_dir` in `run` (after auto-execute S1), per-mode tests, `tools.md`, descriptions, README, spec and design | ~120 + docs |

Total about 1.5-1.8k lines across 7 PRs.

## 11. Open product questions (proposed default)
1. Sizes: write 32 KiB, edit 16 KiB per field, edited file 1 MiB? *(yes)*
2. May `write_file` overwrite? *(no)*
3. Must the user see everything before `y`? *(yes, contiguous coverage)*
4. Warn on executable or CI files? *(yes, tier B, not blocked)*
5. Per-turn limits? *(12 writes or edits including failures; 10 `create_dir` in `run`)*
6. Does `create_dir` create parents? *(yes, depth 16)*
7. Origin == HOME, outside HOME, or no origin? *(refuse all three)*
8. Deny `node_modules`? *(yes; `target/`, `.venv`, `vendor` allowed)*
9. A separate kill switch? *(no; Tools on/off)*
10. Unicode `Cf`: refuse bidi, U+FEFF and ZWSP; allow ZWJ/ZWNJ with a note? *(needs a decision)*
11. CRLF in `edit_file`? *(yes; mixed endings refused)*
12. Trailing newline: add it and say so? *(yes)*
13. Serialize batched writes? *(yes)*
14. Non-UTF-8 files (Latin-1 CSVs, analysts)? *(refused in v1; the main gap)*

## Recommendation
Build W0/W1 → W5:
- `openat2` primitives rooted at the origin, in the host;
- `write_file` create-only;
- `edit_file` with an exact unique match;
- a full-pane review view whose `y` is gated by coverage.

Writes always prompt. `create_dir` is the only call that `run` approves without a prompt.

## Risks
- **Review view.** It is the largest UX piece: it moves from "fits in 6 rows" to "coverage", with resize and zoom to handle.
- **Edit oracle.** Mitigated, not eliminated.
- **Origin policy.** It may surprise users with projects outside HOME, and it is Linux only.
- **Batched writes.** They need the host gate.
- **Deferred-execution files.** They can only be flagged.
- **Spec and README.** They promise "changes only in the user's pane", so they change in the same release.
- **Shim limits.** 32 KiB through the shim with long waits needs a manual check per engine.
- **Dependency.** `create_dir` in `run` depends on auto-execute S1; W0-W4 do not.
