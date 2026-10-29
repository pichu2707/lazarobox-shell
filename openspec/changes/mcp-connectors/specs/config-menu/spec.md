# Config Menu Specification (delta: MCP connectors)

All existing requirements stay as they are. The AI tab's last row is `Hugging Face` (hf-download); this delta adds one row after it. The rule that the menu never writes `ai.mcp.*` stays (config-persistence, C4). Traceability: C1, C2 (a menu wizard), C4 (the menu writes no `ai.mcp.*`), C6 (the menu writes no `secrets.toml`), proposal scope item 8. Slice tag: [S4].

## ADDED Requirements

### Requirement: Connectors row
The AI tab MUST end with a static row `Connectors`, after the row that was last until now (`Hugging Face`). Its value is the number of catalog entries that are declared and enabled in `config.toml` (`<N> declared`), or `none` when there are none (see mcp-connect, Entry list). Enter on the row opens the Connectors area (see mcp-connect); Esc there comes back to the row. The row is not a draft value: it writes no config edit, `s` saves only the other rows' drafts, and the row's value never changes on Esc of the menu.

#### Scenario: Last AI row [auto, TestBackend + insta]
- GIVEN the AI tab
- WHEN rendered with no declared catalog server
- THEN the last two rows are `Hugging Face` and `Connectors` with the value `none`

#### Scenario: Count of declared entries [auto]
- GIVEN declared `clarity` and `ga4`, an unrelated declared server `fs`, and `gsc` declared with `enabled = false`
- WHEN the row is built
- THEN its value is `2 declared`

#### Scenario: Opens the area [auto]
- GIVEN the `Connectors` row selected
- WHEN Enter is pressed, then Esc
- THEN the Connectors area opens, and Esc returns to the row with the menu draft unchanged

#### Scenario: No config edit [auto]
- GIVEN a file with `[[ai.mcp.servers]]` entries and a changed position draft
- WHEN the Connectors area is visited and `s` is pressed
- THEN only the position changes and every `ai.mcp.*` byte is identical
