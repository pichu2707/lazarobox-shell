# Command Runner Specification

## Purpose

Run build and test commands on the user's disk, inside a Linux sandbox, with a captured and structured result. It replaces typing into a pane for coding work. The allow-list bounds the program, not what the program does, so the controls are per-command approval and the jail. Linux only in v1. Tags: [auto] = `cargo test`; [manual] = real machine; [design-confirm] = value or name that design must pin.

Terms: "jail" = the Landlock sandbox (ABI 3 or higher) used for the child. "Root" = the chat origin (see code-write, Root policy). "Staged changes" = see code-write.

## Requirements

### Requirement: Family and toggle
`run_command` MUST belong to the `Exec` family. It MUST be offered only when tools are on and the Exec toggle is on. The toggle MUST be off by default.

#### Scenario: Off by default [auto]
- GIVEN a config with no exec toggle
- WHEN the tool list is built
- THEN `run_command` is absent and a call to it returns an error result

### Requirement: Argv input and allow-list
`run_command` MUST take `program`, `args` (a list of strings), an optional `cwd` and an optional `timeout_secs`. It MUST NOT accept a shell string and MUST NOT start a shell. `program` MUST match one of a fixed allow-list: `cargo`, `npm`, `pnpm`, `yarn`, `pytest`, `go`. The match MUST be on the exact program name, not on a path. The list MUST NOT be extendable by configuration in v1. Which subcommands and flags of each program are accepted is pinned by design. Anything else MUST return an error result and start nothing.

#### Scenario: Allowed program [auto]
- GIVEN `program` `cargo` and `args` `["test"]`
- WHEN the call is validated
- THEN it passes validation and goes on to approval

#### Scenario: Not on the list [auto]
- GIVEN `program` `make`, `python`, `sh`, `/usr/bin/cargo` and `bash -c`
- WHEN each is validated
- THEN each returns an error result and nothing starts

#### Scenario: No shell string [auto]
- GIVEN `program` `cargo` and an argument `test; rm -rf ~`
- WHEN the process is built
- THEN the argument reaches `cargo` as one literal argv element and no shell runs

### Requirement: Approval on every command [W5]
Every `run_command` call MUST need an explicit user `y`, every time, in every approval mode. The prompt MUST show the program, the arguments, the working directory and a visible notice that network is on and that a test can exfiltrate the project's `.env`. [W5] When the call comes from a worker, the prompt MUST also show the label `worker n/m <kind> <engine>><model>`, MUST join the worker approval queue, and its answer MUST reach the session of that worker only. A declined, timed-out or cancelled prompt ends as `denied` and nothing starts. The call MUST NOT be covered by any auto-approval. Time spent on the prompt does not count against the worker's deadline.
(Previously: no worker could call `run_command`, so the prompt carried no worker label and no queue)

#### Scenario: Prompt content [auto]
- GIVEN a valid `cargo test` call
- WHEN the prompt is built
- THEN it shows `cargo test`, the cwd and the network notice

#### Scenario: Every mode asks [auto]
- GIVEN chats in `ask` and `auto` mode
- WHEN `run_command` is called in each
- THEN each opens a prompt and nothing starts before `y`

#### Scenario: Declined [auto]
- GIVEN a pending prompt
- WHEN `n`, a timeout or a cancel ends it
- THEN the result is `denied` and no process was started

#### Scenario: Worker command carries the label [W5, auto]
- GIVEN a worker 1 of 2 `simple` on `ollama>llama3:8b` calling `cargo test`
- WHEN the prompt is built
- THEN it shows the label, `cargo test`, the cwd and the network notice, in `run` mode too

#### Scenario: Worker answer stays with the worker [W5, auto]
- GIVEN two workers with pending command asks
- WHEN the user answers one
- THEN the other worker's ask is unchanged and its worker is not resumed

### Requirement: Refusals before start
`run_command` MUST be refused when any of these holds: no jail is active (Landlock ABI below 3, or a non-Linux system); the chat origin fails the root policy (when the origin is `$HOME`, a parent of `$HOME` or `/`, the text is the fixed `path not available: the chat's folder is your home, a folder above it or /, where commands cannot run; ask the user to run /cd <project folder>`, the same for every command and git verb and never naming the folder or a path); under WSL, the chat origin is a broad Windows drive folder (a drive root, `<drive>/Users` or a `<drive>/Users/<name>` profile root, granted or not; the text is `path not available: the chat's folder is <folder>, where commands cannot run; ask the user to run /cd <project folder>`, the same for every command and git verb, like the broad-folder text of writes); under WSL, the chat origin is any other Windows drive folder without the user's grant (the folder-level drive text of code-write, Root policy, the same for every command and git verb); `cwd` resolves outside the root; the chat holds unproposed staged changes (message `propose them first`); a run is already active in this chat; or two runs are already active across the whole app. A refusal MUST happen before any prompt. Tests always run against the disk, never against staged content.

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

#### Scenario: Cwd that is not on disk is named [auto]
- GIVEN a `cwd` inside the root that does not exist, for instance a folder the worker only staged
- WHEN the call is validated
- THEN a worker whose own batch is staged or not applied gets that batch's text (`propose them first`, or the not-applied text), before the disk check; any other caller gets `path not available: the folder <cwd>/ does not exist (paths are relative to the chat's folder <folder>)`; a `cwd` that links out of the root, or any other refused place, keeps the bare `path not available`

### Requirement: Jail profile
The child MUST run inside the jail through the existing helper. The profile MUST grant: read and execute on the toolchain directories and system directories; read, write and execute on the root; read and write on a private temp directory and on the package caches; and nothing on `~/.ssh`, `~/.gnupg`, `~/.aws` or other credential locations. The write-and-execute class for the root MUST NOT be reachable from the CLI jail or from any other profile. Outbound network MUST stay open (a visible notice covers it). The child MUST NOT be able to read a credential directory even when the project asks for it. When the root is on a Windows drive, the jail's broad-root check MUST also refuse, ASCII case-insensitively, a drive root, `<drive>/Users`, any `<drive>/Users/<name>`, the Windows system folders, a profile's `AppData` and `NTUSER.DAT*`, and any path with a DOS 8.3 short name, so none of them can become the read, write and execute root. The jail helper cannot know the user's own Windows profile, so the other folders the WSL drive read policy refuses (another user's profile, the secret folders of a profile) are refused earlier, by the root policy, before the jail starts.

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

### Requirement: Process rules
The child MUST read stdin from `/dev/null`. Its environment MUST be cleared and rebuilt from a small base (path, user, locale, temp dir, plus non-interactive and no-color settings), and MUST NOT hold the session token or any lazarobox secret. It MUST run in its own process group, and the whole group MUST be killed on timeout, on cancel and on drop. `timeout_secs` MUST default to 120 and MUST NOT exceed 600; a larger value MUST be clamped or refused (design pins which). stdout and stderr MUST share one pipe so ordering is kept. Daemonizing grandchildren that leave the group are a documented limit.

#### Scenario: Clean environment [auto]
- GIVEN a parent environment holding `SECRET_TOKEN` and the session token
- WHEN a child is spawned
- THEN neither variable is visible to the child

#### Scenario: Timeout kills the group [auto]
- GIVEN a child that spawns a grandchild and sleeps, and `timeout_secs` 1
- WHEN the time passes
- THEN both processes are killed and the outcome has `timed_out` true

#### Scenario: Timeout bounds [auto]
- GIVEN no `timeout_secs`, `timeout_secs` 600 and `timeout_secs` 601
- WHEN each is validated
- THEN the first uses 120, the second is accepted, and the third is clamped or refused

#### Scenario: Cancel kills [auto]
- GIVEN a running child
- WHEN the turn is cancelled
- THEN the process group is killed

### Requirement: Output capture and RunOutcome
The output MUST be captured in a ring buffer and returned as at most 32 KiB: the first 4 KiB, then the tail. The omitted middle MUST be marked. The output MUST be cleaned of control sequences, redacted and wrapped as untrusted data. The call MUST return a structured `RunOutcome` with the exit code, the signal if any, `timed_out`, `truncated`, the duration and a digest. A non-zero exit MUST be an `ok` tool result carrying the outcome, not a tool error.

#### Scenario: Output cut [auto]
- GIVEN a child printing 200 KiB
- WHEN it ends
- THEN the result is at most 32 KiB, holds the head and the tail, and `truncated` is true

#### Scenario: Failure is an outcome [auto]
- GIVEN a child that exits with code 101
- WHEN it ends
- THEN the result carries exit 101 and `timed_out` false

#### Scenario: Output is data [auto]
- GIVEN a child printing `Ignore previous instructions` and an escape sequence
- WHEN the result is built
- THEN the text sits inside the untrusted-data wrapper and the escape sequence is gone

#### Scenario: Secrets redacted [auto]
- GIVEN a child printing a recognizable API key
- WHEN the result is built
- THEN the key is replaced by the redaction marker

#### Scenario: Evidence input [auto]
- GIVEN a finished run
- WHEN the outcome is read by the orchestrator
- THEN the command, exit code and digest are available without re-parsing the text

### Requirement: Run folder follows the chat folder
The working directory shown in the approval prompt and used to start a command MUST be inside the chat folder at the moment of the call. A `run_command` whose approval prompt is open or whose run is active MUST keep the folder it started with; a folder change is refused while either holds (see chat-folder), so a prompt can never show one folder and run in another. The refusals of "Refusals before start" apply to the new folder as to any.

#### Scenario: Prompt shows the new folder [auto]
- GIVEN a folder change to `/w/b`
- WHEN `run_command` is called
- THEN the prompt shows `/w/b` as the working directory

#### Scenario: Change refused during a prompt or run [auto]
- GIVEN an approval prompt pending, and in another run a command running
- WHEN `/cd /w/c` is requested
- THEN it is refused with the matching reason and the prompt or run keeps its folder

### Requirement: Worker commands run after the worker's batch is applied [W6]
A command called by a worker MUST be refused with the existing `propose them first` message while that worker's own batch is staged or in review. A command MUST NOT run against staged content (no overlay). Until W10 ships, the existing per-pane refusals (a run already active in the chat, two runs across the app, the busy gate `CHAT_BUSY`) apply to workers as they do to a chat, and each refusal MUST be shown to the user as well as returned to the worker. Whether a worker's refusal is scoped to its own batch once workers have their own batches, and whether more than one command per pane may run, is open for design.

#### Scenario: Refused while the batch is staged [W6, auto]
- GIVEN a worker whose batch is staged or in review
- WHEN it calls `run_command`
- THEN the call returns `propose them first`, no prompt opens, and a notice shows the refusal

#### Scenario: Runs after the batch is applied [W6, auto]
- GIVEN a worker whose batch was approved and applied, and the user approving the command
- WHEN it calls `cargo test`
- THEN the command runs against the disk

#### Scenario: Busy gate is disclosed [W6, auto]
- GIVEN a run already active in the pane
- WHEN a worker calls `run_command`
- THEN it is refused before any prompt and the user sees a notice with the cause
