# AI Usage Specification

## Purpose

What an AI turn used and, where an engine says so, what it cost: how usage is captured from engine streams, summed per turn, kept, shown and warned about. Everything is engine-reported: lazarobox holds no price table and never computes dollars. Tags: [auto] = `cargo test` (fake engines, temporary SQLite files, `TestBackend` + insta); [manual] = real machine.

Terms: "turn" = one user send, which may issue several engine requests (a tool loop); its usage is the sum over those requests. "Usage" = counts, timings and, only when the engine reports it, dollars. "No data" = a value the engine did not report; it is `None` in the model and the text `no data` on screen, and it is NEVER shown or stored as 0. "Local engines" = `ollama`, `lmstudio`, `llamacpp`, `vllm`. "CLI engines" = `claude`, `codex`, `gemini`. "Day" = the local calendar day, starting at local midnight.

## ADDED Requirements

### Requirement: TurnUsage model
A turn's usage MUST be one record with these fields, every count optional: `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens` (whole numbers), `duration_ms` (whole number), `cost_usd` (a non-negative finite decimal, reported by the engine only), the `engine`, the `model`, the time the turn ended (UTC, whole seconds), a `partial` flag, and an optional short `role` identifier (at most 32 characters of `[a-z0-9_-]`, empty or absent in this change; reserved for worker attribution). A missing value MUST be `None` and MUST NEVER be turned into 0. The record MUST hold no text of any message, prompt, answer, reasoning, tool call, path or error. The "total tokens" of a record MUST be `input_tokens + output_tokens` when both are present, and `None` when either is missing; cache and reasoning counts are shown separately and never added to the total (reasoning is already part of output on the engines that report it).

#### Scenario: Missing is None, not zero [auto]
- GIVEN an engine reply that reports `output_tokens` and nothing else
- WHEN the record is built
- THEN `output_tokens` is set, every other count and `cost_usd` are `None`, and the total tokens are `None`

#### Scenario: Zero is a real value [auto]
- GIVEN an engine reply that reports `cache_read_tokens` as 0
- WHEN the record is built
- THEN `cache_read_tokens` is `Some(0)`, distinct from a missing value

#### Scenario: No text in the record [auto]
- GIVEN the record type
- WHEN its fields are listed by a test that compares them with the allowed set above
- THEN no field can carry free text other than the engine id, the model id and the `role` identifier

### Requirement: Capture per engine
Each engine adapter MUST capture the usage its own stream reports, from these fields (design pins the exact paths from captured fixtures; a field absent from the fixtures is `None`):
- claude: the final `result` event: `usage` (input, output, cache read, cache creation), `duration_api_ms`, and the dollar figure `total_cost_usd` (or the sum of `modelUsage[*].costUSD`);
- codex: `turn.completed.usage` (input, cached input, output, reasoning output); no dollars;
- gemini: `result.stats` (input, output and cached tokens, duration); no dollars;
- ollama: the final line `prompt_eval_count`, `eval_count`, and its durations; no dollars;
- llamacpp: the last chunk `timings` (`prompt_n`, `cache_n`, `predicted_n`, durations), or an OpenAI-style `usage` object when present; no dollars;
- lmstudio and vllm: the OpenAI-style `usage` object (`prompt_tokens`, `completion_tokens`) when the stream carries one, else no data.
A local engine MUST NEVER produce `cost_usd`, even if a stream carried a cost-like field. Capture MUST NOT change any request, argv, flag or environment sent to an engine, and MUST NOT add a network connection.

#### Scenario: Claude result [auto]
- GIVEN the captured claude `result` fixture (2 input, 4 output, 606 cache-creation tokens, `total_cost_usd` 0.004936)
- WHEN it is decoded
- THEN the record holds those counts, the cache-write count 606 and `cost_usd` 0.004936

#### Scenario: Codex has no dollars [auto]
- GIVEN the captured codex `turn.completed` fixture
- WHEN it is decoded
- THEN input, cached, output and reasoning counts are set and `cost_usd` is `None`

#### Scenario: Gemini stats [auto]
- GIVEN the captured gemini `result` fixture with `stats`
- WHEN it is decoded
- THEN input, output and cached counts and the duration are set and `cost_usd` is `None`

#### Scenario: Ollama final line [auto]
- GIVEN the final NDJSON line with `prompt_eval_count` 12 and `eval_count` 34
- WHEN it is decoded
- THEN input is 12, output is 34 and `cost_usd` is `None`

#### Scenario: llama.cpp timings [auto]
- GIVEN a last SSE chunk with `timings` (`prompt_n` 10, `cache_n` 4, `predicted_n` 20) and no `usage` object
- WHEN it is decoded
- THEN input is 10, cache read is 4, output is 20

#### Scenario: Server without usage [auto]
- GIVEN an lmstudio or vllm stream with no `usage` object
- WHEN it is decoded
- THEN the record is absent (no data) and the turn completes normally

#### Scenario: Local engine never has dollars [auto]
- GIVEN an ollama final line that also carries a field named `cost`
- WHEN it is decoded
- THEN `cost_usd` is `None`

#### Scenario: Requests are unchanged [auto]
- GIVEN the recorded request, argv and environment of each adapter before and after this change
- WHEN a turn runs
- THEN they are byte-identical

### Requirement: Lenient usage parsing
Usage decoding MUST be separate from, and never able to fail, the text decoding of a turn. A usage value that is missing, of the wrong type, negative, fractional where a whole number is expected, larger than `u64`, `NaN` or infinite MUST become `None` for that field only; the other fields of the same record are kept. A whole usage object that is malformed, truncated, too large to read or of an unknown shape MUST yield no record. These parse cases stay visible as `no data` (see Usage text formats), not as notices; only the storage path has a notice (`Usage not saved: <reason>`, see chat-history). None of these cases may produce an error event, drop or alter a text delta, delay or prevent the final event, panic, or write a log line containing the offending value.

#### Scenario: Bad value is no data [auto]
- GIVEN a codex usage object with `input_tokens` `"many"`, `output_tokens` -5 and `cached_input_tokens` 7
- WHEN it is decoded
- THEN input and output are `None`, cache read is 7 and the turn ends with its normal final event

#### Scenario: Malformed usage keeps the answer [auto]
- GIVEN a stream with text deltas, a malformed usage line (for example `{"usage":`) and then the final line
- WHEN a turn runs
- THEN the text deltas and the final event are delivered as without the bad line, no error event is produced and no record is emitted

#### Scenario: Gemini new stats shape [auto]
- GIVEN a gemini `result` whose `stats` has an unknown nested shape
- WHEN a turn runs
- THEN the answer completes, the unknown parts are `None` and the fail-closed answer decoder is not affected

#### Scenario: Non-finite cost [auto]
- GIVEN a claude `result` with a cost that is negative, then one that is not a number
- WHEN each is decoded
- THEN `cost_usd` is `None` both times and the counts are kept

### Requirement: Summing per turn
The usage of a turn MUST be the field-wise sum over the usage reported by each engine request of that turn. A field is `Some(sum)` when at least one request reported it and `None` when none did. A turn MUST be marked `partial` when it issued more than one request and at least one of them reported no usage record, or when it was cancelled, cut or ended in error after at least one request had reported usage. A single-request turn with no usage record MUST produce no record at all (no data), not a record of `None` fields. One record MUST be written per turn when at least one request reported usage, whatever the turn status (complete, cancelled, interrupted, error); a turn with no reported usage writes nothing. The same request MUST never be counted twice, and a retry the user starts by sending again is a new turn.

#### Scenario: Two requests are summed [auto]
- GIVEN a turn with a tool loop of two requests reporting (100 in, 20 out) and (150 in, 30 out)
- WHEN the turn ends
- THEN one record holds 250 in and 50 out and is not partial

#### Scenario: One request lacks data [auto]
- GIVEN a turn of two requests where only the first reported (100 in, 20 out)
- WHEN the turn ends
- THEN one record holds 100 in and 20 out and is marked partial

#### Scenario: Field reported by one request only [auto]
- GIVEN two requests where only the second reports `cache_read_tokens` 9
- WHEN the turn ends
- THEN the record has cache read 9 and the turn is partial only if a request reported no record at all

#### Scenario: No data, no row [auto]
- GIVEN a single-request turn whose engine reported no usage
- WHEN the turn ends
- THEN no record is written and the answer line reads `usage: no data`

#### Scenario: Cancel after reported usage [auto]
- GIVEN a two-request turn cancelled during the second request after the first reported usage
- WHEN the turn ends
- THEN one partial record with the first request's usage is written

#### Scenario: Error before any usage [auto]
- GIVEN a turn that ended with an error event before any usage was reported
- WHEN the turn ends
- THEN no record is written

### Requirement: Billing modes
Each CLI engine (`claude`, `codex`, `gemini`) MUST have a billing mode, `subscription` or `per-token`; the default is `subscription` for all three. Local engines have no billing mode and MUST NEVER show dollars. The mode MUST change only how a reported dollar figure is labelled, never the counts and never the stored record (the mode is not stored). Under `per-token` a reported `cost_usd` is shown as spend: `$0.0049`. Under `subscription` it is shown with the label: `$0.0049 list-price equivalent`. A dollar amount MUST be formatted as `$` followed by the value with 4 decimals below one dollar and 2 decimals from one dollar up. Where the engine reported no dollars, the dollar part MUST be omitted (not shown as `$0` and not as `no data`). Changing a mode MUST relabel past records too, because it is applied when rendering.

#### Scenario: Subscription label [auto]
- GIVEN a claude record with `cost_usd` 0.004936 and mode `subscription`
- WHEN it is rendered
- THEN the dollar part reads `$0.0049 list-price equivalent`

#### Scenario: Per-token is spend [auto]
- GIVEN the same record and mode `per-token`
- WHEN it is rendered
- THEN the dollar part reads `$0.0049` with no label

#### Scenario: Codex has no dollar part [auto]
- GIVEN a codex record with no `cost_usd` in either mode
- WHEN it is rendered
- THEN no dollar part appears

#### Scenario: Local never shows dollars [auto]
- GIVEN an ollama record in any configuration
- WHEN it is rendered
- THEN the output has tokens only and no `$`

#### Scenario: Default modes [auto]
- GIVEN no `[ai.usage.billing]` table
- WHEN billing modes are read
- THEN claude, codex and gemini are `subscription`

#### Scenario: Large amount [auto]
- GIVEN `cost_usd` 12.3456
- WHEN formatted
- THEN it reads `$12.35`

### Requirement: Usage text formats
Everywhere usage appears as a line it MUST follow these formats (English UI text; counts are plain decimal integers without separators):
- Usage line: `usage: in <N> · out <N>` followed, each only when present, by ` · cached <N>` (cache read), ` · reasoning <N>`, ` · <dollar part>`, and by ` · partial` when the record is partial. A missing `in` or `out` reads `in no data` / `out no data`. A turn with no record reads exactly `usage: no data`.
- Cached, reasoning and dollar parts are omitted when missing; `in` and `out` are never omitted.

#### Scenario: Full line [auto]
- GIVEN a claude record 2 in, 4 out, cache read 0, cost 0.004936, mode `subscription`
- WHEN formatted
- THEN it reads `usage: in 2 · out 4 · cached 0 · $0.0049 list-price equivalent`

#### Scenario: Missing input [auto]
- GIVEN a record with only `output_tokens` 567
- WHEN formatted
- THEN it reads `usage: in no data · out 567`

#### Scenario: Partial marker [auto]
- GIVEN a partial record 100 in, 20 out
- WHEN formatted
- THEN it reads `usage: in 100 · out 20 · partial`

### Requirement: Thresholds
Three optional thresholds MUST exist: per turn (total tokens, `turn_tokens`), per day (total tokens, `day_tokens`) and per day (dollars, `day_usd`). An unset threshold is off. A threshold MUST only produce a passive note and MUST NEVER block, delay, shorten or cancel a send or a stream.
- Per turn: when a finished turn's total tokens are known and exceed `turn_tokens`, the pane shows the system line `turn used <N> tokens (limit <L>)`. A turn whose total is `None` never triggers it.
- Per day, tokens: after a record is stored, when the sum of the total tokens of the day's records (records with `None` total are skipped) reaches `day_tokens`, the pane shows `today's usage reached <N> tokens (limit <L>)`.
- Per day, dollars: when the sum of `cost_usd` of the day's records of engines in `per-token` mode reaches `day_usd`, the pane shows `today's spend reached $<N> (limit $<L>)`. Records of `subscription` engines and local engines never count toward it.
The two per-day notes MUST appear once per threshold per local day per app run (crossing again later the same day does not repeat them); the per-turn note appears on every turn that exceeds it. Notes are system lines: not stored, not sent to any engine.

#### Scenario: Per-turn note [auto]
- GIVEN `turn_tokens` 1000 and a turn with 900 in and 300 out
- WHEN the turn ends
- THEN the pane shows `turn used 1200 tokens (limit 1000)` and the next send is accepted

#### Scenario: Under the limit [auto]
- GIVEN `turn_tokens` 1000 and a turn of 400 in and 100 out
- WHEN the turn ends
- THEN no note appears

#### Scenario: Unknown total never warns [auto]
- GIVEN `turn_tokens` 1 and a turn with `output_tokens` only
- WHEN the turn ends
- THEN no note appears

#### Scenario: Daily tokens once [auto]
- GIVEN `day_tokens` 5000, stored records of today totalling 4800 and a new turn of 400
- WHEN the turn is stored, and again after another turn the same day
- THEN `today's usage reached 5200 tokens (limit 5000)` appears once, not twice

#### Scenario: Daily dollars count per-token engines only [auto]
- GIVEN `day_usd` 1.00, claude in `subscription` with $5 recorded today and codex records with no dollars
- WHEN a turn ends
- THEN no spend note appears; after switching claude to `per-token` and another claude turn, `today's spend reached $5.01 (limit $1.00)` appears

#### Scenario: A new day re-arms [auto]
- GIVEN the daily note shown yesterday and the local date changed
- WHEN the threshold is crossed again
- THEN the note appears again

#### Scenario: Day starts at local midnight [auto]
- GIVEN a record at 23:59 local and one at 00:01 local the next day
- WHEN today's total is computed after midnight
- THEN only the second counts

#### Scenario: Empty means off [auto]
- GIVEN no thresholds
- WHEN any turn ends
- THEN no note appears

#### Scenario: Never blocks [auto]
- GIVEN every threshold crossed
- WHEN a message is sent
- THEN a ChatSend effect is emitted as without thresholds

### Requirement: The `/cost` view
`/cost` MUST be a user-only composer command (see chat-pane). It MUST aggregate every stored usage record, including those of deleted chats, and show these lines as system lines, in this order:
1. `AI usage (alpha)`
2. `today: <T> · <K> turns` where `<T>` is `<N> tokens` (sum of known totals) or `no data` when no record of the day has a known total;
3. `last 7 days: <T> · <K> turns` (the last 7 local days including today);
4. `by engine and model (last 7 days):` followed by one line per engine/model pair, sorted by total tokens descending: `  <engine> › <model>: in <N> · out <N>` plus the cached, dollar and partial parts as in Usage text formats, dollars labelled by the engine's billing mode;
5. `this chat: <T> · <K> turns` for the current conversation (records still linked to it), or `this chat: no data` when it has none.
`<K> turns` counts chat records only (compaction and summary runs add their tokens but are not turns). Sums MUST skip `None` values rather than treating them as 0. A pair line, and every summary line (`today`, `last 7 days`, `this chat`, `this session`), MUST end with ` · partial` when any record in it is partial or lacks its input or output count. A pair line that shows a dollar figure is also `partial` when some of its records reported tokens but no dollars while others did (the figure is then a floor). With no records at all the view is `AI usage (alpha)` followed by `no usage recorded yet`. Reading the store happens off the UI thread; until the answer arrives the pane shows `reading usage…`; if the store is unavailable the view is `usage history is not available` with no other lines. When chat history is off, no usage rows are written; the view then shows only the current session, built from memory, with the line `chat history is off: showing this session only`.

#### Scenario: Aggregates all chats [auto]
- GIVEN records in two chats and in one deleted chat, today
- WHEN `/cost` runs in the first chat
- THEN `today` and `last 7 days` include all three and `this chat` counts only the first chat's records

#### Scenario: Window edges [auto]
- GIVEN a record 6 days ago and one 8 days ago
- WHEN `/cost` runs
- THEN `last 7 days` includes the first and not the second

#### Scenario: By engine and model [auto]
- GIVEN claude `sonnet` 3000 total tokens with $0.50 (mode `subscription`) and ollama `llama3:8b` 5000
- WHEN `/cost` runs
- THEN the pair lines are, in order, `  ollama › llama3:8b: in <N> · out <N>` (no dollars) and `  claude › sonnet: in <N> · out <N> · $0.5000 list-price equivalent`

#### Scenario: No data is not zero [auto]
- GIVEN today's only record has `output_tokens` and no input
- WHEN `/cost` runs
- THEN `today: no data · 1 turns · partial` is shown and the pair line reads `in no data · out <N> · partial`

#### Scenario: Summary lines are partial too [auto]
- GIVEN one partial record and one complete record today
- WHEN `/cost` runs
- THEN `today`, `last 7 days` and the pair line end with ` · partial`, and a view with only complete records has no `partial`

#### Scenario: Cost gap is partial [auto]
- GIVEN two claude records of one model, only one with a dollar figure
- WHEN `/cost` runs
- THEN the pair line shows the figure followed by ` · partial`

#### Scenario: Turns are chat records only [auto]
- GIVEN a chat record and a compaction record of one engine
- WHEN `/cost` runs
- THEN the tokens of both are summed and the turns read 1

#### Scenario: Empty store [auto]
- GIVEN no usage records
- WHEN `/cost` runs
- THEN the lines are `AI usage (alpha)` and `no usage recorded yet`

#### Scenario: Store unavailable [auto]
- GIVEN an unavailable store
- WHEN `/cost` runs
- THEN the view reads `usage history is not available` and nothing panics

#### Scenario: Real session [manual]
- GIVEN a claude and an ollama turn in a real pane
- WHEN `/cost` is typed
- THEN the claude turn shows tokens and a labelled dollar figure, the ollama turn shows tokens and no dollars

### Requirement: Charts
The `/cost` view MUST also show, after the lines above and as plain text system lines (no new widget required), two charts:
- `per day (last 7 days, oldest first):` followed by one line of 7 glyphs, one per local day, scaled to the day with the most known tokens using `▁▂▃▄▅▆▇█`; a day with no record, or only records with unknown totals, is `·`; a day with known tokens of 0 is `▁`; the line ends with the largest value as ` max <N>`;
- `per engine (last 7 days):` followed by one line per engine, `  <engine> <bar> <N>`, where the bar is `█` repeated 1 to 20 times in proportion to the largest engine, at least one glyph for any engine with known tokens, and an engine with no known totals shows `no data` in place of bar and number.
With no records the charts are not shown. Charts MUST use only the same data as the text view and MUST render without panic at any pane width from 0 upward (long lines wrap or clip like other system lines).

#### Scenario: Sparkline scaling [auto]
- GIVEN, oldest to newest, three days with no records and then daily token totals 0, 100, 200 and 400
- WHEN the per-day line is built
- THEN it reads `···▁▂▄█ max 400`

#### Scenario: Unknown is not low [auto]
- GIVEN a day whose only record has an unknown total
- WHEN the per-day line is built
- THEN that day is `·`, not `▁`

#### Scenario: Engine bars [auto]
- GIVEN claude 1000, ollama 250 and codex with unknown totals
- WHEN the per-engine lines are built
- THEN claude has 20 glyphs, ollama 5 and codex shows `no data`

#### Scenario: No charts when empty [auto]
- GIVEN no records
- WHEN `/cost` runs
- THEN no chart header appears

#### Scenario: Narrow pane [auto, TestBackend + insta]
- GIVEN a chat pane of width 10 and a `/cost` result with charts
- WHEN rendered
- THEN nothing panics

### Requirement: Usage privacy and README
The usage table and every surface of this capability MUST contain counts, ids and timestamps only. Usage MUST NOT be written to any log, and no message text, prompt, path or credential may reach a usage row, a `/cost` line or a note. The README MUST state: what usage is kept (counts, engine, model, time), that it contains no message text, that it survives deleting a chat, that it is kept 90 days, where the file is and how to delete it, that dollar figures are the engine's own list-price figures and not an invoice, and that OxideGate is not used.

#### Scenario: Marker absent from the file [auto]
- GIVEN a turn whose prompt and answer contain `ZZ-MARKER-ZZ`
- WHEN the usage row is written to a real temporary database
- THEN the bytes of the database file and of any side file that exists in the `turn_usage` data do not contain the marker

#### Scenario: Usage not logged [auto]
- GIVEN logging captured during a turn with usage
- WHEN the log is searched for the token counts and the cost
- THEN they are not found

#### Scenario: README states the facts [manual]
- GIVEN the README
- WHEN it is read
- THEN it covers the statements above

### Requirement: Alpha status
Cost tracking ships marked ALPHA, because engines report usage in formats that change and the figures may be missing, or may not match the provider's bill, depending on the engine and the account. The `/cost` header MUST read `AI usage (alpha)`, and the README section on `/cost` MUST carry an alpha note that says so and explains how to report a problem (the report skill, or a GitHub issue). The marker is a label only: it MUST NOT change any count, record or behaviour.

#### Scenario: Alpha header [auto]
- GIVEN any `/cost` view (records, no records, history off)
- WHEN it is rendered
- THEN its first line is `AI usage (alpha)`

#### Scenario: README alpha note [manual]
- GIVEN the README section on `/cost`
- WHEN it is read
- THEN it says the feature is alpha, that figures may be missing or differ from the provider's bill, and how to report it

### Requirement: Reading today's figures
The store worker MUST answer a request for today's figures (`UsageToday`) with the known tokens of the local day (the sum of input plus output of the records that have both, `None` when none), the dollars per engine, and the second of the next local midnight, which MUST be later than the time of the request. The request writes nothing and runs no schema change; it is answered with no data and no problem when the usage table does not exist. A read that fails MUST be answered with a read problem (a fixed reason free of text and paths) and no figures; it MUST NOT be confused with a day with no known total, and a panic while reading MUST be a read problem, not a write problem, so it never spends the `Usage not saved` notice.

#### Scenario: Absent table [auto]
- GIVEN a store without the usage table
- WHEN today's figures are requested
- THEN the answer has no tokens, no dollars, a midnight and no problem, and the table is not created

#### Scenario: Unreadable day [auto]
- GIVEN a usage table of the wrong shape
- WHEN today's figures are requested
- THEN the answer carries a read problem and no figures

#### Scenario: Panic while reading [auto]
- GIVEN a panic while reading
- WHEN the request is answered
- THEN the read problem is `internal error` and the write problem is empty
