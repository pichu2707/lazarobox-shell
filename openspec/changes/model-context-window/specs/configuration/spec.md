# Delta for Configuration

## ADDED Requirements

### Requirement: Ollama context cap and keep-alive
`[ai.servers.ollama] num_ctx_cap` MUST be read as a whole number of tokens from 1024 to 1048576; a missing key is 16384. `[ai.servers.ollama] keep_alive_minutes` MUST be read as a whole number of minutes from 0 to 10080 (7 days); a missing key is 30. `0` is valid and means Ollama unloads the model right after each reply. Any other value or type (negative, a cap below 1024 or above 1048576, keep-alive above 10080, float, string) falls back to that key's default and reports `ai.servers.ollama.num_ctx_cap: expected a whole number of tokens from 1024 to 1048576` or `ai.servers.ollama.keep_alive_minutes: expected a whole number of minutes from 0 to 10080`, through the startup notice rules of Fallback and startup notice; the other keys are respected. A fallback MUST NEVER be silent. The menu MUST NOT write either key, and saving other keys keeps them byte for byte. Old configs without them, and old builds reading them, are unaffected.

#### Scenario: Defaults [auto]
- GIVEN a file without the keys
- WHEN it is parsed
- THEN the cap is 16384, keep-alive is 30 and no notice is produced

#### Scenario: Valid override [auto]
- GIVEN `num_ctx_cap = 8192` and `keep_alive_minutes = 5`
- WHEN it is parsed
- THEN the cap is 8192, keep-alive is 5 and no notice is produced

#### Scenario: Bad cap falls back with a notice [auto]
- GIVEN `num_ctx_cap = "big"`, or `0`, `-1`, `1023` or `1048577`
- WHEN the app starts
- THEN the cap is 16384 and the notice is `config: ai.servers.ollama.num_ctx_cap: expected a whole number of tokens from 1024 to 1048576`

#### Scenario: Bad keep-alive falls back per key [auto]
- GIVEN `keep_alive_minutes = 1.5` (or `10081`) and `num_ctx_cap = 8192`
- WHEN the app starts
- THEN keep-alive is 30, the cap is 8192 and the notice names `keep_alive_minutes`

#### Scenario: Menu never writes them [auto]
- GIVEN a file with both keys set
- WHEN the menu saves other keys
- THEN both keys are unchanged byte for byte, and a file without them gains none
