# Verify report: model-marks (#430)

Tracker feat/model-marks at b879d9c4 (S1a, S1b, S2, S3 merged). Mode: Strict TDD.

## Verdict
PASS. CRITICAL 0, WARNING 1, SUGGESTION 2.

## Gate
- cargo fmt --all -- --check: OK
- cargo clippy --all-targets --locked -- -D warnings: OK
- cargo test --locked: 7481 passed, 0 failed, 1 ignored (lib); all other targets OK
- Working tree clean (verify committed nothing).

## Tasks
S1a 10/10, S1b 8/8, S2 4/4, S3 5/5 checked and matched by code and tests. Open as expected: M.1, M.2 (manual), F1 (llama.cpp), F2 (VRAM).

## Spec compliance (auto scenarios)
- ai-engine-detection: tools declared / not declared / old Ollama / failure+timeout / oversized / call limits (32 total, 4 parallel, 2 s deadline) / digest cache / unknown never cached / size missing / traffic audit / POST only api/show / other servers never get a POST / loopback and POST path guards: covered in src/runtime/ai_detect/show_tests.rs, src/core/ai/{marks,parse}.rs tests, loopback tests. COMPLIANT.
- ai-task-models: installed rows (tools, small, exactly 3B, no tools, unknown, switch off, Enter parity, not-detected rows), CLI engine rows (MCP_VERIFIED, switch off, models unmarked, refused, update needed, not detected), text fallback (marks_tests.rs, ui menu tool_badge tests). COMPLIANT.
- config-menu: Popular largest-fitting-size (mix, mix2), llama3.2/smol/chat-only, Sizes per tag with suffix, missing card data and fallback, Confirm without mark, pull flow parity (ollama_tests.rs). COMPLIANT.
- Docs: README and CHANGELOG updated for all slices.

## Issues
CRITICAL: none.

WARNING
- W1: Documented deviation in S2: engines refused by the jail are left unmarked (spec says the same: refused and outdated carry no mark), so it is consistent; but apply-progress calls it a "deviation" to keep a snapshot unchanged. Confirm the spec scenario "Refused or outdated engine" is the intended source (it is; tests a_refused_engine_gets_no_mark and a_claude_that_needs_an_update_gets_no_mark pass). No action needed beyond awareness.

SUGGESTION
- S-1: Run M.1 and M.2 on a real machine (Ollama 0.30.10+, UTF-8 and TERM=linux) before releasing.
- S-2: cargo test needs a `target` dir in the worktree for runtime::notify tests; consider making those tests create it.

## Open manual items
M.1 real-machine picker marks (UTF-8 and TERM=linux, engine rows, Popular/Sizes, Ollama stopped). M.2 ai-engine-detection "Real machine". Follow-ups F1 llama.cpp marks, F2 VRAM marks.

next_recommended: sdd-archive (after M.1/M.2 are accepted by the user).
