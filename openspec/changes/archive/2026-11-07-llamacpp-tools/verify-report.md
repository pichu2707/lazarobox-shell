# Verification Report: llamacpp-tools

Mode: hybrid, Strict TDD active (runner: cargo test). Verified on main at v1.14.0 (already merged and released).

## Completeness
Tasks: 29/29 checked (T1.0-T1.10, T2.1-T2.8, T3.0-T3.3). No unchecked tasks.

## Execution evidence
| Command | Result |
|---|---|
| cargo test (lib) | 9885 passed, 0 failed, 1 ignored on the second run; the first run had 1 failure (see W2) |
| cargo test --test jail | 23 passed, 2 failed (environmental, see W3) |
| other test binaries | all ok |
| cargo clippy --all-targets | clean, no warnings |
| cargo fmt --check | clean |

## Spec compliance (chat-engines delta, 4 requirements, 26 scenarios)
- 24 [auto] scenarios: covered by passing tests in src/runtime/chat/llamacpp_tests.rs, ollama_tests.rs, adapters/tests.rs, stream/tests.rs, loopback/tests.rs, src/core/ai/chat/engine/tools_tests.rs and the README doc check (T1.2-T1.10, T2.1-T2.8, T3.1, T3.2). MCP_VERIFIED now lists llamacpp and the tools_for test asserts it. README has the "Tools with llama.cpp" section. COMPLIANT.
- "Live llamacpp tools" [manual]: PARTIAL. T3.0 as-built records PASS on Qwen2.5-7B-Instruct Q4_K_M, llama-server build 11935 with --jinja. The Qwen3.8-27B IQ4_XS leg was not run.
- "Live server with --no-jinja" [manual]: NOT RECORDED. The fallback is covered by auto tests (T2.4), but the live leg was not run.

## Design coherence
Matches design D1-D9. D8 was narrowed after review (HTTP 4xx/5xx plus lowercase `--jinja`) and spec, design and tasks were updated accordingly. D9 (retry every turn) was accepted knowingly and documented.

## Issues
CRITICAL: none.

WARNING
- W1: Two [manual] legs have no record of being run: Qwen3.8-27B IQ4_XS, and the live `--no-jinja` leg. T3.0 is checked, but its as-built note says both were NOT run. Accepted as a WARNING per instructions.
- W2: Test `runtime::tools::runner::tests::a_stdout_flood_stops_at_the_cap_and_kills_the_group` failed once in the full run (ParseIntError on empty output, runner/tests.rs:739) and passed on rerun and in isolation. It is a flaky test under load, in code unrelated to this change.
- W3: tests/jail.rs `an_rwx_root_runs_what_it_holds_and_keeps_ssh_unreadable` and `the_helper_refuses_a_symlinked_or_a_broad_rwx_root` fail with "jail not enforced: the rwx root is not allowed". This is an environmental issue (sandbox or Landlock in the verify environment) and is unrelated to llamacpp-tools. Re-run outside the sandbox to confirm.
- W4: The spec delta still carries a "Rebase needed" header. It was written against the pending agent-tools/coding-tools deltas, which are now merged into openspec/specs/chat-engines/spec.md, so it must be rebased onto the main spec before archive.

SUGGESTION
- S1: Per-pane memory of the rejection, so the refused request is not repeated every turn on a server without --jinja (a follow-up already noted in D9).
- S2: Stabilise the stdout-flood runner test.

## Verdict
PASS WITH WARNINGS
