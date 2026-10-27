# Apply progress: llama-server-install

## PR1 (branch feat/llama-server-install-pr1) - done except manual task

Mode: Strict TDD. Delivery: stacked-to-main.

- [x] 1.1 RED, [x] 1.2 GREEN
- [ ] 1.3 [manual] pending for the user (download without llama-server)

### TDD Cycle Evidence

| Task | Test file | RED | GREEN | TRIANGULATE | REFACTOR |
|------|-----------|-----|-------|-------------|----------|
| 1.1/1.2 HF row ungated + note | src/core/ai/picker.rs (tests) | compile failure on missing HF_NEEDS_SERVER, then 4 failing asserts | cargo test 6096 passed | NotInstalled and missing-from-detection; installed (NotRunning, Available); pending detection | none needed |
| 1.1/1.2 llamacpp on Engines level | src/core/ai/picker.rs (tests) | failed before `is_missing_llama_server` | green | not-installed vs pending detection vs already assigned (listed once) | none needed |

Existing tests/snapshots updated deliberately (each diff = one added `llamacpp not installed` row, or the HF row now present with a note): hub_tests, disabled_tests, picker_tests, reanchor_tests, 3 insta snapshots.

### Deviations
- Note is a row mark (`needs llama-server to run`), not separate wrapped note rows.
- Scenario "Row still hidden without curl": no test; the picker is pure and has no curl signal.

### PR1 review fixes

- WARNING 2/1: the Models level of a NotInstalled llamacpp now says `llama-server not installed: install it to use these models` instead of `engine not detected` (RED: `an_unmanaged_or_absent_llama_server_keeps_its_rows` pin and a new menu test failed first; GREEN after the new branch in `model_rows`). No version is promised for the install entry.
- WARNING 3: menu-level test `a_not_installed_llamacpp_explains_itself_and_leaves_the_draft_alone` (Enter on the llamacpp row, cursor on the HF row, `s` keeps `ai_draft()`, Esc returns to Engines) plus snapshot `snapshot_models_level_of_llamacpp_without_llama_server` at MIN_WIDTH.
- SUGGESTION 2: `reanchor_tests` also asserts the row count (4).
- SUGGESTION 1: the real detector always emits llamacpp (`reduce` walks the whole CATALOG), so the missing-from-detection case is test-only; the comment says so.
- Known limitation, not fixed: at MIN_WIDTH (24) the explanation row, the footer and the HF row (its mark hides the label) are clipped with an ellipsis; at the 62-wide popup the text fits.
- Pin `hub_tests.rs:61` (`src/core/ai/picker/hub_tests.rs`) pins only the HF mark, so it needed no change.
