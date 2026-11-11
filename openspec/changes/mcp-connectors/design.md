# Design: MCP Connectors

Evidence marks: **[R]** read in code on `main` fa5fbfed for this design; **[I]** inferred, not executed. Decisions C1-C13 of `proposal.md` are binding. The three questions the first draft left open were decided by the owner on 2026-11-11 and are recorded as D18-D20 (D10, D11 and D16 point at them). There are no open questions left.

## S1 shipped without a design: as-built shape

S1 (commit d267359e, `feat(mcp): give each MCP server its own secrets from secrets.toml`) was built before this file existed. S2 and later slices build on it as it is:

| Piece | As built [R] |
|---|---|
| Pure parser | `src/core/ai/mcp_secrets.rs`: `secrets_path(xdg, home)` (next to `config_path`), `Secrets::parse(&[u8]) -> Loaded { secrets, problems }`, `Secrets::value(server, name)`, `Secrets::lookup_for(&McpServer, &host) -> impl Fn(&str) -> Option<OsString>`. Problem texts are constants (`NOT_REGULAR`, `TOO_LARGE`, `UNREADABLE`) or `mode_problem(path)`. A syntax error gives `not valid TOML (line N)` only. `Debug` shows table names and counts, never a name or a value; a rejected key name is passed through `redact_tokens` |
| Bounded read | `src/runtime/mcp_secrets.rs`: `load(path) -> Loaded`. Opens with `O_NONBLOCK \| O_NOCTTY`, judges the open handle (regular file, `mode & 0o077 == 0`, at most 64 KiB), reads at most 64 KiB + 1. Missing file = no values, no problem |
| Per-server lookup | `ExternalSet::start_with(servers, home, host, &Secrets, timeouts)` (src/runtime/mcp/external.rs:115-151); `start_one` wraps the host lookup with `secrets.lookup_for(server, ..)` (:84-97). `ExternalSet::start` is `start_with` with no secrets. `McpClient::spawn` and `child_env` (src/runtime/mcp/client/spawn.rs:74-92, 108-141) are UNCHANGED: the `HOME`, token and `LD_*`/`DYLD_*` filters still run there, after the lookup. The proposal expected a `spawn` signature change; it was not needed |
| Chat wiring | `Externals.secrets: Option<PathBuf>`, set by `Chats::with_secrets` (src/runtime/chat/external.rs:199-202) from `runtime.rs:677-680`. `set_for` reads the file with `spawn_blocking` on each NEW set (:156-161) and sends `mcp: <problem>` notices before the `mcp: started` lines (:187-190) |
| Read-deny | `SECRET_DIRS` gained `.google_workspace_mcp`, `.clasprc.json`, `.config/mcp-gsc` (src/core/ai/tools/paths.rs:36-39) |
| Docs | README "Valores en `secrets.toml`" (README.md:1298-1314) |

Two S1 facts the specs do not state: the extra problem `secrets.toml: cannot be read` (`UNREADABLE`, a file that exists but cannot be opened or read, or a read task that panicked), and that a running set is reused while its active list is unchanged, so a `secrets.toml` edit reaches a chat only after that set restarts (already in the proposal facts). Neither needs a spec change for S2-S6.

## Technical Approach

Pure rules and data in `src/core/ai/` (catalog, class, filter predicate, probe checks, snippets, presence), I/O in `src/runtime/` (probe task, secrets read), keys, screens and effects in `src/core/menu/` and `src/app.rs`, following the existing split. The catalog is a static table; nothing in it is ever written to `config.toml` or `secrets.toml` (C4, C6). The chat start path and the probe share ONE function (`external::connect`), so the probe tests exactly what a chat start does. Every menu action that needs I/O uses the existing pattern `MenuCommand::X` -> `Effect::X` -> runtime task -> `AppEvent::X` -> `MenuState::x_reply` (as `HfAccount`, src/app.rs:2538, src/runtime/hf.rs:900-933).

## Key Decisions

| # | C | Topic | Choice | Rejected, and why |
|---|---|---|---|---|
| D1 | C2, C5 | Catalog form | `src/core/ai/mcp_catalog.rs` (types, lookups, snippet builders) + `mcp_catalog/entries.rs` (static `ENTRIES: &[Entry]`) + `mcp_catalog/checks.rs` (pure checks). `catalog.rs` and `CATALOG` (src/core/ai/mod.rs:7, 91) are taken | An embedded TOML or user-extendable catalog: a parser and errors for data the user never edits. One file: entries plus logic would pass 600 lines by S6 |
| D2 | C5 | Match | `mcp_catalog::entry(name) -> Option<&'static Entry>`, exact equality of `McpServer.name` and `Entry.id` | A new key (C5 forbids; it would also trip `config: unknown key`, src/core/ai/config.rs:1594). Matching by `command`: a user can wrap the binary in a script |
| D3 | C11 | Class | `enum Class { Read, Write }`; `Entry::class(tool)` returns the listed class or `Write`. NO caller in approval: `route`/`answers_itself` (src/core/ai/chat/approval.rs:808, 878) are not touched. S9 (C13) is the first consumer | Wiring a read class to skip the prompt: forbidden by C11 and by agent-tools Approval model until S9 is approved |
| D4 | — | One source for the cap and the cut | `MAX_TOOLS` moves from `src/runtime/mcp/client.rs:51` to `src/core/ai/tools/servers.rs` next to `MAX_PART`; the client imports it. The collision check calls `servers::qualified_name` | A copy of the constant or of the mapping in the catalog: the probe would drift from what a chat really keeps |
| D5 | C9 | Gmail filter | `mcp_catalog::admits(server, tool) -> bool`: true unless the server's entry has `gmail_only_listed` and the tool name contains `gmail` (ASCII case-insensitive) and is not in the entry's tool list. Applied in `McpClient::list_tools_with(keep)` BEFORE `take(MAX_TOOLS)`; `external::connect` passes `\|t\| admits(&server.name, t)`. S3 adds the predicate (every entry admits all); S6 only sets the flag on `google` | Filtering in `qualify` or the registry: the cap (client.rs:191) would already have cut tools, so a send tool could take a read tool's place. Case-sensitive match: a server renaming to `Send_Gmail` would slip through |
| D6 | C2 | Probe path | `pub(crate) async fn connect` in external.rs (today private, :67-81) becomes the shared start; new `src/runtime/mcp/probe.rs::probe(server, home, host, secrets_path, timeouts, cancel)`: load secrets (shared helper extracted from `set_for`), `connect` under `select!` with `cancel`, build the report with pure `checks::report`, then `client.shutdown()` (3 s bound, group kill). One probe at a time; the runtime holds its cancel sender | A second spawn path for the probe: two sets of env rules to keep equal. Running the probe through `ExternalSet::start_with`: its `mcp: started <name>` has no tool count and its tools are already capped |
| D7 | — | Probe counts | `list_tools_with` returns `Listed { tools, admitted: usize }` (`admitted` = usable names after the filter, before the cap). The report uses `admitted` for `lists N tools; only the first 64 are kept`, and the kept list for the collision and not-in-catalog lines | Counting before the filter: a `google` server would warn about tools lazarobox drops anyway |
| D8 | — | Timeout text | `checks::report` writes `timed out after <handshake secs> s` when the reason starts with `ClientError::Timeout.reason()`; the chat notice text (`timed out`, client.rs:99) is unchanged | Changing `ClientError::reason`: changes every chat notice, outside this spec |
| D9 | C6 | Presence | `mcp_secrets::Presence` built from `Loaded` and the catalog: per entry id, the catalog names whose value is empty or missing (`&'static str` from the catalog), plus the problems. Values never leave the runtime task: the type cannot carry one | Handing `Secrets` to the menu: a value would sit in UI state and one `Debug` or render bug from the screen |
| D10 | C7 | Install and login | New `NewPane::Connector { line: String, env: Vec<(OsString, OsString)> }`: a shell tab in `$HOME` (as `NewPane::Update`, src/app.rs:2965-2985), the catalog command written once the pane's first output arrives (fallback 2 s), then the menu closes as Update does (`close_menu(Outcome::Reverted)`, app.rs:1978). The `y` prompt says unsaved menu changes are discarded when the draft is dirty. The text is static catalog data, never user or server text. Enter is written after the line (D19) | Direct argv job like Update: `~` in `--client-id-file` and `gcloud`'s interactive prompts want a shell, and the spec asks for typed text. Keeping the menu open: the pane would be hidden behind it |
| D11 | C7 | `BROWSER` | Never set by lazarobox. Decided: D18 | Setting it on WSL or everywhere: see D18 |
| D12 | — | Opener | No new code. `o`/Enter on a link row emits `MenuCommand::Open(link)` when the host has an opener, else `MenuCommand::Copy(link)`, the HF row's rule (src/core/menu.rs:1290); the app keeps the `https://` check and the failure fallback (src/app.rs:2539-2547, `on_open_failed` :2575). The link is always a visible row | A connectors-specific opener path |
| D13 | C1, C4 | Row and tab | `ItemKind::Connectors` and `CONNECTORS_SECTION` (one item); `TABS[3].sections` points at it (src/core/menu.rs:776-782). `tab_shown` then shows the tab with no navigation change. The hidden-tab tests already use `TEST_TABS` (:792-797) | A hand-written special case in navigation |
| D14 | C1 | Area | `SubView::Connectors` (enum at menu.rs:1195) with its state in new `src/core/menu/connectors.rs` (as `account.rs`, `feedback.rs`): `List { cursor }` or `Entry { id, cursor, scroll }`, the pending `y` prompt, and the last `Presence`. Rows are the existing `Row::Title/Item/Text`; the screen is cut to `VIEW_BODY` (computed, menu.rs:863-899) with a scroll offset, so the popup never changes height and `ui/components/menu.rs` needs no new row kind [I] | A new widget: the generic row renderer already draws text rows |
| D15 | — | Probe results lifetime | Kept in `App` for the app session (`HashMap<&'static str, ProbeLine>`), handed to the menu at open; spec "this session's probes" | In `MenuState`: lost at every menu close |
| D16 | C4 | Declared state source | Servers are read ONCE at startup (`with_servers`, src/runtime.rs:669-675; src/runtime/chat.rs:731-737) and `MenuState.original` comes from the same load: a pasted entry is invisible until restart. Decided: D20 (the area re-reads `config.toml` when it opens) | Reading only the startup list: see D20 |
| D17 | C8, C10, C9, C12 | Entry data | `clarity`, `ga4` (S2), `gsc` (S5), `google`, `apps-script` (S6), with the fields, flags and classes of the mcp-catalog spec. `Var { name, secret: bool, preset: Option<&str> }` gives `GSC_SKIP_OAUTH = "true"`. Binary names marked U stay U until their slice pins them by a manual install | — |
| D18 | C7 | `BROWSER` in the login pane (owner, 2026-11-11) | lazarobox never sets `BROWSER`. Linux and macOS come first: gcloud and clasp open the browser through the platform's default opener. WSL handling is DEFERRED to a later item (see Deferred items), not in S3. If the browser does not open, the link in the login output stays visible in the pane | (b) set it to the first opener found on every platform; (c) set it to `wslview` on WSL: both are WSL-specific work that waits for its own slice |
| D19 | C7 | Enter after the typed line (owner, 2026-11-11) | After the user's `y` (the prompt shows the full command) the typed command runs on its own: Enter is sent. No second confirmation | (b) type without Enter, as `run_in_pane` does: it asks the user twice |
| D20 | C4 | Pasted entry and the running app (owner, 2026-11-11) | The Connectors area re-reads `config.toml` from disk when it opens; states and the probe use that copy. When it differs from the list loaded at startup the area says `restart lazarobox to use it in chats`. Hot reload into chats is a later candidate slice | (c) in-memory only: the area would show `not declared` until restart. Hot reload now: a larger change in the chat runtime |

## Data Flow

    config.toml ──(startup)──> Chats/Externals ──> set_for ──┐
         │                                                   │  external::connect
         └──(area open, D20)──> menu area ──Probe(y)──> Effect::McpProbe ──> probe.rs ──┘
    secrets.toml ──load()──> Loaded ──lookup_for──> child_env (values, runtime only)
                              └──Presence (names only)──> AppEvent ──> menu area
    mcp_catalog (static) ──> snippet/template, classes, admits() ──> list_tools_with
    menu area ──Connector(y)──> NewPane::Connector (shell tab, typed line)
              ──Open/Copy──> existing opener path

## File Changes (production lines; tests and docs excluded)

| Slice | File | Action | Lines |
|---|---|---|---|
| S2 | `src/core/ai/mcp_catalog.rs` | Create: `Entry`, `Group`, `Class`, `Var`, `Login`, `entry`, `class`, `admits` (no flag yet), `snippet`, `secrets_template` | ~120 |
| S2 | `src/core/ai/mcp_catalog/entries.rs` | Create: `clarity`, `ga4` | ~90 |
| S2 | `src/core/ai/mcp_catalog/checks.rs` | Create: cap, collisions (via `qualified_name`), not-in-catalog, `report` | ~60 |
| S2 | `src/core/ai/tools/servers.rs`, `src/runtime/mcp/client.rs`, `src/core/ai/mod.rs` | Modify: move `MAX_TOOLS`; `pub mod mcp_catalog` | ~6 |
| S3 | `src/runtime/mcp/client.rs` | Modify: `list_tools_with(keep) -> Listed`; `list_tools` delegates | ~20 |
| S3 | `src/runtime/mcp/external.rs`, `src/runtime/chat/external.rs` | Modify: `connect` pub(crate) and passes `admits`; extract `load_secrets` | ~20 |
| S3 | `src/runtime/mcp/probe.rs` | Create: probe task, cancel, shutdown | ~90 |
| S3 | `src/app.rs`, `src/runtime.rs` | Modify: `Effect::McpProbe/CancelProbe/McpPresence`, `AppEvent` replies, `NewPane::Connector`, typed line and Enter on first output (D19); no `BROWSER` (D18) | ~150 |
| S3 | `src/core/menu.rs` | Modify: `MenuCommand::Probe/CancelProbe/Connector/LoadPresence` | ~15 |
| S3 | `src/core/ai/mcp_secrets.rs` | Modify: `Presence` | ~30 |
| S4 | `src/core/menu.rs` | Modify: `ItemKind::Connectors`, section, `TABS[3]`, `SubView::Connectors` arms, row value | ~50 |
| S4 | `src/core/menu/connectors.rs` | Create: list, entry screen, scroll, prompts, keys, footers | ~240 |
| S4 | `src/app.rs`, `src/runtime.rs` | Modify: area-open config re-read and restart notice (D20), probe map (D15) | ~60 |
| S5 | `src/core/ai/mcp_catalog/entries.rs` | Modify: `gsc` | ~60 |
| S6 | `src/core/ai/mcp_catalog/entries.rs`, `mcp_catalog.rs` | Modify: `google` (+ `gmail_only_listed`), `apps-script`; `admits` flag | ~150 |
| S2-S6 | `README.md`, `CHANGELOG.md` | Docs per slice | — |

Totals: S2 ~276, S3 ~325, S4 ~350, S5 ~60, S6 ~150: inside the proposal ranges. S3 and S4 sit near the top; if S4 passes 400, the entry screen (connectors.rs) splits from the list into its own PR.

Task mapping (applied in `tasks.md` on 2026-11-11): the menu-level scenario "Nothing without y" (task 3.2) needs the entry screen, which lands in S4; S3 tests the App side only (a `MenuCommand::Connector` creates one pane and writes exactly the catalog line plus Enter, D19) and S4 adds the menu-level `y`/`n` key tests and the config re-read with its restart notice (D20).

## Interfaces

```rust
pub struct Entry {
    pub id: &'static str, pub title: &'static str, pub group: Group,
    pub project: &'static str, pub license: &'static str,
    pub install: &'static str,              // typed line
    pub command: &'static str, pub args: &'static [&'static str],
    pub env: &'static [Var], pub login: Option<&'static str>,
    pub guide: &'static [&'static str], pub links: &'static [&'static str],
    pub token_at: Option<&'static str>,
    pub tools: &'static [(&'static str, Class)],
    pub gmail_only_listed: bool,
}
pub struct Listed { pub tools: Vec<ExternalTool>, pub admitted: usize }
```

## Testing Strategy (strict TDD, no network)

| Layer | What | How |
|---|---|---|
| Unit (core) | Catalog integrity, match, class default, no first-run download, snippet parses back through `Config::parse` (src/core/config.rs:224) with no notice, template, `admits`, cap/collision/not-in-catalog, `report` texts, `Presence` holds no value | Pure tests in `mcp_catalog/tests.rs`, `mcp_secrets/tests.rs`; links checked with `opener::is_web_url` only |
| Approval characterization | `ga4__run_report` with `readOnlyHint` still asks in `auto` and `run` | Existing `route` tests pattern (approval.rs), no new code |
| Runtime | Probe success/70 tools/timeout/Esc/secrets value reached child and absent from report; pid gone within 3 s; Gmail filter before cap | Extend `external/fake.rs`: tool list as an argument, a "never answers `initialize`" mode, env recorder; short `Timeouts`; pid file + `kill(pid, 0)` |
| App | Pane created and line written only on the command; `BROWSER` untouched (D18); the line is written with Enter (D19); menu closes; no `WritePty` without `y` | Assert `Effect::SpawnPane` spec and `Effect::WritePty` bytes; no PTY spawned |
| UI | Tab bar, row value, list groups/states, entry screen, values never drawn, popup height equal on four views | TestBackend + insta |
| Byte identity | `config.toml` and `secrets.toml` unchanged, mode kept, after every area action | Temp config folder, fakes for install/login/probe |
| Manual | Live Clarity, GA4, GSC, Gmail draft, `push_files`, WSL login | Tasks 4.6, 5.3, 6.6 |

No test runs `npm`, `pipx`, `uv`, `gcloud` or `clasp`, or opens a socket beyond the fake's pipes.

## Migration / Rollout

No migration. Each slice reverts alone; no config key is added (C5). S4 makes the `Connectors` tab visible.

## Risks

| Risk | Mitigation |
|---|---|
| Typeahead lost: some shell setups (zsh instant prompt) discard input written before the prompt [I] | Write after first output; manual check per shell; the line is also shown in the area so it can be re-run |
| Pasted entry needs a restart to reach chats (D16) | D20: the area re-reads `config.toml` on open and says `restart lazarobox to use it in chats` |
| `google` filter keys on the name: a workspace-mcp declared under another name is not filtered (spec "Other servers untouched") | README says to use `name = "google"`; the probe of `google` reports unknown tools |
| S4 near 400 lines | Split list and entry screen if needed |
| Two in-flight deltas MODIFY config-menu "Sections and items are data": this one and `orchestrator-worker-flow` (adds `simple`); main code already has `simple` (menu.rs:587-590) and `Build jobs` (:372) which neither the main spec nor this delta lists | Whoever archives second rebases its MODIFIED block on the merged text; flagged for sdd-archive |
| Unverified binary names and workspace-mcp pre-auth (U) | Pinned in S2/S5/S6 by manual install (task 6.4). 2026-11-11: binary names verified; workspace-mcp has no pre-auth (see D21, tasks S6b) |

## Drift check (proposal/spec/tasks references vs `main` fa5fbfed)

Not edited in the source artifacts: the proposal states its line numbers are those of f7d6b6f4, which was correct then. Implementers use the current column.

| Reference | Now [R] |
|---|---|
| `src/app.rs:2536-2543` (`MenuCommand::Open`; proposal, mcp-connect spec, task 3.3) | 2539-2547 |
| `NewPane::Update, src/app.rs:1965-1978` | enum 824-839; use 1969-1979; spec 2965-2985 |
| `src/core/menu.rs:750-785` `TABS`, Connectors `772-778`, `TEST_TABS 789-802`, Hugging Face `589` | 754-790, 776-782, 792-797, 591-594 |
| "fixed 12-row body", `VIEW_BODY` menu.rs:859 | 863; COMPUTED by `view_body()`, now at least 13 (Settings: 10 rows + gap + 2 note rows). Do not hard-code 12 |
| `approval.rs:841-843` `answers_itself`; `773-786` `route` | 878; 808 |
| `src/core/ai/config.rs:386-393` `McpServer`, `1256-1265` `valid_env_name`, `1584-1594` unknown keys | 393, 1263, 1594 |
| `spawn.rs:72-90` `child_env`, `106-110` `spawn` | 74-92, 108-112 |
| `SECRET_DIRS` paths.rs:24-34 | 24-39 (S1 additions) |
| `servers.rs:45-62`, `134-160` | 45-63, 134-163 |
| README.md:1244-1281, :753 | 1275 onward; read-deny paragraph 772 |
| client.rs:51, 73-80, 99, 191 | unchanged |
| S1 "lookup through `McpClient::spawn`" (proposal S1 row, task 1.4) | done in `start_one` instead; `spawn` unchanged |

## Deferred items

1. **WSL login (later item, not S3).** WSL-specific browser handling for the login pane (for example setting `BROWSER` to `wslview` when unset) is out of scope for this change's slices; D18 records the owner's decision to start with Linux and macOS. Until then the link printed in the pane is the fallback.
2. **Hot reload of the server list into chats (later candidate slice).** Re-reading `[[ai.mcp.servers]]` into the running chats when the area closes (running sets already restart by themselves because `set_for` compares lists) is a larger change in the chat runtime. D20 only re-reads on open and shows the restart notice.

## Open questions for the owner

None. Q1-Q3 of the first draft are D18-D20 (decided 2026-11-11).

## S6 amendment (owner decisions of 2026-11-11, revised after review)

The owner amended C6, C7 and C10 (proposal, dated notes) and added C14. S6 is replaced by S6a1, S6a2, S6c, S6b1, S6b2 and S6d (tasks.md). Upstream facts: proposal, "Upstream facts re-verified on 2026-11-11". The S1-S5 decisions above stay; D5 and D17 are extended below. The previous "None" under "Open questions for the owner" holds for D1-D20; proposal open questions 11-15 are resolved by the orchestrator on 2026-11-11 under the owner's instruction to proceed; pending owner confirmation (D23, D26, D27 and the S6c presets follow those proposed resolutions).

| # | From | Topic | Decision | Rejected alternatives |
|---|---|---|---|---|
| D21 | C7, C10 amended | Who receives the OAuth callback | **The MCP server's own flow, never lazarobox.** lazarobox only starts it from the menu after `y`, opens the browser and reports the result; it never opens, reads, copies or writes a Google token (`stat` only). `gsc`: mcp-gsc's own `get_gsc_service_oauth()` runs in the login pane (TTY, the user's full env, so the platform opener works and the URL stays visible; `run_local_server(port=0)` picks a free port), with `GSC_CONFIG_DIR` fixed so the pane and the chat server share one token folder. `google`: lazarobox's link session spawns the declared server through the chat start path and calls `start_google_auth` by name; workspace-mcp's own callback server (`localhost:8000`-`8004`) receives the redirect while the session keeps the process alive. Why: lazarobox stays out of token custody (no new secret store, no refresh logic, nothing to leak or rotate), each server keeps the token where it reads it, and C2 (no home-grown Google code) holds | (a) A loopback server in lazarobox that writes the server's token file: a second OAuth implementation, token custody, coupling to each server's private token format. (b) `gsc` through an MCP call to `reauthenticate`: in the scrubbed env `webbrowser.get()` fails before the URL is printed, and a printed URL goes to stdout, which lazarobox skips (client.rs:299-300), so the call hangs (verified on 0.4.2). (c) Asking the model to call the auth tool: violates "only from the user's keyboard" |
| D22 | C6 amended | Menu save of `secrets.toml` | Pure part in core (`mcp_secrets::edit(text, server, name, value) -> Result<String, Refusal>` on `toml_edit::DocumentMut`: plain-table check, string-only, append rules, line ending of the file kept). I/O in runtime (`secrets_write.rs`) on handles: folder opened `O_DIRECTORY \| O_NOFOLLOW` and judged by `fstat` (owner, no group/other write); file opened `O_RDONLY \| O_NOFOLLOW \| O_NONBLOCK \| O_NOCTTY` relative to the folder and judged by `fstat` (regular, owner, mode exactly 0600, 64 KiB), as `read_private` does; temp `O_CREAT \| O_EXCL \| O_NOFOLLOW` 0600 with a random name (3 tries, an existing name is never followed or removed), `fsync`, `renameat`, folder `fsync`; backup `.bak` by the same steps; a `(dev, ino, len, mtime)` re-check before the rename that narrows, not closes, the race with a hand edit. All I/O behind a `SaveFs` seam for fault injection. 0400 is refused, never chmodded. A symlinked config folder is refused (unlike `config_write`, which writes through links). Menu side: `MenuCommand::SaveSecret` -> `Effect::SaveSecret` -> `AppEvent::SecretSaved(Result<Saved, String>)` | Writing through `config_write` (follows symlinks, owns `config.toml` rules). A keyring: rejected by C6. Writing on the menu's `s`: a secret would sit in the menu draft |
| D23 | C6 amended (Q15, orchestrator, pending owner confirmation) | What is shown | Every `secret: true` var is confidential by default, with an explicit opt-out `Var.shown` (new; the existing `Var.secret` only means "lives in `secrets.toml`"); a test lists each shown var by name; a confidential value is never drawn, the typing mask is a fixed `••••••••`; a non-confidential value is shown in full. `SecretText` (local newtype, no new crate): no `Display`, no `Serialize`, `Debug` = `<redacted>`, bytes overwritten on drop (best effort). Paste routing: while a field is open, `AppEvent::Paste` goes to the menu field before `on_paste` reaches any chat | Showing the last 4 characters (not chosen: Q15 resolved by the orchestrator on 2026-11-11 under the owner's instruction to proceed; pending owner confirmation). Adding `zeroize`: not a dependency today; best effort is stated instead |
| D24 | C10 amended | `gsc` ways | `Entry.ways: &[Way { title, vars, login }]` (empty for single-way entries); `env` = union (with `GSC_CONFIG_DIR`), so a switch is a `secrets.toml` edit only (preset writes go through D22 with the overwrite confirmation). Path presets are written absolute (mcp-gsc does not expand `~` in `GSC_CONFIG_DIR`). The login line is built at prompt time from the resolved values and `uv tool dir` (run by lazarobox, never `$(...)` in the line), after `uv tool list` shows `mcp-search-console v0.4.2`. A chat or probe start of `gsc` with OAuth values and no token is refused with a notice | Two catalog entries `gsc` and `gsc-oauth`. A static login line with `$HOME` and `$(uv tool dir)`: diverges from the server's values when `XDG_CONFIG_HOME` is set or uv is missing |
| D25 | C9 | Filter shape (extends D5) | `Entry.gmail_allowlist: bool` (true on `google`) and `Entry.hidden: &[&str]` (`start_google_auth`, `reauthenticate`), both checked in `admits` before `take(MAX_TOOLS)`; keyed by server name (documented limit); the link session calls `tools/call` directly | Hiding with `--disabled-tools start_google_auth`: the link session needs the tool on the server |
| D26 | C7 amended (Q13, orchestrator, pending owner confirmation) | Link session | `runtime/mcp/link.rs`, sharing `external::connect`/`load_for_start` (values exactly as a chat start) and the `Prober` single-flight slot; 20 s handshake; baseline of the credentials file (`stat`) before the call; `tools/call start_google_auth` with a 30 s timeout; pure `mcp_catalog::link::sign_in_url(text, client_id) -> Option<&str>` (first line that, trimmed, is a single link; printable ASCII; at most 2048 bytes; `https://accounts.google.com/o/oauth2/...`; `client_id` equal; `redirect_uri` on `localhost` or `127.0.0.1` ports 8000-8004); opener through `Effect::OpenUrl`; a 1 s `stat` poll for up to 5 min; Esc -> `Effect::CancelLink`; the server is always stopped and reaped (3 s) | Keeping the server running after the link |
| D27 | Q11 (orchestrator, pending owner confirmation) | OAuth client file (slice S6a3) | The menu writes `google-oauth-client.json` (Desktop client JSON via `serde_json`) from a pasted id and secret with the D22 writer (whole-file variant, backup, overwrite confirmation) | A second, looser writer |

### S6 file plan

| Slice | Files | Production lines (est.) |
|---|---|---|
| S6a1 | `src/core/ai/mcp_secrets.rs` (+`edit`), `src/runtime/secrets_write.rs` (new: handle-based writer, `SaveFs` seam, backup, delete backup), `src/runtime/mcp_secrets.rs` (header text) | 270–360 |
| S6a2 | `src/core/menu/connectors/fields.rs` (new: field list, masked field, delete-backup prompt, command-mismatch confirm), `src/core/menu/connectors.rs` (key `f`), `src/core/ai/secret_text.rs` (new: `SecretText`), `src/app.rs` (paste routing, effect, event) | 240–330 |
| S6a3 | `src/core/ai/mcp_secrets.rs` (client JSON builder), `src/runtime/secrets_write.rs` (whole-file variant), `src/core/menu/connectors/fields.rs` (client form, key `o`) | 110–170 |
| S6c | `src/core/ai/mcp_catalog.rs` (`Way`, `hidden`, `gmail_allowlist`, `confidential`, `admits`), `mcp_catalog/entries.rs` (`google`, `apps-script`, `gsc` ways with `GSC_CONFIG_DIR`, `ga4` pins), `presence.rs` (ways) | 220–300 (`google` alone pins 42 tools, about 60 lines of data) |
| S6b1 | `src/runtime/mcp/uv_check.rs` (new: `uv tool dir`/`uv tool list` with timeouts), `src/core/ai/mcp_catalog/login_line.rs` (new, pure: quoting and the line), `src/core/menu/connectors.rs` (key `g` and `w` for `gsc`, `linked` state), `src/runtime/chat/external.rs` (not-linked refusal), `src/core/ai/tools/paths.rs` (macOS path) | 200–290 |
| S6b2 | `src/runtime/mcp/link.rs` (new), `src/core/ai/mcp_catalog/link.rs` (new, pure: `sign_in_url`), `src/core/menu/connectors.rs` (waiting screen), `src/app.rs` | 260–360 |
| S6d | `docs/conectores.md` (new), `README.md`, guide strings in `entries.rs` | docs; under 30 production lines |
