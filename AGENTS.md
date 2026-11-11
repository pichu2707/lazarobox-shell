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
  changes or untracked files, or one whose commits are not all in the base ref
  (`--base`, default `origin/main`). Ignored files, such as build output, go
  with a removed worktree.
- A build folder holds compiler output only: removing it (`cargo clean`)
  costs a rebuild and nothing else. A worktree can hold uncommitted work:
  check it first, as the script does.
- Tests: `scripts/tests/clean-builds-test.sh`.
