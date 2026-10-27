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
