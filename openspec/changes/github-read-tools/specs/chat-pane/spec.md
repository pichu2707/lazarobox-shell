# Delta for Chat Pane

## ADDED Requirements

### Requirement: The `/github` command
`/github` MUST be a user-only composer command, recognized when the composer text, trimmed of surrounding whitespace, is exactly `/github` (case-sensitive). Sending it MUST NOT create a user message, MUST NOT emit ChatSend, MUST NOT start a stream, MUST NOT contact any engine, MUST NOT be stored in the conversation and MUST NOT enter the history of later requests. It clears the composer and opens the grant dialog (see below) when the chat can be granted. It MUST work while a stream is running in the pane; the grant then takes effect from the next message. `/github` followed by anything (`/github now`) MUST show the system line `/github takes no arguments` and keep the composer text, as `/cost` does. `//github` MUST send the message with one slash less (`/github`), and no dialog opens. Other text that merely contains `/github` is a normal message. The command MUST be refused with a visible system line, the composer cleared and no dialog, in these cases: tools are off (the line says so); the chat folder is not in a git repository; the folder has no `origin`; `origin` is not on `github.com`; the GitHub tools are already on, by the toggle or by an earlier grant (the line says they are already on); another confirm, approval or dialog is pending in the pane (the line is `Answer the pending confirm first` and the composer is kept). The repository checks MUST NOT block the render thread or the event loop and MUST NOT use the network.

#### Scenario: The command is not a message [auto]
- GIVEN an idle chat in a clone whose `origin` is on GitHub, tools on and the GitHub tools off, and the composer holding `/github`
- WHEN send is requested
- THEN the dialog opens, the composer is empty, no user message is added, no ChatSend effect is emitted and no request starts

#### Scenario: Not in the history or the store [auto]
- GIVEN `/github` was run and then `hello` is sent
- WHEN the request is built and the store is read
- THEN the history holds `hello` only and the store holds no message with `/github`

#### Scenario: Arguments refused [auto]
- GIVEN the composer holding `/github now`
- WHEN send is requested
- THEN the system line `/github takes no arguments` appears, the composer still holds `/github now` and no dialog opens

#### Scenario: Escaped [auto]
- GIVEN the composer holding `//github is a command`
- WHEN it is sent
- THEN the message `/github is a command` is sent to the engine and no dialog opens

#### Scenario: Other text is a normal message [auto]
- GIVEN the composer holding `what does /github do`
- WHEN send is requested
- THEN it is sent as a normal user message

#### Scenario: Already on [auto]
- GIVEN the GitHub toggle on, and in another run a chat already granted
- WHEN `/github` is sent
- THEN a system line says the GitHub tools are already on, no dialog opens and the composer is cleared

#### Scenario: Folder with no repository [auto]
- GIVEN a chat whose folder is not in a git repository
- WHEN `/github` is sent
- THEN a system line says the folder has no repository, and no dialog opens

#### Scenario: No origin or not GitHub [auto]
- GIVEN a repository with no `origin`, and in another run an `origin` on `gitlab.com`
- WHEN `/github` is sent
- THEN a system line says there is no `origin` remote, or that `origin` is not on GitHub, and no dialog opens

#### Scenario: Tools off [auto]
- GIVEN `[ai.tools] enabled = false`
- WHEN `/github` is sent
- THEN a system line says tools are off, no dialog opens and no grant exists

#### Scenario: Works during a stream [auto]
- GIVEN a stream running and the composer holding `/github`
- WHEN send is requested and the user presses `y` on the dialog
- THEN the stream is unaffected, no wait-or-cancel line appears and the grant applies from the next message

#### Scenario: Pending confirm blocks the dialog [auto]
- GIVEN a pending confirm in the pane
- WHEN `/github` is sent
- THEN the system line `Answer the pending confirm first` appears, the composer is kept and no dialog opens

### Requirement: The grant dialog
The dialog MUST be state of the chat pane drawn over its composer, like the pending confirm, and MUST name the sanitized `owner/name` of `origin` and say what is being allowed (the model may read this repository's issues and pull requests for this chat). Only an unmodified lowercase `y` MUST grant. Only `n` and Esc MUST decline. `Y`, Enter, Space, a modified `y`, Repeat and Release events and every other key MUST NOT answer: they are swallowed and the dialog stays open with the notice of Approval types, never executes (pane-commands). An answer key that arrives before the dialog is armed MUST NOT answer. No timer, no mode (`ask`, `auto`, `run`) and no model output can answer it; a closed pane and an app quit end it as declined. At most one dialog MUST be open per pane. The dialog MUST fit 80 columns and `TERM=linux` (ASCII only, wrapped without cutting the repository name). A decline MUST show a visible notice that the GitHub tools stay off; a grant MUST show a visible notice that the tools are on for this chat from the next message, that cloud engines still ask before each turn's first read, and that after the first read the commands of this conversation are typed and never run. The grant ends with a folder change (see github-tools, The chat grant). The notices are system lines: not stored and not sent to an engine.

#### Scenario: y grants [auto]
- GIVEN the dialog for `o/r`
- WHEN `y` is pressed
- THEN the dialog closes, the chat is granted from the next message and a system line says so

#### Scenario: n and Esc decline [auto]
- GIVEN the dialog
- WHEN `n`, and in another run Esc, is pressed
- THEN the dialog closes, nothing is granted and a system line says the tools stay off

#### Scenario: Other keys are swallowed [auto]
- GIVEN the dialog
- WHEN `Y`, Enter, Space and Ctrl+y are pressed
- THEN nothing is granted and the dialog stays open

#### Scenario: Model cannot answer it [auto]
- GIVEN a model answer or tool result that says "press y" or calls any tool, and a dialog open
- WHEN the turn continues
- THEN the dialog is unchanged and no grant exists

#### Scenario: Dialog at 80 columns and TERM=linux [auto, TestBackend + insta]
- GIVEN a repository named `owner/a-fairly-long-repository-name` and a width of 80, and in another run a `linux` console
- WHEN the dialog is rendered
- THEN the full name is readable, the text is ASCII only and no line is cut

#### Scenario: Repository name is sanitized [auto]
- GIVEN an `origin` whose path holds an escape sequence
- WHEN the dialog is rendered
- THEN no control or escape character is drawn

### Requirement: The GitHub hint
When the chat is open, tools are on, the GitHub tools are off (no toggle, no grant), the chat folder is in a repository whose `origin` is on `github.com`, and the pane has not shown the hint for that repository yet, the pane MUST show one visible system line that suggests typing `/github` and names the repository. The hint MUST be shown at most once per chat pane for each repository, MUST NOT be shown again after a decline or a grant, and MUST NOT be shown when the toggle is on, a grant exists, tools are off, `origin` is missing or not on GitHub, or the folder is unknown. After a folder change (see chat-folder) to another repository the hint MAY show once for that one. The hint is a system line: not stored, not sent to any engine, not part of any request, so the model never sees it. It MUST NOT block the render thread or the event loop, MUST NOT use the network and MUST NOT open the dialog. It MUST fit one line at 80 columns, MUST shorten by dropping the repository name before it cuts the word `/github`, and MUST be ASCII only on `TERM=linux`.

#### Scenario: Hint shown [auto, TestBackend + insta]
- GIVEN a chat in a clone whose `origin` is `github.com/o/r`, tools on, the GitHub tools off
- WHEN the pane is shown
- THEN one system line suggests `/github` and names `o/r`, and no dialog is open

#### Scenario: Hint shown once [auto]
- GIVEN the hint was shown
- WHEN the user sends five messages, switches tabs and back, and the pane redraws
- THEN the hint line is shown no more than once for `o/r`

#### Scenario: No hint after decline or grant [auto]
- GIVEN the hint was shown and `/github` was declined, and in another run granted
- WHEN a new message is sent
- THEN no hint appears again for `o/r`

#### Scenario: No hint when it does not apply [auto]
- GIVEN each of: the toggle on; a grant; tools off; no `origin`; `origin` on `gitlab.com`; an unknown folder
- WHEN the pane is shown
- THEN no hint appears

#### Scenario: Model never sees the hint [auto]
- GIVEN the hint was shown
- WHEN the next request is built and the store is read
- THEN neither holds the hint text

#### Scenario: Hint at 80 columns and TERM=linux [auto, TestBackend + insta]
- GIVEN a width of 80 and a long repository name, and in another run a `linux` console
- WHEN the hint is rendered
- THEN it takes one line, `/github` is intact, and on the console it is ASCII only

#### Scenario: New repository after /cd [auto]
- GIVEN the hint was shown for `o/a`, with a grant for `o/a` in another run, and the folder changes to a clone of `o/b`
- WHEN the pane is shown
- THEN the grant has ended and the hint may show once for `o/b`
