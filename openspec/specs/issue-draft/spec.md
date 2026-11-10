# Issue draft Specification

## Purpose

The chat model drafts a GitHub issue about lazarobox (an idea or goal, or a bug) from the conversation and opens the existing Feedback form pre-filled. The user reviews it and sends it with Ctrl+S; the model never writes to GitHub.

## Requirements

### Requirement: The tool
There MUST be one tool, `gh_issue_draft`, in the family `IssueDraft` (see agent-tools, Tool families), on by default with `[ai.tools] issue_draft` while the `[ai.tools]` master switch is on. Workers and the orchestrator turn MUST NOT get it. It has three required arguments: `kind` (`idea` or `bug`), `title` (at most 120 characters) and `description` (at most 2000 characters). The caps are the form's own. An empty field, an unknown kind, a title with a line break or a field over its cap MUST return an error to the model; nothing is cut silently.

#### Scenario: A bad draft is the model's to fix [auto]
- GIVEN a call with an empty title, an unknown kind, a 121-character title or a 2001-character description
- WHEN it is parsed
- THEN the model gets an error naming the problem and nothing is shown to the user

#### Scenario: No tool writes to GitHub [auto]
- GIVEN every family on
- WHEN the tools are listed
- THEN `gh_issue_draft` is the only addition to the GitHub tools and `gh_issue_create`, `gh_api` and the like are unknown tools

### Requirement: Permission
Every call MUST ask a y/n at the composer naming the kind and the title (see chat-pane, Issue draft prompt). Only an unmodified lowercase `y` accepts. `n`, Esc, a timeout, a cancel, a closed chat and an app quit end as `denied`. No approval mode (`ask`, `auto`, `run`) answers it. A call while another approval is pending MUST be `denied` at once (`another approval is pending`).

#### Scenario: Every mode asks [auto]
- GIVEN a chat in `ask`, `auto` or `run`
- WHEN the model calls `gh_issue_draft`
- THEN the prompt opens and waits for the user's `y`

### Requirement: One decline per turn
After the user's decline (`n`, Esc, a cancel, a closed chat, an app quit) or a timeout, later `gh_issue_draft` calls in the same turn MUST return `denied` without a prompt. The app's own refusals (`another approval is pending`, `a feedback form is already open`) are not the turn's decline: a later call asks again. A `y` already given MUST end `ok` and its form opens, even if a parallel call of the turn was declined meanwhile. A new turn asks again.

#### Scenario: A refused parallel call keeps the user's y [auto]
- GIVEN two parallel calls, the second refused with `another approval is pending`
- WHEN the user answers `y` on the first
- THEN the first ends `ok` and its form opens at the end of the turn

#### Scenario: One prompt per declined turn [auto]
- GIVEN a call the user denied
- WHEN the model calls the tool again in the same turn
- THEN the call is denied and no second prompt opens

### Requirement: What the model receives
On `y` the tool MUST return `ok` with a short text saying the form will open with the draft for the user to review and send, and MUST NOT wait for Ctrl+S. It runs no `gh` command. On a denial it returns `denied: <reason>`. The model never learns whether the issue was sent.

#### Scenario: Ok does not wait for the send [auto]
- GIVEN an accepted draft
- WHEN the tool returns
- THEN the result is `ok`, says the user will review and send it, and no `gh` was started

### Requirement: The form opens at the end of the turn
The form MUST open when the turn ends, however it ends (including a failure or a cancel after the `y`), not while it streams, with the `Feedback` row selected. A Feedback form already open when the call arrives MUST NOT be overwritten: the call returns `denied` (`a feedback form is already open`). A form opened between the `y` and the end of the turn, another input mode the user is in at that moment, or a menu screen that waits for the user (the Memory warning, the engine and model picker with its delete confirmation, the Hugging Face token field or its disconnect `y/n`, the usage view) MUST NOT be replaced, and that screen, its selection and its pending answer stay untouched: the draft is dropped and a system line says it was not opened, naming its title. An accepted draft the host denied afterwards opens nothing and the chat says `no draft issue was opened`. A later accepted draft of the same turn replaces an earlier one. The send path is the form's own and unchanged: Ctrl+S, the redaction pass, the similar-issue search, the attached facts and the `gh`-or-link fallback.

#### Scenario: An open form is never replaced [auto]
- GIVEN a Feedback form with the user's own text, and in another run a form opened after the `y`
- WHEN a draft arrives or the turn ends
- THEN the first call is denied with `a feedback form is already open` and the second draft is dropped with a system line; the user's text is unchanged in both

#### Scenario: A menu screen that waits is never replaced [auto]
- GIVEN an accepted draft, and the menu open on the model picker, its delete confirmation, the token field, the disconnect `y/n`, the usage view or the Memory warning when the turn ends
- WHEN the turn ends
- THEN the form does not open, the screen and its footer are as they were, and a system line names the draft's title

### Requirement: When the model offers a draft
A turn that offers `gh_issue_draft` MUST end the tools block of its system prompt with one sentence telling the model to call the tool to offer a lazarobox issue only when the user asks for one, when it cannot do what was asked because lazarobox lacks it, or when a lazarobox tool or the chat fails, never for errors in the user's own code or commands, and that the tool's y/n is the user's permission, so it does not ask first (D5). A turn that does not offer the tool (an engine without tools, the master switch or the `issue_draft` toggle off, a worker, the orchestrator turn) MUST NOT carry the sentence.

#### Scenario: Only with the tool [auto]
- GIVEN the tools block of a turn with the `IssueDraft` family, and of turns without it (toggle off, master switch off, an engine without tools, a worker, the orchestrator)
- WHEN the system prompt is built
- THEN only the first has the sentence, once

### Requirement: The `/feedback` hint
When a plain chat turn does not offer `gh_issue_draft` (an engine without tools, the master switch off, or the `issue_draft` toggle off), a failed turn or a tool result that is an `Error` MUST add the system line `If this looks like a lazarobox problem, type /feedback to report it` (D6), at most once per turn. A `Denied` result, a clean turn, a turn that offers the tool and an orchestrated turn MUST NOT add it. A cancelled turn is not a failed one.

#### Scenario: Once per failing turn [auto]
- GIVEN a turn without the draft tool that has two tool errors and then fails
- WHEN its events arrive
- THEN the hint appears once; a later failing turn shows it again

#### Scenario: Not after a refusal [auto]
- GIVEN a turn without the draft tool with a denied call that then ends cleanly
- WHEN it ends
- THEN no hint appears
