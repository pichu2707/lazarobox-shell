# Design: AI Selector (per-task engine and model)

## Technical Approach

The change keeps the functional core / imperative shell split used by config-menu and ai-engines.

- **Pure core (`core::ai`)** gains the task model (`Task`, `Assignment`, `AiConfig` with five slots, `resolve`), a static model catalog (`core/ai/catalog.rs`) and a picker state machine (`core/ai/picker.rs`). Picker rows are **derived on every call** from `(target, draft, detection)`, never cached, so a detection that arrives while the picker is open needs no special sync.
- **`MenuState`** owns the AI draft and the optional `Picker`. `InputMode` does not change. `rows()` returns either the task list or the picker view, and `footer()` (moved from the widget) returns the hint, cue or error.
- **`config_edit`** walks dotted table paths (`ai.tasks.code`) and creates missing parents implicitly. `ConfigEdit` and `EditError` keep their types: every path and prefix is a `&'static str` slice.
- **`App`** routes paste to the menu, mirrors its single-flight flag into the menu (`set_checking`), and saves on `s`.

The user ruling of 2026-10-11 overrides the proposal's custom/wishlist parts. On local servers, the Models level shows the detected models (`✓`, selectable) plus a curated static list of models that are not downloaded (`download required`, not selectable). No network beyond localhost is used and nothing is downloaded. `custom…` is offered only by CLI engines.

ADRs use the `SEL-` prefix. The text is over the 800-word budget on purpose, because the orchestrator asked for 13 decision areas.

## Architecture Decisions

| # | Topic | Options | Decision / rationale |
|---|---|---|---|
| SEL-1 | Task model | keep `engine/model` fields + map; **fixed slots** | `enum Task { General, Code, Chat, Review, Summarize }` with `ALL`, `id()` and `table()` (`"ai"` for general, `"ai.tasks.code"` and so on, all `&'static str`). `Assignment { engine: Option<String>, model: Option<String> }`. `AiConfig { slots: [Assignment; 5] }` (private), with `get(Task)`, `set(Task, Assignment)` and `resolve(Task) -> (Assignment, inherited: bool)`. A task other than general whose engine is `None` inherits general. general never inherits. A fixed array makes "unknown task" unrepresentable and gives `Eq` for free. A test helper `AiConfig::general(e, m)` keeps the app-test churn to one helper (`ai_of`). |
| SEL-2 | Reader | — | `[ai] engine/model` is general, unchanged. `[ai.tasks.<id>] engine/model` for code, chat, review and summarize. Problems: `ai.tasks: expected a table`, `ai.tasks.code: expected a table`, `ai.tasks.code.engine: expected a string`, each falling back on its own. `general` and unknown names under `ai.tasks` are ignored and preserved. `""` means unset. |
| SEL-3 | Set all | general := V, others inherit; **literal: every slot := V** | **Literal** (D3). Choosing `none` sets all five to the default assignment. That clears general and makes the others inherit, because an unset task is the inherit state, so `none` is not a special case. The row shows the common resolved value when all five resolve the same, `(not set)` when all five resolve to nothing, and `(mixed)` otherwise. |
| SEL-4 | Picker state | sub-mode in `InputMode`; **`Option<Picker>` in the AI menu** | `Picker { target: Target, level: Level, cursor: usize, offset: usize }`, with `Target { All, Task(Task) }` and `Level { Engines, Models { engine: String }, Custom { engine: String, text: String } }`. Nothing is committed until a leaf is chosen. `Outcome { Stay, Commit(Assignment), Close }`. A commit applies to the target (All writes all five slots), closes the picker and puts the selection back on the target's row. |
| SEL-5 | Disabled rows and the cursor | skip disabled; **land on them, Enter ignored** | **Land.** The `download required` rows sit at the end of the list. If the cursor skipped them, they could never scroll into the viewport and would be invisible, which defeats the ruling ("must appear"). On a disabled row the footer reads `<reason> · not selectable` (`download required`, `no models detected`, `engine not detected` or `checking…`), the row is drawn reversed in the muted colour, and Enter is a no-op. The Settings Mouse row keeps its skip rule: it is one row that is always visible. |
| SEL-6 | Engines level | all catalog engines; **available + assigned** | Row 0 is `Clear`: `(default · use general)` for a task, and `none` for general and Set all. Then the `Available` catalog engines in catalog order, with their `status_text`. Then every engine assigned anywhere in the draft that is not available, marked `(not detected)` (or `checking…` before the first result). Those rows are **enabled**, so S1 holds and the user can re-enter them. This follows proposal D2/D8. Not-installed engines are hidden to keep the list short. |
| SEL-7 | Models level | — | **Server:** the detected models (`✓`), then the models assigned to this engine in the draft but not detected (`download required`, disabled), then `catalog::curated(engine)` minus what is already listed (`download required`, disabled). Dedupe uses `catalog::same_model` (Ollama: a name without a tag equals `:latest`; the detected spelling wins). An assigned model that is still current keeps its assignment and is drawn with `●`, but it cannot be chosen again. **CLI:** `catalog::cli_models(engine)`, then the draft-assigned models not in the catalog (selectable, no mark), then `custom…`. Before the first detection, server model rows are disabled with `checking…`. An empty list shows one disabled `no models detected` row. A server that is not available shows the draft-assigned models (disabled, `●` when current, no `download required` mark), no curated rows, and one disabled `engine not detected` row. |
| SEL-8 | Curated catalog | fetch the library; **static const** | Ollama only (`OLLAMA_CURATED`): `llama3.2:3b`, `llama3.1:8b`, `qwen3:8b`, `qwen2.5-coder:7b`, `qwen3-coder:30b`, `gemma3:4b`, `mistral:7b`, `deepseek-r1:8b`, `phi4:14b`, `gpt-oss:20b`. Explicit size tags avoid the `:latest` ambiguity. The list is documented as **informational and possibly stale**, and it is never fetched. LM Studio, llama.cpp and vLLM ids depend on files or repositories, so `curated()` returns `&[]` for them. CLI: claude has `default`, `opus`, `sonnet` and `fable`; codex, gemini and opencode have `default`. |
| SEL-9 | Viewport and size | dynamic height; **fixed body** | With AI, the body is fixed at 11 rows: in the task view, Settings (1+3) and AI (1+6); in the picker view, a title and `VIEW = 10` rows. The popup does not jump between views. It is 62x15, and the full popup needs at least 24x14 (the blank row goes first), otherwise the one-line fallback is drawn. `Picker.offset` keeps the cursor visible (`cursor < offset` → `offset = cursor`; `cursor ≥ offset+VIEW` → `offset = cursor+1-VIEW`). j/k wrap and the offset follows. The title shows `n/total` only when the list is longer than VIEW. |
| SEL-10 | Custom buffer | — | `Level::Custom` holds `text`. A char is accepted when it is not a control, format/bidi (`parse::is_format`, made `pub(crate)`) or whitespace character. The cap is 128 chars (`MAX_NAME`), and input beyond it is dropped. Paste (`App::on_paste` → `MenuState::on_paste`) keeps the text up to the first line break, applies the same filter and cap, and is ignored outside Custom. Enter commits a non-empty text, Backspace pops, Ctrl+U clears, and Esc returns to Models with the cursor on `custom…`. |
| SEL-11 | Key model | — | Release is ignored. A pending error clears on a press. **Custom** is checked *before* the modifier guard, so only there are Ctrl+U and typed `s/j/k/h/l/r` allowed. Repeat types and deletes, but Enter and Esc need a press. **Task list:** j/k move; h/l/Space cycle Settings rows; Enter cycles Settings rows forward and opens the picker on a task row; `r` (press, AI rows) → `Refresh`; `s` → `Save`; Esc → `Cancel`. **Picker (Engines/Models):** j/k/↑/↓ move; Enter chooses (no-op when disabled); Esc goes back one level (Engines → task list); `s` → `Save` (the draft as is; the picker is dropped); `r` → `Refresh` in Engines and in Models (not in Custom, where it is typed); h/l are ignored. |
| SEL-12 | Footer | widget picks hint; **`MenuState::footer()`** | `enum Footer<'a> { Error(&'a str), Hint(Cow<'static, str>) }`. The widget only styles it. Hints: Settings `j/k move · h/l change · s save · Esc cancel`; task row `j/k move · Enter pick · r refresh · s save · Esc cancel`; Engines `j/k move · Enter choose · r refresh · Esc back · s save`; Models `j/k move · Enter choose · r refresh · Esc back · s save`; disabled row `<reason> · not selectable`; Custom `type a model · Enter ok · Ctrl+U clear · Esc back`. **Cue** on task rows and on both picker levels: `checking engines… · s save · Esc cancel|back` when `engines` is `None`, and `refreshing… · s save · Esc cancel|back` when `checking` is set (`cancel` on the task list, `back` in the picker). Precedence: error, disabled-row text, cue, hint. The fallback is `MENU · s save · Esc cancel`. |
| SEL-13 | Detection cues, W2, S1 | — | `MenuState::set_checking(bool)`. `App` calls `sync_checking()` (menu.set_checking(self.detecting)) after `open_menu` and after `Refresh`, and `set_engines` clears it. Last-known engines stay. On `set_engines` while the picker is open, the cursor is re-anchored by the previous row's `PickKey` (otherwise clamped). **W2:** Enter never saves or closes, and `s` is an explicit save (an unchanged draft closes, because it was asked for). **S1:** app-level test: an undetected saved engine survives open, refresh, `s` and reload. |
| SEL-14 | Nested edits | `AiEdit` path; **dotted `table`** | `apply_edit` splits `table` on `.` and walks the segments. A missing segment under a `Table` is created as `Table::new()` with `set_implicit(true)` for parents (the leaf is explicit), with `set_dotted(true)` under a dotted parent, and as an `InlineTable` under an inline parent. The comment-only-file prefix rule still applies to the first top-level insertion. Collisions use the existing variants with a static prefix slice (`&edit.table[..end]`): `ai = 3` → `NotATable("ai")`; `[ai] tasks = 3` or `[[ai.tasks]]` → `NotATable("ai.tasks")`; `[ai.tasks] code = "x"` → `NotATable("ai.tasks.code")`; `[ai.tasks.code.engine]` → `KeyIsTable{"ai.tasks.code","engine"}`. `config_write`'s messages need no change. `edits()` puts the Settings edits first, then, for each changed task in `Task::ALL` order, both keys of its table (unset → `""`). |

## Data Flow

```
App::open_menu ─> MenuState::open().with_ai(ai, engines) ─> request_detection ─> sync_checking
key ─> MenuState::on_key ─┬─ task list: Settings cycle | Enter → Picker::open(target)
                          ├─ picker: Picker::{step,enter,back,type} ─> Commit(Assignment) ─> AiConfig::set
                          └─ s → MenuCommand::Save ─> edits() (settings + changed task tables)
paste ─> App::on_paste(Menu) ─> MenuState::on_paste ─> Picker custom buffer
EnginesDetected ─> menu.set_engines (rows re-derived, cursor re-keyed, checking=false)
render ─> rows() (task view | picker title + window[offset..offset+VIEW]) + footer()
Save ─> apply_edit (dotted path walk, implicit parents) ─> atomic write
```

## File Changes

| File | Action | Description |
|---|---|---|
| `src/core/ai/config.rs` | Modify | `Task`, `Assignment`, slot-based `AiConfig`, `resolve`, `[ai.tasks.*]` reader |
| `src/core/ai/catalog.rs` | Create | `cli_models`, `OLLAMA_CURATED`, `curated`, `same_model` |
| `src/core/ai/picker.rs` | Create | `Target`, `Level`, `Picker`, `PickRow`, `PickKey`, `Mark`, `Outcome`, `engine_rows`, `model_rows`, viewport, custom buffer |
| `src/core/ai/menu.rs` | Modify | `AiMenu` = original/draft `AiConfig` + engines + checking + `Option<Picker>`; task row values; old `AiField`/`cycle`/choices deleted |
| `src/core/ai/parse.rs` | Modify | `is_format` → `pub(crate)` |
| `src/core/ai/mod.rs` | Modify | `pub mod catalog; pub mod picker;` |
| `src/core/menu.rs` | Modify | task rows, picker view in `rows()`, `Row::Title(Cow)` + `Row::Pick{..}`, key model, `footer()`, `on_paste`, `set_checking`, per-task `edits()` |
| `src/core/menu/ai_tests.rs` | Rewrite | cycling tests deleted; task-row, picker, key-model and edit tests |
| `src/core/config_edit.rs` | Modify | dotted path walk, implicit parents, collision prefixes |
| `src/ui/components/menu.rs` | Modify | fixed body height, picker rows (marks, `●`, muted-reversed), `footer()`, snapshots |
| `src/app.rs` | Modify | `on_paste` in Menu, `sync_checking`, AI test rewrite, W2/S1 tests |
| `src/runtime/config_write.rs` | Modify | tests only: nested-path messages |

## Interfaces / Contracts

```rust
pub enum Task { General, Code, Chat, Review, Summarize }      // core::ai::config
pub struct Assignment { pub engine: Option<String>, pub model: Option<String> }
impl AiConfig { fn get(&self, t: Task) -> &Assignment; fn set(&mut self, t: Task, a: Assignment);
                fn resolve(&self, t: Task) -> (Assignment, bool /*inherited*/); }
pub enum PickKey { Clear, Engine(String), Model(String), Custom } // core::ai::picker
pub enum Mark { Plain, Downloaded, DownloadRequired, Checking, Status(String) }
pub struct PickRow { pub key: PickKey, pub label: String, pub mark: Mark, pub enabled: bool, pub current: bool }
pub enum Row { Title(Cow<'static, str>), Item { .. }, Pick { label: String, mark: String,
               selected: bool, enabled: bool, current: bool } }       // core::menu
pub enum Footer<'a> { Error(&'a str), Hint(Cow<'static, str>) }
impl MenuState { fn footer(&self) -> Footer<'_>; fn on_paste(&mut self, t: &str); fn set_checking(&mut self, on: bool); }
```

## Testing Strategy (strict TDD; `cargo test`, `clippy --all-targets`, `fmt --check` on every slice)

| Layer | What | Approach |
|---|---|---|
| Config | tasks read, `""`, wrong types per key, non-table `ai.tasks`/task, `general` ignored, resolve/inherit | `read_text` fixtures |
| config_edit | new `[ai.tasks.code]` after `[ai]`, existing nested edited in place byte-for-byte, dotted/inline parents, CRLF, every collision → variant, round-trip parse | exact-string asserts |
| Catalog/picker | dedupe `:latest`, ordering, disabled marks, current `●`, Clear labels per target, viewport offset, wrap, rekey after detection, custom cap/filter/paste/Ctrl+U, Enter on disabled no-op | plain `#[test]` |
| Menu | task values `(default)`/`(not set)`/`(mixed)`, Set all literal, Esc levels, `s` everywhere but Custom, Enter cycles Settings, edits order | `MenuState` |
| UI | updated 7 snapshots; new: task list, Engines, Ollama models scrolled, CLI with `custom…`, Custom entry, checking/refreshing cues; 24x14 threshold; no panic 0..70 x 0..20 | insta + `TestBackend` |
| App | paste reaches buffer and never PTY; cues; W2 (Enter never closes); S1 (survives open/refresh/`s`/reload) | `update` |

## Migration / Rollout

No migration. `[ai]` keeps working, and older builds ignore `[ai.tasks.*]`. Feature-branch chain on `feat/ai-selector`, 400 changed lines per slice (additions + deletions, snapshots included).

| # | Content | Est. | Fallback split |
|---|---|---|---|
| AS1 | SEL-1/2 schema, reader, `ai_of` ripple | ~300 | — |
| AS2 | SEL-14 nested `config_edit` + tests | ~260 | — |
| AS3 | SEL-7/8 catalog + SEL-4/5/9/10 pure picker | ~400 | AS3a catalog+row derivation; AS3b state machine |
| AS4 | Retire cycling: delete `AiField`, `cycle`, Engine/Model/Status rows, old ai_tests and app cycling tests; read-only task rows | ~850 (≈90% deletions) | `size:exception (deletion-only)`, or AS4a/AS4b split by test group (interim tracker state is acceptable: only the tracker merges) |
| AS5 | SEL-3/11/12 picker wiring in `MenuState`, key model, `footer()`, per-task `edits()` | ~400 | AS5a key model + footer + edits; AS5b picker wiring |
| AS6 | Render (fixed body, picker rows, snapshots) + App (paste, `sync_checking`, W2, S1) | ~400 | AS6a render/snapshots; AS6b App |

The total is about 2,600 changed lines including ~950 deletions, higher than the proposal's 1,300–1,600, which counted mostly additions.

## Open Questions

- [x] Spec alignment: resolved by the tasks phase; specs now match SEL-3, SEL-5, SEL-6, SEL-7, SEL-8, SEL-9, SEL-12.
- [x] AS4 deletion-only slice: resolved by the orchestrator: at least two stacked deletion PRs, no size exception (see tasks.md).
