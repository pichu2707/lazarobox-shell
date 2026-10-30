# Delta for Agent Tools

Dependency: these requirements are written against the capabilities of the unarchived change `coding-tools` (agent-tools, command-runner, code-search, git-tools, code-write). This change MUST be archived after `coding-tools`. Wherever those specs say "chat origin" or "root", it now means the chat folder (see chat-folder).

## ADDED Requirements

### Requirement: Tool root is the current chat folder
Every tool family (read, search, write, command, git) MUST take its root from the chat folder at the moment a call is executed, not from the folder the chat was opened in and not from the moment the tool list was offered. After a folder change (see chat-folder) the next call uses the new root; a path argument that was valid in the old folder is judged against the new root only. The root policies are unchanged: choosing a folder never relaxes them. When the chat folder is unknown, `$HOME` or `/`, tools behave as they do today for such a root (refusals, asks). The write and command refusals for such a root name the folder as the reason (`path not available: the chat's folder is ~, where files cannot be written; ask the user to run /cd <project folder>`) so a model stops retrying and asks for `/cd`; a refused path inside a concrete folder keeps one uniform text.

#### Scenario: Same call, new root [auto]
- GIVEN `read_file` of `src/main.rs`, existing in `/w/a` and in `/w/b`, and a folder change from `/w/a` to `/w/b`
- WHEN the call is repeated
- THEN it reads `/w/b/src/main.rs`

#### Scenario: Old path escapes the new root [auto]
- GIVEN the absolute path `/w/a/src/main.rs` and a chat folder `/w/b`
- WHEN `read_file` is called
- THEN it is refused with the usual refusal text

#### Scenario: Choice does not relax policy [auto]
- GIVEN a chat whose folder was picked as `$HOME`
- WHEN a write family call is made
- THEN it is refused as for any `$HOME` root

### Requirement: Unattended execution needs a project folder
In a chat whose folder is `$HOME`, `/` or unknown, run-mode commands and other unattended execution MUST NOT run without asking, and the reason shown MUST remain `the chat's folder is your home or /`. Once a valid project folder is chosen (picker or `/cd`), the same chat MUST behave as any chat in a project folder, with a local or a cloud engine, without reopening it.

#### Scenario: Refused in home [auto]
- GIVEN a cloud engine, run mode and the chat folder `$HOME`
- WHEN the model asks to run a command
- THEN it does not run unasked and the reason is shown

#### Scenario: Runs after the pick [auto]
- GIVEN that chat after `/cd /w/proj` was confirmed
- WHEN the model asks to run an allowed command
- THEN it follows the normal approval flow of a project folder, with working directory `/w/proj`

#### Scenario: Run from rofi [manual]
- GIVEN lazarobox started from rofi, a cloud engine and the picker answered with a project
- WHEN the model runs `cargo test` after approval
- THEN it runs in that project
