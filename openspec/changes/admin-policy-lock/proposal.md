# Proposal: Admin Policy Lock (a system policy file whose settings the user cannot loosen)

## Intent

Goal #659, "Goal: settings an administrator can lock (policy file)": "Let an administrator lock lazarobox's AI settings, starting with the strict local mode (#657), so a user cannot turn them off on a company machine."

The owner's direction (2026-11-10), verbatim:

> "que quede bloqueado puede ser también un goal, no es que ahora esté así, si no que es seleccionable, ese punto tiene que dejarse claro en el README"

Today every setting is the user's choice: it lives in the user's own `config.toml`, which the user (and any program the user runs) can edit, and most of it is also changed from the menu or from the chat at runtime. This change adds **one optional system policy file**, installed by an administrator (root), whose values **bound** what the user's `config.toml`, the menu, the chat commands and the project files (#658) may choose. Without the file, lazarobox behaves exactly as today.

While a policy is installed:

- a locked setting cannot be made looser by the user: not from the menu, not by editing `config.toml`, not by a chat command (`/write`, `/github`, the planned `/exec`, `Prefix a`), not by a repository file;
- the menu shows each locked row greyed, with `locked by policy` and the file path;
- the start-up notice says when the user's file asks for something the policy does not allow;
- the user can still make any setting **stricter** than the policy.

**What this is not.** It is not a security boundary against a user who has root, or who can run another binary (an older lazarobox, a build from source, `claude` typed in a shell pane). It is a guardrail that keeps a managed machine in the posture the company chose, against mistakes, the menu, copied dotfiles, repository files and a model's tool calls (design.md, D10).

**Selectable today, lockable with this goal.** The README must say in so many words that today everything is the user's choice and that a lock arrives only with this goal (owner, 2026-11-10). The proposed Spanish text is task 1.10 of tasks.md; README.md is not changed by this proposal.

**Related changes.**

- #668 `openspec/changes/local-only-mode/` (open, not merged) adds `[ai] local_only` behind one accessor and states how #659 can lock it (local-only design D8). This change builds on that hook and does not edit that change.
- #658 `openspec/changes/project-engine-policy/` (being written) lists the system policy as source 1 of the project rule and says "no source can loosen another" (its proposal, "The rule, in one line"). This change keeps that rule (design.md, D4).
- #570 (team work in the cloud) is a future outbound call class; a policy key can turn it off when it ships (design.md, D3, later keys).

## Decisions (owner, settled)

- **O1 Selectable today, lockable as a goal** (2026-11-10, quoted above). No setting is locked today; this goal makes locking possible, and the README says which is which.
- **O2 The global switch always wins over a project rule** (#658, "To decide when this starts"; #658 proposal O1). The system policy sits above both.

Everything else is a proposal. Open product decisions are at the end (Q1-Q7), each with a recommendation.

## Facts this proposal rests on

V = verified in the code at 4760d0dd (the #668 branch; its source tree equals `main` at e480992e, 2026-11-10). Inferred = read from names, comments or docs, not traced end to end. U = unverified (OS behaviour not checked on a machine).

### How configuration is loaded today

- **One user file, one path.** `config_path` is `$XDG_CONFIG_HOME/lazarobox/config.toml`, else `$HOME/.config/lazarobox/config.toml`; an empty or relative `XDG_CONFIG_HOME` or `HOME` is ignored (src/core/config.rs:501-511). It is resolved once at start (src/runtime.rs:253-254) and read once by `startup` (src/runtime.rs:937-946). With no usable `HOME` and no `XDG_CONFIG_HOME` there is no file and every default applies (src/runtime.rs:945). V
- **Read at start only.** A missing file is silent defaults; an unreadable one gives the notice `config: {error}` and defaults (src/core/config.rs:309-322). The menu applies its saved draft at once; a hand edit is read at the next start (#668 proposal, Facts, inferred there). V for the read; inferred for "no watcher".
- **Problems become one notice.** Each invalid key adds a problem and falls back to its default; the start-up notice shows the first problem, prefixed `config` or `config ({n} problems)` (src/core/config.rs:324-336). V
- **Unknown keys are reported in owned tables** since 1.11.0: `[ai]` and its sub-tables (`note_unknown_ai_keys`, src/core/ai/config.rs:1522-1596) and the top-level `build`, `statusline`, `tabbar`, `updates`, `workspace` (`OWNED_SECTIONS`, src/core/config.rs:362-373). Any other top-level table is the user's own and never reported. V
- **No system-wide file is read.** Nothing in `src/` opens a path under `/etc`, `/Library` or a Windows folder for settings. The only system paths in the code are the jail's read-only grants for the CLI engines (`/etc` among them, src/core/ai/jail.rs:874, 936; `/Library` on macOS, jail.rs:894) and WSL's `/proc/mounts` and `/proc/sys/kernel/osrelease` (src/runtime/win_drives.rs:35-40). V
- **No environment variable changes a setting.** The only `LAZAROBOX_*` names in the code are internal: `LAZAROBOX_MCP_TOKEN`, `LAZAROBOX_SESSION_TOKEN` (the MCP shim), `LAZAROBOX_JAIL_STATUS_FD` (src/core/ai/jail.rs:39) and the `__LAZAROBOX_LOGIN_PATH__` marker (src/runtime/login_path.rs:34). The environment chooses where the user file is (`XDG_CONFIG_HOME`, `HOME`), the default memory port (`ENGRAM_PORT`, src/runtime.rs:256; src/core/config.rs:225-229), the Hugging Face token (`HF_TOKEN`, `HF_TOKEN_PATH`, src/runtime/hf.rs:744-757) and the engines' own homes (`CODEX_HOME`, `GEMINI_CLI_HOME`, src/runtime/chat/cli.rs:210-213). None of them is a lockable setting. V
- **The command line has two flags,** `--dir` and `--check-prefix` (src/core/start.rs:10, 13). Neither changes a setting. V
- **Precedents for trusting a file by its metadata.** `secrets.toml` is opened with `O_NONBLOCK | O_NOCTTY` and judged on the open handle: a regular file, no group or other bit, at most a byte cap (`read_private`, src/runtime/mcp_secrets.rs:29-62). The gemini home must be a folder of the user (`uid`), not writable by group or others (src/runtime/chat/cli/home.rs:224-245). Both use `libc` (Cargo.toml:41). V
- **Tests never touch the real machine.** WSL drives are empty under `cfg!(test)` so tests "never depend on the machine that runs them" (src/runtime/win_drives.rs:24-29); `config_path` takes the environment as parameters (src/core/config.rs:503) and the runtime tests use temporary folders (for example src/runtime.rs:2587). V
- **Platforms.** The code has 152 `target_os = "macos"` gates and no `windows` gate (grep over `src/`); WSL is detected inside Linux (`is_wsl`, src/core/desktop.rs:79-82). So lazarobox runs on Linux, macOS and Linux under WSL; there is no native Windows build. V

### Everything the user can change at runtime today

| # | Where | What it changes | Code | Persisted |
|---|---|---|---|---|
| 1 | Menu, Settings tab: `Statusline position`, `Tab bar position`, `Mouse`, `Build jobs`, `Notify`, `Sound`, `llama-server idle`, `Usage (alpha)` | layout, notifications, build jobs, idle stop | `SECTIONS` src/core/menu.rs:339-439 | `config.toml` on `s` |
| 2 | Menu, `Updates` row | `r` checks crates.io now; Enter installs an update in a pane. `[updates] check` itself is config-only | `ItemKind::Updates` src/core/menu.rs:267-270; `can_check_update` menu.rs:1564-1566; start check src/runtime.rs:408-409 | no |
| 3 | Menu, Models tab: `Set all tasks`, the six task rows, `Hugging Face` (token connect) | engine and model per task | `MODELS_SECTION` src/core/menu.rs:556-592; picker src/core/ai/picker.rs:208-279 | `config.toml` |
| 4 | Menu, Tools tab: `Tools`, `Search`, `Write`, `Exec`, `Git`, `Git write`, `GitHub`, `Issue draft`, `Approval` (`ask`, `auto`, `run`), `Orchestrator` | tool families, approval mode, ORCH gate | `TOOLS_SECTION` src/core/menu.rs:596-665; `ToolToggle` src/core/ai/config.rs:199-207; `ApprovalMode` config.rs:153-168 | `config.toml` |
| 5 | Menu, Chat tab: `Profile`, `Context`, `Memory` | prompt profile, context, engram memory | `CHAT_SECTION` src/core/menu.rs:668-728 | `config.toml` |
| 6 | Menu, Connectors tab | no rows yet (mcp-connectors is a proposal) | `TABS` src/core/menu.rs:745-786 | — |
| 7 | #668 S2 (planned): `Local only` row | `[ai] local_only` | local-only-mode config-menu delta | `config.toml` |
| 8 | Chat `/write` | turns Write on for this chat, after a `y` dialog | src/app/write.rs:1-28, 101-148; parse src/core/ai/chat/folder.rs:99-100 | no (per chat, in memory) |
| 9 | Chat `/github` | turns the GitHub read tools on for this chat | src/app/github.rs:27-41; github-tools spec "The chat grant" (openspec/specs/github-tools/spec.md:234) | no |
| 10 | Chat `/exec` (planned, orchestrator-worker-flow PR16) | turns Exec on for this chat, "on the same path" as `/write` | openspec/changes/orchestrator-worker-flow/tasks.md:578-581 | no |
| 11 | `Prefix a` | cycles the chat's approval mode `ask` → `auto` → `run` | `toggle_approval` src/app/tool.rs:507-530; binding src/core/prefix.rs:255-259 | no (per chat) |
| 12 | WSL drive grant | after `y`, "writes allowed in {folder} until you quit" for a `/mnt/<drive>` folder | `offer_drive_grant` src/app/folder.rs:395-414; `drive_granted_line` src/core/ai/chat/approval.rs:1071-1076; `ChatState.drive_grant` src/core/ai/chat/mod.rs:480, 1134-1141 | no |
| 13 | Ctrl+O | ORCH mode of a chat, gated by `[ai.orchestrator] enabled` | src/app/app_tests/orchestra_toggle.rs:1-2 | no |
| 14 | `y` on each tool call | one call | src/core/ai/chat/approval.rs | no |
| 15 | Hand edit of `config.toml`, e.g. `[[ai.mcp.servers]]` (the menu never writes `ai.mcp.*`) | anything | keys src/core/ai/config.rs:1585-1594; `read_mcp_servers` config.rs:1130-1135 | yes |
| 16 | Planned: the "full grant" of writes in `run` mode (the owner's chat-auto-permissions feature, not yet in the repository) | later writes apply without a `y` for this conversation | — | no |

What a model can and cannot reach: the read and write tools refuse `~/.config/lazarobox` and `~/.local/share/lazarobox` (`SECRET_DIRS`, src/core/ai/tools/paths.rs:23-40), so a tool call cannot edit `config.toml` at its default place; a custom `$XDG_CONFIG_HOME` is not followed (paths.rs:19-22). A command the user approves (`run_command`, `run_in_pane`) runs as the user and can edit any file the user can (inferred). The approval mode is "only the user switches it: the model can never reach it" (src/core/ai/config.rs:153-155). V

## Scope

### In Scope

- One policy file per platform at a fixed path (design.md, D1), with a trust check on its owner and mode (D2) and a bounded, non-blocking read.
- Its format: the same tables and key names as `config.toml`, each key a bound the user cannot loosen (D3, D5).
- Precedence: policy > user `config.toml` > project file (#658); any source may tighten, none may loosen (D4).
- The first lockable keys: `[ai] local_only`; tools (`[ai.tools]` families, `enabled`, `approval`); `[updates] check`; `[ai.orchestrator] enabled`; an engine allow-list `[ai] engines`; the user's MCP servers `[ai.mcp] user_servers` (D3), in slices.
- Locked rows in the menu, the refusal of runtime grants, the start-up notices (D6, D7).
- An invalid, unreadable or untrusted file (D8).
- README "🛡️ Tus datos" and "Seguridad / modelo de amenazas", CHANGELOG.

### Out of Scope

- macOS configuration profiles (`/Library/Managed Preferences`, plist), Windows registry or Group Policy, Intune: Q1 names them.
- Signing or hashing the policy file.
- Pushing the policy (MDM, Ansible): the file is a plain root-owned TOML file, so any tool that can place a file can push it (#659, "To decide"); nothing lazarobox-specific is needed.
- Locking layout and comfort settings (statusline, mouse, notify, build jobs, profile).
- Blocking what the user types in a shell pane.
- Making lazarobox tamper-proof against root or another binary (D10).

## Capabilities

### New Capabilities

- `admin-policy`: the file, its location and trust check, its keys and their bounds, precedence, invalid files, runtime refusals, testing seams.

### Modified Capabilities

- `local-only-mode` (from #668): MODIFIED "The switch is the user's" (the policy may lock it on).
- `config-menu`: ADDED "Locked rows".
- `statusline`: ADDED "Policy mark in the local-only segment".

## Slices and size forecast

Production lines (code, not tests or docs), advisory only. Each slice ships its tests, README and CHANGELOG lines.

| Slice | Content | Prod. lines | Needs |
|---|---|---|---|
| A1 | Path constant per OS; trust check; bounded read; parse of `[ai] local_only` and the policy's unknown-key walk; fail closed (Q3); `local_only()` = policy OR user, `local_only_source()` = `Policy`; start-up notices; status bar `local only (policy) · `; the generic locked-row mechanism in the menu, used by `Local only`; README | 230-330 | #668 S1 (accessor), S2 (row) |
| A2 | Tools: `[ai.tools] enabled`, the seven families, `approval` allowed modes; locked Tools-tab rows; `/write` and `/github` refusals; `Prefix a` skips disallowed modes; drive grant not offered while Write is locked off; `/exec` refusal if PR16 has landed (else PR16 adds it) | 200-300 | A1 |
| A3 | `[updates] check = false` (start check, `r`, llama-server newer build) and `[ai.orchestrator] enabled = false` (row, Ctrl+O) | 80-140 | A1; shares `update_check()` with #668 S5 if it has landed |
| A4 | `[ai] engines` allow-list: refusal in the one availability check, picker hides the others with a note row, task-row mark | 150-230 | A1; #668 S1 (position in `availability_for_with`), S2 (picker note row); aligned with #658 Q4 (b) |
| A5 | `[ai.mcp] user_servers = false`: no user server starts; a notice once per chat set; Connectors tab note | 80-150 | A1; #668 S6 if it has landed (same filter point) |
| A6 (optional, Q5) | `lazarobox --show-policy`: prints the path, the trust verdict and the effective bounds, then exits | 60-100 | A1 |

Total about 800-1250 lines over five or six PRs. **A1 is useful alone:** an administrator who installs `/etc/lazarobox/policy.toml` with `[ai] local_only = true` gets every cloud engine refused for that user, whatever the user's file says, with the status bar and the menu row saying so.

**Dependencies on #658.** None for A1-A3, A5. #658 P1 replaces the `cfg.local_only()` read in `availability_for_with` by a `LocalRule` (its proposal, slice P1). Whichever lands second adapts: the policy is the first input of that rule and names its source `policy`. A4 and a future #658 `engines` list (its Q4 b) combine by intersection.

## Risks

- **Older builds ignore the file.** Every build before A1 never opens it, and a user can install one (`cargo install lazarobox-shell --version <old>`, into `~/.cargo/bin`). The README says from which version the policy is enforced, and that it is a guardrail, not a sandbox (D10).
- **A newer policy key on an older build.** A1 reads only the keys it knows. Q3/Q6 decide whether an unknown key fails closed (recommended) or is reported and ignored.
- **A policy that refuses everything.** `local_only = true` on a machine with no local engine refuses every task; that is the administrator's intent, and each refusal names the file.
- **A wrong trust check locks people out.** A root-owned file in a folder that is group-writable (`/etc/lazarobox` made `775` by a script) is untrusted; with fail closed (Q3) the user gets the strictest bounds and a notice that names the fix. The check follows OpenSSH-style safe-path rules (D2), and the Nix store case is unverified.
- **WSL.** A Windows user can usually start their distro as root (`wsl -u root`, U), so on WSL the lock holds only against the menu and casual edits (D10, Q1).
- **False sense of safety.** The README states what the lock covers and what it does not (task 1.10).

## Open questions

### Q1 The file paths per platform

- **(a) `/etc/lazarobox/policy.toml` on every platform.** + One path to document, test and push; on macOS `/etc` is a link to `/private/etc`, owned by root (U). − Not where macOS administrators look; some MDM tools target `/Library`.
- **(b) One path per OS:** Linux and WSL `/etc/lazarobox/policy.toml`; macOS `/Library/Application Support/lazarobox/policy.toml` (Apple's place for system-wide app files, U). + Conventional on each OS. − Two paths; macOS `/Library/Application Support` is usually `root:admin` and its mode must be checked (U).
- **(c) (b) plus macOS Managed Preferences** (`/Library/Managed Preferences/<id>.plist`, written by MDM configuration profiles, U). + Native MDM. − Plist parsing, a bundle id lazarobox does not have; its own slice.
- **(d) For WSL, also a Windows-side file** (`/mnt/c/ProgramData/lazarobox/policy.toml`) so Intune can push it. − drvfs owner and mode bits are synthesized by mount options, so the trust check means nothing there (U); a Windows user can become root in the distro anyway.
- Never a search list across several folders, never a path from the environment, a flag or `config.toml` (D1): any of those lets the user point lazarobox at a file of their own.
- **Recommendation: (b)**, one fixed path per OS, (c) and (d) as later goals if an administrator asks.

### Q2 Which keys are lockable in the first slice

- **(a) Only `[ai] local_only`** (A1). + The goal's first named setting (#659, "What this adds"); smallest; uses #668's hook. − An administrator who also wants tools off waits for A2.
- **(b) `local_only` plus tools** (A1 + A2). − About 430-630 lines, over the budget for one PR.
- **(c) Everything listed** in one change. − Far over budget.
- **Recommendation: (a)** in A1, then A2 (tools), A3 (updates, orchestrator), A4 (engines), A5 (MCP), in that order of value.

### Q3 An invalid, unreadable or untrusted file: fail closed or open?

Missing (`ENOENT`, `ENOTDIR`) always means "no policy". The question is for a file that exists but cannot be used: unreadable (`EACCES`), not a regular file, too large, not UTF-8 or not TOML, untrusted owner or mode (D2), or holding an unknown key.

- **(a) Fail closed:** every lockable setting at its strictest (D8 lists them), and the notice `policy: {path}: {problem} (every lockable setting is at its strictest until this is fixed)`. + An administrator's intent is never dropped by a typo or a bad `chmod`; matches #668 Q6 and #658 Q5 (fail closed). − A broken file locks a user out of cloud engines, tools and updates until the administrator fixes it.
- **(b) Fail open:** ignore the file, with the notice. + The user can work. − Any mistake turns the policy off silently for the administrator; a user who can make it unreadable (they cannot, if the trust check passes) would bypass it.
- **(c) Per key:** a known key with a wrong value fails closed for that key only; a file that cannot be parsed at all fails closed for everything. + Smallest blast radius. − Same as (a) for the whole-file cases.
- An untrusted file (writable by a non-root user): because every key can only tighten (D5), applying its content can never loosen anything. So **apply it and warn** is safe for the user and honest about the source.
- **Recommendation: (a) with (c) for single keys**: a whole-file failure is strictest everything; a known key with a wrong value is strictest for that key; an untrusted file is applied as written, labelled `policy (untrusted)`, with a notice naming the fix.

### Q4 A locked value or a ceiling?

- **(a) Locked value:** the policy pins the exact value (`local_only = false` would forbid the user to turn it on). + Simple to explain. − A policy could force a looser setting than the user wants (pin the update check on, pin Write on), which contradicts #658's "no source can loosen another" and turns the policy into a way to push risk onto a user.
- **(b) Ceiling (a bound):** each key says how permissive the user may be at most; the user and the project may be stricter. `local_only = true` means "on, at least"; `write = false` means "off, at most"; `approval = ["ask", "auto"]` means "one of these". + Consistent with #658; an untrusted or mistaken file can only tighten; the user is never forced into a looser state. − A value that would loosen (`local_only = false`, `write = true`) locks nothing, which an administrator may not expect: the notice says so.
- **Recommendation: (b).** The menu still says `locked by policy` whenever the user cannot pick the looser value.

### Q5 Does lazarobox show the policy to the user?

- **(a) Only where it bites:** locked rows, the footer with the path, the refusals, the start-up notice. + No new surface; enough for the user. − No single place lists every lock.
- **(b) A `/policy` chat command** that prints the path and every bound into the transcript. + Visible where the user works. − A chat-only view of a machine setting; the transcript is not the place for settings.
- **(c) A `Policy` menu tab** shown only when a policy is installed. + All locks in one place. − A new tab (`TAB_COUNT` bump, jump keys `1-6`, src/core/menu.rs:745-749) and its scenarios; duplicates the locked rows.
- **(d) `lazarobox --show-policy`** (A6): prints the path, the trust verdict and the effective bounds, then exits; no argument, so it cannot point at another file. + What an administrator needs to check a fleet (Ansible can run it); testable without a TUI. − Not visible inside the TUI.
- **Recommendation: (a) in A1-A5, plus (d) as A6.** (b) and (c) only if users ask.

### Q6 Unknown keys in the policy file

- **(a) Fail closed** (strictest everything, as Q3 a). + A newer administrator's restriction is never ignored by an older build. − A fleet with mixed versions gets the older machines fully locked until they update.
- **(b) Reported and ignored**, as in `config.toml` (`policy: unknown key: {path}`). + Mixed fleets keep working. − The older build silently allows what the newer key forbids, which is exactly what a policy must not do.
- **Recommendation: (a)**, and the README tells administrators to deploy a new key only after every machine runs a build that knows it (`--show-policy` from A6 shows the known keys).

### Q7 The first slice

- **(a) A1 as listed:** the file, `local_only`, fail closed, the notices, the status bar, the locked-row mechanism used by `Local only`. About 230-330 lines.
- **(b) A1 without the menu lock** (about 170-240). − The `Local only` row would still toggle and save `false`, which the policy then overrides: a row that lies.
- **(c) A1 before #668 S2**, with no row to lock. + Starts earlier. − The row lock then lands in #668 S2 or a follow-up.
- **Recommendation: (a)**, after #668 S1 and S2 have merged.
