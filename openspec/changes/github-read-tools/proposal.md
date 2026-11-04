# Proposal: GitHub Read Tools

## Intent

The owner follows issues and PRs in a separate agent CLI because the chat cannot read them. This change adds three read-only tools, `gh_issue_list`, `gh_issue_view` and `gh_pr_view`, for the repository of the chat's folder. It is one step of Goal #543.

**For:** the owner, and anyone using the chat on their own GitHub repository, at the moment they ask "what is open?" or "what does this PR change?".

## Scope

### In Scope
- **Off by default.** Two ways on: a menu toggle and a permission granted in the chat.
- **`gh_issue_list`, `gh_issue_view`:** comments only through an explicit option visible in the call.
- **`gh_pr_view`:** a card with title, state, description, branches, changed files with line counts, and check results.
- **Repository:** always `origin`. A fork result says so and names the parent, which is not read.
- **Data, not instructions:** everything read from GitHub is marked as data, after the same secret redaction as `read_file`.
- **Reads while blocked:** in a conversation that read GitHub, on a cloud engine including Ollama `*-cloud`, `read_file` and `grep` ask on each call.
- **Cloud consent:** cloud engines ask once per repo per turn. Ollama `*-cloud` models ask like cloud, in these tools only. Local models read without asking.
- **Auto-run:** from the first GitHub read until the conversation ends, `run_in_pane` auto-run is blocked. A command is only typed and waits for the user's Enter; `y` does not run it. A resumed conversation stays blocked.
- **Visible failures:** no `origin`, missing `gh` or logged-out `gh` each give a clear message. The last two say how to fix it.
- **Views:** any new view works at 80 columns and in `TERM=linux`.

### Out of Scope
- Write tools (the test requiring no `git_push` stays)
- Diffs
- The parent repo of a fork
- GitHub Enterprise
- Free search in `gh_issue_list`
- Orchestra workers
- Guessing another remote when `origin` is missing
- Opening the dialog from a model request or from words in the user's message

**Known limitation:** a contributor working from a fork cannot read the parent repo.

## The Chat Grant

- **Trigger:** the user types `/github`, which opens the dialog. A one-line visible hint suggests it when `origin` is on GitHub. Chat commands already exist (`/cd`, `/cost`, `/feedback`), and they are user-only.
- Only a human keypress on the dialog grants. Neither the model nor any text it read can turn the tools on or answer the dialog.
- Declining shows a visible notice and does not re-prompt in the same turn.
- The tool list is fixed when a turn starts, so a grant takes effect on the next message.
- **Lifetime:** the grant lasts for the current chat session and does not change the saved toggle.
- **Cloud consent is separate.** The grant says "these tools may exist"; consent says "this repo's text may go to this engine now". A cloud engine still asks on the first read of a repo in each turn, even right after a grant.

**Tradeoff accepted by the owner:** this is not fully automatic. Asking about issues with the tools off opens nothing; the hint is what makes the feature findable.

## Capabilities

### New Capabilities
- `github-tools`: the three tools, repository rule, consent, data marking, chat grant, failure messages.

### Modified Capabilities
- `agent-tools`: a new family, off by default, excluded from workers.
- `configuration`, `config-menu`: the new toggle.
- `chat-pane`: the `/github` command, the hint and the grant dialog.
- `pane-execution`: auto-run blocked after a GitHub read.

## Approach

Follow the existing family and approval pattern. `gh` runs on the host with a fixed argument list; the repository comes from lazarobox, never from the model. About 4 chained PRs.

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/ai/tools/` | New tools and family |
| `src/core/ai/chat/approval.rs` | Consent, auto-run block |
| `src/runtime/tools/` | Running `gh`, reading `origin` |
| `src/app/tool.rs`, `src/app/chat.rs` | The offered tools and the context budget must both include a chat grant |
| `src/core/ai/config.rs`, `src/core/menu.rs` | Toggle |
| `src/core/ai/chat/folder.rs` | The `/github` command |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Attacker-written issue text steers the model | High | Data marking, sanitizing, auto-run blocked for the rest of the conversation |
| Private issue text reaches a cloud engine | Med | Per-repo, per-turn consent, including `*-cloud` models |
| Context cost | Med | Off by default, comments on request, capped output; schemas count in the budget |
| Grant dialog answered out of habit | Low | The dialog names the repo; only a keypress grants |
| The feature goes unfound | Med | The visible hint |

## Rollback Plan

A plain revert. Turning the toggle off, or ending the session, removes the tools without a revert. Old configs ignore the unknown toggle key. Whether keeping the auto-run block across a resume needs a `chat.db` change is for the design phase to settle; a schema change would alter this plan and needs the owner's decision.

## Success Criteria

- [ ] From the chat, the owner lists issues, opens one, and reads a PR card, without another CLI.
- [ ] With everything off, no GitHub tool is offered; a model cannot enable them.
- [ ] After a GitHub read, in any later turn and after a resume, no command runs without the user's Enter.
- [ ] A cloud engine asks before the first read of a repo in each turn.
- [ ] No `origin`, no `gh`, logged-out `gh` and a declined grant each show a visible message.
- [ ] The tests still show no `git_push` tool, and workers have no GitHub tools.
