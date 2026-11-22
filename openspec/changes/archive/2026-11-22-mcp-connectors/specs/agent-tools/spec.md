# Delta for Agent Tools

## Base of this delta

This delta targets the owning spec `openspec/specs/agent-tools/spec.md` (the `agent-tools` and `coding-tools` changes were archived on 2026-11-02). The MODIFIED block below is a full copy of its requirement "Path policy" as it stands on f7d6b6f4 (openspec/specs/agent-tools/spec.md:115-211, including the WSL drive read policy wording added by wsl-drive-access): the only text change is in the list of excluded `$HOME` paths (three token locations added, and the note on `$HOME/.config/lazarobox` now names `secrets.toml` instead of "its `config.toml` holds MCP env tokens"), plus one added scenario at the end. Amendment 2026-11-11 (S6b1): mcp-gsc keeps its OAuth token in `platformdirs.user_config_dir("mcp-gsc")`, which is `~/Library/Application Support/mcp-gsc` on macOS (verified on mcp-search-console 0.4.2), so that path joins the list (`SECRET_DIRS` gains `Library/Application Support/mcp-gsc`), with one more scenario. `~/.google_workspace_mcp` (plaintext workspace-mcp tokens) is already refused since S1 and stays; the `google` entry never sets `WORKSPACE_MCP_CREDENTIALS_DIR`, so the tokens stay there. All other requirements stay as they are, except the three S9 blocks at the end of this file (owner decision 2026-11-14, C13: a scoped exception for user-configured MCP servers, level chosen per server; see mcp-servers, Approval level per server): Approval model (openspec/specs/agent-tools/spec.md:231-233 on origin/main 3b9548c2; the sentence "no configuration that approves automatically" and "Tools from a user MCP server MUST always be write tools" change), Chat approval mode (:313-314; "every user MCP tool still need `y`" gets the exception) and Toolsets by session role (:628-629; the coordinator's user tool calls follow the level). Each is a full copy with the edit marked "(Previously: ...)". In code the list is `SECRET_DIRS` (src/core/ai/tools/paths.rs:24-34); it also feeds the write check and the git tools (paths.rs:346; src/core/ai/tools/git.rs:207, 240), so the added paths are refused by them as well.

Traceability: C6 (secrets refused by the read tools), C8 (read-deny additions in the first slice), proposal scope item 2.

## MODIFIED Requirements

### Requirement: Path policy
Tools MUST only return or open paths inside `$HOME` (absolute, or `~`-relative as input) or, under WSL, inside a Windows drive folder that the WSL drive read policy allows (see that requirement). A path MUST be refused when, after resolving `.`, `..` and every symlink to its real location, it is outside `$HOME` and not allowed by the WSL drive read policy, is `$HOME/.ssh` or `$HOME/.gnupg`, is or lies under any entry of `CREDENTIAL_DIRS` directly under `$HOME` (`.claude`, `.gemini`, `.codex`, `.aws`, `.kube`, `.docker`, `.password-store`), or is `$HOME/.config/gh`, `$HOME/.config/gcloud`, `$HOME/.local/share/keyrings`, `$HOME/.local/share/fish/fish_history`, `$HOME/.google_workspace_mcp` (workspace-mcp tokens), `$HOME/.clasprc.json` (clasp tokens), `$HOME/.config/mcp-gsc` (mcp-gsc tokens), `$HOME/Library/Application Support/mcp-gsc` (mcp-gsc tokens on macOS, added 2026-11-11, S6b1), `$HOME/.config/lazarobox` (it holds `config.toml` and `secrets.toml`, the values of the MCP servers; see mcp-secrets) or `$HOME/.local/share/lazarobox` (the app's own store), both at their default XDG place (`XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `GH_CONFIG_DIR` overrides are not followed), or is a file named `.env`, `.env.*`, `.envrc`, `.netrc`, `.git-credentials`, `.pgpass`, `.npmrc`, `.bash_history`, `.zsh_history`, `.zhistory`, `.python_history`, `.node_repl_history`, `.psql_history`, `.mysql_history` or `.sqlite_history` at any depth. The check MUST be made on both the requested path (`~` expanded, a relative input resolved against the origin cwd, `.` and `..` folded by text) and the real path that is opened, and the path is allowed only if both pass. So a symlink inside `$HOME` that points to an excluded or outside path is refused, and so is an excluded name that is itself a symlink to an ordinary place (for example `~/.ssh` linked to `~/dotfiles/ssh`). The real path of every existing secret root (`~/.ssh`, `~/.gnupg`, each `CREDENTIAL_DIRS` entry and each excluded file name directly in `$HOME`) MUST also be resolved, and a real path equal to or under one of those real roots MUST be refused, so the real target of an aliased secret folder (`~/dotfiles/ssh`) and any second link to it are excluded as well. Known limit: a link below `$HOME` depth one named like ANY excluded file (`.env`, `.netrc`, `.npmrc`, a history file...) whose target has an ordinary name is judged by its requested name only, and hard links are not detected here (slice 2b checks `st_nlink`). The resolved roots are a snapshot taken at load: a root created after the load is out of scope for that call, and slice 2b MUST re-check the real path of the opened file descriptor against them. The runtime MUST also pass lazarobox's real config and data folders (honouring `XDG_CONFIG_HOME` and `XDG_DATA_HOME`) as extra roots, resolved with the same fail-closed rules. A `$HOME` whose canonical path is `/`, `/home`, `/Users`, `/usr`, `/var`, `/etc`, `/opt`, `/tmp`, `/bin`, `/lib`, `/proc`, `/sys` or `/dev`, `/run`, `/mnt`, `/media`, `/srv`, `/sbin`, `/boot`, `/lib64`, `/private`, `/private/tmp`, `/private/var`, `/private/etc`, `/System` or `/Library`, compared ASCII case-insensitively per component, MUST be refused (`/root` is allowed). If `$HOME` or any secret root that exists cannot be read (an error other than not-found or not-a-directory, which are skipped as an absent root; for example a permission error or a symlink loop), every call MUST be refused (fail closed). The secret roots MUST be resolved once per tool call, not once per path or listed entry. The deny list is best effort, not complete, and `.envrc` and `.npmrc` are denied at any depth on purpose. Comparison of the excluded names is ASCII case-insensitive. `~user` input, a relative or `/` home and a relative origin MUST be refused. A relative input path is resolved against the chat's origin cwd and then checked like any other. Excluded entries MUST be omitted from `list_dir` and `find_dir` results, and `find_dir` MUST NOT descend into excluded directories. A refusal MUST use one message for excluded, outside and non-existent paths, refusals of the WSL drive read policy included, so the existence of an excluded path is not revealed.

#### Scenario: Credential directories [auto]
- GIVEN `$HOME/.ssh/id_ed25519`, `$HOME/.aws/credentials`, `$HOME/.claude/x`, `$HOME/.gnupg/k`
- WHEN `read_file` is called on each
- THEN each is refused and no byte of any is returned

#### Scenario: Every credential dir [auto]
- GIVEN each entry of `CREDENTIAL_DIRS`
- WHEN `list_dir` is called on it and on a child of it
- THEN both are refused

#### Scenario: Credential files, tool configs, keyrings, histories and the app store [auto]
- GIVEN `$HOME/.netrc`, `$HOME/dev/.npmrc`, `$HOME/.zsh_history`, `$HOME/.config/gh/hosts.yml`, `$HOME/.local/share/keyrings/x` and `$HOME/.local/share/lazarobox/chat.db`
- WHEN `read_file` is called on each
- THEN each is refused with the one identical message

#### Scenario: Dotenv at any depth [auto]
- GIVEN `$HOME/dev/app/.env` and `$HOME/dev/app/.env.production`
- WHEN `read_file` is called on each, and `list_dir` on `$HOME/dev/app`
- THEN both reads are refused and neither name appears in the listing

#### Scenario: Outside HOME [auto]
- GIVEN `/etc/passwd` and `/tmp/x`
- WHEN `read_file` is called on each
- THEN both are refused

#### Scenario: Traversal [auto]
- GIVEN `~/dev/../../etc/passwd` and `~/../other/file`
- WHEN `read_file` is called on each
- THEN both are refused

#### Scenario: Symlink to an excluded path [auto]
- GIVEN `$HOME/dev/link -> $HOME/.ssh/id_ed25519`
- WHEN `read_file` is called on `~/dev/link`
- THEN it is refused

#### Scenario: Excluded name that is a symlink to an ordinary place [auto]
- GIVEN a dotfile manager: `$HOME/.ssh -> $HOME/dotfiles/ssh` and `$HOME/app/.env -> $HOME/secrets/app.env`
- WHEN `read_file` is called on `~/.ssh/id_ed25519` and on `~/app/.env`
- THEN both are refused, because the requested path names an excluded entry even though the real path does not

#### Scenario: Real target of an aliased secret folder [auto]
- GIVEN `$HOME/.ssh -> $HOME/dotfiles/ssh`, `$HOME/foo -> $HOME/dotfiles/ssh` and `$HOME/.env -> $HOME/secrets/app.env`
- WHEN `read_file` is called on `~/dotfiles/ssh/id_ed25519`, on `~/foo/id_ed25519` and on `~/secrets/app.env`
- THEN all three are refused

#### Scenario: HOME that is the root [auto]
- GIVEN a HOME that is a symlink to `/`, or is `/home` (while `/root` is accepted)
- WHEN any path is resolved
- THEN it is refused

#### Scenario: Unreadable secret root [auto]
- GIVEN a `$HOME` folder that cannot be read, or a secret root whose resolution fails with a permission error
- WHEN any path is resolved
- THEN it is refused, while a secret root that is simply absent is skipped

#### Scenario: Oversized argument [auto]
- GIVEN a `path`, `query` or `command` argument of 4097 bytes, and one of 4096 bytes
- WHEN the call is parsed
- THEN the first is refused with a too-long error and the second is accepted

#### Scenario: Tilde input [auto]
- GIVEN `$HOME/dev/link -> /etc`
- WHEN `list_dir` is called on `~/dev`, on `~/dev/link`, on `~/../other/file` and on `~root`
- THEN only `~/dev` is listed; the others are refused

#### Scenario: Symlink out of HOME and symlinked directory [auto]
- GIVEN `$HOME/dev/etc -> /etc` and `$HOME/dev/keys -> $HOME/.aws`
- WHEN `list_dir` is called on each
- THEN both are refused

#### Scenario: Symlink inside HOME is fine [auto]
- GIVEN `$HOME/dev/current -> $HOME/dev/curso-rust`
- WHEN `list_dir` is called on `~/dev/current`
- THEN the entries of the real directory are returned

#### Scenario: Path swapped after the check [design-confirm]
- GIVEN a path that is replaced by a symlink to an excluded file between the check and the open
- WHEN `read_file` runs
- THEN the open is made on the checked real location or fails; excluded bytes are never returned

#### Scenario: find_dir does not descend [auto]
- GIVEN `$HOME/.aws/curso-rust` and `$HOME/dev/curso-rust`
- WHEN `find_dir` is called with `curso-rust`
- THEN only `~/dev/curso-rust` is returned

#### Scenario: Refusals do not leak existence [auto]
- GIVEN an existing excluded file and a missing file in `$HOME`
- WHEN `read_file` is called on each
- THEN both results carry the same error text

#### Scenario: Relative input [auto]
- GIVEN origin cwd `$HOME/dev` and the input `curso-rust`
- WHEN `list_dir` is called
- THEN `$HOME/dev/curso-rust` is checked and listed, and `../../..` from it is refused

#### Scenario: Connector token locations [auto]
- GIVEN `$HOME/.google_workspace_mcp/credentials/u.json`, `$HOME/.clasprc.json`, `$HOME/.config/mcp-gsc/token.json` and `$HOME/.config/lazarobox/secrets.toml`
- WHEN `read_file` is called on each, and `list_dir` on `$HOME` and on `$HOME/.config`
- THEN each read is refused with the one identical message, and none of `.google_workspace_mcp`, `.clasprc.json`, `mcp-gsc` or `lazarobox` appears in the listings

#### Scenario: mcp-gsc token on macOS [auto]
- GIVEN `$HOME/Library/Application Support/mcp-gsc/token.json`
- WHEN `read_file`, `grep` and `list_dir` of `$HOME/Library/Application Support` are called
- THEN the read is refused with the one identical message and `mcp-gsc` is omitted from the listing

### Requirement: Approval model
`gh_issue_draft` needs its own `y` on every call, like `run_command`, `git_add` and `git_commit`, and no approval mode answers it (see issue-draft).
`find_dir` and `list_dir` MUST run without approval, and `read_file` and `grep` MUST run without approval only on a local engine (see Read tools and their approval, code-search). `glob` MUST run without approval. Every other tool call MUST need an explicit user `y`, every time, and the approval of the file write tools is the batch review (see code-write): `create_dir`, `write_file` and `edit_file` stage and ask nothing by themselves, and nothing reaches the disk until the user approves the proposed batch as a whole. `run_command`, `git_add` and `git_commit` MUST each need their own `y` (see command-runner, git-tools). The only exception is a `run_in_pane` call in a chat whose approval mode is `auto` (see Chat approval mode): it is typed without a prompt and never executed. The fixed program allow-list of `run_command` bounds the program and is not an approval. There MUST be no "always allow", no per-session or per-tool trust, no configuration that approves automatically, and nothing MUST ever execute a command for the user without a `y`, with ONE scoped exception (owner decision 2026-11-14, C13): the approval level the user sets for a user-configured MCP server (`approval = "ask" | "reads" | "all"` in its `[[ai.mcp.servers]]` entry, default `ask`; see mcp-servers, Approval level per server). That level is the only configuration that approves a call automatically; it covers only the tools of that server, never a built-in tool, never a command, never a worker, and it does not change anything else in this requirement; and it is not honored in a conversation that read GitHub text (blocked, owner decision 2026-11-15): there every user MCP tool asks `y` whatever the level, and the prompt shows the block reason. A tool from a user MCP server is a write tool unless the catalog classes it `read` for a server declared with the catalog entry's own command (see mcp-catalog, Read/write class), and a server's own read-only hint is never a reason to treat it otherwise. Waiting for an answer MUST NEVER end in an approval: a timeout, a cancel, a closed chat, a closed target pane, a lost connection and an app quit all end as `denied`. A chat MUST have at most one pending approval; another write request while one is pending MUST be denied at once with the reason `another approval is pending`.
(Previously: "...no configuration that approves automatically, and nothing MUST ever execute a command for the user without a `y`. Tools from a user MCP server MUST always be write tools."; C11 kept `y` on every user-tool call, C13 relaxes it per server.)

#### Scenario: No auto approval [auto]
- GIVEN a write tool call from any engine in a chat in `ask` mode, or an external tool call of a server at level `ask`, a `run_command` or a cloud `read_file` in either mode
- WHEN no key is pressed for any length of time (injected clock)
- THEN the call is never executed

#### Scenario: Timeout is denied [auto]
- GIVEN a pending approval and a caller whose wait expires
- WHEN the expiry fires
- THEN the call result is `denied`, the prompt is withdrawn with a system line, and a later `y` writes nothing

#### Scenario: Each call asks again [auto]
- GIVEN a chat in `ask` mode and two `run_in_pane` calls in one turn, the first approved
- WHEN the second is dispatched
- THEN a new approval is required

#### Scenario: Second request while pending [auto]
- GIVEN a pending approval
- WHEN another write tool call arrives
- THEN it returns `denied` (`another approval is pending`) and the first stays pending

#### Scenario: Denied paths [auto]
- GIVEN a pending approval ended in turn by cancel, pane close, target-pane exit and quit
- WHEN each ends
- THEN the result is `denied` each time and nothing is written

#### Scenario: Free read tools around a pending approval [auto]
- GIVEN a pending approval
- WHEN `find_dir`, `list_dir` or `glob` is called (or `read_file` on a local engine)
- THEN it runs and returns without waiting for the approval

#### Scenario: Second read_file while pending [auto]
- GIVEN a pending approval on a cloud engine
- WHEN a `read_file` call arrives
- THEN it returns `denied` (`another approval is pending`)

#### Scenario: Staging asks nothing, apply needs y [auto]
- GIVEN `write_file` and `edit_file` calls in a chat
- WHEN they are dispatched
- THEN no approval opens and the disk is untouched, and only the review of the proposed batch can write

#### Scenario: Allow-list is not an approval [auto]
- GIVEN `run_command` with an allow-listed program in any mode
- WHEN it is dispatched
- THEN a prompt opens and nothing starts before `y`

#### Scenario: A server's level is the only automatic approval [S9, auto]
- GIVEN a user server at level `all`, a chat in `ask` mode, and the same chat's calls to `run_command`, `git_commit`, `write_file` staging and a cloud `read_file`
- WHEN the model calls an `all` tool and then each of those
- THEN the `all` tool runs with no prompt, and each of the others behaves exactly as without any level (prompt or batch review, nothing executed without `y`)

#### Scenario: Default level asks [S9, auto]
- GIVEN a user server with no `approval` key
- WHEN the model calls any of its tools
- THEN an approval opens and the server is not contacted before `y`

#### Scenario: A blocked conversation asks whatever the level [S9, auto]
- GIVEN a conversation that read GitHub text and user servers at level `all` and at level `reads` (with a `read`-class tool)
- WHEN the model calls a tool of each
- THEN each opens an approval showing the block reason and nothing runs before `y`

#### Scenario: Unknown tool is a write tool [S9, auto]
- GIVEN a user server tool the catalog does not list, annotated `readOnlyHint: true`, at level `reads`
- WHEN the model calls it
- THEN it is treated as a write tool and asks `y`

### Requirement: Chat approval mode
Every chat pane MUST have an approval mode, `ask`, `auto` or `run`, for tool calls made by the model. `ask` is the default and is the behaviour of every other requirement here. A chat pane MUST take the mode of `[ai.tools] approval` (see configuration), as it is at that moment, when the pane opens and whenever the user switches to or starts another conversation in that pane; the toggle (see modal-input) MUST cycle the mode `ask`, `auto`, `run`, `ask` of the focused chat pane only, for its current conversation, and the mode MUST NOT be stored or sent to any engine. Only the user can change the mode: no tool, tool result, MCP result, pane output or model text can switch it. Entering `run` needs no extra confirmation. In `auto` a `run_in_pane` call MUST be typed into the default target pane (the last focused shell pane of the chat's tab, no cycling) without a prompt, and MUST NEVER be followed by Enter, `\r` or a newline outside a bracketed paste. In `run` a call whose command is runnable under pane-execution MUST be typed and followed by Enter in one write; every other command MUST open the normal prompt, where `y` runs the model's command like a pane-execution run (one write ending in Enter, the same settle, capture and report, not counted toward the 5-command limit) unless it is multi-line, the pane's directory cannot be read, the pane is executing, line pending or dirty, or a non-ASCII command meets a non-UTF-8 locale, or the write step refuses it; then `y` types it without Enter and the chat's outcome line says why (`not run: <reason>`). `auto` and `run` MUST NOT cover any other ask: `read_file` on a cloud engine and every user MCP tool still need `y` (the mode never answers a user tool; only the approval level of its own server can let a call run unasked, see mcp-servers, Approval level per server, and it does so in every mode alike, except in a conversation that read GitHub text, where every user tool asks), `find_dir` and `list_dir` stay free, and the run action of pane-commands still opens its confirm. Before typing, the same checks as accepting with `y` MUST run (validation, the pane exists, it is a shell pane of the chat's tab, it is not on the alternate screen, a multi-line or tab command needs bracketed paste), and the target pane's foreground process MUST be its shell; when that cannot be known the call is refused in `auto` and takes the prompt in `run`. A failed check MUST return `denied` with a short reason and draw the result row; nothing is written. After an `auto` command is typed, and after a command is typed with `y`, its target pane MUST be marked line pending (only the `auto` and `run` routing reads the mark). The mark MUST clear only when the user sends Enter, Ctrl+C or Ctrl+U to that pane through lazarobox's input, or pastes text holding `\r` or `\n` into it while that pane has bracketed paste off (inside a bracketed paste a newline executes nothing and clears nothing), when that pane closes, or when the typed call ends `denied`; the screen is never read, and the mark belongs to one pane only. The call MUST instead open the normal prompt, as in `ask` (never denied for these reasons), when its target pane is marked line pending, dirty or executing (see pane-execution), when 5 `auto` or `run` commands (one shared named constant) were already typed or run in this turn, when the target pane has the focus, or when the chat's tab is not the shown tab or is zoomed onto another pane. The pane of an update (`cargo install`) MUST NOT be a target. A call that arrives while an approval is pending MUST be denied (`another approval is pending`), and switching mode MUST NOT answer a pending approval. The call row MUST show the exact command, and the result row MUST say what happened: `typed into pane N — no Enter`, or, for a run, the exact command that ran and its status (see pane-execution). A call whose turn ended or was cancelled before the write MUST get no bytes written (the answer is claimed before the write). The registry, tool descriptions and requests MUST be the same in all three modes. While `auto` or `run` is on and tools are on, the chat pane MUST show a marker in high contrast in every chat view, focused or not: ` AUTO ` in amber and ` RUN ` in red (see chat-pane); `ask` shows no marker. Only `run` may execute a command: one that passes pane-execution, or a model's command the user approved with `y`; never by any key or result of the model.
(Previously: "`read_file` on a cloud engine and every user MCP tool still need `y`" with no mention of a server's level; S9 adds the scoped exception, C13.)

#### Scenario: Default is ask [auto]
- GIVEN a file with no `approval` under `[ai.tools]`
- WHEN a chat pane opens and the model calls `run_in_pane`
- THEN the chat is in `ask` mode, a confirm opens and nothing is written until `y`

#### Scenario: Auto types without y and without Enter [auto]
- GIVEN a chat in `auto` mode, shell pane 2 idle at its prompt, not marked line pending, focused elsewhere, in the shown tab
- WHEN the model calls `run_in_pane` with `cd ~/dev/curso-rust`
- THEN no confirm opens, exactly one write reaches pane 2 with the command bytes and no `\r` or trailing `\n`, and the result is `typed into pane N — no Enter`

#### Scenario: Run executes a safe command [auto]
- GIVEN a chat in `run` mode and the same pane state
- WHEN the model calls `run_in_pane` with `cd ~/dev/curso-rust`
- THEN no confirm opens and exactly one write reaches pane 2: the command bytes then `\r`

#### Scenario: Run prompts for an off-list command [auto]
- GIVEN a chat in `run` mode
- WHEN the model calls `run_in_pane` with `git status`, and `y` is pressed
- THEN a confirm opens first, and the write after `y` has no `\r`

#### Scenario: Auto never covers reads or external tools [auto]
- GIVEN a chat in `auto` mode, and in another run `run` mode, on a cloud engine with a user MCP server at the default level
- WHEN the model calls `read_file` and then an external tool
- THEN each opens an approval and nothing runs until `y`; `find_dir` and `list_dir` still run free

#### Scenario: The mode never answers a user tool, the server's level may [S9, auto]
- GIVEN chats in `ask`, `auto` and `run` mode with a user server at level `reads`, a `read`-class tool and a `write`-class tool
- WHEN the model calls each in every chat
- THEN in every mode the read-class tool runs with no prompt and the write-class tool opens an approval; the same server at level `ask` prompts for both in every mode

#### Scenario: User run action unchanged [auto]
- GIVEN a chat in `auto` mode, and in another run `run` mode
- WHEN the run action of pane-commands is used on a bash block
- THEN the confirm opens as in `ask` and `y` writes no Enter

### Requirement: Toolsets by session role [S1, W2, W6, W8, W11]
The tool registry MUST offer by role: chat session = chat tools as before; orchestrator session = chat read tools plus `list_panes` and `read_pane` with the same asks as in a chat [W11], plus `spawn_agent`, `wait`, `collect`, `cancel` ([S4] and `finish`), plus the tools of the user's MCP servers, each call decided exactly as in a chat: it asks `y` unless the approval level of its own server lets it run, and never in a GitHub-blocked conversation (see mcp-servers, Approval level per server; user decisions 2026-10-29 and, for the level, 2026-11-14), and never `run_in_pane` and never write tools or `run_command` (the coordinator keeps Read plus Orchestra whatever the toggles say); worker session = read tools for every kind, plus the coding-tools write tools for a `code` or `simple` worker, plus `run_command` for a `code` or `simple` worker ([W6]), never `run_in_pane`, never user server tools, never orchestration tools. [W2] The Write and Exec toggles are a per-role ceiling: a write tool is offered to a worker only when Write is on, `run_command` only when Exec is on, and a toggle never adds a tool the role does not have (a `review` worker stays read-only with both on). Orchestration tools MUST NOT be reachable from user MCP servers, from a chat pane whose ORCH mode is off, or while the global orchestrator gate is off. The orchestrator session exists only for a turn of an orchestrating pane.
(Previously: "...plus the tools of the user's MCP servers, each call asking `y` exactly as in a chat (user decision, 2026-10-29)"; and, before that, "worker session = read tools only (until write tools exist)"; the menu did not limit workers; no worker could be offered `run_command`)

#### Scenario: The coordinator reads panes but never types in them [W11, auto]
- GIVEN an orchestrator turn, a `code` worker and a `review` worker
- WHEN each tool list is built and the orchestrator calls `run_in_pane`
- THEN the orchestrator holds `list_panes` and `read_pane` and not `run_in_pane`, its `run_in_pane` call is refused unasked, and neither worker holds any of the three

#### Scenario: Role toolsets [auto]
- GIVEN each role
- WHEN the tool list is built
- THEN it matches the role definition exactly

#### Scenario: Orchestrator user server tool asks [auto]
- GIVEN an orchestrator turn and a user MCP server with tool `search` at the default level
- WHEN the tool list is built and `search` is called
- THEN `search` is listed beside the orchestration tools, the call asks `y`, and the server is contacted only after `y`; a worker's call to it is refused unasked

#### Scenario: Orchestrator user server tool follows the level [S9, auto]
- GIVEN an orchestrator turn and a user MCP server at level `all`, and in another run at level `reads` with `search` of class `write`
- WHEN `search` is called
- THEN at `all` it runs with no prompt, at `reads` it asks `y`; in both a worker's call to it is refused unasked and the coordinator still holds no write tool and no `run_command`

#### Scenario: Ceiling limits a writer [W2, auto]
- GIVEN Write off and Exec off
- WHEN the tool lists of a `code` worker, a `simple` worker and a `review` worker are built
- THEN each holds the read tools only

#### Scenario: Ceiling never widens [W2, auto]
- GIVEN Write and Exec on
- WHEN the tool lists of a `review` worker and of the orchestrator are built
- THEN the `review` worker holds the read tools only and the orchestrator holds no write tool and no `run_command`
