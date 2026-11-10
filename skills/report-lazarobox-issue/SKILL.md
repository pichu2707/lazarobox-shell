---
name: report-lazarobox-issue
description: Report a defect of lazarobox itself to GitHub, with the user's consent, after searching open and closed issues. Trigger when lazarobox crashes, hangs, prints an error, or behaves against its README while you work.
---

# Report a lazarobox defect

Any AI agent working on or with this repository follows this procedure when
lazarobox itself fails. Nothing is reported without the user's explicit yes.

Repository: `pichu2707/lazarobox-shell`. Tool: the user's own `gh` CLI. If
`gh` is missing or not signed in, give the user the filled issue text and the
link `https://github.com/pichu2707/lazarobox-shell/issues/new?template=bug.yml`
instead.

## 1. Is it a lazarobox defect?

Report only what lazarobox produced:

- a panic, crash or hang of `lazarobox`;
- an error message or a wrong result from lazarobox;
- behaviour that contradicts the README or a spec under `openspec/specs/`;
- a test that fails on an unmodified `main`.

Do **not** report, and do not offer to report:

- failures of the model provider (context or rate limits, refusals);
- failures of your own agent runtime (a crashed sub-agent, a session that must restart);
- the environment (network, a missing tool, permissions of the machine);
- a test or build broken by changes in the working tree, yours or the user's.

In those cases say plainly what blocked the work and go on or stop as the
task requires.

## 2. Ask first

Ask the user one question with exactly these three choices, in their
language, and wait:

1. **Report it and continue**: search the issues, then comment or create.
2. **Continue without reporting**: no GitHub call at all.
3. **Stop here**: no GitHub call; stop the current work.

Only choice 1 goes on to step 3.

## 3. Gather the facts

The issue follows the repository's bug form (`.github/ISSUE_TEMPLATE/bug.yml`):

| Section | What to put |
|---|---|
| What happened | What was seen, with the error message exactly as it appeared. |
| What you expected | The behaviour the README, spec or user expected. |
| Steps to reproduce | The smallest sequence that shows it: commands, keys, config keys involved. |
| Environment | Two lines, the same shape **Menu ▸ Feedback** attaches (see below). |

Environment lines:

```text
lazarobox <version> · <os>[ (WSL)] · <shell> · <terminal>
engines: <engine> [<version>] <status> · …
```

- `<version>`: `lazarobox --version`. For a source build, the `Cargo.toml`
  version plus `source build <short commit>`.
- `<os>`: `linux`, `macos`…; add `(WSL)` under WSL.
- `<shell>` and `<terminal>`: program names only (`zsh`, `WezTerm`).
- `engines`: only the engines involved, with their status.

Add, when it helps and only in short excerpts: the failing test name and its
assertion, a few log lines, the config keys and values involved.

## 4. Privacy pass (before any GitHub call)

Remove or mask in everything you will send:

- absolute paths (write `~/…` or a repository-relative path);
- user names, host names, e-mail addresses;
- tokens, keys, passwords, cookies, and any environment variable value;
- names and content of the user's private projects or files;
- whole config files, whole diffs, chat transcripts.

If masking would change what the report means, stop and ask the user.

## 5. Search open and closed issues

```sh
gh issue list -R pichu2707/lazarobox-shell --state all --limit 30 \
  --search "<2-4 keywords of the symptom> in:title,body"
gh issue list -R pichu2707/lazarobox-shell --state all --limit 30 \
  --search "\"<distinctive part of the error message>\""
```

An issue is **equivalent** only when it describes the same observable defect
in the same behaviour, shown by its content, not by a similar title. If a
search fails or you cannot tell, make no GitHub change: tell the user and
continue as in choice 2.

## 6. Act on what you found

- **No equivalent** → create one issue:

  ```sh
  gh issue create -R pichu2707/lazarobox-shell --label bug \
    --title "<area>: <symptom>" --body-file <file>
  ```

  The body uses the four sections of step 3 as `### ` headings, then ends
  with the line `_Reported by an AI agent with the user's consent._`.
  Creation counts only when `gh` returns the new issue URL.

- **Equivalent and open** → add one comment with the new evidence only
  (`gh issue comment <n> -R pichu2707/lazarobox-shell --body-file <file>`),
  ending with the same closing line. Do not change its labels.

- **Equivalent and closed as fixed** → find the release with the fix
  (`CHANGELOG.md`, `gh release list -R pichu2707/lazarobox-shell`):
  - the user's version is older: recommend updating and trying again; no comment;
  - the user's version contains the fix and the defect still happens: create
    a new issue titled `Regression of #<n>: <symptom>` that links it. Never
    reopen an issue.

- **Equivalent and closed as not planned** → tell the user and post nothing
  unless they ask.

If any `gh` call fails, times out or its result is unclear, make no further
GitHub change and do not retry: tell the user what is known.

## 7. Close

Give the user the issue or comment URL in one line and continue the work
that was interrupted. Report each distinct defect at most once per session.

## Finding these reports later

Every report ends with `_Reported by an AI agent with the user's consent._`,
so they can be listed with:

```sh
gh issue list -R pichu2707/lazarobox-shell --state all \
  --search "\"Reported by an AI agent\" in:body"
```
