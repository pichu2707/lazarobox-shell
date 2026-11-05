# Command Runner Specification (delta: WSL drive access)

## MODIFIED Requirements

### Requirement: Refusals before start
`run_command` MUST be refused when any of these holds: no jail is active (Landlock ABI below 3, or a non-Linux system); the chat origin fails the root policy (when the origin is `$HOME`, a parent of `$HOME` or `/`, the text is the fixed `path not available: the chat's folder is your home, a folder above it or /, where commands cannot run; ask the user to run /cd <project folder>`, the same for every command and git verb and never naming the folder or a path); under WSL, the chat origin is a Windows drive folder without the user's grant (the folder-level drive text of code-write, Root policy, the same for every command and git verb), or the jail cannot confine a granted drive folder on this system (`commands cannot run in a Windows drive folder on this system`); `cwd` resolves outside the root; the chat holds unproposed staged changes (message `propose them first`); a run is already active in this chat; or two runs are already active across the whole app. A refusal MUST happen before any prompt. Tests always run against the disk, never against staged content.

#### Scenario: No jail [auto]
- GIVEN a system where the jail probe fails
- WHEN `run_command` is called
- THEN it is refused and no prompt opens

#### Scenario: Unproposed staged changes [auto]
- GIVEN a staged batch that was not proposed
- WHEN `run_command` is called
- THEN it is refused with `propose them first`

#### Scenario: One run per chat [auto]
- GIVEN a run active in chat A
- WHEN chat A calls `run_command` again
- THEN it is refused, and chat B is not affected

#### Scenario: Global cap [auto]
- GIVEN two runs active in two chats or workers
- WHEN a third `run_command` is called
- THEN it is refused

#### Scenario: Cwd outside the root [auto]
- GIVEN `cwd` `..` and `cwd` `/tmp`
- WHEN each is validated
- THEN each is refused with the refusal text `path not available`

### Requirement: Jail profile
The child MUST run inside the jail through the existing helper. The profile MUST grant: read and execute on the toolchain directories and system directories; read, write and execute on the root; read and write on a private temp directory and on the package caches; and nothing on `~/.ssh`, `~/.gnupg`, `~/.aws` or other credential locations. The write-and-execute class for the root MUST NOT be reachable from the CLI jail or from any other profile. Outbound network MUST stay open (a visible notice covers it). The child MUST NOT be able to read a credential directory even when the project asks for it. When the root is on a Windows drive, the jail's broad-root check MUST also refuse, ASCII case-insensitively, a drive root, `<drive>/Users`, any `<drive>/Users/<name>` and any folder the WSL drive read policy refuses, so none of them can become the read, write and execute root.

#### Scenario: Secrets unreachable [auto, tests/jail.rs]
- GIVEN a jailed child
- WHEN it tries to open `~/.ssh/id_ed25519` and `~/.aws/credentials`
- THEN both fail

#### Scenario: Root is writable and executable [auto, tests/jail.rs]
- GIVEN a jailed child
- WHEN it writes a script into `target/` in the root and executes it
- THEN both succeed

#### Scenario: Outside the root [auto, tests/jail.rs]
- GIVEN a jailed child
- WHEN it writes to `$HOME/x` outside the root
- THEN the write fails

#### Scenario: CLI jail unchanged [auto]
- GIVEN the CLI jail spec before and after this change
- WHEN both are built
- THEN they are identical

#### Scenario: Broad drive roots [auto]
- GIVEN WSL with the drive `/mnt/c`
- WHEN the jail profile is built with `/mnt/c`, `/mnt/c/Users`, `/mnt/c/Users/ana`, `/mnt/c/Windows` and `/mnt/c/windows` as root
- THEN each is refused as a root
