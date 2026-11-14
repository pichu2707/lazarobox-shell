# Delta for agent-tools

Change `chat-write-grant`. Base: `openspec/specs/agent-tools/spec.md`, Requirements `Approval model` and `Chat approval mode`.

## MODIFIED Requirements

### Requirement: Approval model
(Everything in the base requirement stays as written, except the sentence below, which replaces the matching one.)

There MUST be no "always allow", no per-session or per-tool trust, no configuration that approves automatically, and nothing MUST ever execute a command for the user without a `y`, with one exception: the write grant of a chat in `run` mode (see Chat approval mode and code-write), which only the user gives, with a key in a batch review, and which covers only the batches of that chat pane's current conversation.

#### Scenario: The grant covers no other tool [auto]
- GIVEN a chat in `run` mode holding the write grant
- WHEN the model calls `run_command`, `git_commit`, `gh_issue_draft` and a user MCP tool
- THEN each opens its own approval and nothing runs until `y`

### Requirement: Chat approval mode
(Everything in the base requirement stays as written; the paragraph below is added.)

**Write grant.** Only in `run`, a chat MUST be able to hold a write grant, given by the user with `a` in the review of one of its own batches (see code-write, Batch review and coverage gate). While it is held, the chat's later batches apply without a review (see code-write). The grant MUST NOT be stored, sent to any engine, or given by any tool, tool result, MCP result, pane output or model text. It MUST end when the user starts or switches to another conversation in that pane, resumes a saved conversation, toggles the mode out of `run`, changes the chat's folder (`/cd`), turns tools or the Write family off, closes the pane, or quits; a pending review is not answered by its end. While it is held and tools are on, the marker MUST read ` RUN+W ` in red instead of ` RUN `. The grant belongs to the pane that gave it (its own `ChatState`): another chat pane MUST NOT inherit it, and a batch of another pane MUST be reviewed as without the grant.

#### Scenario: A new conversation asks again [auto]
- GIVEN a chat in `run` mode holding the write grant
- WHEN the user starts a new conversation in the pane, and in another run resumes a saved one, and the model proposes a batch
- THEN the review opens and waits for `y` or `a`

#### Scenario: Leaving run ends the grant [auto]
- GIVEN a chat in `run` mode holding the write grant
- WHEN the user toggles the mode to `ask` and back to `run`, and the model proposes a batch
- THEN the review opens and the marker reads ` RUN `

#### Scenario: Another pane does not inherit the grant [auto]
- GIVEN two chat panes in `run` mode and the write grant held by the first
- WHEN the model of the second pane proposes a batch
- THEN the review opens in the second pane, its marker reads ` RUN `, and nothing is written until the user's key

#### Scenario: The model cannot give the grant [auto]
- GIVEN a chat in `run` mode without the grant
- WHEN a tool result or model text asks for it, and the model proposes a batch
- THEN the review opens and nothing is written until the user's key
