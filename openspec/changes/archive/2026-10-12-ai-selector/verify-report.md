# Verification Report: ai-selector

Mode: hybrid, Strict TDD. Checkout: feat/ai-selector-12-manual (tracker 7bef880 + manual ticks).

## Verdict: PASS WITH WARNINGS (0 CRITICAL, 3 WARNING, 2 SUGGESTION)

## Gate
- cargo test: 1095 unit + 1 integration passed, 0 failed.
- cargo clippy --all-targets -- -D warnings: clean.
- cargo fmt --check: clean.
- No stray .snap.new files; git status clean.

## Completeness
All tasks 1.1-12.5 are ticked, 0 unchecked.

## Spec compliance
169 scenarios across 4 specs: 163 auto (150 plain + 13 TestBackend/insta), all covered by passing tests per the tasks.md coverage table (suite green); 6 manual (Docs say stale, Positions in Kitty, Persistence in Kitty, Menu in Kitty, AI menu in Kitty, Real Ollama models), ticked 12.1-12.5 by the user in Kitty. Docs say stale is also covered by README lines 63-67.

## Strict TDD
apply-progress records RED/characterization evidence per PR; test count grew 1076 -> 1094 -> 1095. All files referenced exist and pass. Note: the coverage mapping is from tasks.md, not a per-test runtime trace.

## Design ADRs SEL-1..SEL-14
Checked against code and tests: no deviations found. Space in the custom field: spec (config-menu "Space is NOT typed") and design SEL-10 (whitespace dropped) / SEL-11 are consistent. SEL-13 gotcha (App::on_paste not calling MenuState::on_paste, set_checking never called) fixed in PR11.

## Findings
WARNING 1: clip() (src/ui/components/tab_bar.rs:80) measures a VS16 emoji with the wrong width, so the ` · download required` mark can be pushed out of the line. Cosmetic, pre-existing helper; does not block archive. Fix in a follow-up.
WARNING 2: the engine label shrinks to `…` at 24 columns (the minimum full popup). The row stays usable and the footer explains it; does not block archive.
WARNING 3: model_rows / disabled_reason show `engine not detected` for an empty detection (src/core/ai/picker.rs:245, src/core/menu.rs:159); wording is imprecise when the engine is detected with no models. Does not break a spec scenario; does not block archive.
SUGGESTION 1: footer() rebuilds pick_rows() on every draw (src/core/menu.rs:268). Small lists, negligible cost; cache if the catalog grows.
SUGGESTION 2: no per-test runtime trace of scenario -> test; consider naming tests after scenarios.

## Archive readiness
Nothing blocks archive. Remember the archive step must update the Purpose lines of the config-menu and configuration main specs (noted in the delta specs).
