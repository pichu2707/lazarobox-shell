# Delta for Configuration

Tags as in the main spec.

## ADDED Requirements

### Requirement: Usage keys
`[ai.usage]` MAY hold:
- `line` (boolean, default `true`): the per-answer usage line;
- `statusline` (boolean, default `true`): the status-bar usage segment;
- `turn_tokens` (whole number greater than 0): per-turn token threshold;
- `day_tokens` (whole number greater than 0): per-day token threshold;
- `day_usd` (number greater than 0, a TOML float or integer): per-day dollar threshold.
A missing threshold key means that threshold is off. `[ai.usage.billing]` MAY hold `claude`, `codex` and `gemini`, each the string `subscription` or `per-token`; a missing key is `subscription`. Any other key under `[ai.usage.billing]` (including local engine ids) and any unknown key under `[ai.usage]` MUST be ignored. A wrong value MUST fall back to that key's default (thresholds: off) and produce one problem under the startup-notice rules, in this order after the `ai.notify` problem: `ai.usage.line: expected true or false`, `ai.usage.statusline: expected true or false`, `ai.usage.turn_tokens: expected a whole number above 0`, `ai.usage.day_tokens: expected a whole number above 0`, `ai.usage.day_usd: expected a number above 0`, `ai.usage.billing.claude: expected "subscription" or "per-token"` (same for `codex`, `gemini`). `ai.usage` holding a non-table value falls back to all defaults with the problem `ai.usage: expected a table`. Values are case-sensitive. The keys are independent of every other `[ai]` key.

#### Scenario: Defaults [auto]
- GIVEN no `[ai.usage]` table
- WHEN the config is read
- THEN line and statusline are on, all thresholds are off, billing is `subscription` for all three and there is no problem

#### Scenario: Explicit values [auto]
- GIVEN `[ai.usage]\nline = false\nturn_tokens = 4000\nday_usd = 5\n[ai.usage.billing]\nclaude = "per-token"`
- WHEN the config is read
- THEN line is off, turn limit 4000, day dollars 5.0, claude `per-token`, codex and gemini `subscription`, and no problem

#### Scenario: Wrong types and ranges [auto]
- GIVEN `turn_tokens = 0`, `day_tokens = -3`, `day_usd = "5"`, `line = "yes"`
- WHEN the config is read
- THEN each falls back (thresholds off, line on) and one problem per key is reported with the exact texts above

#### Scenario: Wrong billing value [auto]
- GIVEN `[ai.usage.billing]\ncodex = "Subscription"`
- WHEN the config is read
- THEN codex is `subscription` and the problem is `ai.usage.billing.codex: expected "subscription" or "per-token"`

#### Scenario: Local engines ignored [auto]
- GIVEN `[ai.usage.billing]\nollama = "per-token"`
- WHEN the config is read
- THEN there is no problem and ollama still never shows dollars

#### Scenario: Not a table [auto]
- GIVEN `[ai]\nusage = 3`
- WHEN the config is read
- THEN all defaults apply and the problem is `ai.usage: expected a table`

#### Scenario: Fractional whole number [auto]
- GIVEN `turn_tokens = 1.5`
- WHEN the config is read
- THEN the threshold is off and the problem is `ai.usage.turn_tokens: expected a whole number above 0`
