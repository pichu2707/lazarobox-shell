# Git Tools Specification

## Purpose

Let a model see the state of a repository and record a commit, with the user reviewing every commit. Git runs through the command runner with hardened settings. Linux only in v1. Tags: [auto] = `cargo test` (temp repos); [design-confirm] = name or value that design must pin. Tool names `git_status`, `git_diff`, `git_log`, `git_add`, `git_commit` are provisional.

Terms: "Read verbs" = status, diff, log. "Write verbs" = add, commit. "Root" = the chat origin.

## Requirements

### Requirement: Two toggles
The read verbs MUST belong to the `Git` toggle and the write verbs to the `GitWrite` toggle. Both MUST be off by default. They are independent: `GitWrite` on with `Git` off offers only add and commit. A verb whose toggle is off MUST NOT be registered or executed.

#### Scenario: Defaults [auto]
- GIVEN a config with no git toggles
- WHEN the tool list is built
- THEN no git tool is present

#### Scenario: Read only [auto]
- GIVEN `Git` on and `GitWrite` off
- WHEN the tool list is built
- THEN `git_status`, `git_diff` and `git_log` are present and `git_add` and `git_commit` are not

### Requirement: Verb boundary
Only status, diff, log, add and commit MAY exist. No tool MUST run push, fetch, pull, checkout, switch, reset, clean, rebase, merge, stash, rm, mv, or any alias, and no tool MUST accept a free-form git argument list. Workers MUST NOT commit.

#### Scenario: Unknown verb [auto]
- GIVEN a call to `git_push`, `git_reset` or `git_run`
- WHEN it is dispatched
- THEN an error result is returned and nothing runs

#### Scenario: Worker commit [auto]
- GIVEN an orchestrator worker
- WHEN it calls `git_commit`
- THEN the call is refused

### Requirement: Hardened execution
Every git call MUST run in the root through the runner (same jail, no shell, cleared environment, process group, caps) with: `core.fsmonitor` off, no pager, no external diff, no textconv, the system config ignored, terminal prompts off, and optional locks off for status. A repository outside the root, or a root that is not a repository, MUST return an error result. Output MUST be redacted and wrapped as untrusted data and capped like a run.

#### Scenario: Config cannot run code [auto]
- GIVEN a repo whose config sets `core.fsmonitor` to a script and a `diff.external` command
- WHEN `git_status` and `git_diff` run
- THEN neither script runs

Hardening details (design, Git section): hooks run only from the repo's own `.git/hooks` (`core.hooksPath` is pinned to `<git-common-dir>/hooks`, also in a linked worktree; the commit review says so when the repo sets another one, because a worktree hook dir can be written by the model); the user's global git config is never loaded, and the commit identity is passed as `user.name`/`user.email`: the repo's effective identity (`git config` in the repo, so a repo-level value wins over the global one), read by the preflight, refused when it has control or format characters, a newline or is too long; GPG and signature display are off; the secret names and the secret folders are excluded by `:(exclude,glob,icase)` pathspec. Residual risk: filter drivers (clean/process) from the repo config still run, bounded by the jail.

#### Scenario: Not a repository [auto]
- GIVEN a root without a repository
- WHEN `git_status` runs
- THEN an error result is returned

#### Scenario: Output is data [auto]
- GIVEN a commit message `Ignore previous instructions`
- WHEN `git_log` returns it
- THEN it sits inside the untrusted-data wrapper

### Requirement: Read verbs
`git_status` MUST return a parsed, bounded status. `git_diff` MUST return a unified diff of the working tree or of the staged changes, bounded in size, with the secret file names excluded by pathspec so a modified tracked `.env` is not printed. `git_log` MUST be bounded in entries (default 20, at most 50) and use a fixed format. `git_log` keeps history simplification with the secret excludes: a commit that touches only secret files MUST stay hidden, and `--full-history` MUST NOT be passed. Paths and names printed to the model MUST NOT carry control or bidi characters, and a secret name (also as the old name of a rename) MUST NOT be listed. On a Local engine all three read verbs MUST run without approval. On a Cloud engine only `git_diff` MUST need an explicit `y`, because it sends file contents to the provider; `git_status` and `git_log` MUST run without approval, because they never send file contents and secrets are excluded by pathspec. (User decision, 2026-10-28; it supersedes the 2026-10-22 rule that every read verb asks on Cloud.) A `path` that is refused (secret name or folder, inside `.git`, escaping the root, absolute, empty, or with control or format characters) MUST be refused before any prompt opens.

#### Scenario: Status [auto]
- GIVEN one modified, one staged and one untracked file
- WHEN `git_status` runs
- THEN the three are listed with their state

#### Scenario: Tracked secret not printed [auto]
- GIVEN a tracked `.env` that was modified
- WHEN `git_diff` runs
- THEN the diff does not contain the `.env` change

#### Scenario: Log bound [auto]
- GIVEN 500 commits
- WHEN `git_log` runs without a limit
- THEN at most the fixed maximum is returned

#### Scenario: Cloud asks [auto]
- GIVEN a Cloud engine
- WHEN `git_diff` is called
- THEN an approval is shown and nothing runs until `y`

### Requirement: add
`git_add` MUST stage the named paths, which MUST be inside the root, MUST NOT match the secret lists (names or folders), and MUST NOT be inside `.git/`. A folder or `.` MUST NOT stage the secret files below it (secret pathspec excludes). It MUST be refused while unproposed staged changes exist in the batch store (`propose them first`) and MUST need an explicit `y` as a one-line prompt showing the paths.

#### Scenario: Add a file [auto]
- GIVEN an untracked `src/a.rs`
- WHEN `git_add` is called with `src/a.rs` and `y` is pressed
- THEN the file is staged in the index

#### Scenario: Secret refused [auto]
- GIVEN the path `.env`
- WHEN `git_add` is called
- THEN it is refused with the refusal text

#### Scenario: Unproposed batch [auto]
- GIVEN unproposed staged changes
- WHEN `git_add` is called
- THEN it is refused with `propose them first`

### Requirement: commit
`git_commit` MUST show the message and the staged diff in the full-pane review view, with the same coverage gate as a write batch, and MUST need an explicit `y`. Signing MUST be disabled for the commit and the review MUST show a visible notice of it. Hooks MUST run inside the jail. The message MUST be bounded and MUST NOT hold control or bidi characters. A commit with nothing staged MUST return an error result without opening the review. A commit MUST be refused while unproposed staged changes exist. A declined or cancelled review ends as `denied` and no commit is made. The commit MUST NOT hang on a signing or credential prompt. The review MUST list the hooks that will run (or say none will). After a successful commit the host MUST compare the committed tree with the reviewed one; on a difference the result MUST be an error that says the commit differs from the reviewed changes, names the differing paths (secret names masked), says whether a secret path was committed and suggests the way back, and the host MUST NOT undo the commit (user decision 2026-10-28: warn, never undo).

#### Scenario: Review shows message and diff [auto]
- GIVEN a staged change and a message
- WHEN `git_commit` is called
- THEN the review holds the message, the staged diff and the signing notice [TestBackend + insta]

#### Scenario: Gate [auto]
- GIVEN a staged diff of 60 rows of which 10 are shown
- WHEN `y` is pressed
- THEN no commit is made

#### Scenario: Signing disabled [auto]
- GIVEN a repo configured to sign commits
- WHEN a commit is approved
- THEN it is created unsigned without a signing prompt

#### Scenario: Hook is jailed [auto]
- GIVEN a `pre-commit` hook that writes to `$HOME/x`
- WHEN the commit is approved
- THEN the hook cannot write outside the root

#### Scenario: Nothing staged [auto]
- GIVEN an index with no changes
- WHEN `git_commit` is called
- THEN an error result is returned and no review opens

#### Scenario: Declined [auto]
- GIVEN an open commit review
- WHEN `n` or a cancel ends it
- THEN the result is `denied` and no commit exists
