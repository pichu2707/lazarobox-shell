# Code Write Specification (delta: WSL drive access)

## ADDED Requirements

### Requirement: Creating a file on a Windows drive
When the no-replace rename that publishes a new file fails with `EINVAL` (drvfs does not support it), the app MUST publish the file with `linkat` from the temp name to the target, which fails when the target exists, and then remove the temp name. A target that exists MUST give the same result as the no-replace rename (`Exists`) and MUST keep its bytes. A temp name that cannot be removed MUST NOT change the result; it is removed by the usual temp cleanup. No other error of the rename MAY fall back.

#### Scenario: New file on drvfs [auto]
- GIVEN the rename seam answering `EINVAL` and a missing target
- WHEN a new file is applied
- THEN the target holds the bytes and no temp name is left

#### Scenario: Existing file on drvfs [auto]
- GIVEN the rename seam answering `EINVAL` and a target that already exists
- WHEN a new file is applied over it
- THEN the result is `Exists` and the target's bytes are unchanged

## MODIFIED Requirements

### Requirement: Root policy
Writes MUST be allowed only under one root: the chat origin directory. They MUST be refused when the origin is missing, is `$HOME`, is a parent of `$HOME`, or is a system directory, and, under WSL, when the origin is a Windows drive folder (agent-tools, WSL drive read policy) that the user has not granted (chat-folder, WSL drive grant). A target path MUST be refused when, after resolving `..` and symlinks, it is outside the root. Every refusal, including the refusals of the deny rules below, MUST return the same text, `path not available (paths are relative to the chat's folder <folder>)`, where `<folder>` is the chat origin with `$HOME` shown as `~`, so the result does not reveal whether a path exists. The one exception is a chat folder that is itself broad (`$HOME`, a parent of `$HOME`, or `/`): every write then returns the folder-level text `path not available: the chat's folder is <folder>, where files cannot be written; ask the user to run /cd <project folder>`, which depends only on the folder and never on the path, so it still reveals nothing about any path. The same holds for a Windows drive folder without a grant: every write then returns `path not available: the chat's folder <folder> is on a Windows drive, where writes, commands and git need your permission; ask the user to run /cd <folder>`. One case MAY say more: when the path passed this policy and the deny rules and only its parent folder is missing, the text MUST be `path not available: the folder <parent>/ does not exist (paths are relative to the chat's folder <folder>)`, with `<parent>` as the model wrote it.

#### Scenario: Origin refused [auto]
- GIVEN origins `$HOME`, `/home`, `/usr` and a missing directory
- WHEN `write_file` is called with each
- THEN each returns `path not available` and nothing is staged

#### Scenario: Escape refused [auto]
- GIVEN a root and the paths `../x`, `/etc/x` and `link/x` where `link` points outside the root
- WHEN `write_file` is called with each
- THEN each returns `path not available`

#### Scenario: One refusal text [auto]
- GIVEN an existing denied path and a missing path outside the root
- WHEN `edit_file` is called on each
- THEN both results carry the identical text

#### Scenario: Missing parent named [auto]
- GIVEN a chat opened in `~/Descargas` with no `Descargas` folder inside it
- WHEN `write_file` is called with `Descargas/fibonacci.py`
- THEN it returns `path not available: the folder Descargas/ does not exist (paths are relative to the chat's folder ~/Descargas)` and nothing is staged
- AND `nope/.env` below a missing `nope` returns the one refusal text

#### Scenario: Drive folder without a grant [auto]
- GIVEN WSL and a chat in `/mnt/c/Users/ana/Documents/app` that the user did not grant
- WHEN `write_file` is called with `main.py` and with `../x`
- THEN both return the folder-level drive text and nothing is staged

#### Scenario: Granted drive folder [auto]
- GIVEN the same chat after the user granted it
- WHEN `write_file` is called with `main.py` and with `../x`
- THEN the first is staged and the second returns the one refusal text
