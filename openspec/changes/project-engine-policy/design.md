# Design: Project Engine Policy

Code references are at e480992e (identical source tree on this branch's base). "#668" is the unmerged local-only proposal (`openspec/changes/local-only-mode/`); its D-numbers are cited as "#668 D2". Decisions marked "recommended" follow the proposal's open questions and change with them.

## D1 The rule is a value, not a boolean

#668 D1 gives one accessor, `AiConfig::local_only() -> bool`, and #668 D2 reads it inside `availability_for_with`. This change needs the answer per chat, so the check takes a value instead:

```text
enum LocalSource {            // the order the UI names them (strongest first)
    Policy,                   // #659, later
    User,                     // [ai] local_only (#668)
    UserProject { root },     // [[ai.projects]] entry (P1)
    RepoFile { path },        // .lzr/policy.toml (P3)
}
struct LocalRule { by: Option<LocalSource> }   // None = no rule: today's behaviour

fn local_rule(global: Option<LocalSource>, project: Option<LocalSource>) -> LocalRule
    // first Some in source order; never "less local" than any input
```

- `LocalRule::required()` is `by.is_some()`. There is no "cloud allowed" value: a source that says `local_only = false` contributes `None` (proposal Q2, recommended a).
- The combination is pure and order-independent for the decision; the order only picks which source the text names.

## D2 User entries: `[[ai.projects]]` (P1)

```toml
[[ai.projects]]
root = "~/clients/acme"     # absolute, or ~ / ~/...
local_only = true
```

- Read next to `read_mcp_servers` (src/core/ai/config.rs:1135) into `Vec<ProjectEntry { root: PathBuf, local_only: bool }>`. `projects` joins the `[ai]` known list (src/core/ai/config.rs:1535-1550) and each entry's keys `root`, `local_only` are walked like the MCP entry keys (src/core/ai/config.rs:1588).
- `root`: absolute or starting with `~` (expanded with an absolute `$HOME`, as `[workspace] dir`, configuration spec:537). Anything else skips the entry with `ai.projects[N].root: expected an absolute path`: an entry whose place is unknown cannot be applied anywhere, the same reason MCP skips an unreadable `project` (src/core/ai/config.rs:1228-1237).
- `local_only`: missing is `false`; a non-boolean counts as `true` with `ai.projects[N].local_only: expected true or false (local only is on for this project until this is fixed)` (fail closed, proposal Q5; as #668 Q6).
- Several entries may cover the same folder; any `true` wins (D1). Duplicate roots are allowed and not reported.
- **Match (proposal Q1, recommended):** the chat folder is the root or lies under it, comparing canonical paths; an entry whose root cannot be canonicalized is compared by its literal text as well, so a symlink never fails open (the MCP lesson, src/runtime/mcp/active.rs:1-8, 29-35). Canonicalization is runtime work: done in the folder probe (D4), never in the pure check.
- The menu never writes `ai.projects` before P6; a menu save keeps its bytes (config-persistence).

## D3 The check in `availability_for_with`

#668 D2 places "refused by local only" right after `Unassigned`. This change changes its input, not its place:

```text
fn availability_for_with(task, cfg, rule: &LocalRule, detection, jail, gated_off, adapter_exists)
resolve(task) -> engine
rule.required() && engine is Kind::Cli                         -> LocalOnly { engine, model: None, by }
rule.required() && ollama cloud model (#668 S3)                -> LocalOnly { engine, model: Some(m), by }
```

- `Availability::LocalOnly` (#668) gains `by: LocalSource`, so the guidance can name it (D5).
- Callers build the rule from `cfg.local_only()` plus the chat's project source:
  - `chat_availability()` (src/app/chat.rs:480-482) becomes `chat_availability(id)`; its callers already know the pane: `turn_availability(id)` (src/app/orchestra.rs:1184-1195), `chat_summary_key(id)` (src/app/summary.rs:40), `status_context` uses the focused pane (src/app/chat.rs:232-239);
  - `start_compaction(id)` (src/app/compact.rs:32);
  - `roster(config)` (src/app/orchestra.rs:797-806) becomes `roster(config, rule)`, the rule of the run (D6);
  - the run resume goes through `turn_availability(id)` (src/app/run_resume.rs:179).
- A caller with no chat (none known today besides the roster, which gets the run's rule) passes the global rule only.
- **Size note.** If #668 S1 lands with `availability_for_with` reading `cfg.local_only()` inside, P1 changes that read into the parameter; if #668 S1 already takes the switch as a parameter (worth suggesting on PR #668), P1 only builds the rule. Either way the existing callers of `availability_for` (tests at src/core/ai/chat/engine/availability_tests.rs, src/runtime/orchestra/tests.rs:317) keep a wrapper that passes the global rule.

## D4 Where the chat's rule comes from: the folder probe

Every way a chat gets a folder goes through `Effect::ProbeFolder` and `AppEvent::FolderProbed` (src/app.rs:199-205, 522-528; purposes at src/core/ai/chat/folder.rs:27-40; resume at src/app/folder.rs:296-304). The rule is computed there and travels with the folder:

- `Probed` (src/core/ai/chat/folder.rs:16-23) gains `project: Option<LocalSource>`. The runtime, which already resolves the folder, also (P1) matches the user entries against the resolved path, and (P3) looks for the repository file (D7).
- `change_folder` (src/app/folder.rs:79-122) stores it on the chat next to `origin_cwd`. A chat with no folder has no project rule (only the global one applies), and it cannot send anyway (`folder_pending`, src/app/chat.rs:279, 432).
- **When it is read again.** At every folder set (open, `/cd` confirm, pick, resume). The user's `[[ai.projects]]` changes only at start or on a menu save (no hot reload, configuration spec:9-10); after P6's menu save, every open chat's entry match is recomputed from the saved entries and its resolved folder (pure, no I/O). A repository file edited by hand while a chat is open applies at the next folder set or start; the README says so (proposal Risk). Alternative considered: re-read the file before each send. − An extra I/O round trip on the send path for a rare edit; rejected.

## D5 Texts (UI copy in English)

| Where | Text |
|---|---|
| Guidance, a cloud CLI, project rule (chat, `Prefix s`, ORCH pane, worker reason) | `{engine}: cloud engine, not used in this project (local only by {source}): pick a local engine in the menu (Ctrl+Space m)` |
| Guidance, an Ollama cloud model, project rule (#668 S3) | `ollama: {model} runs in Ollama's cloud, not used in this project (local only by {source}): pick a local model in the menu (Ctrl+Space m)` |
| `{source}`, user entry | `ai.projects root {root}` (the root as written, `~` kept) |
| `{source}`, repository file | `{path}` (absolute path of the file) |
| Global switch (#668, unchanged) | `{engine}: cloud engine, not used while local only is on: pick a local engine in the menu (Ctrl+Space m)` |
| Status bar, focused chat, project rule only | `local only (project) · ` before the engine text |
| Chat badge (P2) | ` LOCAL ` (short `L`) |
| Task-row mark, focused chat, project rule only (P2) | ` · off in this project` |
| Bad user entry `root` | `ai.projects[N].root: expected an absolute path` |
| Bad user entry `local_only` | `ai.projects[N].local_only: expected true or false (local only is on for this project until this is fixed)` |
| Bad repository file (P3, system line in the chat, once per folder set) | `{path}: {problem} (this project is local only until this is fixed)` |
| `{problem}` values | `cannot be read`, `not a regular file`, `larger than 4 KiB`, `not valid TOML`, `ai.local_only: expected true or false`, `unknown key: {key}` |
| Worker stopped by a stricter folder (P4) | `stopped: this project is local only` |
| Chat turn line when `/cd` moves into a local project (P4) | `folder changed: {old} -> {new}` (existing) and then `local only in this project ({source})` |

The global text wins when both apply (D1 order). As in #668 D4, no text tells the user how to remove a rule, so #659 needs no copy change.

## D6 Orchestrator runs (P4)

- At run start the run takes `rule_at_start = local_rule(global, project of OrchScope.origin)` (src/runtime/orchestra.rs:298-312). Its roster is built with it (D3).
- Every later spawn, retry and relaunch builds the roster with `local_rule(global_now, rule_at_start.by, chat's current project)`: the run is never less local than it started, and a stricter current rule applies at once. Relaunches already re-read the menu part (src/app/orchestra.rs:793-796).
- **`/cd` during a run.** `folder_busy` has no run check (src/core/ai/chat/folder.rs:283-300), so (inferred) the chat can move between coordinator turns while workers run. When the new folder's rule is local and the run's was not, running workers on a refused engine are cancelled through #668 S4's path (src/core/ai/orchestra/run.rs:1152, 1264) with `stopped: this project is local only`. Moving to a looser folder changes nothing for the run. Alternative considered: refuse `/cd` while a run is live. + Simpler. − Changes chat-folder behaviour for every user; kept as a fallback if the cancel proves fragile.
- A user-added read root (agent-orchestrator spec:180, "later, from the user") adds the rule of that root to the run's rule. Not built here; recorded so that feature does not loosen.

## D7 Repository file `.lzr/policy.toml` (P3)

- **Shape.** The same table and key as the user file and #659: `[ai] local_only = true`. `local_only = false` or a missing key is "no rule". Any other key, anywhere in the file, is unknown and fails closed (proposal Q5): a later build that adds `engines = [...]` (proposal Q4 b) is then never silently ignored by an older one.
- **Search (proposal Q1, recommended).** From the resolved chat folder upward, each directory's `.lzr/policy.toml`, stopping after the git root (`git_root`, src/runtime/workspace.rs:95-97, same walk limit, src/runtime/workspace.rs:17). Outside a repository only the chat folder itself. The nearest file that says local is the one named; any one saying local is enough.
- **Read.** `read_regular` with a 4 KiB cap (src/runtime/bounded_read.rs:23-27): no blocking open, type checked on the handle. A symlinked `.lzr` or `policy.toml` is not followed (as a symlinked `.git`, src/runtime/workspace.rs:104-109) and fails closed: `not a regular file`. Inferred alternative: follow a symlink to a shared company file; rejected for now because the file's location could then be outside the checkout the user sees.
- **Trust.** The file can only add a rule. It is never a reason to start a process, offer a tool, choose an engine or contact the network. Its content is parsed, never sent to an engine or logged. That is why reading it is acceptable where the mcp-servers spec forbids reading a repository file for servers (openspec/specs/mcp-servers/spec.md:11-38): a server file could add a process; this one can only refuse.
- **Notice.** A file that fails closed adds the system line of D5 to the chat once per folder set; a valid file adds nothing beyond the badge and status text.

## D8 Per-chat model calls (P5)

#668 S5/S6 turn off, while the mode is on, the GitHub read tools (model calls) and the MCP servers without `local = true`. Those are per chat, so they follow the chat's `LocalRule`, not only the global switch:

- the GitHub family is not offered and `/github` refuses with `GitHub tools are off in this project (local only by {source})`;
- the chat's MCP set (src/runtime/chat/external.rs:215) starts only `local = true` servers, with #668's notice ending `(local only by {source})`.

App-wide calls (update checks, CLI model listings in detection) carry no project data and follow only the global switch (#668 S5).

## D9 How #659 can lock it later

- The policy file can set `[ai] local_only` (#668 D8) and can hold its own `[[ai.projects]]` entries; both become `LocalSource::Policy` in D1, first in the order.
- Because no source can loosen another (D1), a locked rule needs no new combination logic: the user can add rules, never remove the policy's.
- Later #659 options that this design keeps open: "ignore repository files" is unnecessary (they only tighten); "require a repository file" or "an allow-list of engines" map to proposal Q4 (b).
- Nothing in D3-D8 reads which source decided, except texts.

## D10 Tests

- Pure: entry read (default, true, false, wrong type fails closed, bad root skipped, `~` expansion, unknown keys reported, every key known silent); `local_rule` order and that no input loosens; entry match (equal, under, sibling, symlinked root, literal fallback); availability for each task with a project rule, the global rule, both; guidance texts with each source; repository file parse (valid, false, missing key, wrong type, unknown key, not TOML, over cap).
- Runtime (temporary directories): probe finds the file in the folder, in a parent up to the git root, not above the git root, not above a non-git folder; a symlinked `.lzr` and `policy.toml` fail closed; a FIFO does not block.
- App: open a chat under a marked root and send on claude (refused, composer kept); `/cd` out of and into a marked folder; resume a conversation stored under a marked folder from an unmarked pane; ORCH coordinator; worker roster; `/cd` into a local project with a cloud worker running (P4); status bar and badge (TestBackend + insta).
- Characterization: with no `[[ai.projects]]` and no repository file, every existing test stays green, the request bytes are identical and a menu save is byte-identical.
