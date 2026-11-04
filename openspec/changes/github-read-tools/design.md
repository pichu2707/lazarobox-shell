# Design: GitHub Read Tools

One new family, `Github`, with three read tools that run `gh` on the host from a fixed argv and a slug lazarobox read from `origin`. The results are rendered by us and wrapped as data. A session grant is one field on `ChatState`, and one function turns it into the families for both the tool list and the budget. The auto-run block is a chat flag read by the route, `y` and the one door that presses Enter. It survives a resume through a marker row in `messages`: **no `chat.db` schema change**.

> Code refs are from `main` at `17a1e20f` (v1.5.0), read with no shell available. Lines may drift; function names are the anchor. `TOOLS_RESERVE_TOKENS` is gone; the reserve is `tools_reserve_tokens(families)`, adapters.rs:320.

## Data flow

```
/github (user only) ─ parse_command ─ Effect::ProbeOrigin ─ AppEvent ─ dialog ─ y ─ chat.github_granted
                                                                                      │
send_with ── App::offered_families(id) ──┬── tools_reserve_tokens (budget)            │
                                         └── offer_tools → request.families ◄─────────┘
host::execute: gate → origin → slug → [cloud? consent once per repo per turn] → GithubStarted → gh → render (clean, redact) → label::wrap
      │  (channel is FIFO; GithubStarted is sent before gh is spawned, so before any result)
      ▼
on_chat: GithubStarted → chat.github_read = true
      ├─ StoreCmd::MarkGithubRead, and every later row carries the marker in its own transaction
      ├─ AutoCtx.github_read, y_run_blocker, auto_run guard → typed, never run
      └─ answers_itself(…, blocked) → read_file and grep on a cloud engine ask every time
resume: messages row role='github_read' → StoreReply::Resumed.github_read → flag set again
```

## Answers to the design questions

### 1. The block across a resume

**It cannot be derived from what is stored today.** Tool rows are never stored (`Entry::Tool`, mod.rs:194: "shown, never stored"), `insert_message` (worker.rs:1188) writes only `user` and `assistant`, and `StoreReply::Resumed` says no system lines or reasoning exist. Only the model's prose about an issue survives, and text cannot prove a read.

**A marker row works with no schema change.** Schema v3 dropped the role CHECK so that "a new profile, role (`tool`) or status needs no table rebuild" (chat_store.rs:104). `resume` (worker.rs:1129) already leaves out and counts a role it does not know. So one `INSERT` with `role='github_read'`, `content=''`, `status='complete'` adds no DDL and keeps `user_version` at 3.

| Option | Tradeoff | Decision |
|---|---|---|
| Marker row in `messages` | No DDL; older builds open the DB. A downgrade shows "1 saved message(s) from a newer version are not shown" and does not enforce the block | **Chosen** |
| New column or table with `user_version` 4 | Clean, but `StoreError::Newer` makes older builds refuse the whole history | Rejected |
| Reserved tag | `replace_tags` deletes tags on any edit, and the user sees it | Rejected |
| `turn_usage` row | Pruned by age (worker.rs:539), and `SET NULL` on delete | Rejected |
| Sentinel in assistant text | The model can forge it, and it is sent to engines | Rejected |

**Readable after trimming.** `fit` and `MAX_HISTORY_MESSAGES` only shape the request. `resume` reads every row `ORDER BY seq`, and the only `DELETE` that reaches `messages` removes a whole conversation (worker.rs:839, 1180). `list` filters `role='user'`, and those are the only two readers.

**Deleted, forked, copied.** No per-message delete or edit exists. `StoreCmd` and `ListOutcome` have no fork or copy. Deleting a conversation cascades the marker with it, so a reused id starts clean. Another pane holding it open keeps its in-memory flag after `detach_store_deleted`.

**Failure direction: stay blocked.**

| Case | Result |
|---|---|
| Marker write fails, a later row succeeds | Cannot happen: `store_cmds` sets `StoredMessage.github_read` once the flag is up, and `append`/`create` insert the marker (`WHERE NOT EXISTS`) in the row's own transaction |
| Marker write fails, nothing later is stored | Blocked live, `SAVE_FAILED_LINE` shows, and the store holds no text from after the read |
| Crash before the marker | The answer is saved only at turn end or close, so no tainted text is stored either. `Resumed` holds no CLI session id, so nothing returns through an engine |
| Resume query fails | `ResumeFailed`: nothing opens |
| Unknown role rows (`skipped > 0`) | Treated as blocked: a newer build may have written tool-derived rows |
| History off or store detached | Live flag only; the conversation cannot be resumed |
| A second lazarobox process has it open | Not blocked until its next resume, and it does not hold the issue text until then (residual) |

### 2. Where the block enters the route

`paused` only gates `route_run` and resets each turn (mod.rs:1408). The block is a new `AutoCtx.github_read`, filled in `auto_route` (tool.rs:293), checked **first** in `route_run`, and added to the `counts_toward_pause` exclusions. It gives `Route::Prompt(Some(Fallback{ why: GITHUB_READ }))`, so the prompt shows the reason.

`y` has its own decision. `RUN_NOW_KEYS` is chosen in `pending_prompt` (run.rs:556) and `approve_run` (run.rs:284), and both go through `y_run_blocker`. The block follows the `MULTI_LINE` route exactly: `y_run_blocker` gains the chat id and returns `GITHUB_READ` first. The keys line becomes `RUN_KEYS` ("y type it (no Enter)"), `y` calls `type_run`, and the line reads "sent to pane 1; press Enter there to run it (not run: …)". **No key does nothing.** It is computed per frame, so a prompt opened before the read changes too.

`GITHUB_READ = "auto-run is off: this conversation read GitHub text"` (the spec wording, C1; ASCII).

Other paths: `Checked` re-routes through `auto_route(…, true)` (auto_run.rs:238), so a check in flight falls back. `auto_run` is the one door that adds `\r` (its module doc), so it gets a guard that types instead. `Prefix e` and `auto` already never press Enter. `toggle_approval`'s `run` line must say commands are typed when the flag is up, because its usual text would be false. One system line says the same when the block starts (first read, in any mode) and when a blocked conversation is resumed (C4); both lines are ASCII, not stored and not sent. The result line of `y` is the existing one, `sent to pane N; press Enter there to run it (not run: auto-run is off: this conversation read GitHub text)`; the spec's `typed into pane N — no Enter` was replaced by it (C2: the string already exists in `run_sent_line`, and the em dash is not ASCII).

**What starts the block:** `gh` being started for a GitHub tool call (spec: pane-execution). The host sends a new tool event, `GithubStarted` (name to confirm in apply), through its reserved slot immediately before it spawns `gh`. So a not-found number, a timeout, a logged-out `gh` and a dropped future all start the block, because `gh` ran. A call refused before the spawn (no `origin`, a non-GitHub origin, missing `gh`, bad arguments, declined or denied consent) sends nothing and does not. Tracking is in `on_chat` **before** `apply` and ignores `live`, so a stale event still blocks. The event is FIFO-ordered before any `ToolResult` of that call, and calls of the same round cannot depend on it.

### 2b. Reads in a blocked conversation

Decided by the owner: once a conversation is blocked, on a cloud engine every `read_file` and `grep` MUST ask for `y` each time, in every mode, including in-root paths. Local engines are unchanged. This is about 12 lines in PR 1 and 10 in PR 3.

- **Existing behaviour.** `ask_for` already returns `Read`/`Search` on `Locality::Cloud`, and only `answers_itself` frees the in-root ones in `run` mode (approval.rs:737, :729). `ask` and `auto` already ask for them. So the work is to stop `answers_itself` answering while blocked.
- **PR 1, the app side.** `answers_itself(ask, mode, blocked)` gets the chat's `github_read` flag; `app/tool.rs:163` passes it. With `blocked`, a `Read` or `Search` returns `None` in every mode, except `Reach::Refused`/`Unshowable` (the path guard's own refusal, which opens nothing and leaks nothing, stays as today). The prompt shows the reason `GITHUB_READ_ASK = "this conversation read GitHub: reads of your files need your y"` through the same `Fallback{ why }` path. The flag is the same `chat.github_read`, so the resume marker feeds it with no extra code.
- **PR 3, the Ollama `*-cloud` part.** Today `ask_for` receives `Locality::Local` for an Ollama `*-cloud` model, so no ask exists to answer. `TurnScope.ollama_cloud` (section 5) plus a per-turn `github_read` bit on the scope, set from the chat flag at turn start and set again by the host when it sends `GithubStarted`, make the `ReadFile`/`Grep` arms pass `Locality::Cloud` to `ask_for`. `Kind::locality` is unchanged; this narrow `*-cloud` carve-out now applies to this one path as well as to GitHub reads.
- **Read-family tools checked.** `read_file` and `grep` return file content and are covered. `find_dir`, `list_dir` and `glob` return names only and are not covered (left for the owner). `git_diff` already asks on a cloud engine; `git_log` and `git_status` return commit text and names and are not covered (left for the owner).

### 3. The session grant

`ChatState.github_granted`, plus one function used at tool.rs:501 and chat.rs:610, the only two non-test readers of `ai.families()`:

```rust
fn offered_families(&self, id: PaneId) -> FamilySet {
    let saved = self.ai.families();
    let granted = self.ai.tools_enabled()
        && self.panes.get(&id).and_then(PaneState::chat_state)
            .is_some_and(ChatState::github_granted);
    if granted { saved.with(Family::Github) } else { saved }
}
```

A pane holds one conversation for life: `open_list` is only called from `new_chat_state`, and a new chat is a new pane (`open_chat_pane`).

| Event | Grant | Why |
|---|---|---|
| Pane closes, app exits | Ends | The state is gone; never stored |
| New chat | Not inherited | New `ChatState` |
| Resume | Ends (cleared in `resume_into`) | Today it can only be false there; pinned so it stays so |
| `/cd`, folder restore | Ends, with a visible line | The dialog named a repo and the folder may now be another |
| Engine or model change | **Stays** | The grant says the tools may exist. Consent is what depends on the engine, and it is asked per turn |
| Master tools off | Ends (`sync_tools`) | Nothing is offered |
| Saved toggle turned on then off in a menu save | Ends for every chat | Proposal: turning the toggle off removes the tools |

The auto-run block is separate and survives all of these except a new conversation.

### 4. `FamilySet` and toggles

`FamilySet(u8)` becomes `u16`, with `1u16 << family as u8` in `with` and `contains`. It has no serde derive and is never stored; it only passes through `TurnScope` and `hub.open`, so nothing persistent changes. `ToolToggle` gains `Github` (key `github`), `ALL` and `toggles` become 6 (config.rs:206, 413, 1096). Reads are by key, so old configs default off and older builds ignore the key. `core/config.rs:159` follows `ALL`. Menu row: `family!("GitHub", Github, "github", 66, true, None)`; rank 66 is free (Git write 65, Approval 70). The caution is on because private text can reach an engine. `max_rounds` stays as is: these are not coding tools. `Toolset::families` never adds it, so workers and the orchestrator have none, and `files::gate` gets the second check `is_github`.

### 5. Cloud consent

`ApprovalAsk::GithubRead { repo, what, ollama_cloud }`. `ask_for` returns it with an empty `repo` when `locality == Cloud`, and the host fills it in (the `Panes` pattern). The host mirrors the `EditFile` arm (host.rs:644) with keys in `scope.allowed`, which is new per turn: `github:<slug>` skips the prompt, and `github-denied:<slug>` denies at once, so a decline does not re-prompt in that turn. `answers_itself` already returns `None` for any other ask, and `route` gives `Prompt(None)`, so no mode answers it.

`Kind::locality` is untouched. `TurnScope.ollama_cloud: bool` is set at runtime/chat.rs:949 from `github::forwards_off_machine(engine, model)`: engine `ollama` and a model whose name ends in `-cloud` or whose tag is `cloud`, compared ASCII case-insensitively. An Ollama request with no model counts as cloud, failing closed. Only the GitHub arm reads it, passing `Locality::Cloud` to `ask_for`. Every other tool and `run_scope` stay as they are.

**Expired or changed engine:** `TurnScope` is built per turn from the request, so a new turn or engine has an empty `allowed` and asks again. A turn that ends withdraws its prompt and the runtime denies it (tool.rs:106). A consent cannot outlive its turn.

### 6. Running `gh`

```
gh issue list --repo github.com/O/R --state S --limit N --json number,title,state,author,labels,updatedAt
gh issue view N --repo github.com/O/R --json number,title,state,author,labels,body,createdAt,updatedAt,url[,comments]
gh pr view N --repo github.com/O/R --json number,title,state,isDraft,author,body,baseRefName,headRefName,files,changedFiles,additions,deletions,statusCheckRollup,url
gh repo view github.com/O/R --json isFork,parent
```

| Topic | Choice | Rejected (why) |
|---|---|---|
| Argv | Built in pure `tools/github.rs` from an enum and integers. `--repo` uses `HOST/OWNER/REPO`, so a user logged only into Enterprise cannot be redirected by gh's default host | `OWNER/REPO` alone. A model-supplied repo (out of scope) |
| Issue number | `parse_github` reads the raw value and refuses `< 1` with `WrongType`, like `parse_orchestra`. `ArgKind::Int` alone clamps, so `0` would silently read issue #1. `limit` may clamp (1..=50, default 20). `state` is checked against `open`, `closed`, `all` | Table-only parse |
| Cwd | `/`: `--repo` makes the folder irrelevant, and a hostile repo folder holds nothing gh reads | The chat folder |
| Environment | The inherited one minus `GH_HOST`, `GH_REPO`, `GH_ENTERPRISE_TOKEN`, `GITHUB_ENTERPRISE_TOKEN`, `GH_DEBUG`, `DEBUG`, `GH_PAGER`, `PAGER`, plus `GH_PROMPT_DISABLED=1`, `GH_NO_UPDATE_NOTIFIER=1`, `NO_COLOR=1`, and `PATH` set to the login PATH. A pure filter, passed as `Run.env` | An allow-list: gh's keyring needs D-Bus and session variables, so login would break. The host has no login PATH today: add `ToolHost.gh_path: OnceLock<OsString>`, set at runtime.rs:301 |
| Runner | New `runner::spawn_capped(run, stdout_cap)`: separate stdout and stderr, same `GroupKill`, `wait_exit_unreaped`, ETXTBSY retry. At the cap it stops reading and kills | `spawn_core`: it merges stderr into stdout and `Capture` keeps head and tail at 32 KiB, which breaks JSON. `runtime/gh.rs`: uncapped stdout |
| Limits | 30 s per run; stdout 4 MiB; stderr 8 KiB | |
| Cancel | Dropping the future kills the group, as `spawn_core` does. A dropped call gives no `Ok` result, so no block and no text | A cancel channel like `run_gh`: the host drops futures |
| Origin | New `runtime/tools/git.rs::origin_url`: same unjailed call as `config()` (:446) with its env allow-list, `git config -z --get remote.origin.url`, cwd = `root_of(origin)`. Raw value: `git remote get-url` would apply `insteadOf`. A linked worktree reads the common config. Exit 1 is no origin; any other failure refuses | Extending the regexp in `config()`: it is only reached through `open`, which needs the jail, and these tools do not. `gh repo set-default` |
| Slug | Pure `slug_of`: `https://`, `ssh://git@`, `git@…:`, `git://`, with an exact, ASCII case-insensitive host `github.com`, optional `.git` and `/`. Owner `[A-Za-z0-9-]{1,39}`, repo `[A-Za-z0-9._-]{1,100}`, neither `.` nor `..`, no leading `-`. Anything else is "origin is not on github.com". A token in the URL is never kept or shown | Substring host test (`github.com.evil.org`) |
| Fork | `gh repo view … --json isFork,parent` before the first read of a repo, cached by slug on the host for the run (success only). It reads names only, then a line: `note: O/R is a fork of P/Q; the parent is not read`. If the probe fails the read goes on with `note: fork status unknown` | Reading the parent. `gh api`, for no gain |

**Exit mapping.** No `gh` on the login PATH: "gh is not installed: https://cli.github.com". Exit 4 or stderr naming `gh auth login`: "gh is not logged in: run gh auth login in a shell pane". Timeout: "gh timed out" (spec text: GitHub did not answer in time). Issue or PR not found (stderr naming it, to confirm from a recorded fixture): "#N was not found in O/R". Over the cap: "the reply is too large" (with comments: "try without comments"). Exit 0 with bad JSON: "gh returned something unreadable". Other: `bounded_error` of stderr, 120 characters. All are `ToolStatus::Error`, so they show in the tool row and tell the model; the ones that happen after `gh` was started (exit 4, timeout, cap, bad JSON, not found, other) have already started the block. A missing `gh` has not. No origin or a non-GitHub origin is refused before any prompt, so `y` is never offered on a read that cannot run. A logged-out `gh` can only be known after the consent. **C6:** a folder that is not a repository is a distinct refusal, "the folder has no repository", separate from "no origin"; `origin_url` returns a distinct result for it (exit 1 is no origin, a non-repository exit is the other, anything else refuses).

### 7. Rendering

`gh_pr_view` needs **one** `gh` call: `files` and `statusCheckRollup` come in the same `--json`. If `files.len() < changedFiles` the card says how many are not listed.

Typed serde structs with `#[serde(default)]`, so an unknown field is ignored and we write every line. Order: `clean` each attacker field first, because zero-width characters can hide `<!-\u{200b}-`; then strip HTML comments, an unterminated one to the end; then flatten one-line fields (title, labels, login, branches, check names) so a title cannot fake a header; then **redact** each field with the `read_file` function (below); then cap per field; then `label::wrap`, which cleans again, caps at 32 KiB with `CAP_MARKER` and adds the nonce and `DATA_NOTE`. Caps: body 12 KiB, comment 3 KiB, newest 30 comments, 100 files, each with its own note such as `[older comments omitted: 12 of 42]`. The row summary is counts only (`issue #12, 3 comments`), never attacker text. `Call::argument` shows `#12 comments`, so the option is visible.

**Secret redaction.** The function is `crate::core::ai::memory::redact::redact(&str) -> Redacted { text, hits }` (checked: `read_file` in `runtime/tools/fs.rs` uses `redact(&clean(&text)).text`, and so do `grep` and `run_in_pane` output). The pipeline is `clean`, strip comments, flatten, **`redact`**, cap, `label::wrap`. It runs on every field the model sees, comments included, and before the cap so a cut can never split a secret (a mask is already one unit). The indication is what `read_file` gives: the `[REDACTED]` mask in the text, with no extra notice; the row summary stays counts only. One `field()` helper in `tools/github.rs` does `clean` plus `redact` so no field is skipped. This is about 10 lines in PR 2.

### 8. The hint

`chat_guidance` (chat.rs:470), in `.or_else(|| self.github_hint(id))` placed **before** `toggle_hint`, because that one is on for every idle pane while the orchestra gate is on. `guidance_is_hint` is extended, so it is drawn muted and does not stand in for the transcript. `viewport_rows` already counts its rows.

Text (C3, spec behaviour): one muted guidance row that names the repository, for example `Type /github to let this chat read owner/name's issues and PRs`, ASCII, one row at 80 columns and safe in `TERM=linux`. When too long it shortens by dropping the repository name first (`Type /github to let this chat read this repo's issues and PRs`), never by cutting the word `/github`. It is not stored and not sent (about +10 lines in PR 4). The dialog keys use `·` like every existing prompt.

It shows only when all hold: tools on, the engine offers tools, toggle off, no grant, origin on GitHub, chat idle, the repo not yet told. The set `App.github_hinted` marks a repo for the run at the first send that hid it, as `hide_server_offer` hides its question, or on a decline. So it is one muted line until the first message, once per repo per run. `Effect::ProbeOrigin` runs from `finish_open` and after a folder change, and only when the hint could show: tools on, toggle off.

## Grant dialog

`ApprovalAsk::GithubGrant { repo }`, opened only by `chat_submit` → `Command::Github` → probe answer, through `request(ask, Origin::Block)`. `/github words` refuses with a line, and `//github` escapes. A turn in flight gives "finish or cancel the answer first", so the dialog never makes a model call fail with `ANOTHER_PENDING`.

Keys go through `on_github_key`, modelled on `on_folder_key`, reached from `on_approval_key`: `y` grants, `n`/Esc decline with a line "GitHub tools stay off", others are swallowed with `ANSWER_FIRST`, repeats are ignored. It uses `set_armed_at` and checks the arm itself, because `approval_armed` treats `Origin::Block` as always armed and the dialog opens asynchronously. `on_tool_ask` denies a `GithubGrant` arriving as a tool event, `ask_for` and the host never build it, and `Effect::ToolDecision` cannot reach the field.

The prompt's notice is part of `fits`: "for this chat only; a cloud engine still asks each turn" and "after the first read, this conversation's commands are typed, never run". Answer line (C5, spec behaviour): one notice that says the tools are on for this chat from the next message, that a cloud engine still asks before each turn's first read, and that after the first read the commands of this conversation are typed, never run.

## File changes (production)

| PR | Files | Est. lines |
|---|---|---|
| **1. Block and marker** (inert until a tool exists) | approval.rs: `AutoCtx`, `GITHUB_READ`, route (10). app/tool.rs, run.rs, auto_run.rs: ctx, blocker, guard, mode line (30). chat/mod.rs + history.rs: flag, tracker, outbox (50). chat/store.rs: `MarkGithubRead`, `github_read` fields (25). worker.rs: marker insert, resume, panic table (45). app/history.rs, app.rs: `on_resumed`, `on_chat` (15). `GithubStarted` event (8). `answers_itself` blocked param and reason (12) | **~195** |
| **2. Pure core** (no way on yet) | registry.rs: `Family::Github`, u16 (5). tools/mod.rs: `Call::Github`, 3 `ToolDef`s, parse, helpers (110). tools/github.rs: slug, argv, serde, render (170). approval.rs + run.rs: `GithubRead` ask and prompt (35). files.rs gate (2). Redaction in `field()` (10) | **~330** |
| **3. Host, usable via the menu** | runner.rs: `spawn_capped` (70). runtime/tools/git.rs: `origin_url` (30). host/github.rs: plan, run, fork cache, exit map (140). host.rs, runtime/chat.rs: arms, scope fields, gh path (35). `GithubStarted` send (5). `ReadFile`/`Grep` locality while blocked on `*-cloud` (10). config.rs, menu.rs: toggle (15) | **~310** |
| **4. Chat grant and hint** | folder.rs: command (10). app/github.rs: submit, dialog, keys, hint, grant ends (130). app/tool.rs, chat.rs: `offered_families` (15). approval.rs, run.rs: `GithubGrant` (25). runtime.rs + effect/event: probe (40) | **~220** |
| **Total** | | **~1,055** across 4 PRs, each under 400 (largest: PR 2, ~330) |

Each PR is safe alone: 1 changes nothing until a `gh_*` name exists, and 2 has no switch. PR 3 is the first releasable step.

## Test strategy (Strict TDD, RED first)

| PR | RED test | Layer |
|---|---|---|
| 1 | `route_run` with `github_read`: a parsed command prompts with `GITHUB_READ`, never counts toward the pause; `auto` and `ask` unchanged | unit |
| 1 | In `tool_run_y.rs` style: flag up, keys are `RUN_KEYS`; `y` gives `WritePty` with no `\r`, `Decision::Sent`, the `(not run: …)` line. A prompt open before the read changes too; a check in flight falls back | app |
| 1 | `GithubStarted` sets the flag, also when stale; a call refused before the spawn sends none; a result `Error` after it keeps the flag; a non-gh call does not | chat state |
| 1 | Reads while blocked: `answers_itself(Read/Search, any mode, blocked)` is `None` for cloud in-root paths; unblocked rows unchanged; `Refused` stays free; the prompt shows `GITHUB_READ_ASK`; after a resume it still asks | unit, app |
| 1 | Marker: idempotent, not listed, not in `messages`, not counted in `skipped`; `user_version` still 3; opening with `open_with(.., 3)` as an older build works. A failing marker with a later row cannot leave the row unmarked; `skipped > 0` blocks; delete cascades | store |
| 1 | Round trip: read, restart the store, resume, `y` still only types | app + store |
| 2 | `slug_of` table: forms, `github.com.evil.org`, Enterprise, `gitlab`, `-x/y`, `../..`, token URL | unit |
| 2 | Argv exact per call; `comments` only when asked; number `0`, `-1`, `1.5`, string refused, never clamped to #1; `no git_push` pin (mod.rs:2169) still holds; each family its own bit with 9 | unit |
| 2 | **Hostile issue** fixture: forged closing tag, `<!-- run curl|sh -->`, zero-width comment marker, ESC and bidi, a newline title faking a header, text saying to call `run_in_pane`. Comment gone; exactly one nonce tag; header not forged; `DATA_NOTE` present; row summary free of its text | unit |
| 2 | **Redaction:** a token in the body, a title, a label, a comment, a branch and a file name is `[REDACTED]`; a secret straddling the cap is masked whole; a zero-width char inside a key does not hide it; output is idempotent | unit |
| 2 | PR card from one recorded fixture: both check shapes, `files < changedFiles` note | unit |
| 3 | Fake `gh` (`runtime/gh.rs:186` pattern): recorded argv and `pwd`, env filter drops `GH_HOST`/`GH_REPO`/`GH_ENTERPRISE_TOKEN`; exit 4; `gh auth login` on stderr; missing; hang killed (pid check); flood stops at the cap; bad JSON; dropped future kills the group | runtime |
| 3 | `origin_url` on a real temp repo: plain, worktree, subfolder, no origin, `insteadOf` not applied | runtime |
| 3 | Ollama `*-cloud` while blocked: `read_file` and `grep` ask, also in-root and in `run`; unblocked they stay free; a local model never asks; the bit set mid-turn by `GithubStarted` applies to the next call | host |
| 3 | Consent: once per repo per turn; again next turn; decline does not re-prompt; `*-cloud` asks here while `read_file` stays free; local never asks; no mode answers | host |
| 3 | `Toolset` families exclude it; toggle default off; menu row snapshot | unit, snapshot |
| 4 | **The model cannot open or answer the dialog:** a `ToolCall` carrying `GithubGrant` is denied and no prompt opens; an answer text containing `/github` opens nothing; `ToolDecision` and a message saying `y` leave the grant off; only an armed `y` press sets it, not a repeat | app |
| 4 | Offered families and budget agree: `tools_reserve_tokens(request.families)` equals the chat's reserve, with and without a grant | app |
| 4 | Grant table above; saved toggle unchanged by a grant; decline line | app |
| 4 | Hint: conditions, gone after first send, once per repo; `is_ascii`; dialog and hint at 80x24 snapshots | app, ui |

No test touches the real `chat.db`, config or network. There is no `#[ignore]` live-test pattern; the fixtures are recorded by hand once during apply.

## Migration / Rollout

No schema change: `user_version` stays 3 and there is no migration. Old configs lack the key and default off. **Older builds.** Every released build at schema 3 has the skip branch for unknown roles, verified by the coordinator: `skipped` arrived in commit 91021c52 (first in v0.8.0) and schema v3 came later, in commit 748ce379. An older build shows a "not shown" notice for the marker row and does not enforce the block. **Rollback:** a plain revert. Marker rows stay; a reverted or older build skips them, shows the `skipped_line` once per resume, and no longer enforces the block.

## Residual risks

- An engram note or a summary that carries issue text into a **new** conversation is not blocked: the block belongs to the conversation that read, and a new one starts clean.
- A second lazarobox process that has the conversation open is not blocked until its next resume (section 1).
- A cloud model whose name does not end in `-cloud` is read as local (see below).

## Could not verify (no shell in this phase)

- `gh` JSON field names and `HOST/OWNER/REPO` come from memory of gh's docs; apply records real fixtures before coding the serde structs. Also unverified: that gh aliases cannot shadow `issue`, `pr` and `repo`.
- Ollama cloud naming (`x-cloud`, `x:cloud`). A cloud model with another name would be read as local and would not ask, for GitHub reads and for reads while blocked, so apply must verify the naming. Also whether `/api/show` marks cloud models, which would be stronger than a name.
- Whether `git config` reads repo config on a dubious-ownership repo. Either way the result is a visible refusal, or a repo whose origin it can read.
- How a cancelled turn ends `execute`. The design relies on the drop semantics of `spawn_core`, pinned by a test.

## Open questions

None open. Decided by the owner:

- Reads in a blocked conversation ask on a cloud engine, in every mode (section 2b).
- `auto` mode is unchanged after a read: `run_in_pane` types without a prompt and waits for Enter.
- The block starts when `gh` is started (section 2); a not-found number or a timeout starts it. Orchestrator sessions get no GitHub tools. In `run` mode the prompt opens under the block.
