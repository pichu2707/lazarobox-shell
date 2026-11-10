# Design: Admin Policy Lock

Code references are at 4760d0dd (source tree equal to `main` at e480992e, 2026-11-10). Decisions marked "recommended" follow the proposal's open questions (Q1-Q7) and change with them. "Inferred" and "U" (unverified) as in the proposal.

## D1 Where the file is (Q1, recommended b)

| Platform | Path | Notes |
|---|---|---|
| Linux | `/etc/lazarobox/policy.toml` | — |
| Linux under WSL | `/etc/lazarobox/policy.toml` inside the distro | the same binary and path as Linux; `is_wsl` (src/core/desktop.rs:79-82) changes nothing here |
| macOS | `/Library/Application Support/lazarobox/policy.toml` | U: owner `root`, group `admin`, mode `755` on a stock system |

- **One constant per OS**, chosen with `cfg(target_os)` in a new pure module `src/core/policy.rs` (`POLICY_PATH`). It is never derived from the environment (`XDG_*`, `HOME`), a command-line flag or `config.toml`, and there is no search list: any of those would let a user point lazarobox at a file of their own.
- **Independent of `HOME`.** Today, with no usable `HOME` and no `XDG_CONFIG_HOME`, there is no user file (src/runtime.rs:945). The policy is still read then.
- **Read once at start,** next to the user file in `startup` (src/runtime.rs:937-955), before it, so the policy problems come first in the start-up notice (which shows only the first problem, src/core/config.rs:324-336). A change of the file applies at the next start, like a hand edit of `config.toml`.
- **The file is not secret.** It is world-readable by design (D2), the CLI engines' jail already grants `/etc` read-only (src/core/ai/jail.rs:874, 936), and the README tells administrators never to put a token in it.

## D2 The trust check

Judged at the runtime edge (`src/runtime/policy.rs`), following the two precedents in the code: the open-then-judge-the-handle read of `secrets.toml` (src/runtime/mcp_secrets.rs:29-62) and the owner/mode check of the gemini home (src/runtime/chat/cli/home.rs:224-245).

1. Open `POLICY_PATH` read-only with `O_NONBLOCK | O_NOCTTY` (a FIFO never blocks the start). `ENOENT` or `ENOTDIR`: **no policy**, silent, today's behaviour. Any other open error (`EACCES`, `ELOOP`...): the file exists but cannot be used (D8).
2. On the open handle: a regular file, at most 64 KiB (`MAX_BYTES`, assumption: a policy is tens of lines), owner uid 0, no write bit for group or others (`mode & 0o022 == 0`).
3. Each folder of the canonical path of the file, from `/` down (`fs::canonicalize`, so macOS `/etc` → `/private/etc` and a config manager's link are followed, U): owner uid 0, and no write bit for group or others **unless** the sticky bit is set and the next component is owned by uid 0 (the usual safe-path rule; it lets a root-owned file live under a sticky store such as `/nix/store`, U).
4. Read at most `MAX_BYTES + 1` bytes from the handle (as `read_private` does), then UTF-8.

The verdict is `Missing`, `Trusted(text)`, `Untrusted { text, at }` (steps 2-3 failed on owner or mode only, `at` is the first failing component) or `Unusable(problem)`. An untrusted file is **applied as written** (proposal Q3): every key can only tighten (D5), so a file another user could write can never loosen anything below the user's own choice; it is labelled `policy (untrusted)` and the notice names the fix.

The check is a pure function over a list of `(path, uid, mode, kind)` facts (`judge`, in `src/core/policy.rs`), fed by `lstat`/`fstat` at the edge, so it is tested without root (D11).

## D3 Format and keys

The policy uses **the same tables and key names as `config.toml`** (local-only design D8 already assumes `[ai] local_only = true` in the policy), so an administrator reads it like the user file, and one parser reads both value types. Only the keys below are allowed; the policy has its own unknown-key walk, and an unknown key fails closed (Q6).

| Key | Type | Bound it sets | Strictest value (D8) | Slice |
|---|---|---|---|---|
| `[ai] local_only` | bool | `true`: local-only mode on, at least. `false`: no bound (notice) | `true` | A1 |
| `[ai.tools] enabled` | bool | `false`: tools off for every chat. `true`: no bound | `false` | A2 |
| `[ai.tools] search`, `write`, `exec`, `git`, `git_write`, `github`, `issue_draft` | bool | `false`: that family off, and its chat grant refused. `true`: no bound | `false` | A2 |
| `[ai.tools] approval` | string or array of `"ask"`, `"auto"`, `"run"` | the modes a chat may use; `"ask"` is always allowed (it is the strictest) | `["ask"]` | A2 |
| `[updates] check` | bool | `false`: no update check (start, `r`, llama-server newer build). `true`: no bound | `false` | A3 |
| `[ai.orchestrator] enabled` | bool | `false`: ORCH gate off | `false` | A3 |
| `[ai] engines` | array of engine ids from `CATALOG` (src/core/ai/mod.rs:90-105) | only these engines may be used by any task | no extra bound (`local_only = true` already refuses every cloud engine) | A4 |
| `[ai.mcp] user_servers` | bool | `false`: no entry of the user's `[[ai.mcp.servers]]` starts | `false` | A5 |

An empty `[ai] engines = []` is a valid bound that refuses every engine (an administrator who wants no AI). An unknown engine id in it is a wrong value for that key (D8).

**Later keys, not specced here** (each its own small change): `[ai.memory] enabled = false` (engram notes leave `chat.db`'s `0600`, README.md:1567); `[ai.tools] drive_writes = false` (the WSL drive grant, src/app/folder.rs:395-414); the planned full write grant of `run` mode; `[ai] downloads = false` for the Hugging Face, Ollama and llama-server downloads that #668 Q4 keeps as user calls; administrator-declared MCP servers (`[[ai.mcp.servers]]` in the policy, the only ones that start); a `[policy] contact` string shown in the refusals; `[[ai.projects]]` entries in the policy (`root`, `local_only = true`), which #658 design D9 already maps to `LocalSource::Policy`; whatever #570 ships.

The repository file of #658 (`.lzr/policy.toml`) has the same shape for `[ai] local_only` but is another file: it is the user's, tighten-only, and never trusted as an administrator's.

## D4 Precedence: policy > user config > project file

**Every lockable key has a strictness order. The effective value is the strictest of what the policy, the user's `config.toml` and the project sources (#658) ask for. The policy is a bound: nothing below it can loosen it, and anything below it may tighten.**

| Key | Combination |
|---|---|
| `local_only` | policy OR user OR project rule (#658 lists the policy as source 1 of the same OR) |
| tool flags, `updates.check`, `orchestrator.enabled`, `mcp.user_servers` | policy AND user (a `false` anywhere is off) |
| `approval` | the user's mode if the policy allows it, else `ask` |
| `engines` | an assignment whose engine is not in the list is refused (it never selects another engine, as chat-engines "Never switches"; #668 D2) |

- This is the rule of #658 ("A chat requires local engines when any source says so; no source can loosen another"), extended to the other keys. When #658 adds its own `engines` list (its Q4 b), the allowed set is the intersection of the policy's and the project's.
- **The UI names the strongest source:** policy, then user, then project. With the policy and the user both on, the status bar reads `local only (policy) · `.
- **The user's file is never rewritten.** A looser user value stays in `config.toml` as written; the start-up notice says it is overridden (D7). Removing the policy brings it back.

## D5 Why a bound, not a pinned value (Q4, recommended b)

- A pinned `false` would let a policy forbid the user's own privacy choice (`local_only = false` pinned) or force a riskier state (`write = true` pinned). A bound cannot.
- A file another user could write (D2, untrusted) can then only make lazarobox stricter, so applying it is safe.
- A value that would loosen (`local_only = false`, `write = true`, `check = true`, `approval` without `ask`) locks nothing; the notice `policy: {key} = {value} locks nothing (a policy can only make settings stricter)` says so, once per key. `approval = ["auto"]` is read as `["ask", "auto"]` with that notice.

## D6 Locked rows in the menu

- **When a row is locked:** when the policy bound excludes at least one value of the row (for `Local only`, when the policy says `true`; for `Approval`, when the list is shorter than the three modes). A row whose current value the bound allows but whose other values it forbids is locked too.
- **How it shows:** the row is drawn as a disabled row (muted; the cursor can land on it, Enter does nothing; config-menu spec, Terms, openspec/specs/config-menu/spec.md:7; `Row::Item.enabled` and `row_style`, src/core/menu.rs:1100-1105, src/ui/components/menu.rs:183-195). Its value is the effective value followed by ` · locked by policy` (for example `on · locked by policy`). While it is selected the footer is `locked by policy: set by your administrator in {path}`.
- **Keys on it:** `h`, `l`, Left, Right, Space and Enter change nothing and show the footer notice `locked by policy: set by your administrator in {path}`. For `Approval` with two allowed modes, the row cycles between those two and is not drawn disabled; its value still carries the suffix.
- **Saving:** a locked row's draft never changes, so `s` writes nothing for it (a row writes its keys "each only when it changed", `edits` src/core/menu.rs:215-217; inferred for every row kind). Every other byte of `config.toml` is kept (config-persistence).
- **Picker (A4):** engines not in `[ai] engines` are not listed; a disabled note row ends the list: `engines hidden: not allowed by policy`. The task row of a refused assignment gets the mark ` · off: policy`.
- One mechanism (a `locked(&Policy) -> Option<Lock>` per row descriptor, beside `badge`, src/core/menu.rs:140-160) serves every slice: A1 uses it for `Local only`, A2 for the Tools tab, A3 for `Orchestrator`.

## D7 Exact texts (UI copy, English)

`{path}` is the policy path, sanitized as other shown paths are (`clean`, as src/core/ai/mcp_secrets.rs:57-62).

| Where | Text |
|---|---|
| Locked row value suffix | ` · locked by policy` |
| Locked row footer, and the notice on a change key | `locked by policy: set by your administrator in {path}` |
| Start-up, a user value the policy overrides | `config: {key} = {value} is overridden by the policy in {path}` |
| Start-up, a file that cannot be used (D8) | `policy: {path}: {problem} (every lockable setting is at its strictest until this is fixed)` |
| `{problem}` words | `cannot be read`, `not a regular file`, `larger than 64 KiB`, `not UTF-8`, `not valid TOML: {message}`, `unknown key: {key}` |
| Start-up, a known key with a wrong value | `policy: {key}: {expected} ({key} is at its strictest until this is fixed)` |
| Start-up, an untrusted file | `policy: {at} can be changed by users other than root; the policy is applied as written (fix: chown root {at} && chmod go-w {at})` |
| Start-up, a value that loosens | `policy: {key} = {value} locks nothing (a policy can only make settings stricter)` |
| Status bar, local only from the policy | `local only (policy) · ` before the engine or shell text (statusline delta) |
| `/write` | `/write refused: Write is locked off by policy ({path})` |
| `/exec` (once PR16 ships) | `/exec refused: Exec is locked off by policy ({path})` |
| `/github` | `/github refused: the GitHub tools are locked off by policy ({path})` |
| `/write`, `/exec`, `/github` with `enabled = false` | `{command} refused: tools are locked off by policy ({path})` |
| `Prefix a` with one allowed mode | notice `approval is locked to ask by policy ({path})` |
| Guidance, an engine not in `[ai] engines` (A4) | `{engine}: not allowed by policy ({path}): pick another engine in the menu (Ctrl+Space m)` |
| Picker note row (A4) | `engines hidden: not allowed by policy` |
| Task row mark (A4) | ` · off: policy` |
| MCP set (A5), once per chat set | `MCP servers not started: user servers are off by policy ({path})` |
| `Updates` row suffix (A3) | ` · checks off: policy` |
| `r` on `Updates` (A3) | `update checks are off by policy ({path})` |

- `/write` and `/github` already refuse with `{command} refused: {reason}` (src/app/write.rs:28, 135; src/app/github.rs:33). The policy reason is checked first in `write_blocker` (src/app/write.rs:101-111) and its `/github` twin, because it is the one reason the user cannot fix.
- No text tells the user how to remove the policy. #668's local-only texts never tell how to turn the mode off (local-only design D4), so they need no change when the value is locked.

## D8 A file that cannot be used: fail closed (Q3 a+c, Q6 a, recommended)

| Case | Result |
|---|---|
| Missing (`ENOENT`, `ENOTDIR`) | no policy, no notice |
| Unreadable, not a regular file, too large, not UTF-8, not TOML, an unknown key | **strict baseline**, notice of D7 |
| A known key with a wrong value (`local_only = "yes"`, `approval = 3`, `engines = ["clawd"]`) | that key at its strictest; the other keys as written; notice |
| Untrusted owner or mode (D2) | applied as written, labelled `policy (untrusted)`, notice |

The **strict baseline** is the "Strictest value" column of D3: local-only mode on, tools off, approval `ask`, no update check, orchestrator off, no user MCP server; no extra engine list. It is a fixed table in `src/core/policy.rs`, so a test pins it.

An unknown key fails closed so that an older build meeting a newer administrator's restriction never ignores it (Q6); the README tells administrators to roll a new key out only after every machine runs a build that knows it.

## D9 The environment, flags and other ways around it

- The path is a constant (D1). No `LAZAROBOX_*` variable changes a setting today (proposal, Facts), and this change adds none; the spec forbids one for a lockable key.
- `XDG_CONFIG_HOME` and `HOME` choose the user file only; pointing them elsewhere gives another user file, still bounded.
- No flag takes a policy path. `--show-policy` (A6) takes no argument.
- Tests reach the reader only through parameters (D11); the release constant is used in one place, `run` in src/runtime.rs.
- What remains possible for the user is listed in D10.

## D10 Not a security boundary

**What it protects against:**

- a change from the menu, `/write`, `/github`, `/exec`, `Prefix a`, the WSL drive grant (when Write is locked off), or a hand edit of `config.toml`;
- a model: its tools cannot reach the policy (it is root-owned; writes outside the chat's roots are refused) and cannot loosen a bound even through a command the user approves that edits `config.toml`;
- a copied `config.toml` or dotfiles repository, a repository's `.lzr/policy.toml` (#658 can only tighten);
- drift across a fleet: one root-owned file, pushed by any tool that places files, gives every user the same bounds and says so on screen.

**What it does not protect against:**

- a user with root (or `sudo`), who edits or deletes the file;
- a user who runs another binary: a build before A1, a build from source with the check removed, a copy installed with `cargo install` into `~/.cargo/bin`;
- a user who runs `claude`, `codex`, `curl` or anything else in a shell pane: the terminal is the user's (#668, Out of Scope);
- an unprivileged user namespace that maps the user to uid 0 and bind-mounts another file over the path (U: depends on the distro allowing unprivileged namespaces);
- WSL: a Windows user can usually start the distro as root (`wsl -u root`, U);
- engines or MCP servers that the policy allows and that call out themselves.

The README says this in the data and threat-model sections (task 1.10).

## D11 Testing without root

- **Pure core** (`src/core/policy.rs`): `Policy::parse(text) -> (Policy, problems)`, the bounds, the combination of D4, the strict baseline, the texts, and `judge(facts, root_uid)` over a list of path facts. No I/O; most tests live here.
- **Runtime edge** (`src/runtime/policy.rs`): `read_policy(path: &Path, root_uid: u32) -> Verdict`. The production call passes `POLICY_PATH` and `0`; tests pass a file in a `tempfile` folder and `geteuid()`, so files the test creates count as "owned by root" for the check, and a `chmod g+w` on them proves the untrusted path. A FIFO, a symlink, a 65 KiB file and an unreadable file (`chmod 000`, skipped when the tests run as root) cover D8.
- **`startup`** gains a `policy: Option<&Path>` parameter next to `config_path` (src/runtime.rs:937-946); app tests build the app with a parsed `Policy`, never a path.
- **One string test** asserts `POLICY_PATH` per OS (`/etc/lazarobox/policy.toml` on Linux, the macOS path on macOS) without opening it.
- **Never** open the real `/etc`, `/Library` or `~/.config/lazarobox/` in a test; `cfg!(test)` additionally makes the production entry return "no policy", as WSL drives do (src/runtime/win_drives.rs:24-29).
- **Characterization:** with no policy every existing test stays green, and a menu save is byte-identical.

## D12 Code shape (assumption, for the implementer)

- `Policy` is parsed before the user file and handed to `AiConfig` and `Config` once (`with_policy`). `AiConfig`'s fields are private and read through accessors (`tools_enabled`, `approval`, `toggle`, `families`, src/core/ai/config.rs:629-660; `local_only` from #668 D1), so applying the bound inside those accessors covers every reader, the menu draft included. The raw user values stay in the struct, so a save writes only what the user changed.
- `local_only_source()` (#668 D1, D8) returns `Policy` when the policy sets it, else `User`; #658's `LocalRule` takes it as `LocalSource::Policy`, first in its order (#658 design D1, D9), so no new combination logic is needed there.
- `[updates] check` is read into `Config.update_check` (src/core/config.rs:461-476; src/runtime.rs:408-409; `can_check_update` src/core/menu.rs:1564-1566); A3 bounds it at the same place #668 S5 adds `update_check() && !local_only`.
- The MCP filter (A5) sits where #668 S6 filters by `local` (the chat set start, src/runtime/chat/external.rs:107-150).
