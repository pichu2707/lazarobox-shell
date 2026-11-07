# Delta for AI Engine Detection

## ADDED Requirements

### Requirement: Ollama context window from the existing show probe
Detection MUST take each Ollama model's max context window from the SAME `POST /api/show` reply that Ollama model size and tool capability already reads: the positive whole `model_info.<architecture>.context_length`. It MUST NOT make any additional call. The probe's per-call timeout, body cap, parallel and total call limits, and fixed total bound apply unchanged, and the window shares the digest cache: an unchanged digest is not probed again and keeps its window, a new digest is probed and gets its window re-read. A failed probe (timeout, connection error, non-200, oversized or malformed body, missing, zero or non-integer `context_length`, call limit reached) MUST leave that model's window unknown, never zero and never guessed, MUST NOT fail the run or change the engine status or model list, and MUST be reported when the model is used (see chat-engines, Context window). The llama.cpp window stays the `n_ctx` of `GET /props`, unchanged. Windows live in RAM only: no file, no table, and chat.db stays at schema `user_version` 3, mode 0600, untouched.

#### Scenario: Window read from the same reply [auto]
- GIVEN a fake Ollama listing `llama3:8b` whose `/api/show` reply has `capabilities` `["completion","tools"]` and `model_info` `{"llama.context_length":8192}`
- WHEN a detection run completes
- THEN the model has capability `tools` and window 8192, and exactly one `/api/show` was recorded for it

#### Scenario: Bad reply leaves the window unknown [auto]
- GIVEN `/api/show` answers 500, a malformed body, a body above the cap, no `context_length`, or `context_length` 0
- WHEN a detection run completes
- THEN that model's window is unknown, the engine is still `available · N models`, and other models keep theirs

#### Scenario: Digest cache keeps and refreshes windows [auto]
- GIVEN a completed run and a next run where one model keeps its digest and another has a new digest and a new `context_length`
- WHEN the next run completes
- THEN no `/api/show` is recorded for the unchanged digest, which keeps its window, and the new digest is probed and its window updated

#### Scenario: Call limit leaves the rest unknown [auto]
- GIVEN a fake Ollama listing more models than the total call limit
- WHEN a detection run completes
- THEN no more `/api/show` calls than the limit were recorded and the remaining models have an unknown window

#### Scenario: A failed probe keeps the last known window [auto]
- GIVEN a model whose window was read on an earlier run
- WHEN a later run cannot read it (failed or slow probe)
- THEN the earlier window is kept, so `num_ctx` does not change and the model is not reloaded; a new answer replaces it, and a model no longer listed drops it

#### Scenario: llama.cpp unchanged [auto]
- GIVEN a llama-server whose `/props` reports `n_ctx` 4096
- WHEN detection runs
- THEN its window is 4096 and no `/api/show` is sent to it

#### Scenario: chat.db is untouched [auto]
- GIVEN a fresh chat.db at `user_version` 3 with mode 0600
- WHEN detection runs and a chat turn is sent
- THEN chat.db is still `user_version` 3, mode 0600, with no new table, and no new file exists
