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
- `←/→ kind · Tab next · Ctrl+S link · Esc back` on the kind;
- `Tab next · Ctrl+S link · Esc back` on a text field;
- `Space attach · Tab next · Ctrl+S link · Esc back` on the attach line.

#### Scenario: Ctrl+S copies the link [auto]
- GIVEN a bug report titled `Scroll jumps`, described `It jumps\nback`, with the environment attached
- WHEN Ctrl+S is pressed
- THEN the menu copies `https://github.com/pichu2707/lazarobox-shell/issues/new?template=bug.yml&title=Scroll%20jumps&what-happened=It%20jumps%0Aback&environment=<the facts>`, percent-encoded
- AND the view reads `Link copied. Paste it in your browser:`, `GitHub opens the issue form filled in, for you`, `to review and send.`, with the footer `Copied <n> characters`, then `Esc back`
- AND Esc goes back to the row

#### Scenario: An idea, without the environment [auto]
- GIVEN an idea with the attach line unchecked
- WHEN Ctrl+S is pressed
- THEN the link uses `template=idea.yml`, puts the description in `idea=` and has no `environment=`

#### Scenario: Required fields [auto]
- WHEN Ctrl+S is pressed with an empty title (or an empty description)
- THEN no link is made, the footer error reads `Title is required` (or `Description is required`), and the cursor moves to that field

#### Scenario: Secrets are masked first [auto]
- GIVEN a description holding `token=hf_…`
- WHEN Ctrl+S is pressed
- THEN no link is made, the field now reads `[REDACTED]` in its place, and the footer error reads `Secrets were masked; review and press Ctrl+S again`
- AND the next Ctrl+S copies a link without the secret

#### Scenario: Too long [auto]
- WHEN the link would be longer than 8000 bytes
- THEN no link is made, and the footer error reads `Too long for a link: shorten the description`

### Requirement: No network
The form MUST NOT send anything. It only builds the link and copies it like the other menu copies.
