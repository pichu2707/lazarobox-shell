# Delta for Configuration (chat-runs-commands)

## MODIFIED Requirements

### Requirement: Approval mode key
The table `[ai.tools]` MAY hold `approval`, the string `"ask"` or `"run"` (default `"ask"`, compared exactly, lowercase). It is the approval mode a chat pane takes when it opens and at each conversation switch inside it (see agent-tools, Chat approval mode); the current conversation of an open pane keeps its mode. The value `"auto"` was removed: a file holding `approval = "auto"` MUST load with the mode `ask`, and lazarobox MUST rewrite that one value to `"ask"` in the file once, on load, through the existing safe config-edit path (`config_edit::apply_edit`, atomic write, every other byte and comment kept; owner decision 2026-11-22). When the rewrite succeeds it MUST show one visible notice, `config: ai.tools.approval: "auto" was removed; the file now says "ask"`, and the notice MUST NOT repeat at later starts because the file no longer holds `auto`. When the rewrite fails it MUST show the notice `config: ai.tools.approval: "auto" was removed; using "ask" (the file was not changed: <reason>)` and treat the mode as `ask`; that notice repeats at each start while the file holds `auto`. No other value is ever rewritten on load. Any other value or type (including `"RUN"`) falls back to `"ask"` for that key only and reports `ai.tools.approval: expected "ask" or "run"`, after the `ai.tools.enabled` problem in the notice order. A non-table `ai.tools` uses the default with the existing `ai.tools: expected a table` problem only. The key is read at startup and when changed from the config menu. The default is never `run`.
(Previously: `"auto"` was a valid mode; the bad-value notice read `expected "ask", "auto" or "run"`.)

#### Scenario: Default ask [auto]
- GIVEN a file with no `approval` under `[ai.tools]`, and a file with no `[ai.tools]`
- WHEN each is parsed
- THEN the approval mode is `ask` and no notice is produced

#### Scenario: Parsing auto reads ask and touches nothing [auto]
- GIVEN `[ai.tools]` with `approval = "auto"`
- WHEN the pure parser reads it
- THEN the approval mode is `ask`, the problem is `ai.tools.approval: "auto" was removed; using "ask"`, and no file is written by the parser

#### Scenario: Load rewrites auto to ask once [auto]
- GIVEN a file with `approval = "auto"`, a comment and other keys
- WHEN the app loads it, and then starts a second time
- THEN the first load changes only that value to `"ask"` (all other bytes identical), shows `config: ai.tools.approval: "auto" was removed; the file now says "ask"` once, and the second start shows no notice

#### Scenario: Rewrite failure is visible [auto]
- GIVEN a file with `approval = "auto"` that cannot be written (read-only)
- WHEN the app loads it, and then starts again
- THEN the mode is `ask`, the notice is `config: ai.tools.approval: "auto" was removed; using "ask" (the file was not changed: <reason>)` at each start, and the file bytes are unchanged

#### Scenario: Run [auto]
- GIVEN `[ai.tools]` with `approval = "run"`
- WHEN it is parsed
- THEN the approval mode is `run` and no notice is produced

#### Scenario: Bad value [auto]
- GIVEN `approval = "AUTO"`, `approval = "Run"`, `approval = "yes"` and `approval = true` in separate files
- WHEN the app starts
- THEN the mode is `ask` each time and the notice is `config: ai.tools.approval: expected "ask" or "run"`

#### Scenario: Notice order [auto]
- GIVEN `enabled = 1` and `approval = 2` under `[ai.tools]`
- WHEN the app starts
- THEN the notice is `config (2 problems): ai.tools.enabled: expected a boolean`, tools are on and the mode is `ask`

#### Scenario: Auto with another problem [auto]
- GIVEN `enabled = 1` and `approval = "auto"` under `[ai.tools]`
- WHEN the app starts
- THEN the notice is `config (2 problems): ai.tools.enabled: expected a boolean`, and the problem list also names `ai.tools.approval: "auto" was removed; using "ask"` (the file was not changed while another problem is reported: the rewrite runs only when the file has no other problem), tools are on and the mode is `ask`

#### Scenario: Older build reads run as a bad value [manual]
- GIVEN a file saved with `approval = "run"` and a build without this change
- WHEN it starts
- THEN the mode is `ask` and the notice names `ai.tools.approval`

#### Scenario: Newer build reads an old auto file [manual]
- GIVEN a file saved with `approval = "auto"` by a build before this change
- WHEN this build starts
- THEN the chat works in `ask`, the file now holds `approval = "ask"`, and the user sees the removal notice once
