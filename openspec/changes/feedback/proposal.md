# Proposal: Feedback from the terminal, and a goals zone

## Intent

Bug reports often arrive without the context needed to act on them, and ideas have no common place. Let people report a bug or share an idea from inside lazarobox, with the environment attached. GitHub forms ask for the same context, and ideas gather on a Goals board.

## Scope

In scope:
- GitHub issue forms in `.github/ISSUE_TEMPLATE/`:
  - `bug.yml`, labelled `bug`, with the fields `what-happened`, `expected`, `steps` and `environment`;
  - `idea.yml`, labelled `idea` and added to project 17, with the fields `kind`, `idea`, `why` and `environment`;
  - blank issues off.
- A `Feedback` row at the end of the Settings tab. It opens a form with:
  - a kind (bug report, idea or goal), a title and a description;
  - the environment, attached by default and shown whole before anything leaves.
- The environment holds:
  - the lazarobox version, the OS and whether it is WSL;
  - the shell's name and the terminal (`TERM_PROGRAM`, or Windows Terminal through `WT_SESSION`);
  - the detected engines with their versions.
- `Ctrl+S` sends the issue with the user's own `gh` (`gh issue create`), shaped like the issue form and labelled `bug` or `idea`.
  - The app sends only on Ctrl+S, through `gh`; lazarobox stores no GitHub token and makes no other network request.
  - Without `gh`, or with `gh` not logged in, it opens the prefilled page instead.
- `Ctrl+O` builds the prefilled new-issue link (`issues/new?template=…&title=…&<field>=…&environment=…`) and opens it in the browser (or copies it with OSC 52 when the host has no opener).
- The title and the description are redacted first.

Out of scope:
- Sending the issue through the GitHub API directly, or with a token lazarobox keeps.
- Adding an idea to project 17 from the app.
- Creating the project's «Goals» view and its auto-add workflow; the GitHub API cannot create them.

## Approach

`core::menu::feedback` is pure: the form state, the facts, the link and the lines it draws. `runtime::feedback::host_facts` reads the environment and `/proc/sys/kernel/osrelease` once at start-up, and the app adds the engines when the menu opens. The view takes over the body like the Hugging Face token field.

## Risks

- A secret pasted into the description. It goes through the memory redaction pass, and a change stays in the form for review.
- A link longer than GitHub accepts. Links over 8000 bytes are refused with a message.
- A Settings row more: the no-AI popup is one row taller, and its smallest full height is 9 instead of 8.
