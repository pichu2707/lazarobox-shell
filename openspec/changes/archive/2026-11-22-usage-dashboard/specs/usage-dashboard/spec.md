# Usage Dashboard Specification (new capability)

## Purpose

The `/cost` overlay as a dashboard: range selector, KPI cards, tokens by day, weekday x hour heatmap, Models and Agents tables, provider identity by glyph + colour + text label, width tiers and a text-report fallback. It is display-only: it reads the usage store (see `ai-usage`) and never sends anything to a model, a log or the network. Tags: [auto] = `cargo test` (temporary SQLite, `TestBackend` + insta); [manual] = real machine.

Terms: "range" = 7 days, 30 days, 90 days or All (All = the whole retained history, at most 90 days). "Period" = the local calendar days of the range, today included. "Turns" = usage rows with `role = 'chat'`. "Task" = see Requirement: KPI cards. "Billed engines" = CLI engines under the existing billing-mode rules; "local engines" = `ollama`, `lmstudio`, `llamacpp`, `vllm`. "Compact" and "grouped" = see Requirement: Number formatting.

## ADDED Requirements

### Requirement: Number formatting
A pure formatter MUST provide: `compact(n)`, `tok(n)` (= `compact(n)` followed by ` tok`) and `grouped(n)`. `compact` MUST show values below 1,000 as plain integers, then one decimal with the suffixes `k`, `M`, `B` (`.` as decimal point, never a locale separator); a trailing `.0` is kept, so `1000` reads `1.0k`. `grouped` MUST write the exact integer with `,` as thousands separator. A missing value MUST stay `no data` and MUST NEVER be formatted as `0`. The dashboard MUST use `compact` for every token figure except the KPI counts (tasks, turns), which use `grouped`.

#### Scenario: Compact ranges [auto]
- GIVEN the values 999, 1000, 123456, 1234567 and 11600000000
- WHEN `compact` is applied
- THEN they read `999`, `1.0k`, `123.5k`, `1.2M` and `11.6B`

#### Scenario: Tok unit [auto]
- GIVEN 1234567
- WHEN `tok` is applied
- THEN it reads `1.2M tok`

#### Scenario: Grouped exact [auto]
- GIVEN 123456, 1110 and 999
- WHEN `grouped` is applied
- THEN they read `123,456`, `1,110` and `999`

#### Scenario: Missing is not zero [auto]
- GIVEN a figure that is `None`
- WHEN it is formatted for the dashboard
- THEN it reads `no data`, never `0`

### Requirement: Range selector
The dashboard MUST offer the ranges `7d`, `30d`, `90d` and `All`, default `7d`, always visible, with the active one marked by text (not only by colour), for example `[7d] 30d 90d All`. Changing the range MUST re-query off the UI thread and show `reading usage…` until the answer arrives. The selector MUST be operable by keyboard (left/right and `1`-`4`) and MUST NOT change any stored data.

#### Scenario: Default range [auto, TestBackend + insta]
- GIVEN the dashboard opened with data
- WHEN rendered
- THEN the selector shows `7d` marked as active and all four ranges are visible

#### Scenario: Switching range [auto]
- GIVEN the dashboard on `7d`
- WHEN the user selects `30d`
- THEN a new read for 30 days is asked of the store worker, `reading usage…` shows until it answers, and `30d` is then marked active

#### Scenario: Active range without colour [auto]
- GIVEN the buffer rendered with colours stripped
- WHEN the selector is read
- THEN the active range is still distinguishable by its brackets

### Requirement: KPI cards
The dashboard MUST show four KPI cards for the range:
1. **Input**: the input tokens (compact) with the cache share as sub-line (cache read / (input + cache read), as a whole percent, omitted when no cache data);
2. **Output**: the output tokens (compact) with the average per turn as sub-line;
3. **Tasks**: `grouped` count of tasks with sub-line `<grouped turns> turns`;
4. **Agent time**: the summed `wall_ms` of chat and orchestrator worker rows as a duration (agent time: parallel agents add up, and a chat turn counts from send to end), with sub-line `up to <N> at once` (peak overlap of worker rows in the range).
A **task** is a distinct non-null `conversation_id` among the range's rows, or a distinct orchestrator run (run id) within the range; a conversation and a run are counted once each, and a run launched from a chat is a task of its own (a chat with 2 runs is 3 tasks). The sub-line of the Tasks card is the turns (`role = 'chat'` rows). Rows of deleted chats keep their tokens in every figure but have a NULL link, so the task count undercounts them; this is a known caveat and not an error. Each of cards 1-3 and the sparkline MUST show a delta against the previous equal period for `7d` and `30d`, as a signed percentage (`+12%`, `-8%`, `0%`); for `90d` and `All` the delta MUST read exactly `n/a`. A delta whose previous period has no data (or, for Agent time, no wall data) MUST read `n/a`, never `+inf%` or `0%`. Each card shows a sparkline of its metric by day.

#### Scenario: Four cards at 7 days [auto, TestBackend + insta]
- GIVEN data of two conversations (1,110 tokens-in total reads `1.1k`) and one orchestrator run in the last 7 days
- WHEN rendered at 100 columns
- THEN four cards show input, output, tasks and agent time, and the Tasks card reads `3` with sub-line `<turns> turns`

#### Scenario: Task counting [auto]
- GIVEN 5 chat rows of conversation A, 2 chat rows of conversation B and 3 worker rows of one run (linked or not to a conversation), all in range
- WHEN the tasks are counted
- THEN tasks is 3 and turns is 7

#### Scenario: Deleted chat undercounts tasks [auto]
- GIVEN rows whose conversation was deleted (NULL link) with 500 input tokens
- WHEN the dashboard is built
- THEN the 500 tokens are in the Input card and the rows do not add a task

#### Scenario: Delta for 7 and 30 days [auto]
- GIVEN 200 tokens in the last 7 days and 100 in the 7 days before
- WHEN the 7d dashboard is built
- THEN the delta reads `+100%`

#### Scenario: Delta n/a for 90 days and All [auto]
- GIVEN any data
- WHEN the range is `90d`, and then `All`
- THEN every delta reads exactly `n/a`

#### Scenario: No previous data [auto]
- GIVEN data only in the current 7 days
- WHEN the 7d dashboard is built
- THEN the delta reads `n/a`

#### Scenario: Agent time with old rows [auto]
- GIVEN chat and worker rows with `wall_ms` NULL (stored before the column existed) only
- WHEN the dashboard is built
- THEN the Agent time card reads `n/a`, never `0s`

#### Scenario: Agent time approximate [auto]
- GIVEN some chat or worker rows with `wall_ms` and some without
- WHEN the dashboard is built
- THEN Agent time sums the known values and is marked approximate (leading `~`), and its delta reads `n/a` unless both periods are complete, and `up to N at once` counts only rows with `started_at` and `wall_ms`

### Requirement: Tokens by day
The dashboard MUST show a tokens-by-day chart for the range: one column per local day, input drawn above the axis and output below it, scaled to the largest day. A day with no row is empty (never drawn as a known zero); a known non-zero day has at least one cell. The chart MUST use the same data as the cards and MUST render with no colour (shape and a legend `in` / `out` carry the meaning). For `90d` and `All`, days MAY be bucketed to fit the width; the bucket size MUST be stated in the legend.

#### Scenario: Mirrored columns [auto, TestBackend + insta]
- GIVEN three days with input 400, 100, 0 and output 100, 200, 0
- WHEN rendered at 100 columns
- THEN input is above the axis and output below, the first day's input is the tallest, and the third day is empty

#### Scenario: Small day is visible [auto]
- GIVEN daily totals 1 and 100000
- WHEN the chart is built
- THEN the first day has at least one cell

### Requirement: Weekday and hour heatmap
The dashboard MUST show a heatmap of token volume by weekday (rows, Monday first) and local hour (columns), using shade glyphs (` ░▒▓█`) so it is readable without colour. At widths under 60 columns it MUST use 3-hour buckets. A cell with no rows is blank; a cell with rows is never blank. Rows without a usable `started_at` use the row's end time `ts` for placement.

#### Scenario: Peak cell [auto, TestBackend + insta]
- GIVEN rows concentrated on Tuesday 14:00
- WHEN rendered at 100 columns
- THEN the Tuesday/14 cell is the darkest glyph and empty cells are blank

#### Scenario: 3-hour buckets [auto]
- GIVEN a width of 59 columns
- WHEN the heatmap is built
- THEN it has 8 columns per weekday instead of 24

#### Scenario: No colour [auto]
- GIVEN colours stripped from the buffer
- WHEN the heatmap is read
- THEN intensity is still distinguishable by glyph

### Requirement: Models table
The dashboard MUST show a Models table, one row per engine/model pair sorted by total tokens descending, with columns provider (glyph + label), model, input, output (compact) and, for billed engines only, dollars under the existing billing-mode rules (`$0.0049`, with the `list-price equivalent` label for `subscription`, see `ai-usage`). Local engines MUST show tokens and no dollars. A model that is NULL reads `unknown`, `mixed` reads `mixed`. A provider stacked bar MUST show each provider's share of total tokens, rounded with the largest-remainder method so the shares sum to exactly 100% and every provider with known tokens gets at least one cell; its legend lists each provider with its glyph, label and percent.

#### Scenario: Table order and dollars [auto, TestBackend + insta]
- GIVEN claude `sonnet` 3,000 tokens with $0.50 (`subscription`) and ollama `llama3:8b` 5,000 tokens
- WHEN rendered at 100 columns
- THEN the ollama row comes first with no `$`, and the claude row shows `$0.5000 list-price equivalent`

#### Scenario: Unknown model [auto]
- GIVEN a row with a NULL model and a row with model `mixed`
- WHEN rendered
- THEN the labels read `unknown` and `mixed` and neither takes the label from the current configuration

#### Scenario: Largest remainder [auto]
- GIVEN three providers with shares 33.4%, 33.3% and 33.3%
- WHEN the bar is built
- THEN the rounded shares are 34, 33 and 33 and sum to 100

#### Scenario: Single engine [auto]
- GIVEN rows of one engine only
- WHEN rendered
- THEN the stacked bar is one full segment at 100% and the Models table has one row

### Requirement: Agents table
The dashboard MUST show an Agents table, one row per engine used by orchestrator worker rows in the range, with exactly the columns `input`, `output` and `time` (summed `wall_ms`). It MUST NOT show dollars or a turns column. A row whose workers have no `wall_ms` shows `n/a` in `time`; a partly known row shows an approximate `~` value. Per-worker and per-run drill-down are out of scope.

#### Scenario: Columns [auto, TestBackend + insta]
- GIVEN two engines with worker rows with wall time
- WHEN rendered
- THEN the header is `input`, `output`, `time` after the engine column and no `$` appears

#### Scenario: Old rows [auto]
- GIVEN worker rows with NULL `wall_ms`
- WHEN rendered
- THEN `time` reads `n/a` and `input` and `output` are shown

#### Scenario: No workers [auto]
- GIVEN no worker rows in the range
- WHEN rendered
- THEN the Agents section shows `no agent runs in this range` and no table

### Requirement: Provider identity
Every provider MUST be identified by a Unicode glyph, a colour and a text label together; meaning MUST NEVER rely on colour alone and image logos MUST NOT be used. The map is: Anthropic `✻`, Google `✦`, OpenAI a glyph that is always display width 1 (the ambiguous `◎` MUST NOT be used), local engines (`ollama`, `lmstudio`, `llamacpp`, `vllm`) a purple `⬡`, anything else `Other` with a neutral glyph. Classification is a pure function of (engine, model): CLI engines map by engine (`claude` Anthropic, `gemini` Google, `codex` OpenAI), local engines by engine, any other pair to `Other` (an unknown, NULL or `mixed` model does not change a known engine's provider). A unit test MUST assert that every glyph in the map has `unicode-width` 1 and that the OpenAI glyph is not `◎`. Colours live only in the theme (a neutral token for OpenAI, `ai_purple` for local engines); no colour literal appears in the dashboard.

#### Scenario: Classification [auto]
- GIVEN (`claude`, `sonnet`), (`gemini`, NULL), (`codex`, `mixed`), (`ollama`, `llama3:8b`) and (`foo`, `bar`)
- WHEN classified
- THEN the providers are Anthropic, Google, OpenAI, Local and Other

#### Scenario: Glyph width [auto]
- GIVEN every glyph of the provider map
- WHEN measured with `unicode-width`
- THEN each is exactly width 1 and the OpenAI glyph is not `◎`

#### Scenario: Local glyph and colour [auto]
- GIVEN an ollama row rendered
- WHEN the cell styles are inspected
- THEN the glyph is `⬡` with the theme's `ai_purple` foreground and the text label is next to it

#### Scenario: No colour keeps identity [auto]
- GIVEN colours stripped
- WHEN a provider row is read
- THEN the glyph and the text label are both present

### Requirement: Width tiers and fallback
The dashboard MUST pick a layout by the overlay's width: 80 columns or more, the four KPI cards in one row; 60-79, the cards in a 2x2 grid; 40-59, the cards in one column; under 40 columns, the old text report (the system-line view of `ai-usage`, kept for this tier only). Section content MUST be clipped or dropped by whole parts, never cut mid-glyph, and rendering MUST NOT panic at any width or height from 0 upward, including double-width glyphs in model names.

#### Scenario: Wide layout [auto, TestBackend + insta]
- GIVEN width 100
- WHEN rendered
- THEN four cards are on one row

#### Scenario: Medium layout [auto, TestBackend + insta]
- GIVEN width 70
- WHEN rendered
- THEN the cards form a 2x2 grid

#### Scenario: Narrow layout [auto, TestBackend + insta]
- GIVEN width 50
- WHEN rendered
- THEN the cards are stacked in one column

#### Scenario: Under 40 columns [auto, TestBackend + insta]
- GIVEN width 39
- WHEN rendered
- THEN the old text report is shown (first line `AI usage (alpha)`) and no card is drawn

#### Scenario: Tier boundaries [auto]
- GIVEN widths 80, 79, 60, 59, 40 and 39
- WHEN the tier is picked
- THEN they give four-in-a-row, 2x2, 2x2, one column, one column and text report

#### Scenario: Any size [auto]
- GIVEN widths and heights from 0 to 120
- WHEN rendered
- THEN nothing panics

### Requirement: Empty, loading and unavailable states
With no usage rows in the range the dashboard MUST show `no usage recorded yet` with the range selector and no charts, tables or cards. While the answer is pending it MUST show `reading usage…`. When the store is unavailable it MUST show `usage history is not available` and nothing else. None of these may panic.

#### Scenario: No data [auto, TestBackend + insta]
- GIVEN an empty store
- WHEN the dashboard is rendered
- THEN `no usage recorded yet` is shown, no chart or table is drawn and nothing panics

#### Scenario: Loading [auto]
- GIVEN the read not yet answered
- WHEN rendered
- THEN the dashboard reads `reading usage…`

#### Scenario: Store unavailable [auto]
- GIVEN an unavailable store
- WHEN rendered
- THEN it reads `usage history is not available` with no other lines

#### Scenario: Range with no rows but history elsewhere [auto]
- GIVEN rows only 20 days ago and range `7d`
- WHEN rendered
- THEN the empty state is shown for `7d` and switching to `30d` shows the data

### Requirement: Dashboard privacy
The dashboard MUST show counts, durations, ids-derived counts and timestamps-derived buckets only. It MUST NOT read, show or log any message text, prompt, path or credential, MUST NOT add a network connection, and its data MUST NEVER be sent to a model, an engine or a log.

#### Scenario: Not logged [auto]
- GIVEN logging captured while the dashboard is built and rendered
- WHEN the log is searched for token counts, durations and costs
- THEN they are not found

#### Scenario: Not sent to a model [auto]
- GIVEN the dashboard opened in a chat pane
- WHEN the next message is sent
- THEN the request to the engine contains none of the dashboard figures

### Requirement: Accessibility without colour
With colours stripped, every section MUST stay readable: the range selector by brackets, providers by glyph and label, charts by shape and legend, the heatmap by shade glyphs.

#### Scenario: Colourless render [auto]
- GIVEN the dashboard rendered with a style-stripped buffer at 100 columns
- WHEN every section is read
- THEN each is understandable from its glyphs and text alone
