# Delta for chat-engines

Slice tags: S1 seam/probe/CI, S2 SBPL renderer, S3 macOS helper exec and tests, S4 enablement and docs. Tags [auto] = `cargo test`, [ci-macos] = macOS CI runners, [manual] = real Mac.

## ADDED Requirements

### Requirement: Seatbelt backend and probe (S1)
On macOS the jail backend MUST be Seatbelt, applied through the absolute path `/usr/bin/sandbox-exec`, never resolved through `PATH`. A startup probe, run off the render thread, MUST prove enforcement with a positive control (an allowed program runs and exits 0) and a negative control (a write to a denied directory fails). Any failed control, a missing `/usr/bin/sandbox-exec`, or a failure to spawn it MUST yield `Unavailable`. The helper path MUST come from `current_exe`, not `/proc/self/exe`.

#### Scenario: Probe proves enforcement [ci-macos]
- GIVEN a macOS 14+ runner (arm64 and Intel)
- WHEN the probe runs
- THEN the positive control succeeds, the negative control fails, and support is Available

#### Scenario: sandbox-exec missing [auto]
- GIVEN `/usr/bin/sandbox-exec` does not exist
- WHEN the probe runs
- THEN support is Unavailable and no CLI process is started

#### Scenario: Negative control succeeds wrongly [auto]
- GIVEN a probe whose denied write succeeds (profile not enforced)
- WHEN support is derived
- THEN it is Unavailable, never Available

#### Scenario: Older macOS with a passing probe [auto]
- GIVEN macOS older than 14 and a probe that passes
- WHEN support is derived
- THEN it is Available (no version block); the README lists such versions as unsupported (S4)

### Requirement: Seatbelt profile rendering (S2)
The profile MUST be rendered by a pure function from the one shared `JailSpec` into fixed SBPL text plus an ordered parameter list. It MUST start from `(deny default)`. Every path MUST enter only as `(param "Pn")` bound by `-D Pn=<path>`, never interpolated into the text. Rendering MUST be deterministic.

#### Scenario: Golden profile [auto]
- GIVEN the spec of a node CLI (nvm layout) and one of a vendor-binary CLI
- WHEN rendered
- THEN the output equals the checked-in golden text and parameter list

#### Scenario: Hostile path cannot alter the profile [auto]
- GIVEN rx, ro or rw paths containing `"`, `)`, `;` or a newline
- WHEN rendered
- THEN the profile text is byte-identical to the one for benign paths and the hostile value appears only in the parameter list

#### Scenario: Same policy data as Linux [auto]
- GIVEN any CLI layout
- WHEN the macOS spec is built
- THEN `$HOME`, `~/.ssh`, scratch dirs and `CREDENTIAL_DIRS` appear in no allow rule, and only the system-read set is added

### Requirement: Seatbelt access rules (S2 rendered, S3 enforced)
Under the profile: `process-exec` MUST be allowed only for the `rx` set (interpreter, CLI package, shim, vendor binary). Broad reads of system locations (`/usr/lib`, `/System/Library`, `/private/etc`, the Homebrew prefixes, dyld needs) MUST be allowed. `$HOME` MUST be denied except the CLI's own state dir and the private tmp dir (read-write). Outbound network MUST be allowed; bind and inbound MUST be denied.

#### Scenario: Exec outside rx denied [ci-macos]
- GIVEN a fake CLI jailed with one granted binary
- WHEN it execs `/usr/bin/curl`, `/usr/bin/ssh` and `/bin/sh`
- THEN each fails and the granted binary runs

#### Scenario: Home outside the state dir denied [ci-macos]
- GIVEN a fake CLI with its state dir granted
- WHEN it reads or writes another path under `$HOME`, or writes outside rw
- THEN the access fails; the state dir read-write succeeds

#### Scenario: Network [ci-macos]
- GIVEN a jailed fake CLI and a local listener
- WHEN it connects outbound, then tries to bind a socket
- THEN connect succeeds and bind fails

### Requirement: Seatbelt apply failure is a jail error (S3)
A `sandbox-exec` exit of 71 (profile not applied) MUST end the turn with a jail error. It MUST NOT be reported as a CLI error and MUST NOT lead to a bare run. A unique failure marker MUST let the runtime tell it from a CLI exit.

#### Scenario: Exit 71 [auto]
- GIVEN a fake helper that exits 71 with the failure marker
- WHEN a turn runs
- THEN the turn ends with a jail error event and no CLI process ran bare

### Requirement: Enabling codex and gemini on macOS (S4)
Until S4, `codex` and `gemini` MUST stay refused on macOS even with an Available probe. From S4, both MUST run jailed on macOS when the probe is Available, with the same profile and fake-CLI coverage; a failure MUST refuse, never run bare. The README MUST state gemini is "not yet verified on a real Mac".

#### Scenario: Refused before S4 [auto]
- GIVEN macOS, an Available probe, S1-S3 only, `chat` = `codex`
- WHEN a message is sent
- THEN no process is started

#### Scenario: Enabled in S4 [auto, manual]
- GIVEN macOS, an Available probe and `chat` = `codex` (or `gemini`)
- WHEN a message is sent
- THEN the CLI starts only through `__jail-exec` with Seatbelt; a real codex turn completes with network on a real Mac

#### Scenario: Jail failure never runs bare [auto]
- GIVEN macOS with an Unavailable probe, or exit 71
- WHEN a message is sent to `codex` or `gemini`
- THEN the turn is refused or ends with a jail error, never bare

### Requirement: claude is never jailed on macOS (S1)
On macOS `claude` MUST run bare with its text-only flags, never through the helper, whatever the probe says, to protect its Keychain login.

#### Scenario: claude on macOS [auto]
- GIVEN macOS with an Available probe and `chat` = `claude`
- WHEN a message is sent
- THEN the CLI is started directly with its flags and no helper

## MODIFIED Requirements

### Requirement: CLI jail
On Linux, a CLI MUST run inside a Landlock jail applied by a re-exec helper (`__jail-exec`) before the CLI is executed. On macOS the backend is Seatbelt (see Seatbelt requirements). The jail has two goals: a CLI MUST NOT read, write or list the user's files outside its own state, and a CLI MUST NOT be able to execute any program other than its own interpreter, its own package and the helper binaries proven necessary. This gives a CLI connection the same guarantee as an API connection: text only. The jail MUST be applied in a single-threaded helper (never in `pre_exec`), MUST require full enforcement of every right of the probed ABI (Landlock ABI 3 or newer), and a partial or absent enforcement MUST count as not jailed: the helper exits 125 with `jail not enforced` and never executes the CLI.

Filesystem rules. Read and execute (rx) is granted only to the CLI's interpreter binary (for example the canonical node path), the CLI's own package root, and the specific helper binaries a manual check proves necessary, each by exact path and never as a whole directory. `/usr/lib`, `/usr/share`, `/lib`, `/lib64` and `/etc` MUST be read-only with NO execute right (shared libraries still load, because mapping needs only read). The folders that hold programs (`/usr/bin`, `/usr/sbin`, `/bin`) are NOT granted at all, not even for reading: Landlock does not gate `mmap(PROT_EXEC)`, so a granted ELF loader could otherwise run any program it can read (`ld.so /usr/bin/curl`). Read-only is also granted to `/etc`, the minimal `/proc` and `/sys/fs/cgroup` entries, and `/dev/urandom`, `/dev/random`; `/dev/null` is read-write. The private empty cwd is read-only and a private per-turn `TMPDIR` is read-write. Each CLI's own state directory is read-write (claude `~/.claude/` and `~/.claude.json`; codex `$CODEX_HOME` else `~/.codex/`; gemini `~/.gemini/`). Everything else, including `$HOME`, projects, `~/.ssh`, `/tmp` and other CLIs' directories, is denied. The network is not restricted, because the CLIs need their APIs. Paths MUST be resolved by existence checks and directory handles, never by reading a credential file.

Policy. `codex` and `gemini` are jail-required: without a working jail they MUST NOT start and resolve to `no jail`, shown as `<engine>: not available on this system yet`. A helper exit of 125 becomes a jail error. `claude` is jail-layered: jailed when the jail is available, and run with its flags alone when it is not, except on macOS where it is never jailed. Any OS without a backend (everything except Linux and macOS) is `Unavailable`; Ollama, the OpenAI-compatible servers and `claude` keep working there.
(Previously: macOS was an OS without a backend, so it was always Unavailable and had no claude exemption.)

If a CLI cannot work without a general shell or a wider execute right, its slice is GATED (`gated off`) and the user is told; the jail is never widened to a whole directory.

#### Scenario: Landlock probe [auto]
- GIVEN a probe reporting ABI 2, and another reporting ABI 6
- WHEN the jail support is derived
- THEN the first is Unavailable and the second is Available with its ABI

#### Scenario: Jail spec for an nvm layout [auto]
- GIVEN the resolved paths of a node CLI installed under an nvm directory
- WHEN the jail spec is built
- THEN the rx set holds exactly the canonical node binary and the CLI package root, `/usr`, `/bin`, `/lib` and `/lib64` are read-only and not in the rx set, and no path under `$HOME` other than the CLI's own package and state appears

#### Scenario: No path to ssh keys [auto]
- GIVEN any CLI layout
- WHEN the jail spec is built
- THEN `~/.ssh`, `$HOME` itself and `/tmp` are in no rule

#### Scenario: Jailed read outside the allow-list fails [auto, tests/jail.rs]
- GIVEN the jail helper started with an allow-list that does not contain a temporary `outside` directory
- WHEN the jailed program reads `outside/secret`
- THEN the read fails with EACCES and the exit status is not zero; with the directory added as read-only the file is printed

#### Scenario: Write rules [auto, tests/jail.rs]
- GIVEN a read-only directory and a read-write directory in the allow-list
- WHEN the jailed program writes in each
- THEN the write to the read-only directory fails with EACCES and the other succeeds

#### Scenario: Execve of system programs is denied [auto, tests/jail.rs]
- GIVEN the jail helper with `/usr`, `/bin`, `/lib` and `/lib64` read-only and no execute right, and one exact helper binary granted
- WHEN a jailed program attempts to execve `/usr/bin/ssh`, `/usr/bin/curl` and `/bin/sh`
- THEN each attempt fails with EACCES, while the granted binary runs

#### Scenario: Shared libraries still load [auto, tests/jail.rs]
- GIVEN the granted binary is dynamically linked against libraries under `/usr/lib` or `/lib`
- WHEN it runs inside the jail
- THEN it starts without a loader error

#### Scenario: Partial enforcement refuses to run [auto, tests/jail.rs]
- GIVEN the helper started with a test-only minimum ABI above the kernel's
- WHEN it runs
- THEN it exits with status 125, prints `jail not enforced` and never executes the program

#### Scenario: No jail, no codex or gemini [auto]
- GIVEN a jail probe that is Unavailable and `chat` assigned to `codex`
- WHEN a message is sent
- THEN no process is started and the guidance reads `codex: not available on this system yet`

#### Scenario: Claude runs bare without a jail [auto]
- GIVEN a jail probe that is Unavailable and `chat` assigned to `claude`
- WHEN a message is sent
- THEN the CLI is started directly with its flags and no helper

#### Scenario: Helper exit 125 is a jail error [auto]
- GIVEN a fake helper that exits with status 125
- WHEN a turn runs
- THEN the turn ends with a jail error event

#### Scenario: The granted loader cannot run a program it cannot read [auto, tests/jail.rs]
- GIVEN a jail that grants the ELF loader of a program and read-only access to `/usr/lib`, `/usr/share`, `/lib`, `/lib64` and `/etc`
- WHEN the loader is asked to run `/usr/bin/true` (a direct execve of it is also denied)
- THEN it fails with `Permission denied`, and the same command runs when `/usr` is also readable

#### Scenario: A jailed CLI cannot run ssh or curl [manual]
- GIVEN a jailed `codex` or `gemini` with network access
- WHEN the message asks it to `ssh` to a host or to `curl` a URL
- THEN it cannot do it, no connection is made by those programs, and the answer is text only

#### Scenario: Linux unchanged (S1-S4) [auto]
- GIVEN Linux with Landlock ABI 3 or newer
- WHEN any slice is applied
- THEN the Landlock backend, helper, rules, policy and all scenarios above behave exactly as before

#### Scenario: Other OS stays Unavailable [auto]
- GIVEN an OS that is neither Linux nor macOS
- WHEN the jail support is derived
- THEN it is Unavailable
