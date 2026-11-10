# Config Menu Specification (delta: Feedback)

## ADDED Requirements

### Requirement: Feedback row
The Settings tab MUST end with a row `Feedback` whose value is `bug report or idea`, after `Updates`. Its footer reads `Enter open · j/k move · s save · Esc cancel`. Enter opens the Feedback form, which replaces the rows like the other sub-views (the tab keys are inert there). The row writes no config edit.

#### Scenario: The row is last [auto]
- GIVEN the menu without the AI section
- WHEN its rows are built
- THEN the last row is `Feedback  bug report or idea`

### Requirement: Feedback form
The form shows, under the title `Feedback`:
- `Kind: Bug report` or `Kind: Idea or goal`;
- `Title` and one line of the title;
- `Description` and up to six lines of the description;
- `[x] Attach the environment` (or `[ ]`), and while it is checked, the facts.

The focused field is marked `›`, and the focused text field draws its cursor as `▏`.

The facts are two lines:
- `lazarobox <version> · <os>[ (WSL)] · <shell> · <terminal>`;
- `engines: <id> [<version>] <available|not running> · …`, or `engines: not detected`.

They are wrapped between items, so no fact is cut at 48 columns. An engine that is not installed is left out. A shell, terminal or version that is not printable ASCII is left out.

Keys:
- Tab and BackTab move between the fields.
- ←/→ change the kind.
- Space or Enter on the attach line toggles it.
- Enter in the title goes to the description, and Enter in the description adds a line.
- A paste goes to the focused text field; in the title, its newlines become spaces.
- Esc goes back to the row.

The footer reads:
- `←/→ kind · Tab · Ctrl+S send · Ctrl+O open · Esc` on the kind;
- `Tab next · Ctrl+S send · Ctrl+O open · Esc back` on a text field;
- `Space attach · Ctrl+S send · Ctrl+O open · Esc` on the attach line.

Ctrl+S sends the issue with the user's own `gh` (see Requirement: Sending with gh). Ctrl+O builds the prefilled new-issue link and opens it in the browser through the host's opener; the view then reads `Opened in your browser. GitHub shows the issue`, `form filled in, for you to review and send.` until Esc. On a host with no opener Ctrl+O copies the link instead, and the view reads `Link copied. Paste it in your browser:`, `GitHub opens the issue form filled in, for you`, `to review and send.`, with the footer `Copied <n> characters`, then `Esc back`. When the opener fails (it exits with an error within 5 seconds of its start, for example over SSH with no browser; a later exit is the browser's own), the link is copied instead, the view reads the copied text, the footer says `The browser did not open: link copied`, and later links are copied.

Both keys check the form first: the title and the description are required, and both go through the redaction pass. The link of Ctrl+O is capped at 8000 bytes; the body Ctrl+S sends is capped at GitHub's 65536 characters.

#### Scenario: Ctrl+S sends the issue [auto]
- GIVEN a bug report titled `Scroll jumps`, described `It jumps\nback`, with the environment attached
- WHEN Ctrl+S is pressed
- THEN the runtime runs `gh issue create --repo pichu2707/lazarobox-shell --title "Scroll jumps" --body-file - --label bug`, with the body on stdin
- AND the body reads `### What happened`, a blank line, `It jumps\nback`, a blank line, `### Environment`, a blank line, and the facts in a `text` code block
- AND the footer reads `Sending to GitHub…` until the answer

#### Scenario: Ctrl+O opens the prefilled page [auto]
- GIVEN the bug report above, on a host with an opener
- WHEN Ctrl+O is pressed
- THEN `https://github.com/pichu2707/lazarobox-shell/issues/new?template=bug.yml&title=Scroll%20jumps&what-happened=It%20jumps%0Aback&environment=<the facts>`, percent-encoded, is opened in the browser, and the view reads `Opened in your browser. GitHub shows the issue`, `form filled in, for you to review and send.`
- AND Esc goes back to the row

#### Scenario: Ctrl+O without an opener copies the link [auto]
- GIVEN the bug report above, on a host with no opener
- WHEN Ctrl+O is pressed
- THEN the link is copied, and the view reads `Link copied. Paste it in your browser:`, `GitHub opens the issue form filled in, for you`, `to review and send.`, with the footer `Copied <n> characters`, then `Esc back`
- AND Esc goes back to the row

#### Scenario: An idea, without the environment [auto]
- GIVEN an idea with the attach line unchecked
- WHEN Ctrl+S is pressed
- THEN the issue is labelled `idea` and its body is only `### The idea`, a blank line and the description
- AND Ctrl+O uses `template=idea.yml`, puts the description in `idea=` and has no `environment=`

#### Scenario: Required fields [auto]
- WHEN Ctrl+S is pressed with an empty title (or an empty description)
- THEN nothing is sent, the footer error reads `Title is required` (or `Description is required`), and the cursor moves to that field

#### Scenario: Secrets are masked first [auto]
- GIVEN a description holding `token=hf_…`
- WHEN Ctrl+S is pressed
- THEN nothing is sent, the field now reads `[REDACTED]` in its place, and the footer error reads `Secrets were masked; review and press Ctrl+S again`
- AND the next Ctrl+S sends the issue without the secret

#### Scenario: Too long for a link [auto]
- WHEN the link of Ctrl+O would be longer than 8000 bytes
- THEN no link is made, and the footer error reads `Too long for a link: shorten the description`
- AND Ctrl+S still sends the same text

### Requirement: Sending with gh
The app MUST send only on the user's Ctrl+S, and only through the user's own `gh`: lazarobox stores no GitHub token. `gh` is found on the login PATH the engines use and run with no shell, as `gh issue create --repo <owner/repo> --title <title> --body-file - --label <bug|idea>`, the body on stdin, stdout and stderr captured, the environment inherited. It runs off the UI thread and is killed 30 seconds after Ctrl+S, the retry without labels included. One send at a time: while it runs, every key of the form, Ctrl+S and Esc included, does nothing.

The issue keeps the shape of the repository's issue forms: one `### <label>` section per filled field (`What happened` for a bug report, `The idea` for an idea), and `### Environment` with the facts when attached. The label `bug` or `idea` is asked for; GitHub keeps it only for users with write access, so a failure naming a label or a permission is tried once more without the label. Project 17 is not set from the app.

`gh` is taken as missing when it is not on PATH, and as not logged in when it exits with code 4 or its message names `gh auth login`.

#### Scenario: Sent [auto]
- GIVEN Ctrl+S was pressed
- WHEN `gh` prints one line `https://github.com/pichu2707/lazarobox-shell/issues/<n>`
- THEN the view reads `Sent:` and that link on a row of its own, with the footer `Ctrl+O open · Esc back` (`Ctrl+O copy link · Esc back` with no opener)
- AND Ctrl+O, or a left click on the link row, opens it
- AND Esc goes back to the row
- AND any other output counts as a failure

#### Scenario: gh fails [auto]
- WHEN `gh` fails or times out
- THEN the form stays with everything typed, and the footer error reads `gh: <its message>` on one line, control characters removed, cut to 48 columns
- AND Ctrl+S sends again

#### Scenario: No gh [auto]
- WHEN `gh` is missing or not logged in
- THEN the prefilled page opens in the browser, as Ctrl+O would, and the view reads `gh is not installed or not logged in:`, `opened the form in your browser, for you to`, `review and send.`
- AND on a host with no opener the link is copied, and the view reads `gh is not installed or not logged in: link`, `copied. Paste it in your browser to review`, `and send the issue.`
- AND when the link would be too long, the form stays with the footer error `No gh, and too long for a link: shorten it`
