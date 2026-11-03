# Delta for Configuration

## ADDED Requirements

### Requirement: Ollama context cap
`[ai.servers.ollama] num_ctx_cap` MUST be read as a whole number of tokens from 1024 to 1048576; a missing key is 16384. Any other value or type (negative, a cap below 1024 or above 1048576, float, string) falls back to 16384 and reports `ai.servers.ollama.num_ctx_cap: expected a whole number of tokens from 1024 to 1048576`, through the startup notice rules of Fallback and startup notice; the other keys are respected. A fallback MUST NEVER be silent. The menu MUST NOT write the key, and saving other keys keeps it byte for byte. Old configs without it, and old builds reading it, are unaffected. There is no separate keep-alive key in this change: the keep-alive is the released `[ai.servers.ollama] keep_alive` (see Prompt-cache options of the local servers).

#### Scenario: Default [auto]
- GIVEN a file without the key
- WHEN it is parsed
- THEN the cap is 16384 and no notice is produced

#### Scenario: Valid override [auto]
- GIVEN `num_ctx_cap = 8192`
- WHEN it is parsed
- THEN the cap is 8192 and no notice is produced

#### Scenario: Bad cap falls back with a notice [auto]
- GIVEN `num_ctx_cap = "big"`, or `0`, `-1`, `1023` or `1048577`
- WHEN the app starts
- THEN the cap is 16384 and the notice is `config: ai.servers.ollama.num_ctx_cap: expected a whole number of tokens from 1024 to 1048576`

#### Scenario: Menu never writes them [auto]
- GIVEN a file with `num_ctx_cap` set
- WHEN the menu saves other keys
- THEN the key is unchanged byte for byte, and a file without it gains none

## MODIFIED Requirements

### Requirement: Prompt-cache options of the local servers
`[ai.servers.ollama] keep_alive` and `[ai.servers.llamacpp] cache_prompt` MAY be set (#515). `keep_alive` MUST be a Go duration as Ollama reads it (an optional sign, then numbers each with a unit `ns`, `us`, `µs`, `ms`, `s`, `m` or `h`, or a bare `0`) or a TOML integer of seconds; `cache_prompt` MUST be a boolean. Any other value is left out with the problem `ai.servers.ollama.keep_alive: expected a duration such as "30m" or a whole number of seconds` or `ai.servers.llamacpp.cache_prompt: expected true or false`. Unset, `cache_prompt` sends nothing, and an unset `keep_alive` sends `"30m"` to Ollama (Ollama's own default is five minutes). Set, every request to that server (chat, summary, compaction, tool rounds, orchestrator workers) MUST carry `keep_alive` as written (the text; an integer `n` as `"<n>s"`) or `cache_prompt`, and no other server MUST receive either. Both are read at startup; the menu MUST keep them when it saves the table.
(Previously: unset, nothing was sent to either server.)

#### Scenario: Unset keep-alive sends 30m to Ollama [auto]
- GIVEN no `keep_alive` and no `cache_prompt`
- WHEN a turn is sent to Ollama
- THEN its body holds `model`, `messages`, `stream` and `"keep_alive": "30m"`, and no other server's body changes

#### Scenario: Explicit keep-alive is sent as written [auto]
- GIVEN `keep_alive = "5m"` (or `0`, `-1`, `"1h"`)
- WHEN a turn is sent to Ollama
- THEN its body holds that value in place of `"30m"`

#### Scenario: Each to its server [auto]
- GIVEN `keep_alive = "30m"` and `cache_prompt = true`
- WHEN turns go to Ollama, llama.cpp, LM Studio and vLLM
- THEN Ollama's body holds `"keep_alive": "30m"`, llama.cpp's `"cache_prompt": true`, and the others neither
