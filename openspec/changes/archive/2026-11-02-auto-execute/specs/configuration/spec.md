# Delta for Configuration

Applies on top of the unarchived `agent-tools` delta for configuration. Tags as in the main spec.

## MODIFIED Requirements

### Requirement: Approval mode key
The table `[ai.tools]` MAY hold `approval`, the string `"ask"`, `"auto"` or `"run"` (default `"ask"`, compared exactly, lowercase). It is the approval mode a chat pane takes when it opens and at each conversation switch inside it (see agent-tools, Chat approval mode); the current conversation of an open pane keeps its mode. Any other value or type (including `"RUN"`) falls back to `"ask"` for that key only and reports `ai.tools.approval: expected "ask", "auto" or "run"`, after the `ai.tools.enabled` problem in the notice order. A non-table `ai.tools` uses the default with the existing `ai.tools: expected a table` problem only. The key is read at startup and when changed from the config menu. The default is never `run`.
(Previously: accepted `"ask"` or `"auto"`; the problem text read `expected "ask" or "auto"`)

#### Scenario: Default ask [auto]
- GIVEN a file with no `approval` under `[ai.tools]`, and a file with no `[ai.tools]`
- WHEN each is parsed
- THEN the approval mode is `ask` and no notice is produced

#### Scenario: Auto [auto]
- GIVEN `[ai.tools]` with `approval = "auto"`
- WHEN it is parsed
- THEN the approval mode is `auto` and no notice is produced

#### Scenario: Run [auto]
- GIVEN `[ai.tools]` with `approval = "run"`
- WHEN it is parsed
- THEN the approval mode is `run` and no notice is produced

#### Scenario: Bad value [auto]
- GIVEN `approval = "AUTO"`, `approval = "Run"`, `approval = "yes"` and `approval = true` in separate files
- WHEN the app starts
- THEN the mode is `ask` each time and the notice is `config: ai.tools.approval: expected "ask", "auto" or "run"`

#### Scenario: Notice order [auto]
- GIVEN `enabled = 1` and `approval = 2` under `[ai.tools]`
- WHEN the app starts
- THEN the notice is `config (2 problems): ai.tools.enabled: expected a boolean`, tools are on and the mode is `ask`

#### Scenario: Older build reads run as a bad value [manual]
- GIVEN a file saved with `approval = "run"` and a build without this change
- WHEN it starts
- THEN the mode is `ask` and the notice names `ai.tools.approval`
