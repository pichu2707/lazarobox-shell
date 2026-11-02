# Start Dir Specification

## Purpose

Let the user choose the folder lazarobox starts in, so a desktop launcher or a command line does not depend on where the process happens to be launched. Three sources: the `--dir` flag, the `[workspace] dir` config key and the launch cwd, plus `install-desktop --dir` which writes `Path=` in the launcher. How the start folder reaches panes is in terminal-session; the chat picker is in chat-folder. Tags: [auto] = `cargo test` (temporary directories, fake environment); [manual] = real machine.

Terms: "start dir" = the folder chosen by the precedence below. "Launch cwd" = the process working directory when no flag or config applies.

## Requirements

### Requirement: Precedence
The start dir MUST be, in order: the `--dir` flag, else a valid `[workspace] dir` from the config, else the launch cwd. A present but invalid source (`--dir` or the config value) MUST fall back to the launch cwd and MUST show a notice inside lazarobox (user decision 2026-10-28: stderr is invisible when launched from rofi, so the app never exits 1 for a bad folder). An invalid `--dir` does not fall through to the config value.

#### Scenario: Flag wins [auto]
- GIVEN `--dir /a` and `[workspace] dir = "/b"`
- WHEN the start dir is resolved
- THEN it is `/a`

#### Scenario: Config over launch cwd [auto]
- GIVEN no flag, `[workspace] dir = "/b"` and launch cwd `/c`
- WHEN the start dir is resolved
- THEN it is `/b`

#### Scenario: Neither [auto]
- GIVEN no flag, no config key and launch cwd `/c`
- WHEN the start dir is resolved
- THEN it is `/c`

### Requirement: The `--dir` flag
`--dir <path>` MUST be a top-level flag of the plain TUI start. The path may be absolute, relative to the launch cwd, or start with `~`. It SHOULD name an existing directory; otherwise the app MUST start normally in the launch cwd and show a notice inside lazarobox naming the path and the reason (`--dir: folder not found: <path>` or `--dir: not a folder: <path>`), without exiting and without writing to stderr. A `--dir` without a value is also not fatal: the app starts in the launch cwd with the notice `--dir: missing value`. (Amended 2026-10-28; the draft exited with status 1.) A `--dir` after an internal subcommand is not recognised (except for `install-desktop`, see below).

#### Scenario: Valid directory [auto]
- GIVEN `--dir /w/proj` which exists
- WHEN the app starts
- THEN the first shell pane's cwd is `/w/proj`

#### Scenario: Relative path [auto]
- GIVEN launch cwd `/w` and `--dir proj`
- WHEN the start dir is resolved
- THEN it is `/w/proj`

#### Scenario: Missing directory [auto]
- GIVEN `--dir /nope` which does not exist
- WHEN the binary starts
- THEN the app starts in the launch cwd, a notice naming `/nope` is shown inside lazarobox, nothing is written to stderr, the process does not exit and the config value is not used

#### Scenario: A file, and no value [auto]
- GIVEN `--dir /etc/hostname` (a file), and in another run `--dir` alone
- WHEN the app starts
- THEN each starts in the launch cwd with its own notice and no exit

#### Scenario: Tilde [auto]
- GIVEN `HOME=/h` and `--dir ~/code`
- WHEN the start dir is resolved
- THEN it is `/h/code`

### Requirement: The `[workspace] dir` key
The config MAY hold `[workspace]` with `dir`, a string: an absolute path or one starting with `~`. An empty string is unset. A relative path, a non-string value or a non-table `workspace` MUST fall back to unset for that key only and report `workspace.dir: expected an absolute path`, `workspace.dir: expected a string` or `workspace: expected a table` under the startup-notice rules of configuration. A path that does not exist or is not a directory at startup MUST be ignored with the notice `config: workspace.dir: folder not found` and the launch cwd used. The same rule applies to the `Path=` folder of the launcher: when it is missing, the terminal starts in its own cwd and lazarobox starts there; when `--dir=<dir>` is also in `Exec` the `--dir` notice is shown. Unknown keys under `workspace` are ignored.

#### Scenario: Valid key [auto]
- GIVEN `[workspace] dir = "/w/proj"` which exists
- WHEN the app starts
- THEN the start dir is `/w/proj` and no notice is shown

#### Scenario: Missing folder [auto]
- GIVEN `dir = "/gone"` which does not exist
- WHEN the app starts
- THEN the start dir is the launch cwd and the notice is `config: workspace.dir: folder not found`

#### Scenario: Relative and wrong type [auto]
- GIVEN `dir = "proj"`, and in another file `dir = 3`
- WHEN each is parsed
- THEN the key is unset and the matching notice is shown

### Requirement: Start dir is the launch folder
The start dir MUST be the launch cwd used by the rest of the app: the first shell pane starts there, and every fallback "launch cwd" (see terminal-session) is the start dir. A chat opened from that first pane starts in it (the picker applies if it is broad, see chat-folder). `--dir ~` or a start dir equal to `$HOME` is allowed; the picker still appears for chats there.

#### Scenario: Fallback uses the start dir [auto]
- GIVEN start dir `/w/proj` and a focused pane whose cwd was removed
- WHEN a split is requested
- THEN the SpawnPane effect names `/w/proj`

#### Scenario: Chat from the first pane [auto]
- GIVEN start dir `/w/proj` (an acceptable folder)
- WHEN a chat is opened from the first pane
- THEN the chat folder is `/w/proj` and no picker is shown

#### Scenario: Home start dir still asks [auto]
- GIVEN `--dir ~`
- WHEN a chat is opened from the first pane
- THEN the picker is shown

### Requirement: `install-desktop --dir`
`install-desktop --dir <path>` MUST write the launcher with a `Path=` line holding the resolved absolute path of an existing directory, AND the same path as `--dir=<dir>` in the `Exec` line (user decision 2026-10-28, so the folder holds even when the terminal ignores `Path=`), so the desktop environment starts lazarobox there. Relative paths and `~` are resolved at install time. A missing or non-directory path MUST make the command fail with one stderr line and a non-zero exit status, writing nothing (this is an install-time command run from a shell, not a launch). Without `--dir` the launcher MUST contain neither the `Path=` line nor `--dir=` in `Exec`, so re-running `install-desktop` without `--dir` removes a previous one. `Terminal` is unchanged. A path with spaces or other special characters MUST be written so the launcher stays valid.

#### Scenario: Path written [auto]
- GIVEN `install-desktop --dir /w/proj` with `/w/proj` existing
- WHEN the launcher text is produced
- THEN it contains `Path=/w/proj`, an `Exec` line with `--dir=/w/proj` added, and the same `Terminal` line as without the flag

#### Scenario: Without the flag [auto]
- GIVEN `install-desktop` with no `--dir`
- WHEN the launcher text is produced
- THEN it has no `Path=` line

#### Scenario: Invalid path writes nothing [auto]
- GIVEN `--dir /nope`
- WHEN `install-desktop` runs
- THEN it exits non-zero with one stderr line and no launcher file is created or changed

#### Scenario: Reinstall removes it [auto]
- GIVEN a launcher with `Path=/w/proj` already installed
- WHEN `install-desktop` runs without `--dir`
- THEN the installed launcher has no `Path=` line and no `--dir=` in `Exec`

#### Scenario: Spaces in the path [auto]
- GIVEN an existing directory `/w/my proj`
- WHEN `install-desktop --dir "/w/my proj"` produces the launcher
- THEN the `Path=` value is the exact folder and the entry stays valid

#### Scenario: From rofi [manual]
- GIVEN the installed launcher with `--dir` set to a project
- WHEN lazarobox is started from rofi
- THEN the first shell opens in that project

### Requirement: Documentation
The Spanish README MUST document `--dir`, `install-desktop --dir`, `[workspace] dir` and the precedence, and MUST replace the "cd in a shell pane first" workaround with the picker and `/cd`.

#### Scenario: README updated [manual]
- GIVEN the README
- WHEN it is read
- THEN the workaround is gone and the four items above are described
