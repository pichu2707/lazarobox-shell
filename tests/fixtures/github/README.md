# GitHub fixtures

Recorded on 2026-11-04 with `gh` 2.96.0 from read-only calls against the public repository
`pichu2707/lazarobox-shell`, always as `--repo github.com/OWNER/REPO` (`gh repo view github.com/OWNER/REPO`
for the repo probe), with exactly the `--json` field list of design section 6. Every field was accepted.

| File | Command | Notes |
|---|---|---|
| `issue-list.json` | `gh issue list --state open --limit 3 --json number,title,state,author,labels,updatedAt` | Real. |
| `issue-view.json` | `gh issue view 541 --json number,title,state,author,labels,body,createdAt,updatedAt,url` | Real. |
| `issue-view-comments.json` | same plus `comments` | Real, one comment. Two-comment and oversized cases are built in the tests. |
| `pr-view.json` | `gh pr view 559 --json number,title,state,isDraft,author,body,baseRefName,headRefName,files,changedFiles,additions,deletions,statusCheckRollup,url` | Real. `labels` (`[]`) was added after review from `gh pr view --json labels` on the same PR; the extra fields `createdAt`, `updatedAt`, `url` are in the files but no longer requested. Six check runs: success and skipped. |
| `pr-view-many-files.json` | same, PR 537 | TRIMMED by hand: `files` cut to 5 of the 100 gh returned, `body` cut to 400 characters, `statusCheckRollup` cut to 2. `changedFiles` is the real 175. |
| `pr-view-checks.json` | `pr-view.json` plus three entries | HAND-BUILT. This repository has no failing, in-progress or status-context check. The failing and in-progress check runs follow the real `CheckRun` shape; the `StatusContext` entry (`context`, `state`, `targetUrl`) follows the GraphQL schema and was NOT seen live. |
| `repo-view.json`, `repo-view-fork.json` | `gh repo view github.com/OWNER/REPO --json isFork,parent` | Real. The fork is `pichu2707/gentle-ai` (this repository is not a fork). |

Facts the fixtures settled (they win over the design):

- `author` is `{id, is_bot, login, name}`; `labels` entries are `{id, name, description, color}`; `files` entries are `{path, additions, deletions, changeType}`; `parent` is `{id, name, owner: {id, login}}` or `null`.
- `state` is upper case (`OPEN`, `MERGED`, `CLOSED`).
- `gh` caps `files` at 100: PR 537 has `changedFiles` 175 and 100 files.
- A `CheckRun` has `name`, `status` (`COMPLETED`, `IN_PROGRESS`, `QUEUED`) and `conclusion` (`SUCCESS`, `SKIPPED`, `CANCELLED`, `NEUTRAL`, `FAILURE`; empty while running). A `StatusContext` has `context` and `state`.
- `--repo github.com/O/R` is accepted by `issue list`, `issue view`, `pr view`; `gh repo view github.com/O/R` works.
- A `gh` alias cannot shadow `issue`, `pr` or `repo`: `gh alias set` refuses with "already a gh command or extension" (gh 2.96.0, temporary config, verified by the coordinator).

## Failure recordings (PR 3, read-only, gh 2.96.0)

`*.stderr` is the stderr; `*.exit` is the exit code.

| Files | Recorded with | Facts |
|---|---|---|
| `not-found-issue.*` | `gh issue view 99999999 --repo github.com/pichu2707/lazarobox-shell --json number` | exit 1, `GraphQL: Could not resolve to an issue or pull request with the number of 99999999. (repository.issue)` |
| `not-found-pr.*` | same with `gh pr view` | exit 1, `GraphQL: Could not resolve to a PullRequest with the number of 99999999. (repository.pullRequest)` |
| `logged-out.*` | `GH_TOKEN`/`GITHUB_TOKEN` unset and a temporary empty `GH_CONFIG_DIR` | exit 4, stderr names `gh auth login` |

The matcher for "not found" is exit 1 plus the substring `with the number of`; a repository that cannot be resolved
says `Could not resolve to a Repository`, which does not match, so it stays a generic failure.

`git config -z --get remote.origin.url` exits 1 both with no `origin` and OUTSIDE a repository (it falls back to
the global config), so it cannot tell them apart. `git config --local -z --get remote.origin.url` exits 1 with no
`origin` and 128 outside a repository; a repository git refuses as dubious ownership also gives 128
(`GIT_TEST_ASSUME_DIFFERENT_OWNER=1`). The message is localized (it came out in Spanish), so only the exit code is
read. A linked worktree and a subfolder both return the common config's value. git 2.47.3.
