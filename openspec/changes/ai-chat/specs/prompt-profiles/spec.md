# Prompt Profiles Specification

## Purpose

A profile is the persona given to the model. There are exactly two, `analyst` and `programmer`. A profile changes ONLY the prompts: no engine, model, key, layout or behaviour changes with it. Each profile has a built-in system prompt per task, and the user may override any of them with a file. Override files are untrusted input. Only the `chat` task is used at runtime in this change; the other tasks resolve the same way for later phases. Tags: [auto] = `cargo test` (temporary directories); [manual] = real machine.

## ADDED Requirements

### Requirement: Profiles and default
The profiles MUST be exactly `analyst` and `programmer`, in that order, with the lowercase ids as the values of `[ai] profile`. The default MUST be `programmer` when the key is absent or empty. Custom profiles do not exist. The profile of the config MUST be read once at startup (like the rest of the config) and updated when the menu saves.

#### Scenario: Profile set integrity [auto]
- GIVEN the profile table
- WHEN it is validated
- THEN it has exactly `analyst` and `programmer`, unique, in that order

#### Scenario: Default [auto]
- GIVEN a config without `[ai] profile`, and one with `profile = ""`
- WHEN the profile is read
- THEN both give `programmer`

#### Scenario: Unknown value [auto]
- GIVEN `[ai]` with `profile = "wizard"`
- WHEN the config is parsed
- THEN the profile is `programmer` and the startup notice names `ai.profile` (see configuration)

#### Scenario: Profile changes only prompts [auto]
- GIVEN the same assignments and history under `analyst` and under `programmer`
- WHEN a turn is prepared for each
- THEN engine, model, history and request shape are equal and only the system prompt text differs

### Requirement: Built-in prompts
Each profile MUST have a built-in prompt for each task (`general`, `code`, `chat`, `review`, `summarize`), compiled into the binary. Every built-in MUST be written in English, be non-empty and at most 4 KiB, and MUST tell the model to reply in the language the user writes in. The prompts of the two profiles MUST differ for every task. The chat prompt MUST state that the model is a text assistant without access to files, a shell or the terminal.

#### Scenario: Prompt table integrity [auto]
- GIVEN the built-in table
- WHEN it is validated
- THEN there are 10 prompts (2 profiles x 5 tasks), each non-empty, at most 4 KiB, and the two profiles differ for every task

#### Scenario: Reply in the user's language [auto]
- GIVEN every built-in prompt
- WHEN it is inspected
- THEN each contains the instruction to reply in the user's language

#### Scenario: Chat prompt states the limits [auto]
- GIVEN the chat prompt of each profile
- WHEN it is inspected
- THEN it states there is no access to files, a shell or the terminal

### Requirement: Prompt resolution per task
The system prompt for a task and profile MUST be resolved in this order: the override file for that task, then the override file for `general`, then the built-in prompt for that task. Override files live in `prompts/<profile>/<task>.md` under the lazarobox config directory (the directory of `config.toml`, see configuration); a profile never reads another profile's files. An invalid override file (see below) is skipped with a notice and resolution continues to the next level. Resolution is done once when a conversation is started or resumed, and the result is kept for that conversation's turns. The prompt of the resumed conversation MUST come from the conversation's stored profile.

#### Scenario: Built-in when no files [auto]
- GIVEN profile `analyst` and no override directory
- WHEN the prompt for `chat` is resolved
- THEN it is the built-in `analyst` chat prompt and no notice is produced

#### Scenario: Task override wins [auto]
- GIVEN `prompts/analyst/chat.md` and `prompts/analyst/general.md` both valid
- WHEN the prompt for `chat` is resolved
- THEN it is the content of `chat.md`

#### Scenario: General override is the second level [auto]
- GIVEN only `prompts/analyst/general.md` is valid
- WHEN the prompt for `chat` is resolved
- THEN it is the content of `general.md`

#### Scenario: Other profile ignored [auto]
- GIVEN profile `analyst` and a valid `prompts/programmer/chat.md`
- WHEN the prompt for `chat` is resolved
- THEN it is the built-in `analyst` prompt

#### Scenario: Resolved once per conversation [auto]
- GIVEN a started conversation and then `chat.md` is edited on disk
- WHEN the next message is sent
- THEN the system prompt is still the one resolved at the start

#### Scenario: Override content is exact [auto]
- GIVEN `chat.md` containing `Be brief.\n` in UTF-8
- WHEN resolved
- THEN the prompt equals `Be brief.` after trimming trailing whitespace

### Requirement: Override files are untrusted
An override file MUST be a regular file, valid UTF-8, at most 16 KiB (16384 bytes), and readable. It MUST be read with a bound (never more than the cap plus one byte), off the UI thread, and the read MUST NOT block on a special file. A file that is missing is not an error and produces no notice. A file that is empty or only whitespace is treated as missing. A file that is not UTF-8, is over the cap, cannot be read (permission denied, I/O error), or is not a regular file (directory, FIFO, device) MUST be skipped and MUST produce a system line `prompt override ignored: <profile>/<task>.md <reason>` with reason `not valid UTF-8`, `larger than 16 KiB`, `cannot be read` or `not a regular file`. A file over the cap MUST NOT be truncated and used. The built-in prompt is the final fallback, so a conversation always has a prompt. The path shown MUST be sanitized, and the content MUST NOT be logged.

#### Scenario: Missing file [auto]
- GIVEN no file for `chat` and `general`
- WHEN resolved
- THEN the built-in is used and no notice is produced

#### Scenario: Not UTF-8 [auto]
- GIVEN `chat.md` holding the bytes `\xff\xfe\x00`
- WHEN resolved
- THEN the notice reads `prompt override ignored: analyst/chat.md not valid UTF-8` and the next level is used

#### Scenario: Over the cap [auto]
- GIVEN `chat.md` of 16385 bytes
- WHEN resolved
- THEN the notice names `larger than 16 KiB`, the file is not truncated into use, and the next level is used

#### Scenario: Exactly at the cap [auto]
- GIVEN `chat.md` of exactly 16384 valid bytes
- WHEN resolved
- THEN it is used and no notice is produced

#### Scenario: Unreadable [auto]
- GIVEN `chat.md` without read permission
- WHEN resolved
- THEN the notice names `cannot be read` and the next level is used

#### Scenario: Not a regular file [auto]
- GIVEN `chat.md` is a FIFO (or a directory)
- WHEN resolved
- THEN resolution returns without blocking, the notice names `not a regular file` and the next level is used

#### Scenario: Huge special file [auto]
- GIVEN `chat.md` is a symlink to `/dev/zero`
- WHEN resolved
- THEN no more than 16385 bytes are read, the file is ignored with a notice

#### Scenario: Empty file [auto]
- GIVEN `chat.md` containing only whitespace
- WHEN resolved
- THEN it is treated as missing and no notice is produced

#### Scenario: Bad task file, good general file [auto]
- GIVEN an invalid `chat.md` and a valid `general.md`
- WHEN resolved
- THEN the notice for `chat.md` is shown and the prompt is the content of `general.md`

#### Scenario: Notice is clean [auto]
- GIVEN an override path containing an escape sequence
- WHEN the notice is built
- THEN the notice contains no control characters

### Requirement: Profile in the menu
The AI section of the config menu MUST have a `Profile` row (see config-menu) whose value is the profile id. Changing it is a draft change; `s` saves it as `ai.profile` (see config-persistence) and Esc reverts it. The value MUST be read from the current config when the menu opens.

#### Scenario: Row shows the profile [auto, TestBackend + insta]
- GIVEN the config profile is `programmer`
- WHEN the menu is rendered
- THEN the AI section has a `Profile` row reading `programmer`

#### Scenario: Cycle and save [auto]
- GIVEN the default profile `programmer` and the `Profile` row selected
- WHEN `l` is pressed and `s` is pressed and the save succeeds
- THEN the current profile is `analyst` and the file holds `profile = "analyst"` under `[ai]`

#### Scenario: Esc reverts [auto]
- GIVEN the `Profile` row changed in the draft
- WHEN Esc is pressed on the task list
- THEN the profile is the one the app had at open

### Requirement: A switch applies to new conversations only
Changing the profile MUST NOT change the system prompt of any conversation that already started, in any pane, nor of a resumed conversation (which keeps its stored profile). It applies to conversations started after the change. The profile of a new conversation MUST be stored with it and shown in the list.

#### Scenario: Open conversation keeps its profile [auto]
- GIVEN a conversation started under `analyst`
- WHEN the profile is switched to `programmer` and saved, and a message is sent
- THEN the turn uses the `analyst` prompt

#### Scenario: New conversation uses the new profile [auto]
- GIVEN the profile switched to `programmer`
- WHEN a new conversation starts and a message is sent
- THEN the turn uses the `programmer` prompt and the stored profile is `programmer`

#### Scenario: Resumed conversation keeps the stored profile [auto]
- GIVEN a stored `analyst` conversation and the current profile `programmer`
- WHEN it is resumed and a message is sent
- THEN the turn uses the `analyst` prompt

### Requirement: Documentation
The README MUST document the two profiles, the override directory layout, the 16 KiB cap and the resolution order.

#### Scenario: README documents overrides [manual]
- GIVEN the README
- WHEN it is read
- THEN it names `prompts/<profile>/<task>.md`, the cap, the order task, general, built-in, and that a profile switch applies to new conversations only
