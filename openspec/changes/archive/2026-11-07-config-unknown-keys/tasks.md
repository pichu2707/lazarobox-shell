# Tasks: Config Unknown Keys

This change shipped before its design and tasks were written: commit
4cd64b02 `feat(config): report unknown keys in the tables lazarobox owns`,
released in v1.11.0 (CHANGELOG). The tasks below record that work so the
change can be verified and archived. No production code remains.

## 1. Walk the owned tables

- [x] 1.1 `note_unknown_keys` and `OWNED_SECTIONS` in `src/core/config.rs`
  report unknown keys in `[statusline]`, `[tabbar]`, `[updates]` and
  `[workspace]`.
- [x] 1.2 `note_unknown_ai_keys` in `src/core/ai/config.rs` walks `[ai]` and
  every table under it, `[[ai.mcp.servers]]` entries included.
- [x] 1.3 The walk runs after the readers in `Config::parse_with_home`, so
  its problems come after every other problem, and reach the startup notice.
- [x] 1.4 Keys that are not bare TOML keys are shown quoted with control
  characters escaped.

## 2. Tests

- [x] 2.1 Every key lazarobox reads loads without a problem
  (`every_known_key_loads_without_a_problem`).
- [x] 2.2 Unknown keys, unknown tables and unknown task names are reported
  with their full path; the root of the file is not walked.
- [x] 2.3 The save keeps unknown keys byte for byte (`config_edit.rs`).

## 3. Docs

- [x] 3.1 README and CHANGELOG (v1.11.0).

## Review Workload Forecast

- Remaining production lines: 0 (shipped).
- Chained PRs recommended: No
- 400-line budget risk: Low
- Decision needed before apply: No
