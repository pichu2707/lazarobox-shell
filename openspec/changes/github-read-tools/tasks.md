# Tasks: GitHub Read Tools

## Review Workload Forecast

Only production lines count. Tests, fixtures, snapshots, openspec documents, README and CHANGELOG are excluded.

| PR | Scope | Design estimate | Planned estimate (range) | 400-line risk |
|----|-------|-----------------|--------------------------|---------------|
| 1 | Block and marker (inert until a tool exists) | ~195 | ~215 (195-240) | Low |
| 2 | Pure core (no way on yet) | ~330 | ~340 (330-380) | Medium: the largest, the `mod.rs` parse and helper estimate (110) is the softest number |
| 3 | Host and menu toggle (first releasable step) | ~310 | ~335 (310-380) | Medium: `spawn_capped`, host and `origin_url` are new runtime code |
| 4 | Chat grant and hint | ~220 | ~225 (220-260) | Low |
| Total | | ~1,055 | ~1,115 | |

Why the planned numbers differ from the design: PR 1 gains the blocked-conversation marker in the bar and the system line on resume (about +20, both in the spec, neither in the design table). PR 3 gains the not-a-git-folder distinction in `origin_url` (about +10) and the `skipped > 0` rule moved from PR 1 (about +3). See Order fixes.

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,115 production across 4 PRs (range 1,055-1,260) |
| 400-line budget risk | Low per PR as planned; Medium for PR 2 and PR 3 |
| Chained PRs recommended | Yes |
| Suggested split | 4 stacked PRs, each merged to main after green CI |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main |

Decision needed before apply: No. The planned size of every PR is under 400, the strategy is decided and the owner authorised unattended merge and release.
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Low (PR 2 and PR 3 Medium)

**Budget guard for the unattended run.** Count production lines (`git diff --numstat` minus test files, fixtures, snapshots, docs) before opening each PR. If PR 2 or PR 3 measures above 400, STOP that PR and report; do NOT take `size:exception` silently. Pre-agreed split points if that happens: PR 2 into 2a (registry, `Call::Github`, parse, `slug_of`, argv) and 2b (serde, render, redaction, `GithubRead` ask); PR 3 into 3a (`spawn_capped`, `origin_url`, host, consent) and 3b (blocked `*-cloud` locality, toggle, menu, docs), with the toggle always last so `main` never exposes a half feature.

### Suggested Work Units (commits, one per unit, tests inside)

| PR | Unit | Goal | Commit |
|----|------|------|--------|
| 1 | A | Wording reconciliation and anchors | `docs(github): add change artifacts` |
| 1 | B | Block flag, event, route, `y` | `feat(chat): block auto-run after a github read` |
| 1 | C | Reads ask while blocked | `feat(chat): ask for file reads in a blocked conversation` |
| 1 | D | Marker row and resume | `feat(chat): keep the github block across a resume` |
| 2 | E | Fixtures, family, parse | `feat(tools): add the github family and its three tools` |
| 2 | F | Argv, slug, render, redaction | `feat(tools): render github text as clean data` |
| 2 | G | Consent ask | `feat(chat): add the github read approval` |
| 3 | H | Runner and origin | `feat(runtime): run capped processes and read origin` |
| 3 | I | Host | `feat(tools): run github reads on the host` |
| 3 | J | Cloud locality while blocked | `feat(chat): treat ollama cloud as cloud for blocked reads` |
| 3 | K | Toggle, menu, docs | `feat(config): add the github tool toggle` |
| 4 | L | Grant state and families | `feat(chat): grant github tools for one chat` |
| 4 | M | Command, dialog, probe | `feat(chat): add /github and its dialog` |
| 4 | N | Hint, docs | `feat(chat): hint at /github` |

No attribution lines, conventional commits only. Each commit carries its own tests.

## Order Fixes (design order checked against "never expose half a feature")

The design order (1 block, 2 pure core, 3 host and toggle, 4 grant and hint) holds. Checked: with the toggle off and no grant, nothing a user can reach changes until PR 3, and the toggle is the last thing PR 3 lands. Two details of the design would have broken this and are fixed here.

1. **`skipped > 0` means blocked** (design section 1) was in PR 1. PR 1 has no tool, so a user who once downgraded and has unknown-role rows would see a block with no GitHub involved. Moved to PR 3 (task 3.12), when the tools exist. PR 1 still reads the marker row on resume, which is inert because nothing writes it.
2. **Spec items missing from the design table.** The bar marker for a blocked `run` chat and the system line on resume (pane-execution spec) are added to PR 1 (tasks 1.9, 1.10) so that the first PR that can start a block already shows it.

Also: README and config docs for the toggle go in PR 3 (the toggle and the menu row land there), the `/github` section goes in PR 4. CHANGELOG `[Unreleased]` entries go in PR 3 (user-visible: tools reachable through the menu) and PR 4. PR 1 and PR 2 have nothing user-visible by design.

## Spec and Design Contradictions (do not pick silently)

The coordinator must resolve these before or during PR 1 (task 1.0 amends the artifacts in PR 1). Where a task needs a string, it is written as a constant and the test names the source.

| # | Topic | Spec says | Design says |
|---|-------|-----------|-------------|
| C1 | Block reason shown in the prompt | `auto-run is off: this conversation read GitHub text` | `GITHUB_READ = "this conversation read GitHub: commands are typed, not run"` |
| C2 | Result of `y` while blocked | `typed into pane N — no Enter` | `sent to pane 1; press Enter there to run it (not run: …)` (the MULTI_LINE wording). The em dash in the spec is also non-ASCII for `TERM=linux` |
| C3 | Hint | One system line, names the repository, shortens by dropping the name, not stored | A muted guidance row (`guidance_is_hint`), fixed text `Type /github to let this chat read this repo's issues and PRs`, does NOT name the repository |
| C4 | System line when the block starts | On first read and on resume of a blocked conversation, any mode | "On the first read in `run` mode"; nothing for resume |
| C5 | Grant notice | One notice: tools on from the next message, cloud still asks, commands typed after the first read | The warning text is in the prompt; the answer line is only "GitHub tools on for your next message" |
| C6 | Not a git folder | Distinct error: "folder has no repository" | `origin_url` says exit 1 is no origin and any other failure refuses; a non-repo folder is not named |

Task rule: tests assert the spec's user-visible text unless task 1.0 records a decision otherwise. If no decision exists at apply time, the agent MUST use the spec wording for C1, C2 (ASCII hyphen on a `linux` console), C4, C5, C6 and the spec behaviour for C3 (name the repository, shorten by dropping it), report it in apply notes, and amend the design in PR 1. C3 changes PR 4 size by about +10.

## PR 1: Block and marker (target ~215 production lines)

Branch: `feat/github-read-block` from `main`. Inert: no tool exists, so nothing starts the block. Requests stay byte-identical.

### Phase 0: Anchors and documents (Unit A)

- [x] 1.0 Verify every anchor of design.md with `git show origin/main:<path>` by function name (`route_run`, `y_run_blocker`, `pending_prompt`, `approve_run`, `auto_run`, `answers_itself`, `insert_message`, `resume`, `store_cmds`, `Entry::Tool`). Record drift in "Apply notes" below. Record the decisions for C1 to C6 in design.md and the specs (amend text, keep the two consistent). Commit the whole `openspec/changes/github-read-tools/` directory (proposal, design, tasks, six specs) in this PR.

### Phase 1: Block flag and event (Unit B)

- [x] 1.1 RED `src/core/ai/chat/approval.rs` tests: `route_run` with `AutoCtx.github_read` gives `Route::Prompt(Some(Fallback{ why: GITHUB_READ }))` for a command that would run, checked first; the block does not count toward `MAX_RUN_FALLBACKS` (`counts_toward_pause` exclusion); `auto` and `ask` routes unchanged. Spec: Auto-run block, "Auto and ask unchanged", "Later turns stay blocked".
- [x] 1.2 GREEN: `AutoCtx.github_read`, `GITHUB_READ`, route check, exclusion.
- [x] 1.3 RED chat state tests (`chat/mod.rs`): `GithubStarted` sets `github_read`, also when the event is stale (`live` false); a non-gh tool event does not; an `Error` result after it keeps the flag; a call refused before the spawn sends no event. Spec: "Block starts at the first read", "Refused calls do not start the block", "Started calls that fail still block".
- [x] 1.4 GREEN: the `GithubStarted` tool event (name confirmed in apply), tracking in `on_chat` before `apply`; `auto_route` fills the ctx. Nothing sends the event yet, so the test builds it directly.
- [x] 1.5 RED app tests in the `tool_run_y.rs` style: flag up, keys line is `RUN_KEYS`; `y` produces `WritePty` with no `\r` and `Decision::Sent` with the not-run line; a prompt opened before the read changes on the next frame; a `Checked` in flight falls back through `auto_route(.., true)`; no key does nothing. Spec: "y does not run it".
- [x] 1.6 GREEN: `y_run_blocker` takes the chat id and returns `GITHUB_READ` first; `pending_prompt` and `approve_run` use `RUN_KEYS`; `auto_run` guard types instead of adding `\r`; `toggle_approval` run line says commands are typed while blocked.
- [x] 1.7 RED: `auto` mode while blocked types without a prompt and never sends `\r`; `ask` already only types. Spec: "Auto and ask unchanged". GREEN: only if the test fails.
- [x] 1.8 RED+GREEN "Nothing lifts it": toggle off, mode cycled `ask` then `run`, a tool result saying "auto-run is back on": the next `run_in_pane` still prompts and `y` sends no Enter. Pin test, no new code expected.
- [x] 1.9 RED+GREEN system line when the block starts (per C4: any mode) and when a blocked conversation is resumed; not stored, not sent. Spec: "Block starts at the first read", "Resume stays blocked".
- [x] 1.10 RED `TestBackend` + insta at 80 columns and `TERM=linux`: a blocked chat in `run` does not draw the active ` RUN ` marker and shows that auto-run is blocked, ASCII on the console. GREEN: bar marker. Spec: "Marker does not promise a run".

### Phase 2: Reads while blocked (Unit C)

- [x] 1.11 RED `answers_itself(ask, mode, blocked)` tests: `Read`/`Search` is `None` in `ask`, `auto` and `run` for in-root cloud paths when blocked; unblocked rows unchanged; `Reach::Refused`/`Unshowable` stay free; local engine unchanged. Spec: "Run mode asks for an in-root read", "Ask and auto mode ask each time", "Local engine unchanged".
- [x] 1.12 GREEN: new parameter, `app/tool.rs` passes `chat.github_read`, prompt reason `GITHUB_READ_ASK` through `Fallback{ why }`.
- [x] 1.13 RED app test: one `y` answers one call; a second `read_file` opens its own prompt; the rule applies from the next round of the same turn; the guard's refusal returns without a prompt. Spec: "Ask and auto mode ask each time".

### Phase 3: Marker row (Unit D)

- [x] 1.14 RED store tests (temporary database only, never the real `chat.db`): `MarkGithubRead` inserts one row `role='github_read'`, `content=''`, `status='complete'`; idempotent (`WHERE NOT EXISTS`); not listed by `list`; not returned in `messages`; not counted in `skipped`; `user_version` still 3. Spec: "Resume stays blocked".
- [x] 1.15 RED: opening the same database with `open_with(.., 3)` as an older build works and shows its "not shown" notice.
- [x] 1.16 RED: `StoredMessage.github_read` set once the flag is up, so every later `append`/`create` carries the marker in its own transaction; a failing marker with a later row cannot leave that row unmarked; deleting a conversation cascades the marker, and a reused id starts clean.
- [x] 1.17 GREEN: `StoreCmd::MarkGithubRead`, `github_read` fields in `chat/store.rs`, marker insert and panic table in `worker.rs`, `store_cmds` in `history.rs`.
- [x] 1.18 RED **resume test with a temporary database**: build a conversation, set the flag, close the store, reopen the same temp file, resume: `StoreReply::Resumed.github_read` is true, the block system line shows (1.9), `y` still only types (no `\r`), and `read_file` on a cloud engine in `run` asks (1.11). A conversation with no marker resumes unblocked; a new conversation is not blocked. Spec: "Resume stays blocked", "Other conversations are not blocked", "Resume keeps it". GREEN: `resume` reads the marker, `on_resumed` sets the flag.
- [x] 1.19 RED+GREEN "History off or store detached": the live flag still blocks, nothing is written.

### Phase 4: PR 1 close

- [x] 1.20 (checks only; opening the PR, CI and merge are the coordinator's) `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. Confirm `chat.db` `user_version` is 3 and mode 0600 in the tests that open a temp database. Measure production lines. Open PR to `main`, wait for green CI, merge.
- [ ] 1.21 Chain Context in the PR body: dependency diagram with the current PR marked, start/end, follow-ups (PR 2 to 4), out of scope. No CHANGELOG entry (nothing user-visible).

## PR 2: Pure core (target ~340 production lines)

Branch: `feat/github-read-core` from `main` after PR 1 merged. No way on: no toggle and no host arm. Requests stay byte-identical.

### Phase 0: Fixtures (first, design's unverified items) (Unit E)

- [ ] 2.1 Record real `gh` JSON into `tests/fixtures/github/` with an authenticated `gh` against a small repo the owner controls (or a public one), one file per command: `issue-list.json`, `issue-view.json`, `issue-view-comments.json`, `pr-view.json` (include a failing check, a successful check, a pending one), `pr-view-many-files.json` (`files` shorter than `changedFiles` if it can be produced, otherwise hand-trim and mark the file as trimmed in `tests/fixtures/github/README.md`), `repo-view-fork.json`. Request exactly the `--json` fields of design section 6 and confirm gh accepts them (a bad field is an error). Redact nothing by hand that the tests rely on; strip real private text and tokens before committing.
- [ ] 2.2 Check `statusCheckRollup` shapes: record a check-run entry and a status-context entry (they differ: `name`/`conclusion` versus `context`/`state`). Record `author` (`login`), `labels` (`name`), `files` (`path`, `additions`, `deletions`), `parent` (`owner.login`, `name`).
- [ ] 2.3 Check that `--repo github.com/O/R` is accepted by `gh issue list`, `gh issue view`, `gh pr view` and that `gh repo view github.com/O/R` works; check `gh` aliases cannot shadow `issue`, `pr`, `repo` (try `gh alias set issue ...` in a temporary `GH_CONFIG_DIR`).
- [ ] 2.4 **If a fixture contradicts the design:** the fixture wins. Amend design.md section 6 and 7 and the serde structs in the same commit and list the delta in "Apply notes". Specific cases: a field name differs, change the `--json` list and the struct; `statusCheckRollup` has a third shape, add it to the card and the test; `HOST/OWNER/REPO` is rejected, use `OWNER/REPO` with `GH_HOST=github.com` set explicitly in `Run.env` and update the env filter test of PR 3; an alias can shadow a subcommand, add `GH_CONFIG_DIR` to a temporary empty directory to the PR 3 env plan and note it; a field is missing from the output, make the field `#[serde(default)]` and render a placeholder. If no authenticated `gh` is available to the apply agent: STOP PR 2 before 2.9, report `blocked`, and do not write fixtures from memory.

### Phase 1: Family, tools, parse (Unit E, continued)

- [ ] 2.5 RED `registry.rs`: `Family::Github` has its own bit with nine families; `FamilySet` is `u16`; each family sets only its own bit. RED pin: the "no `git_push`" test (mod.rs:2169) still passes. GREEN: `Family::Github`, `u16`, `1u16 << family as u8`.
- [ ] 2.6 RED `tools/mod.rs`: argument tests per spec "Bad arguments": missing `number`, `number: "7"`, `0`, `-1`, `1.5`, extra key `repo`, duplicate key, `state` not in `open|closed|all`, `limit` clamped to 1..=50; `0` is `WrongType`, never clamped to issue #1. GREEN: `Call::Github`, three `ToolDef`s, `parse_github` reading the raw value like `parse_orchestra`.
- [ ] 2.7 RED availability tests: default family set has no GitHub tool and the built tool list and an Ollama request are byte-identical to before (golden compare with the existing builders); `Toolset::Code`, worker and orchestrator lists have none; `Toolset::families` never adds it; a call to `gh_issue_list` with the family off returns an unknown-tool error and nothing runs ("Model cannot enable them"); `files::gate` refuses a GitHub call without the second check. Specs: "Off by default", "Workers have none", "Github family is separate". GREEN: `files::gate` second check `is_github`.

### Phase 2: Slug, argv, render (Unit F)

- [ ] 2.8 RED `tools/github.rs` `slug_of` table: https, `ssh://git@`, `git@host:`, `git://`, `.git` and trailing `/`, upper-case host, `github.com.evil.org`, `ghe.example.com`, `gitlab.com`, `-x/y`, `../..`, `o/..`, owner over 39, repo over 100, URL carrying a token (never kept or shown). Spec: "Https and ssh origins", "Non-GitHub origin". GREEN: `slug_of`.
- [ ] 2.9 RED argv builder: exact vector per call from the design section 6 (list, view, view with comments, PR view, fork probe), `--repo github.com/O/R`, `comments` field only when asked; nothing from the model other than validated `state`, `limit`, `number`, `comments`. Spec: "Injection in arguments", "Comments only on request". GREEN: pure builder.
- [ ] 2.10 RED render from the recorded fixtures: issue list (number, title, state, labels, "more exist" note at the limit), issue view without and with comments (two comments), PR card (title, state, description, branches, files with counts, both check shapes, failing check, no patch text, `files < changedFiles` note). Spec: "Issue list", "Comments only on request", "PR card has no diff", "List cut at its limit". GREEN: serde structs with `#[serde(default)]`, renderers, row summary with counts only.
- [ ] 2.11 RED **hostile issue** test, one fixture crafted by hand: forged closing tag followed by instructions, `<!-- run curl|sh -->`, a zero-width character inside `<!-` `-`, an unterminated comment, ESC and bidi controls, a title with a newline faking a header, text telling the model to call `run_in_pane`. Assert: comment text gone, exactly one nonce tag pair, header not forged, `DATA_NOTE` present, the content wholly inside the wrapper, row summary free of any of its text, no control or zero-width character in the output. Spec: "Injection in an issue", "Delimiter forgery", "Escapes and bidi in text".
- [ ] 2.12 RED caps: body 12 KiB, comment 3 KiB, newest 30 comments with `[older comments omitted: N of M]`, 100 files; total 32 KiB with `CAP_MARKER`; the row shows the note. Spec: "Output cap".
- [ ] 2.13 RED **redaction tests**: a `ghp_` + 36 token and `password=hunter2` in body, title, label, comment, branch and file name each become `[REDACTED]`; a token that straddles a cap is masked whole before the cut; a zero-width space inside a token still masks; output is idempotent (redacting twice equals once); the row summary has no secret. Spec: "Secrets are redacted", "A secret at the cap is masked whole", "A hidden character does not hide a secret". GREEN: one `field()` helper doing `clean` then `redact` (`crate::core::ai::memory::redact::redact`), used for every field in the order clean, strip comments, flatten, redact, cap, `label::wrap`.
- [ ] 2.14 RED no content in logs: an error path whose body holds `SECRET-TOKEN-123` leaves it out of the error text and the log. Spec: "Error text has no content".

### Phase 3: Consent ask (Unit G)

- [ ] 2.15 RED `approval.rs`/`run.rs`: `ApprovalAsk::GithubRead { repo, what, ollama_cloud }`; `ask_for` returns it with an empty `repo` on `Locality::Cloud` and nothing on local; `answers_itself` returns `None` for it in `ask`, `auto` and `run`; the prompt shows the sanitized `owner/name` and the engine; only lowercase `y` accepts, `n`, Esc, timeout, cancel, closed chat and quit end as `denied`; the one-pending-approval rule gives `another approval is pending`. Spec: "Auto and run do not answer", "Declined consent". GREEN: the variant and prompt.

### Phase 4: PR 2 close

- [ ] 2.16 `cargo test`, clippy, fmt. Measure. Confirm no code path reaches the new tools with the default config. Merge after green CI. No CHANGELOG entry (unreachable). Chain Context in the PR body.

## PR 3: Host and menu toggle (target ~335 production lines)

Branch: `feat/github-read-host` from `main` after PR 2 merged. First releasable step: the owner turns the toggle on in the menu. The toggle is the LAST commit so every earlier commit leaves the tools unreachable.

### Phase 0: External checks first (design's unverified items) (Unit H)

- [ ] 3.1 Record the not-found stderr: run `gh issue view 99999999 --repo github.com/O/R --json number` and the PR equivalent, save `tests/fixtures/github/not-found-issue.stderr` and `not-found-pr.stderr` with the exit code in a sidecar. Also record logged-out (`GH_TOKEN` empty and a temporary `GH_CONFIG_DIR`): stderr text and exit code (design assumes exit 4 and `gh auth login` in stderr). **If a fixture contradicts the design:** the observed text wins; update the matcher and design section 6. If the not-found text is not distinguishable from other failures (for example one generic GraphQL error), match on the combination exit code plus the substring the fixture does show, and if nothing reliable exists, report `partial` and keep the generic "gh failed" message for that case, marking the "Nonexistent number" scenario as not met in the apply notes. Never keep a matcher no fixture supports.
- [ ] 3.2 Verify Ollama cloud naming: from a signed-in Ollama, record `/api/tags` (and `/api/show` for one cloud model) into `tests/fixtures/ollama/api-tags-cloud.json` and `api-show-cloud.json`; confirm models end in `-cloud` or have tag `cloud`, and note whether `/api/show` marks cloud models. **If another name exists** (for example `:cloud-xl`, a `remote_host` field), extend `forwards_off_machine` to cover it, add the test row, and record it; a marker in `/api/show` is only noted for a follow-up because the scope is built from the request without a probe. If no signed-in Ollama is available to the apply agent: keep the design predicate (fails closed on a missing model), record `Ollama cloud naming: unverified` in apply notes and tell the owner in the manual checklist. Do not guess further names.
- [ ] 3.3 Git behaviour: confirm with a temporary repo what `git config -z --get remote.origin.url` returns outside a repository (exit code and stderr, expected 128, "not in a git directory") and on a dubious-ownership repo, and record it. This settles C6.

### Phase 1: Runner and origin (Unit H, continued)

- [ ] 3.4 RED `runner::spawn_capped` with a fake script: separate stdout and stderr; stdout stops at 4 MiB and the group is killed ("flood stops at the cap"); stderr capped at 8 KiB; a hang is killed at 30 s with a fake clock or a short bound and a pid check proves no survivor; a dropped future kills the group; ETXTBSY retry reused. Spec: "UI stays responsive". GREEN: `spawn_capped`.
- [ ] 3.5 RED `origin_url` on a real temporary repo: plain, linked worktree (reads the common config), subfolder, no origin (exit 1), `insteadOf` not applied (raw value), not a repository (distinct result per 3.3, maps to "the folder has no repository"). Spec: "No origin", "Not a git folder", "Https and ssh origins". GREEN: `runtime/tools/git.rs::origin_url`.

### Phase 2: Host (Unit I)

- [ ] 3.6 RED host tests with a fake `gh` (pattern `runtime/gh.rs:186`) recording argv, `pwd` (`/`) and env: argv matches PR 2 builder; env drops `GH_HOST`, `GH_REPO`, `GH_ENTERPRISE_TOKEN`, `GITHUB_ENTERPRISE_TOKEN`, `GH_DEBUG`, `DEBUG`, `GH_PAGER`, `PAGER` and sets `GH_PROMPT_DISABLED=1`, `GH_NO_UPDATE_NOTIFIER=1`, `NO_COLOR=1` and the login `PATH` (`ToolHost.gh_path`); `GithubStarted` is sent before the spawn and nothing is sent when refused before it. Spec: "Injection in arguments", "Block starts at the first read".
- [ ] 3.7 RED exit mapping, using the fixtures from 3.1: missing `gh`, exit 4 and `gh auth login`, timeout, not found (`#9999 was not found in o/r`), over the cap (mentions comments), exit 0 with bad JSON, other failure (120 characters, no content: stderr holding `SECRET-TOKEN-123` never reaches the row or the log). Each is `ToolStatus::Error`. The block starts for every case after the spawn and for none refused before it ("Started calls that fail still block", "Refused calls do not start the block"). Specs: all "Visible failures" scenarios.
- [ ] 3.8 RED origin gating: no origin, not a repository and non-GitHub origin (including `ghe.example.com`) refuse before any prompt and any spawn; folder change makes the next call read the new `origin` ("Folder change moves the repository").
- [ ] 3.9 RED fork: the probe runs once per slug and is cached on success; the result carries `note: O/R is a fork of P/Q; the parent is not read`; no call is made for the parent; a failed probe adds `note: fork status unknown` and the read goes on. Spec: "Fork names the parent".
- [ ] 3.10 RED consent in the host: local engine never asks; a cloud engine asks once per repo per turn (`github:<slug>` in `scope.allowed`) and not for the second and third call; the next turn asks again; a decline sets `github-denied:<slug>`, shows a notice and later calls in that turn return `denied` without a prompt; `n`, Esc, timeout and cancel are `denied`; the prompt appears before `gh` starts. Specs: "Cloud engine asks once per turn", "Next turn asks again", "Declined consent", "Local engine reads without asking". GREEN: `host/github.rs`, arms in `host.rs`, scope fields in `runtime/chat.rs`, `gh_path` set at `runtime.rs:301`.

### Phase 3: Cloud locality while blocked (Unit J)

- [ ] 3.11 RED `forwards_off_machine(engine, model)` table: `gpt-oss:120b-cloud`, `x:cloud`, upper case, no model on Ollama (cloud, fail closed), a local model, `claude` (not Ollama), plus every extra name found in 3.2. RED host test: an Ollama `*-cloud` model asks for GitHub reads while `find_dir`/`list_dir`/`list_panes` stay free; while blocked, `read_file` and `grep` ask, also in-root and in `run`; unblocked they stay free; a local model never asks; the per-turn bit set by `GithubStarted` mid-turn applies to the next call. Specs: "Ollama cloud model asks", "Ollama cloud model asks while blocked", "Ollama cloud model differs by tool". GREEN: `TurnScope.ollama_cloud` and `github_read`, `ReadFile`/`Grep` arms pass `Locality::Cloud` while blocked, `Kind::locality` unchanged.
- [ ] 3.12 RED resume with `skipped > 0` (temp database holding an unknown-role row) opens blocked; with `skipped == 0` and no marker it does not. GREEN (moved here from PR 1; see Order Fixes).

### Phase 4: Toggle, menu, docs (Unit K, last commit)

- [ ] 3.13 RED configuration: `[ai.tools]` with no key gives six toggles off and no notice; `github = true` gives GitHub on and the other five off; `github = "yes"` gives `config: ai.tools.github: expected a boolean`; `enabled = false` with `github = true` offers nothing; an old file saved by the menu gains no `github` key unless the row is changed; an old build ignores the key. Specs: configuration scenarios. GREEN: `ToolToggle::Github` (key `github`), `ALL` and `toggles` to six, `core/config.rs:159` follows `ALL`.
- [ ] 3.14 RED menu snapshots (`TestBackend` + insta, 80 columns and `TERM=linux`): Tools tab rows in the order `Tools, Search, Write, Exec, Git, Git write, GitHub, Approval, Orchestrator`; nine `j` presses wrap; toggle saved writes `github = true`; the next turn offers the three tools. GREEN: `family!("GitHub", Github, "github", 66, true, None)`; update the existing tab-order tests per the config-menu spec.
- [ ] 3.15 RED end-to-end with fakes: toggle on, local engine, `gh_issue_list` returns three issues in the wrapper; toggle off again and requests are byte-identical to before the change.
- [ ] 3.16 Docs: README (neutral Spanish prose, repo convention) section on the toggle, the three tools, consent, the auto-run block ("after reading an issue, commands are typed and need Enter for the whole conversation, including tests"), `gh` requirements, `gh auth login`; config documentation for `[ai.tools] github`. CHANGELOG `[Unreleased]` `### Added`: GitHub read tools behind the `[ai.tools] github` toggle. No attribution.
- [ ] 3.17 `cargo test`, clippy, fmt, measure. Merge after green CI. Release is the owner-authorised step after PR 4 or after PR 3 (coordinator decides; PR 3 is releasable on its own: see "Release small and often").

## PR 4: Chat grant and hint (target ~225 production lines)

Branch: `feat/github-read-grant` from `main` after PR 3 merged.

### Phase 1: Grant state and families (Unit L)

- [ ] 4.1 RED `offered_families(id)`: saved set without a grant; granted adds `Github` only when `tools_enabled()`; master switch off with a grant offers nothing ("Master switch wins"). RED: `tools_reserve_tokens(request.families)` equals the chat's reserve with and without a grant ("Grant counts in the budget"); both readers (`app/tool.rs:501`, `app/chat.rs:610`) use the function. GREEN: `ChatState.github_granted`, `offered_families`.
- [ ] 4.2 RED grant lifetime table, one test per row: ends on pane close, new chat not inherited, resume ends (cleared in `resume_into`), `/cd` and folder restore end with the system line "GitHub tools are off because the folder changed", master tools off then on ends, toggle off to on to off in menu saves ends it for every chat; engine or model change keeps it and a cloud engine still asks. The auto-run block survives all of these. Specs: all "The chat grant" scenarios, "Consent right after a grant". GREEN: the clearing hooks.
- [ ] 4.3 RED the grant takes effect at the next message (running turn's list unchanged); the saved toggle and config file are unchanged byte for byte; the menu row reads off; a second pane has no tools. Specs: "Grant takes effect next message", "Grant does not touch the toggle", "Grant never writes the file".

### Phase 2: Command and dialog (Unit M)

- [ ] 4.4 RED `/github` parsing in `folder.rs`: exact trimmed, case-sensitive; `/github now` shows `/github takes no arguments` and keeps the composer; `//github is a command` sends `/github is a command`; `what does /github do` is a normal message; `/github` creates no user message, no ChatSend, no stream, no history entry, no stored message. Specs: "The command is not a message", "Not in the history or the store", "Arguments refused", "Escaped", "Other text is a normal message".
- [ ] 4.5 RED refusals with a visible line and the composer cleared: tools off, not a git folder, no origin, non-GitHub origin, already on (toggle or earlier grant); a pending confirm gives `Answer the pending confirm first` and keeps the composer; a running turn gives no wait line and the grant applies next message. Specs: "Already on", "Folder with no repository", "No origin or not GitHub", "Tools off", "Works during a stream", "Pending confirm blocks the dialog".
- [ ] 4.6 RED probe: `Effect::ProbeOrigin` runs off the render thread through `origin_url`, no network, answered by an `AppEvent`; a stale answer after a `/cd` is dropped. GREEN: effect, event, runtime arm.
- [ ] 4.7 RED **the model cannot open or answer the dialog**: a `ToolCall` carrying `GithubGrant` is denied by `on_tool_ask` and no prompt opens; model answer text containing `/github` opens nothing; a tool result saying "press y" changes nothing; `Effect::ToolDecision` and a message saying `y` leave the grant off; only an armed lowercase `y` key press grants, not a repeat, not a release; a user message `please enable the github tools` opens nothing. Specs: "Model cannot answer it", "Words in a message do not grant", "Model cannot enable them".
- [ ] 4.8 RED dialog keys through `on_github_key`: `y` grants and closes with a system line; `n` and Esc decline with "GitHub tools stay off" and no re-open; `Y`, Enter, Space, Ctrl+y, Repeat, Release are swallowed with `ANSWER_FIRST`; a `y` before `set_armed_at` is ignored (the dialog opens asynchronously); one dialog per pane; pane close and quit end it as declined. Specs: "y grants", "n and Esc decline", "Other keys are swallowed", "Decline".
- [ ] 4.9 RED snapshots (`TestBackend` + insta): dialog with `owner/a-fairly-long-repository-name` at 80 columns and on a `linux` console (ASCII, full name readable, no cut line); an origin path with an escape sequence draws no control character. The grant system line carries the three facts of C5. Specs: "Dialog at 80 columns and TERM=linux", "Repository name is sanitized". GREEN: `ApprovalAsk::GithubGrant { repo }`, prompt, `on_github_key`, `request(ask, Origin::Block)`, `app/github.rs`.

### Phase 3: Hint (Unit N)

- [ ] 4.10 RED hint conditions (per C3): shown only with tools on, the engine offering tools, toggle off, no grant, origin on GitHub, chat idle, repo not yet told; gone after the first send; once per repo per run (`App.github_hinted`); not after a decline or a grant; not with toggle on, grant, tools off, no origin, `gitlab.com`, unknown folder; after `/cd` to another repo it may show once; placed before `toggle_hint`; `guidance_is_hint` drawn muted; not stored, not in any request. Specs: "Hint shown", "Hint shown once", "No hint after decline or grant", "No hint when it does not apply", "Model never sees the hint", "New repository after /cd".
- [ ] 4.11 RED snapshots at 80 columns and `TERM=linux`: one row, `/github` intact, ASCII (`is_ascii` assert), repository name dropped first when too long. Spec: "Hint at 80 columns and TERM=linux".
- [ ] 4.12 Docs: README section for `/github`, the hint, grant lifetime, the separate cloud consent. CHANGELOG `[Unreleased]` `### Added`: `/github` and the hint.
- [ ] 4.13 `cargo test`, clippy, fmt, measure. Merge after green CI.

## Verify (after PR 4, by the verify phase)

- [ ] V.1 Every spec scenario maps to at least one test above; list any scenario without a test.
- [ ] V.2 No test touches the real `chat.db`, config or network; `git grep -n "chat.db"` in new tests shows only temporary paths.
- [ ] V.3 The "no `git_push`" test and the byte-identical request test pass with the toggle off.

## Owner-only manual checklist (NOT blocking any merge or release)

Run after PR 3 and again after PR 4. Tests cannot show these.

- [ ] M.1 Real `gh`, real repo: list issues, open one, open one with `comments: true`, read a PR card with checks, on a local model.
- [ ] M.2 Real grant dialog (PR 4): `/github` shows the repository name, `y` grants, `n` and Esc decline, `Y` and Enter do nothing; ask the model "enable github tools" with the toggle off and see that nothing opens.
- [ ] M.3 80 columns and `TERM=linux`: the dialog, the hint, the Tools tab row and the bar in a blocked `run` chat are readable, ASCII only, no cut line.
- [ ] M.4 Real cloud engine (`claude`, and an Ollama `*-cloud` model): consent prompt names the repo, once per turn, asked again next turn; after a read, `read_file` asks on each call; no secret from a test issue appears in the model's answer.
- [ ] M.5 Block: read an issue, then ask the model to run `git status`; `y` types it and you must press Enter; close and resume the conversation and see it still blocked; a new conversation is not blocked.
- [ ] M.6 Logged-out `gh` and a folder with no `origin` show the expected messages.
- [ ] M.7 Check the `gh` JSON fields on a repo with a different shape (a fork, a draft PR, a PR with more than 100 files if available) after the fixtures.

## Apply notes

### PR 1 (Unit A, task 1.0)

Anchors checked by function name against the worktree (main at 17a1e20f): `route_run` (approval.rs), `y_run_blocker`, `pending_prompt`, `approve_run` (app/run.rs), `auto_run` (app/auto_run.rs), `answers_itself` (approval.rs, called from app/tool.rs `on_tool_ask`), `insert_message`, `resume` (runtime/chat_store/worker.rs), `store_cmds` (chat/history.rs), `Entry::Tool` (chat/tool.rs, mod.rs), `counts_toward_pause`, `auto_route`, `toggle_approval`, `on_chat` (app.rs), `on_resumed` (app/history.rs). No drift beyond line numbers. `StreamEvent` has no tool-event variant of its own: `GithubStarted` is a new `StreamEvent` variant (the app's `ToolEvent::of` reads `StreamEvent`).

Decisions for C1 to C6 (spec wording wins; design.md amended to match):
- C1: `GITHUB_READ = "auto-run is off: this conversation read GitHub text"`.
- C2: the result line of `y` is the string that already exists in `run_sent_line` (`sent to pane N; press Enter there to run it (not run: <reason>)`). The spec's `typed into pane N — no Enter` was replaced by it in the pane-execution spec (two places). Reason: the form already exists in code, and the spec's em dash is not ASCII for `TERM=linux`. The keys line is the existing `RUN_KEYS` (`y type it (no Enter)`).
- C3: spec behaviour (name the repository, drop the name when too long); design section 8 amended. PR 4 grows by about 10 lines.
- C4: system line on the first read in any mode and on resume of a blocked conversation; design section 2 amended. Both lines ASCII.
- C5: one grant notice with the three facts; design grant dialog section amended (PR 4).
- C6: distinct "the folder has no repository" refusal; design section 6 amended (PR 3).

### PR 1 (Units B, C, D)

Implemented in one code commit (the three units share hunks in approval.rs, app/tool.rs and chat/mod.rs, and the `github_read` field of `StoredMessage` touches test constructors in six files). Names as built:
- `StreamEvent::GithubStarted` (new variant; tracked in `App::on_chat` before `apply`, passed through `runtime::chat::Pipe`). `ChatState::{github_read, note_github_read, adopt_github_read}`.
- `approval.rs`: `GITHUB_READ`, `GITHUB_READ_ASK`, `GITHUB_BLOCK_LINE` ("auto-run is off here: GitHub text was read; commands are only typed", shown at the start and on resume, ASCII), `AutoCtx.github_read`, `answers_itself(ask, mode, blocked)`, `blocked_read_reason`.
- Marker: `StoreCmd::MarkGithubRead`, `StoredMessage.github_read`, `StoreReply::Resumed.github_read`, role `github_read` in `worker.rs`; `user_version` stays 3. `store_cmds` stamps every outgoing row with the flag, and sends `MarkGithubRead` only when the conversation exists (a fresh one marks itself through its first row).
- Bar marker: ` RUN OFF ` (one letter `r` when narrow) replaces ` RUN ` for a blocked chat in `run`; `Prefix a` run line says blocked.

Deviations: (1) Task 1.10 asserts the drawn buffer (text, colours, ASCII) and, after review, also an insta snapshot of the bar at 80 columns (`blocked_run_marker_80x6`). (2) Task 1.8 has no GitHub toggle yet (PR 3); the test cycles the mode and sends a tool result that says "auto-run is back on". (3) `WorkerRead` outside a worker's scope is covered by the blocked-read rule after review; a worker read inside its scope and a worker `grep` are not (see design, Residual risks). (4) Units B, C, D are one commit. (5) `y_run_blocker` and the pending prompt now take the chat id.

Sizes: about 246 production lines added and 26 removed (comments and doc lines included), under the 215-240 plan plus the system lines; tests, fixtures and docs excluded.

Review fixes (after the first apply): F1 `WorkerRead{cloud:true}` joins `blocked_read_reason` and the `answers_itself` exclusion; in-scope worker reads and worker `grep` stay unasked (documented in the spec and design). F2 the blocked reason leads the head row of a short prompt that has no spare row. F3 `GITHUB_BLOCK_LINE` and the new `RUN_BLOCKED_LINE` are at most 80 columns, with a test. F4 insta snapshot for the bar marker. F5 `pending_prompt` derives the block reason for a `run` prompt opened before the read, and a worker read prompt shows its reason. Gap: worker test that a Create with the marker stores it before the first row and resumes blocked.
