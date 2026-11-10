# Delta for Agent Tools

## Base of this delta

This delta targets the owning spec `openspec/specs/agent-tools/spec.md` (the `agent-tools` and `coding-tools` changes were archived on 2026-11-02). The MODIFIED block below is a full copy of its requirement "Path policy" as it stands on f7d6b6f4 (openspec/specs/agent-tools/spec.md:115-211, including the WSL drive read policy wording added by wsl-drive-access): the only text change is in the list of excluded `$HOME` paths (three token locations added, and the note on `$HOME/.config/lazarobox` now names `secrets.toml` instead of "its `config.toml` holds MCP env tokens"), plus one added scenario at the end. All other requirements stay as they are, including Approval model ("Tools from a user MCP server MUST always be write tools", openspec/specs/agent-tools/spec.md:228; C11 keeps it). In code the list is `SECRET_DIRS` (src/core/ai/tools/paths.rs:24-34); it also feeds the write check and the git tools (paths.rs:346; src/core/ai/tools/git.rs:207, 240), so the added paths are refused by them as well.

Traceability: C6 (secrets refused by the read tools), C8 (read-deny additions in the first slice), proposal scope item 2.

## MODIFIED Requirements

### Requirement: Path policy
Tools MUST only return or open paths inside `$HOME` (absolute, or `~`-relative as input) or, under WSL, inside a Windows drive folder that the WSL drive read policy allows (see that requirement). A path MUST be refused when, after resolving `.`, `..` and every symlink to its real location, it is outside `$HOME` and not allowed by the WSL drive read policy, is `$HOME/.ssh` or `$HOME/.gnupg`, is or lies under any entry of `CREDENTIAL_DIRS` directly under `$HOME` (`.claude`, `.gemini`, `.codex`, `.aws`, `.kube`, `.docker`, `.password-store`), or is `$HOME/.config/gh`, `$HOME/.config/gcloud`, `$HOME/.local/share/keyrings`, `$HOME/.local/share/fish/fish_history`, `$HOME/.google_workspace_mcp` (workspace-mcp tokens), `$HOME/.clasprc.json` (clasp tokens), `$HOME/.config/mcp-gsc` (mcp-gsc tokens), `$HOME/.config/lazarobox` (it holds `config.toml` and `secrets.toml`, the values of the MCP servers; see mcp-secrets) or `$HOME/.local/share/lazarobox` (the app's own store), both at their default XDG place (`XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `GH_CONFIG_DIR` overrides are not followed), or is a file named `.env`, `.env.*`, `.envrc`, `.netrc`, `.git-credentials`, `.pgpass`, `.npmrc`, `.bash_history`, `.zsh_history`, `.zhistory`, `.python_history`, `.node_repl_history`, `.psql_history`, `.mysql_history` or `.sqlite_history` at any depth. The check MUST be made on both the requested path (`~` expanded, a relative input resolved against the origin cwd, `.` and `..` folded by text) and the real path that is opened, and the path is allowed only if both pass. So a symlink inside `$HOME` that points to an excluded or outside path is refused, and so is an excluded name that is itself a symlink to an ordinary place (for example `~/.ssh` linked to `~/dotfiles/ssh`). The real path of every existing secret root (`~/.ssh`, `~/.gnupg`, each `CREDENTIAL_DIRS` entry and each excluded file name directly in `$HOME`) MUST also be resolved, and a real path equal to or under one of those real roots MUST be refused, so the real target of an aliased secret folder (`~/dotfiles/ssh`) and any second link to it are excluded as well. Known limit: a link below `$HOME` depth one named like ANY excluded file (`.env`, `.netrc`, `.npmrc`, a history file...) whose target has an ordinary name is judged by its requested name only, and hard links are not detected here (slice 2b checks `st_nlink`). The resolved roots are a snapshot taken at load: a root created after the load is out of scope for that call, and slice 2b MUST re-check the real path of the opened file descriptor against them. The runtime MUST also pass lazarobox's real config and data folders (honouring `XDG_CONFIG_HOME` and `XDG_DATA_HOME`) as extra roots, resolved with the same fail-closed rules. A `$HOME` whose canonical path is `/`, `/home`, `/Users`, `/usr`, `/var`, `/etc`, `/opt`, `/tmp`, `/bin`, `/lib`, `/proc`, `/sys` or `/dev`, `/run`, `/mnt`, `/media`, `/srv`, `/sbin`, `/boot`, `/lib64`, `/private`, `/private/tmp`, `/private/var`, `/private/etc`, `/System` or `/Library`, compared ASCII case-insensitively per component, MUST be refused (`/root` is allowed). If `$HOME` or any secret root that exists cannot be read (an error other than not-found or not-a-directory, which are skipped as an absent root; for example a permission error or a symlink loop), every call MUST be refused (fail closed). The secret roots MUST be resolved once per tool call, not once per path or listed entry. The deny list is best effort, not complete, and `.envrc` and `.npmrc` are denied at any depth on purpose. Comparison of the excluded names is ASCII case-insensitive. `~user` input, a relative or `/` home and a relative origin MUST be refused. A relative input path is resolved against the chat's origin cwd and then checked like any other. Excluded entries MUST be omitted from `list_dir` and `find_dir` results, and `find_dir` MUST NOT descend into excluded directories. A refusal MUST use one message for excluded, outside and non-existent paths, refusals of the WSL drive read policy included, so the existence of an excluded path is not revealed.

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
