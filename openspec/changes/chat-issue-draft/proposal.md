# Proposal: Chat Issue Draft

## Intent

The owner asked the chat to write up a goal from the conversation (seeing the shell while working over ssh) and the chat could not do it. This is not a bug: the chat's GitHub tools are read-only by design, and `/feedback <text>` (#551, v1.3.0) opens the Feedback form only from text the user types. This change adds one tool, `gh_issue_draft`, with which the chat model drafts an issue (kind, title, description) from the conversation and opens the existing Feedback form pre-filled. The user reviews it, edits it if needed, and sends it with Ctrl+S. The model never writes to GitHub. Refs #551.

**For:** the owner, and any user of the chat, at two moments:
- when they ask the chat to "write that up as a goal" or "report this bug";
- when the chat hits something lazarobox cannot do, or an error, and the model offers a draft on its own, asking the user first.

**User decision (2026-11-06), option A:** the model drafts, the form opens, the user sends. Option B (the model creates the issue after a `y`) was rejected.

## Scope

### In Scope
- **One new tool, `gh_issue_draft`**, with three required arguments:
  - `kind`: `idea` or `bug`, shown as `Idea or goal` or `Bug report`;
  - `title`: at most 120 characters;
  - `description`: at most 2000 characters.
  The caps are the form's own (`TITLE_CAP`, `DESCRIPTION_CAP`). An empty field, an unknown kind, or a field over its cap returns an error to the model, so it can fix the draft; nothing is cut silently (D4).
- **Preset kind:** the form opens on the draft's kind. Today `Feedback::new` always starts on `Bug report`.
- **Separate fields:** the title and description fill their own fields. The first-line rule of `/feedback <text>` stays for `/feedback` only.
- **Permission first (S4):** every call opens a y/n prompt at the composer, naming the kind and the title. Only an unmodified lowercase `y` accepts. `n`, Esc, a timeout, a cancel, a closed chat and an app quit end as `denied`. No approval mode (`ask`, `auto`, `run`) answers it, like the GitHub consent and the drive grant. The one-pending-approval rule applies: a call while another approval is pending is `denied` at once (`another approval is pending`).
- **What the model receives:** on `y`, `ok` with a short text saying the form will open with the draft for the user to review and send; on `n`, `denied`. The tool never waits for Ctrl+S (D2).
- **When the form opens:** at the end of the turn, not while it streams (D3). A form that is already open, with the user's own text, is never overwritten; the call returns `denied` (`a feedback form is already open`).
- **One decline per turn:** after a `denied` answer, later calls in the same turn return `denied` without a prompt, as GitHub consent does.
- **Gating:** a new tool family with its own `[ai.tools]` toggle, on by default (D1). It is offered only while the `[ai.tools]` master switch is on.
- **Engines:** the engines that get tools today (`claude`, `codex`, `gemini`, `ollama`).
- **Prompt guidance:** one sentence in the tools block, only when the family is on. It tells the model to offer a draft when the user asks for one, and when it cannot do something because lazarobox lacks it, or a lazarobox tool fails (D5). Calling the tool is the offer: its y/n is the permission, so the model does not ask twice.
- **Fallback without tools:** on an engine without tools, or with the family off, a one-line hint suggests `/feedback` after a failed turn or a tool `Error` (D6).
- **Send path unchanged:** Ctrl+S, the redaction pass, the similar-issue search, the attached facts and the `gh`-or-link fallback all work as today.
- **Docs:** README, CHANGELOG, the tools list and the `[ai.tools]` key.

### Out of Scope
- Any write to GitHub by the model, or a send without the user's Ctrl+S.
- Issues in the chat folder's own repository (`origin`). The Feedback form sends to the lazarobox repository only.
- Reading back whether the issue was sent, or its URL.
- Detecting "I can't do that" in the model's text on engines without tools.
- Orchestra workers and the orchestrator turn.
- Comments on existing issues by the model. The similar-issue step of the form still lets the user comment.

**Known limitation:** the draft goes to the lazarobox repository, so it fits lazarobox goals and bugs, not the user's own project. A user who wants an issue in their own repository still uses `gh` or GitHub.

## The Draft Flow

1. The model calls `gh_issue_draft` with kind, title and description.
2. The host validates the arguments. A bad argument returns an error; nothing is shown.
3. The composer shows the prompt: `draft issue: Idea or goal · <title>` with `y`/`n`.
4. `n` (or a timeout, cancel, close) returns `denied`; a notice says no draft was opened.
5. `y` returns `ok` to the model. The draft is kept with the chat pane.
6. When the turn ends, the menu opens on the Feedback form with the draft. The `Feedback` row is selected, as `/feedback` does.
7. The user edits, then presses Ctrl+S (send) or Esc (nothing sent).

**Why at turn end:** while the menu has the keys, a later approval of the same turn would wait behind it and could end `denied` by timeout. Opening at turn end leaves no approval under the form.

## Capabilities

### New Capabilities
- `issue-draft`: the tool, its arguments and caps, the y/n prompt, the result texts, the opening at turn end, and the one-decline-per-turn rule.

### Modified Capabilities
- `agent-tools`: the new family and its approval (Tool registry, Approval model); workers get no draft tool.
- `configuration`, `config-menu`: the new `[ai.tools]` toggle, on by default; the Feedback form opened with a preset kind and separate fields.
- `chat-pane`: the y/n prompt, the opening at turn end, the `/feedback` hint.

### Unchanged on purpose
- `github-tools`, Fixed read, no write: still holds. `gh_issue_draft` does not run `gh` and creates no GitHub object; only the user's Ctrl+S in the form sends, through the existing send path.

## Approach

Reuse what exists:
- **Approval:** the host asks through the approver, the app draws the prompt (`on_tool_ask`), and the answer comes back as a tool decision. A denial already reaches the model as `denied: <reason>`.
- **Form:** `MenuState::open_feedback` already opens the menu on the form; it gains a variant that takes a kind, a title and a description.
- **Gating:** a new `Family` and `ToolToggle`, so the tool lists of today stay the same when the toggle is off. Workers are excluded because `Toolset::families` never adds the family.

New parts:
- A host arm for a tool whose `y` hands a draft to the app instead of running something.
- A per-pane pending draft, opened when the turn ends.
- A per-toggle default, because every toggle is off by default today.

Expected delivery is two PRs:

| PR | Scope | Estimate (production lines) |
|----|-------|-----------------------------|
| 1 | Form: preset kind and separate fields. Tool, family, toggle, y/n, host arm, opening at turn end | ~300 |
| 2 | Prompt guidance, `/feedback` fallback hint, docs | ~100 |

PR 1 alone answers S1-S3: the user can ask the chat for a draft.

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/ai/tools/registry.rs` | New tool and family |
| `src/core/ai/config.rs` | New toggle, default on |
| `src/core/menu/feedback.rs`, `src/core/menu.rs` | Form with preset kind and separate fields |
| `src/runtime/tools/host.rs` | Host arm: validate, ask, hand over the draft |
| `src/app/tool.rs`, `src/app.rs` | Pending draft, opening the form at turn end |
| `src/app/chat.rs` | `/feedback` hint on engines without the tool |
| `src/core/ai/chat/prompts.rs` | Guidance sentence gated on the family |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Text the model read (a file, an issue) steers it into drafting an issue that carries private data | Med | The y/n names the title; the form shows every word; the redaction pass runs before a send; only the user's Ctrl+S sends |
| Prompt fatigue: the model offers a draft after every small failure | Med | Guidance limited to lazarobox limits and errors; one decline per turn ends the asking |
| The y/n answered out of habit | Low | A `y` only opens a form; the send is a second, separate act |
| The form covers a streaming turn or a pending approval | Med | The form opens at turn end; an open form is never overwritten |
| A draft about the user's own project lands in the lazarobox repository | Med | Guidance limited to lazarobox (D5); the user reads the draft before Ctrl+S |
| Context cost of one more schema | Low | One small schema; the toggle turns it off |

## Rollback Plan

A plain revert. Turning the toggle off removes the tool without a revert. Older versions ignore the unknown toggle key. No stored data changes: the pending draft lives in memory and is lost on quit.

## Success Criteria

- [ ] Asked "write that up as a goal", the chat opens the Feedback form on `Idea or goal`, with a title and a description from the conversation, after the user's `y`.
- [ ] Nothing reaches GitHub until the user presses Ctrl+S in the form; Esc sends nothing.
- [ ] `n`, a timeout and every approval mode end as `denied`; no mode answers the prompt.
- [ ] A form already open with the user's text is never replaced.
- [ ] With the toggle off, or on an orchestra worker, no draft tool is offered.
- [ ] On an engine without tools, a failed turn shows the `/feedback` hint.
- [ ] The `github-tools` tests still show no tool that writes to GitHub.

## Evidence

From the worktree at `f4900e12` (v1.10.0):

- **Kind is fixed:** `Feedback::new` sets `kind: Kind::Bug` (`src/core/menu/feedback.rs:513-516`). The labels are `Bug report` and `Idea or goal` (`feedback.rs:141-151`).
- **One string, first-line rule:** `Feedback::prefilled` splits one text into title and description (`feedback.rs:528-541`).
- **Caps:** `TITLE_CAP = 120`, `DESCRIPTION_CAP = 2000` (`feedback.rs:28-30`). A paste over the cap is cut silently (`src/core/ai/chat/composer.rs:95-97`).
- **Target repository:** the form sends to `CARGO_PKG_REPOSITORY`, the lazarobox repository (`feedback.rs:125-136`).
- **Send path to reuse:** redaction before any issue or link, and the similar-issue search before creating (`feedback.rs:8-15`).
- **Opening the form:** `MenuState::open_feedback` (`src/core/menu.rs:1274-1285`), called by `/feedback` (`src/app/chat.rs:491-501`).
- **Denial text:** a denied decision reaches the model as `denied: <reason>` (`src/runtime/tools/host.rs:925-931`).
- **Families and toggles:** `Family` (`src/core/ai/tools/registry.rs:12-24`); `ToolToggle`, all off by default (`src/core/ai/config.rs:194-204`, `629-633`); offered families (`config.rs:641-646`).
- **Workers:** `Toolset::families` gives only `Read`, `Write` or `Orchestra` (`src/core/ai/orchestra/mod.rs:93-100`).
- **Engines with tools:** `MCP_VERIFIED = ["claude", "codex", "gemini", "ollama"]` (`src/core/ai/chat/engine.rs:131`).
- **Prompt guidance:** `with_tools` builds the tools block from the families of the turn (`src/core/ai/chat/prompts.rs:160-193`).
- **Fallback hooks:** `turn_failure` (`src/app.rs:619-631`, used at `3001-3006`); tool `Over` with `Denied` or `Error` (`src/app/tool.rs:112-127`, `src/core/ai/chat/engine.rs:175-180`); the hint line `chat_guidance` (`src/app/chat.rs:470-486`).
- **Pending confirm owns the keys:** `ANSWER_FIRST` (`src/app/github.rs:106-108`, `src/app/run.rs:34`).
- **Owning specs:** `config-menu` Feedback form from a chat (`openspec/specs/config-menu/spec.md:1273`), Feedback form (`:1286`); `chat-pane` Approval prompt (`openspec/specs/chat-pane/spec.md:657`), `/feedback` (`:934`); `agent-tools` Tool registry (`openspec/specs/agent-tools/spec.md:11`), Approval model, with one pending approval and timeouts ending `denied` (`:221-222`); `github-tools` Fixed read, no write (`openspec/specs/github-tools/spec.md:143-144`).

## Decisions

The owner answered each question on 2026-11-09, taking the recommended option every time. The symbols in Evidence were rechecked on `81e0a5a2`; their line numbers have moved a little since `f4900e12`.

- **D1. Toggle default: on,** while the `[ai.tools]` master switch is on. Nothing leaves without a `y` and a Ctrl+S. Cost: one more schema in every tool turn, the golden tool lists change, and `ToolToggle` needs a per-toggle default.
- **D2. Return on `y`, do not wait for Ctrl+S.** The tool returns `ok` once the draft is accepted, and `denied` on `n`. The model never learns whether the issue was sent, or its URL.
- **D3. Open the form at turn end.** It appears a few seconds after the `y`, and the pane holds a pending draft, so a later approval of that turn is not left waiting behind the menu.
- **D4. Reject over-cap fields, do not cut.** An empty field, an unknown kind, or a field over 120 or 2000 characters returns an error, so the model rewrites it.
- **D5. The model offers on its own only for lazarobox limits and errors.** The guidance names three cases: the user asks for an issue; the model cannot do what was asked because lazarobox lacks it; a lazarobox tool or the chat fails. Not for errors in the user's own code or commands.
- **D6. Fallback hint: failed turns and tool `Error`, not `Denied`.** On an engine without the tool, or with the family off, the hint suggests `/feedback` after a failed turn or a tool `Error`.
- **D7. Draft language: English,** the language of the repository's docs and specs. The user can edit the text in the form.
