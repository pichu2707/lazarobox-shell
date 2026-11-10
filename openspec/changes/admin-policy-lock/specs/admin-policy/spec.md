# Admin Policy Specification

## Purpose

An optional system policy file, installed by an administrator, that bounds the settings the user may choose: a locked setting can be made stricter by the user, never looser, from any surface. Without the file lazarobox behaves exactly as without this capability. It is a guardrail against mistakes, the menu, copied files, repository files and a model's tool calls; it is not a security boundary against a user with root or who runs another binary. Goal #659. Slice tags: [A1] file, trust, `local_only`, notices, locked-row mechanism; [A2] tools and grants; [A3] updates and orchestrator; [A4] engine allow-list; [A5] user MCP servers; [A6] `--show-policy`. Scenario tags: [auto] = `cargo test` (pure parse, temporary folders, `TestBackend`); [manual] = a real machine with root. Items that depend on an open question of the proposal name it (Q1-Q7) and follow its recommendation.

Terms: "the policy" = the parsed policy file. "Path" = the fixed policy path of the platform. "Bound" = what a policy key allows (design D3). "Strict baseline" = the "Strictest value" column of design D3. "Tighten" = move a setting to a stricter value; "loosen" = the opposite. "Lockable key" = a key of design D3 for a slice that has shipped.

## Requirements

### Requirement: The policy is optional [A1]
lazarobox MUST look for the policy at start, once, at the path of the platform. When the path does not exist (`ENOENT` or `ENOTDIR`), there MUST be no policy, no notice, and every behaviour MUST be identical to a build without this capability.

#### Scenario: No file [auto]
- GIVEN a temporary folder with no policy file, passed as the policy path
- WHEN lazarobox starts with any `config.toml`
- THEN no notice mentions a policy, and every setting and menu row is as without this capability

### Requirement: Fixed location [A1]
The path MUST be `/etc/lazarobox/policy.toml` on Linux, Linux under WSL included, and `/Library/Application Support/lazarobox/policy.toml` on macOS (Q1 b). It MUST NOT be derived from any environment variable, command-line flag, `config.toml` value or project file, and lazarobox MUST NOT search other folders for it. It MUST be read even when there is no user configuration file (no usable `HOME`).

#### Scenario: Constant per OS [auto]
- GIVEN a Linux build, and a macOS build
- WHEN the policy path constant is read (no file is opened)
- THEN it is `/etc/lazarobox/policy.toml`, and `/Library/Application Support/lazarobox/policy.toml`

#### Scenario: The environment changes nothing [auto]
- GIVEN `XDG_CONFIG_HOME`, `HOME` and every `LAZAROBOX_*` variable set to folders that hold a `policy.toml`
- WHEN the policy path is resolved
- THEN it is the constant of the platform

### Requirement: Trust check [A1]
The file MUST be opened without blocking (a FIFO never waits) and judged on the open handle: a regular file, at most 64 KiB, owned by uid 0, with no write bit for group or others. Each folder of its canonical path MUST be owned by uid 0 and have no write bit for group or others, unless it has the sticky bit and the next component is owned by uid 0. A file that fails only on owner or mode MUST be applied as written (every key can only tighten), its source MUST be shown as `policy (untrusted)`, and the start-up notice MUST be `policy: {at} can be changed by users other than root; the policy is applied as written (fix: chown root {at} && chmod go-w {at})`, where `{at}` is the first failing component.

#### Scenario: Trusted [auto]
- GIVEN a policy file and its folders owned by the trusted uid given to the reader (the test's own uid), mode `644` and `755`
- WHEN it is read
- THEN it is trusted and no trust notice is shown

#### Scenario: Group-writable folder [auto]
- GIVEN the same file in a folder of mode `775`
- WHEN it is read
- THEN the policy is applied, its source is `policy (untrusted)` and the notice names that folder

#### Scenario: Not a regular file [auto]
- GIVEN a FIFO at the path, then a 65 KiB file
- WHEN each is read
- THEN the start does not block, and the result is the strict baseline with the notice `policy: {path}: not a regular file (...)`, then `policy: {path}: larger than 64 KiB (...)`

### Requirement: Format [A1]
The policy MUST use the tables and key names of `config.toml` and MUST accept only the lockable keys (design D3). Each key MUST be a bound: a value that would loosen (`local_only = false`, a tool flag `true`, `check = true`, an `approval` list without `ask`) MUST lock nothing beyond its strict part and MUST produce `policy: {key} = {value} locks nothing (a policy can only make settings stricter)`. `approval` MUST accept a string or an array and MUST always allow `ask`.

#### Scenario: Local only [auto]
- GIVEN the policy `[ai]\nlocal_only = true`
- WHEN it is parsed
- THEN local-only mode is bounded on and there is no problem

#### Scenario: A value that loosens [auto]
- GIVEN the policy `[ai]\nlocal_only = false`
- WHEN it is parsed
- THEN nothing is bounded and the problem is `policy: ai.local_only = false locks nothing (a policy can only make settings stricter)`

#### Scenario: Approval as a string [auto]
- GIVEN the policy `[ai.tools]\napproval = "auto"`
- WHEN it is parsed
- THEN the allowed modes are `ask` and `auto`, with the "locks nothing" problem for the missing `ask`

### Requirement: Precedence [A1]
For every lockable key the effective value MUST be the strictest of the policy bound, the user's `config.toml` and the project sources (see project-engine-policy, #658): `local_only` is on when any of them says so; a tool flag, `updates.check`, `orchestrator.enabled` and `mcp.user_servers` are off when any of them says so; the approval mode is the user's when the policy allows it, else `ask`; an engine not in `[ai] engines` is refused. A lower source MUST be able to tighten and MUST NOT be able to loosen. Where the UI names a source it MUST name the strongest: the policy, then the user, then the project.

#### Scenario: The user cannot loosen [auto]
- GIVEN the policy `local_only = true` and the user's `[ai] local_only = false`
- WHEN the config is read
- THEN local-only mode is on, its source is the policy, and the user's file is unchanged

#### Scenario: The user can tighten [auto]
- GIVEN the policy `[ai.tools] approval = ["ask", "run"]` and the user's `local_only = true`, `approval = "ask"`
- WHEN the config is read
- THEN local-only mode is on (source: user), and the approval mode is `ask`

#### Scenario: A disallowed mode falls back to ask [auto]
- GIVEN the policy `approval = ["ask", "auto"]` and the user's `approval = "run"`
- WHEN the config is read
- THEN the approval mode is `ask`

### Requirement: A user value the policy overrides [A1]
When the user's `config.toml` sets a lockable key to a value the policy does not allow, the start-up problem `config: {key} = {value} is overridden by the policy in {path}` MUST be added after the policy's own problems. The user's file MUST NOT be rewritten, and a menu save MUST NOT write that key unless the user changed it to an allowed value.

#### Scenario: Notice and no rewrite [auto]
- GIVEN the policy `local_only = true` and the user's `local_only = false`
- WHEN lazarobox starts and the menu is opened, another row is changed and `s` is pressed
- THEN the start-up notice includes `config: ai.local_only = false is overridden by the policy in /etc/lazarobox/policy.toml`, and the saved file still holds `local_only = false`

### Requirement: A file that cannot be used fails closed [A1]
A file that exists but cannot be read, is not a regular file, is larger than 64 KiB, is not UTF-8, is not TOML or holds an unknown key MUST give the strict baseline and the notice `policy: {path}: {problem} (every lockable setting is at its strictest until this is fixed)` (Q3 a, Q6 a). A known key with a wrong value MUST put that key at its strictest, keep the other keys as written, and give `policy: {key}: {expected} ({key} is at its strictest until this is fixed)`. The policy problems MUST come before the user file's problems in the start-up notice.

#### Scenario: Not TOML [auto]
- GIVEN a policy file with `[ai` on its first line
- WHEN lazarobox starts
- THEN local-only mode is on, tools are off, the approval mode is `ask`, no update check runs, the orchestrator gate is off, no user MCP server starts, and the notice starts with `policy: `

#### Scenario: Unknown key [auto]
- GIVEN the policy `[ai]\nlocal_only = true\ncloud_sync = false`
- WHEN it is parsed
- THEN the result is the strict baseline and the problem names `unknown key: ai.cloud_sync`

#### Scenario: Wrong value of one key [auto]
- GIVEN the policy `[ai]\nlocal_only = "yes"` and `[updates]\ncheck = true`
- WHEN it is parsed
- THEN local-only mode is bounded on, the update check is not bounded (with the "locks nothing" problem), and the problem `policy: ai.local_only: expected true or false (ai.local_only is at its strictest until this is fixed)` is listed

#### Scenario: Unreadable [auto]
- GIVEN a policy file of mode `000` (the test is skipped when it runs as root)
- WHEN lazarobox starts
- THEN the result is the strict baseline with `policy: {path}: cannot be read (...)`

### Requirement: Runtime grants are refused [A2]
While the policy bounds a tool family off, its chat grant MUST be refused before any other reason, with `/write refused: Write is locked off by policy ({path})`, `/github refused: the GitHub tools are locked off by policy ({path})` and, once `/exec` exists, `/exec refused: Exec is locked off by policy ({path})`; while it bounds `[ai.tools] enabled` off, each of them MUST be refused with `{command} refused: tools are locked off by policy ({path})`. No dialog MUST open, the composer MUST be cleared as for any refusal, and nothing MUST be granted. A family bounded off MUST NOT be offered to any engine or worker. While Write is bounded off, the WSL drive grant MUST NOT be offered.

#### Scenario: /write [auto]
- GIVEN the policy `[ai.tools]\nwrite = false`, tools on, a chat with a folder on an engine that offers tools
- WHEN `/write` is sent
- THEN no dialog opens, the transcript shows `/write refused: Write is locked off by policy (/etc/lazarobox/policy.toml)` and the next turn offers no write tool

#### Scenario: /github with tools locked off [auto]
- GIVEN the policy `[ai.tools]\nenabled = false`
- WHEN `/github` is sent
- THEN the transcript shows `/github refused: tools are locked off by policy (/etc/lazarobox/policy.toml)`

#### Scenario: Drive grant [auto]
- GIVEN the policy `write = false` under WSL and a chat moved to a `/mnt/c` folder
- WHEN the folder probe answers
- THEN no drive grant prompt opens

### Requirement: Approval modes [A2]
`Prefix a` MUST cycle only through the modes the policy allows, in the order `ask`, `auto`, `run`. With only `ask` allowed it MUST NOT change the mode and MUST show the notice `approval is locked to ask by policy ({path})`.

#### Scenario: Two modes [auto]
- GIVEN the policy `approval = ["ask", "auto"]` and a chat in `ask`
- WHEN `Prefix a` is pressed three times
- THEN the modes are `auto`, `ask`, `auto`

#### Scenario: One mode [auto]
- GIVEN the policy `approval = "ask"` (read as `["ask"]`)
- WHEN `Prefix a` is pressed
- THEN the mode stays `ask` and the notice is `approval is locked to ask by policy (/etc/lazarobox/policy.toml)`

### Requirement: Updates and orchestrator [A3]
`[updates] check = false` in the policy MUST stop the start-up check, the llama-server newer-build check and `r` on the `Updates` row, whose value MUST end with ` · checks off: policy` and whose `r` MUST show `update checks are off by policy ({path})`. Enter on the row MUST still install an update already known (a user action). `[ai.orchestrator] enabled = false` MUST keep the ORCH gate off: the `Orchestrator` row is locked and Ctrl+O behaves as with the gate off.

#### Scenario: No check [auto]
- GIVEN the policy `[updates]\ncheck = false` and the user's `check = true`
- WHEN lazarobox starts and `r` is pressed on `Updates`
- THEN no check is started and the footer shows `update checks are off by policy (/etc/lazarobox/policy.toml)`

### Requirement: Engine allow-list [A4]
With `[ai] engines` in the policy, the availability of any task whose effective engine is not in the list MUST be refused with the guidance `{engine}: not allowed by policy ({path}): pick another engine in the menu (Ctrl+Space m)`, checked where local-only mode is checked (see local-only-mode). It MUST NOT select another engine. The picker MUST NOT list the other engines and MUST end with the disabled note row `engines hidden: not allowed by policy` when it hid one; the task row of a refused assignment MUST carry ` · off: policy`. An empty list refuses every engine.

#### Scenario: Refused [auto]
- GIVEN the policy `engines = ["ollama"]` and `chat` `lmstudio` / `qwen`
- WHEN a message is sent
- THEN nothing is sent, the message stays in the composer and the guidance is `lmstudio: not allowed by policy (/etc/lazarobox/policy.toml): pick another engine in the menu (Ctrl+Space m)`

### Requirement: User MCP servers [A5]
`[ai.mcp] user_servers = false` in the policy MUST keep every entry of the user's `[[ai.mcp.servers]]` from starting, in every chat set, and MUST show `MCP servers not started: user servers are off by policy ({path})` once per chat set that had at least one enabled entry.

#### Scenario: Not started [auto]
- GIVEN the policy `user_servers = false` and two enabled user entries
- WHEN a turn with tools on starts the chat set
- THEN no server process is spawned and the notice is shown once

### Requirement: No bypass through the environment or flags [A1]
No environment variable, command-line flag or file other than the policy MUST change a lockable key beyond its bound. A future variable or flag that sets a lockable key MUST be bounded like the user's file.

#### Scenario: Another user file [auto]
- GIVEN the policy `local_only = true` and `XDG_CONFIG_HOME` pointing to a folder whose `config.toml` says `local_only = false`
- WHEN lazarobox starts
- THEN local-only mode is on

### Requirement: Testable without root [A1]
The reader MUST take the path and the trusted uid as parameters; only the start of the real program MUST pass the platform path and uid 0. Tests MUST NOT open the real policy path, `/etc`, `/Library` or the user's `~/.config/lazarobox/`.

#### Scenario: Production entry in tests [auto]
- GIVEN a test build
- WHEN the production policy entry is called
- THEN it returns "no policy" without opening any file

### Requirement: Show the policy [A6]
`lazarobox --show-policy` MUST take no argument, MUST print the path, whether the file was found, its trust verdict, every problem and the effective bound of each lockable key, one per line, and MUST exit with status 0 without starting the terminal UI. A missing file MUST print `no policy at {path}`.

#### Scenario: No file [auto]
- GIVEN no policy file at the path given to the command's pure core
- WHEN `--show-policy` runs
- THEN the output is `no policy at {path}` and the status is 0

#### Scenario: A real machine [manual]
- GIVEN `/etc/lazarobox/policy.toml` installed by root with `[ai]\nlocal_only = true`
- WHEN a non-root user runs `lazarobox --show-policy`, then starts lazarobox and opens the menu
- THEN the output names the file as trusted and `ai.local_only: on`; the Models tab shows `Local only` `on · locked by policy` and the status bar starts with `local only (policy) · `
