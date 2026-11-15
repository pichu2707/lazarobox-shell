# Agent instructions

Instructions for any AI agent (Codex, OpenCode, Gemini CLI, Cursor, Claude
Code…) working in this repository.

## Skills

Read the skill's `SKILL.md` before acting when its trigger matches.

| Skill | Trigger |
|---|---|
| [`report-lazarobox-issue`](skills/report-lazarobox-issue/SKILL.md) | lazarobox crashes, hangs, prints an error or behaves against its README while you work: offer to report it on GitHub, with the user's consent, after searching open and closed issues. |

## Worktrees and build folders

Each git worktree builds into its own `target/`, which grows to 10-20 GB in
this repository (most of it `target/debug/incremental`). Nothing removes it
when the branch merges.

- When a branch's pull request merges, remove its worktree. Its commits are
  in `origin/main`, so nothing is lost; the build folder goes with it.
- Run `scripts/clean-builds.sh` after `git fetch` to see every worktree and
  build folder with its size, branch and merge state, and what removing each
  one affects. It only lists by default; `--apply` removes the oldest merged
  worktrees, with their local branches, until the total is under `--cap`
  (default 40G).
- It never removes the main checkout, the worktree it runs from, a locked
  worktree, the worktree of the base branch, a worktree with uncommitted
  changes or untracked files, one whose commits are not all in the base ref
  (`--base`, default `origin/main`), or one where a process is running a binary
  from inside it (for example `target/release/lazarobox`): it lists it as
  `running` and says which binary. Only a running executable counts, not a
  shell whose cwd is inside the worktree. The check reads `/proc` on Linux and
  `ps` on macOS; on other systems it prints a note that it was skipped, and on
  Linux a note when some processes (other users') could not be read. Ignored
  files, such as build output, go with a removed worktree.
- It never removes a build folder on its own, including a stray top-level
  `target-*`: those are listed only. Free one with `cargo clean`, after
  checking that no process runs a binary from it.
- No fixed build slots (`target-w1`, `target-w2`). What replaced them: each
  agent worktree builds into its own `target/`, runs `rm -rf target` after its
  checks, and the lighter dev profile (#677) keeps a cold build cheap.
- Never run the full test suite from a worktree under `/tmp`: the app's jail
  refuses `/tmp` roots as too broad, and 168 lib tests fail there.
- A build folder holds compiler output only: removing it (`cargo clean`)
  costs a rebuild and nothing else. A worktree can hold uncommitted work:
  check it first, as the script does.
- Tests: `scripts/tests/clean-builds-test.sh`.
