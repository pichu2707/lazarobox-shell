# Delta for AI Usage (usage-dashboard)

## MODIFIED Requirements

### Requirement: TurnUsage model
A turn's usage MUST be one record with these fields, every count optional: `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens` (whole numbers), `duration_ms` (whole number), `cost_usd` (a non-negative finite decimal, reported by the engine only), the `engine`, the `model`, the time the turn ended (UTC, whole seconds), a `partial` flag, an optional short `role` identifier (at most 32 characters of `[a-z0-9_-]`): `worker` for an orchestrator worker turn, empty or absent for a chat turn, and two nullable timing fields: `started_at` (the time the turn started, UTC, whole seconds) and `wall_ms` (whole number, elapsed wall-clock milliseconds from start to end of the turn). A missing value MUST be `None` and MUST NEVER be turned into 0. The record MUST hold no text of any message, prompt, answer, reasoning, tool call, path or error. The "total tokens" rule is unchanged.
(Previously: no `started_at` and no `wall_ms`.)

The store MUST gain the two columns as nullable, additive columns through the existing ensure-schema path, with no data migration and no backfill: rows written before this change keep NULL in both and old binaries ignore them. `started_at` and `wall_ms` MUST be filled when a turn is finished (chat and worker turns alike); where the worker's start instant cannot be determined, `wall_ms` stays NULL for it. They are counts and timestamps only. Retention stays 90 days.

#### Scenario: New columns are nullable and additive [auto]
- GIVEN a database created before this change with usage rows
- WHEN the store opens it
- THEN `started_at` and `wall_ms` exist, existing rows hold NULL in both, no row is changed otherwise and no error occurs

#### Scenario: Finished turn stores timing [auto]
- GIVEN a chat turn that started at T and ended 4,200 ms later
- WHEN its record is stored
- THEN `started_at` is T and `wall_ms` is 4200

#### Scenario: Unknown start stays NULL [auto]
- GIVEN a worker turn whose start instant is unavailable
- WHEN its record is stored
- THEN `wall_ms` and `started_at` are NULL and the record is otherwise unchanged

#### Scenario: Old rows read as no data [auto]
- GIVEN a row with NULL `wall_ms`
- WHEN it is read
- THEN the value is `None`, never 0

### Requirement: Usage text formats
Everywhere usage appears as a line it MUST follow these formats (English UI text). Token counts MUST be written compact (`999`, `1.0k`, `123.5k`, `1.2M`, `11.6B`; `.` decimal point, a single unit, see `usage-dashboard`, Number formatting), except exact figures in limit notes (see Thresholds). `<N>` below means a compact count.
- Usage line: `usage: in <N> · out <N>` followed, each only when present, by ` · cached <N>` (cache read), ` · reasoning <N>`, ` · <dollar part>`, and by ` · partial` when the record is partial. A missing `in` or `out` reads `in no data` / `out no data`. A turn with no record reads exactly `usage: no data`.
- Cached, reasoning and dollar parts are omitted when missing; `in` and `out` are never omitted.
(Previously: counts were plain decimal integers without separators.)

#### Scenario: Full line [auto]
- GIVEN a claude record 2 in, 4 out, cache read 0, cost 0.004936, mode `subscription`
- WHEN formatted
- THEN it reads `usage: in 2 · out 4 · cached 0 · $0.0049 list-price equivalent`

#### Scenario: Compact counts [auto]
- GIVEN a record with 1,234,567 in and 123,456 out
- WHEN formatted
- THEN it reads `usage: in 1.2M · out 123.5k`

#### Scenario: Missing input [auto]
- GIVEN a record with only `output_tokens` 567
- WHEN formatted
- THEN it reads `usage: in no data · out 567`

#### Scenario: Partial marker [auto]
- GIVEN a partial record 100 in, 20 out
- WHEN formatted
- THEN it reads `usage: in 100 · out 20 · partial`

### Requirement: Thresholds
Unchanged, except that the limit notes MUST write their figures as exact whole numbers grouped with `,` as thousands separator (never compact, never rounded), so the user sees the exact crossing:
- per turn: `turn used <N> tokens (limit <L>)`;
- per day, tokens: `today's usage reached <N> tokens (limit <L>)`;
- per day, dollars: `today's spend reached $<N> (limit $<L>)` (dollar formatting unchanged).
(Previously: counts were plain integers without separators.)

#### Scenario: Per-turn note with grouping [auto]
- GIVEN `turn_tokens` 100000 and a turn with 100000 in and 23456 out
- WHEN the turn ends
- THEN the pane shows `turn used 123,456 tokens (limit 100,000)`

#### Scenario: Small figures unchanged [auto]
- GIVEN `turn_tokens` 1000 and a turn with 900 in and 300 out
- WHEN the turn ends
- THEN the pane shows `turn used 1,200 tokens (limit 1,000)`

#### Scenario: Daily tokens grouped [auto]
- GIVEN `day_tokens` 5000 and stored records of today totalling 5200
- WHEN the note is shown
- THEN it reads `today's usage reached 5,200 tokens (limit 5,000)`

### Requirement: The `/cost` view
`/cost` MUST remain a user-only composer command (see chat-pane) and MUST open the usage dashboard (see `usage-dashboard`), which replaces the former system-line report and the Charts requirement. It MUST still aggregate every stored usage record, including those of deleted chats, with worker records included in the totals. The previous text report (header `AI usage (alpha)`, the `today`, `last 7 days`, `of which workers`, `by engine and model` and `this chat` lines, the partial and `unknown` / `mixed` rules, and the `no usage recorded yet`, `reading usage…` and `usage history is not available` messages) MUST be kept as the fallback shown only when the available width is under 40 columns. The Charts (`per day`, `per engine`) are removed in every tier; the dashboard's own sections replace them. Reading the store still happens off the UI thread. The view MUST NEVER take a model label from the current configuration.
(Previously: a plain system-line report plus two text charts at every width.)

#### Scenario: Opens the dashboard [auto, TestBackend + insta]
- GIVEN usage records and a pane at least 40 columns wide
- WHEN `/cost` runs
- THEN the dashboard is shown, not the system-line report

#### Scenario: Narrow pane falls back to text [auto, TestBackend + insta]
- GIVEN a pane of 39 columns
- WHEN `/cost` runs
- THEN the text report is shown, its first line is `AI usage (alpha)`, and it contains no `per day` or `per engine` chart line

#### Scenario: Text report content kept [auto]
- GIVEN the fallback and a chat record of 300 total tokens plus a worker record of 200 today
- WHEN rendered
- THEN `today` and `last 7 days` read `500 tokens · 1 turns` (500 is below 1,000, so compact form is unchanged), and `of which workers` follows the `last 7 days` line

#### Scenario: Aggregates all chats [auto]
- GIVEN records in two chats and in one deleted chat, today
- WHEN `/cost` runs
- THEN the dashboard totals and the text fallback include all three

#### Scenario: Empty store [auto]
- GIVEN no usage records
- WHEN `/cost` runs
- THEN the dashboard shows `no usage recorded yet` and the text fallback shows `AI usage (alpha)` then `no usage recorded yet`

#### Scenario: Store unavailable [auto]
- GIVEN an unavailable store
- WHEN `/cost` runs
- THEN the view reads `usage history is not available` and nothing panics

#### Scenario: Real session [manual]
- GIVEN a claude and an ollama turn in a real pane
- WHEN `/cost` is typed
- THEN the claude turn shows tokens and a labelled dollar figure, the ollama turn shows tokens and no dollars

## REMOVED Requirements

### Requirement: Charts
(Reason: replaced by the tokens-by-day chart, the heatmap and the provider bar of `usage-dashboard`; the plain-text sparkline and per-engine bars are no longer shown at any width, including the under-40 fallback.)

## MODIFIED Requirements

### Requirement: Worker usage
Unchanged, except: worker records now also store `started_at` and `wall_ms` (see TurnUsage model), which feed the dashboard's Agent time card and Agents table. A worker without a determinable start keeps NULL in both. The scenario `Charts include workers` is replaced by `Dashboard includes workers`.

#### Scenario: Dashboard includes workers [auto]
- GIVEN worker records only today
- WHEN the dashboard is built
- THEN the cards, the Models table and the Agents table count their tokens

#### Scenario: Worker wall time stored [auto]
- GIVEN a worker turn of 12,000 ms
- WHEN its record is stored
- THEN `wall_ms` is 12000 and `started_at` is set

### Requirement: Usage privacy and README
The usage table and every surface of this capability, the dashboard included, MUST contain counts, ids, timestamps and durations only. The new `started_at` and `wall_ms` columns are timestamps and durations and carry no text. Usage MUST NOT be written to any log, usage data MUST be display-only and MUST NEVER reach a model, and no message text, prompt, path or credential may reach a usage row, the dashboard, the text report or a note. The README (`README.md` and `README.es.md`, changed together with the same sections in the same order) MUST state: what usage is kept (counts, engine, model, start time and duration), that it contains no message text, that it survives deleting a chat, that it is kept 90 days (so the delta shows `n/a` for 90 days and All), where the file is and how to delete it, that dollar figures are the engine's own list-price figures and not an invoice, that local engines show tokens and never dollars, that agent time is `n/a` or approximate for turns recorded before the timing columns existed, and that OxideGate is not used. It MUST also describe the `/cost` dashboard: the ranges, the width tiers and the text fallback under 40 columns.
(Previously: no timing columns, no dashboard description.)

#### Scenario: Marker absent from the file [auto]
- GIVEN a turn whose prompt and answer contain `ZZ-MARKER-ZZ`
- WHEN the usage row, including `started_at` and `wall_ms`, is written to a real temporary database
- THEN the bytes of the database file and of any side file do not contain the marker

#### Scenario: Usage not logged [auto]
- GIVEN logging captured during a turn with usage and while the dashboard is opened
- WHEN the log is searched for token counts, durations and cost
- THEN they are not found

#### Scenario: README states the facts [manual]
- GIVEN `README.md` and `README.es.md`
- WHEN they are read
- THEN both cover the statements above with the same sections in the same order, and the English phrases the code matches are quoted as the code has them

### Requirement: Alpha status
Unchanged. The header `AI usage (alpha)` MUST remain the first line of the text fallback; the dashboard MUST carry the same `alpha` marker in its title, and the README `/cost` section keeps its alpha note.

#### Scenario: Alpha marker [auto]
- GIVEN the dashboard and the text fallback
- WHEN rendered
- THEN the dashboard title contains `alpha` and the fallback's first line is `AI usage (alpha)`
