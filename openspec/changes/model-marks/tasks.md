# Tasks: Tool marks on models and engines (#430)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~330 production total (S1a ~170, S1b ~100, S2 ~15, S3 ~45); tests, fixtures and docs not counted |
| 400-line budget risk | Low total; Low per PR (S1a is the largest, ~170) |
| Chained PRs recommended | Yes (4 small PRs requested to keep each review focused; total would also fit one PR) |
| Suggested split | S1a → S1b → S2 → S3 (follow-ups: llama.cpp, VRAM marks) |
| Delivery strategy | ask-on-risk |
| Chain strategy | feature-branch-chain (tracker `feat/model-marks`) |

Decision needed before apply: Yes (confirm the 4-PR chain, or fold into a single PR since ~330 production lines fit the 400 budget)
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: Low

Rules for every PR:
- Strict TDD: RED with compiling stubs and exact assertions, then GREEN, then refactor. Tests are never trimmed.
- Gate = `ENGRAM_PORT=8123 cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
- One work-unit commit per task group (conventional commits, no AI attribution). Nothing is committed by the planning phase.
- Production lines only count toward the budget. Re-measure before opening each PR; above 400 production lines, split further.
- PR S1a base = `feat/model-marks`; each later PR base = the previous PR branch (`feat/model-marks-<slice>`). Only the tracker merges to main.
- Docs: doc comments in every PR; README and CHANGELOG in the PR where the behaviour becomes user-visible (S1b), extended in S2 and S3.

### Suggested Work Units

| Unit | Goal | PR | Est. (prod) | Base | Depends on |
|------|------|----|-------------|------|-----------|
| S1a | Fixtures, `marks.rs`, `ModelFacts`, tags facts, show parser, bounded POST probe, `ShowCache`, spec rule | PR1 | ~170 | tracker | - |
| S1b | `ToolGlyph` wiring, `PickRow.tools`, Ollama installed rows, render tail | PR2 | ~100 | PR1 | S1a |
| S2 | CLI engine rows badge (`MCP_VERIFIED`) | PR3 | ~15 | PR2 | S1b |
| S3 | Popular and Sizes badges | PR4 | ~45 | PR3 | S1b |
| F1 | llama.cpp marks (separate change, after llamacpp-tools PR3 and `--jinja`) | - | later | - | llamacpp-tools |
| F2 | VRAM speed marks (separate change) | - | later | - | - |

Order note: S2 and S3 are independent of each other (both need S1b); the chain keeps S2 first because it is the smaller one.

## Reconciliation applied (spec and design edits by this phase)

- `specs/config-menu/spec.md`: Popular row decided by the LARGEST FITTING size (was "any shown size"); new scenario `Popular row with mixed sizes`.
- `specs/ai-engine-detection/spec.md`: request body `{"model": "<name>"}` and "never from /api/tags capabilities" (verified live); new MODIFIED scenario `Detection stays GET-only with chat present` (only `POST /api/show` on Ollama). The delta now holds all three: `Traffic audit`, `Only the allowed POST paths`, `Detection stays GET-only with chat present`.
- `design.md`: verified facts table (Ollama 0.30.10, 2026-11-01: tags carry digest, `parameter_size`, `capabilities`, but tags can omit `tools` that show reports; `/api/show` stays the source of truth; request key `model`); open questions closed; S1 split into S1a and S1b.

## S1a: Probe and data (`src/core/ai/{marks,mod,parse}.rs`, `src/runtime/ai_detect.rs`, `src/runtime.rs`, `tests/fixtures/ollama/`)

- [x] S1a.1 Fixtures: record real replies from the local Ollama (127.0.0.1:11434) into `tests/fixtures/ollama/`: `api-tags.json` (`curl /api/tags`), and `api-show-<model>.json` (`curl -d '{"model":"<name>"}' /api/show`) for 2-3 models including a small one (e.g. `llama3.2:3b-ctx`) and `deepseek-r1:70b` (tags omit `tools`, show has it). Add a hand-made `api-show-old.json` with no `capabilities` (old Ollama shape). Strip nothing but any personal paths. Fixtures come BEFORE the parser.
- [x] S1a.2 RED: `marks::params_of` table (`"8.0B"`, `"137M"`, `"70.6B"`, `"8x7B"`, empty, junk → None) and `marks::tool_mark` table (Yes+3B → Capable, Yes+2.9B → Unreliable, exactly 3_000_000_000 → Capable, No/Unknown/params unknown → None). Stubs compile.
- [x] S1a.3 RED: `parse::show_tools` over the recorded fixtures (with tools → Yes, without → No, old shape → Unknown, malformed → Unknown) and `parse::ollama_tag_facts` over `api-tags.json` (name, params, digest; missing `parameter_size` → params None).
- [x] S1a.4 RED: `Detection::facts(engine, name)` and `reduce` fill (`model_facts` with `tools: Unknown`); `server_state` and existing tests unchanged.
- [x] S1a.5 (note: refusal of non-loopback hosts and of `/api/delete`, `/api/push` is structural: `http_post_show` has no host or path parameter, and the shared guards stay covered by `loopback/pull_tests.rs`; the response cap is covered by `http_get_rejects_a_reply_over_the_cap` and the oversized-reply show test; "GET-only with chat present" is covered by `the_other_servers_never_get_a_post`, since detection does not depend on a chat) RED, fake loopback server with paused clock (`ai_detect.rs` tests): traffic audit (only `GET /api/tags` and `POST /api/show`, body holds only the model name, no Authorization/Cookie); `Tools declared`, `Tools not declared`, `Old Ollama without capabilities`; probe timeout / 500 / malformed → Unknown with engine still `available · N models`; oversized reply → Unknown and read no more than the cap; call limits (≤32 total, ≤4 parallel, 2 s total deadline); digest cache (second run sends nothing for unchanged digests, new digest probed, Unknown retried); show phase runs only when ollama is `Available`; no POST on lmstudio/llamacpp/vllm; `Detection stays GET-only with chat present`; non-loopback host and `/api/delete`, `/api/push` POSTs refused.
- [x] S1a.6 GREEN: `marks.rs` (`Tools`, `ToolMark`, `params_of`, `tool_mark`); `ModelFacts` + `Detection::facts` + `reduce` fill; `parse::ollama_tag_facts`, `parse::show_tools`.
- [x] S1a.7 GREEN: `http_post_show(port, model)` with const `SHOW_PATH = "/api/show"`, host fixed `127.0.0.1`, `exchange` takes the request text, shares `MAX_BODY` and `parse_response`; `ShowCache` (`Mutex<HashMap<digest, bool>>`, cap 256, cleared when full, only Yes/No cached); show phase joined into the `tokio::join!` with bounds (4 parallel, 1 s per call, 2 s total, ≤32 uncached); cache created at `runtime.rs` next to `ListingCache`. Update the GET-only comment near `ai_detect.rs:545`.
- [x] S1a.8 REFACTOR: single request builder for the POST; no duplicate guard logic; name the bound constants.
- [x] S1a.9 Gate.
- [x] S1a.10 Docs: module doc for `marks.rs`; doc comments on `ModelFacts`, `ShowCache`, bounds and why show (not tags) is the source of truth.

## S1b: Badge and render (`src/core/ai/{marks,picker}.rs`, `src/core/ai/picker/{hub,manage}.rs`, `src/core/menu.rs`, `src/app.rs`, `src/runtime.rs`, `src/ui/mod.rs`, `src/ui/components/menu.rs`, `README.md`, `CHANGELOG.md`)

- [ ] S1b.1 RED: `marks::tool_glyph(term, utf8)` table (`linux` → Text, non-UTF-8 → Text, `xterm-256color`+UTF-8 → Emoji, None term + UTF-8 → Emoji); pin `unicode-width` of `🔧` == 2.
- [ ] S1b.2 RED: picker tests for installed Ollama rows: `llama3:8b ✓ 🔧`, `tiny:1b ✓ tools: unreliable`, `mid:3b` (exactly 3B) `🔧`, `no tools` and `unknown` → no mark, size unknown → no mark, `[ai.tools] enabled = false` still marks, Enter on a marked row assigns exactly as an unmarked one, not-detected rows (`old:7b`, `mistral:7b`) carry only `download required`.
- [ ] S1b.3 RED: render tests (`TestBackend`, `ui/components/menu.rs`): tail `✓ 🔧 ●`, text mode `✓ tools`, `tools: unreliable` in both modes, width of the badge counted in `mark_cap`, badge kept (not clipped) in a narrow popup, no misalignment.
- [ ] S1b.4 GREEN: `PickRow.tools: Option<ToolMark>` and `Row::Pick.tools`; installed Ollama rows call `tool_mark(facts)`; `core/menu.rs` carries it through; `Mark` and `disabled_reason` untouched.
- [ ] S1b.5 GREEN: `App::with_tool_glyph`, `MenuPopup::with_glyph`; glyph computed once at `runtime.rs` next to `approval::names_utf8`; default `Text`; render tail = mark + badge + dot.
- [ ] S1b.6 REFACTOR: one function maps `(ToolMark, ToolGlyph)` to text; no env reads in the widget.
- [ ] S1b.7 Gate.
- [ ] S1b.8 Docs: README (tool marks, `tools` fallback on `TERM=linux`, detection now sends `POST /api/show` to local Ollama only); CHANGELOG entry; doc comments.

## S2: CLI engine rows (`src/core/ai/picker.rs`, README, CHANGELOG)

- [ ] S2.1 RED: engine list with claude, codex, gemini, opencode, ollama available → `🔧` on the first three only; shown with `[ai.tools] enabled = false`; status text unchanged and first, mark after; `(not detected)` engine gets no mark; CLI model rows unmarked; text fallback gives `tools` (`Linux console`, `Non-UTF-8 locale`, `Normal terminal`).
- [ ] S2.2 GREEN: in the `push` closure, set `tools` for `Kind::Cli` ids in `MCP_VERIFIED`; ignore the tools config; ollama row never marked.
- [ ] S2.3 Gate.
- [ ] S2.4 Docs: README and CHANGELOG lines for the engine mark.

## S3: Download screens (`src/core/ai/picker/ollama.rs`, `src/core/ai/picker/hub.rs`, README, CHANGELOG)

- [ ] S3.1 RED: Popular rows: `llama3.2` (1b 3b, tools) `🔧`, `smol` (1b, tools) `tools: unreliable`, `chat-only` (7b, no tools) none; mixed sizes use the LARGEST FITTING size (`mix` fits only 1b → unreliable; `mix2` 1b 3b 8b → `🔧`); exactly 3B → `🔧`; missing card data and fallback list → no mark and no row dropped; text fallback `tools` on `TERM=linux`.
- [ ] S3.2 RED: Sizes rows: `llama3.2:1b` unreliable, `llama3.2:3b` `🔧`, ` · too big for this PC` suffix still ends the row; `Screen::Sizes` carries `tools: bool` taken from the same-name Popular card; Confirm screen carries no mark; `Pick without typing` effects and order identical on a marked row; rows stay selectable and keep order.
- [ ] S3.3 GREEN: `Screen::Sizes.tools`; Popular badge from `card.capabilities` + largest fitting size; Sizes badge per tag via `size_params`; `item()` rows get `tools` set after. No new request.
- [ ] S3.4 Gate.
- [ ] S3.5 Docs: README and CHANGELOG lines for download screens.

## Manual

- [ ] M.1 [manual] On a real machine with Ollama (a tool model ≥3B, a tool model <3B, a model without tools): open the model picker on a UTF-8 terminal (first `🔧`, second `tools: unreliable`, third none), then on `TERM=linux` (`tools` in place of `🔧`); the picker opens without visible delay; CLI engine rows show `🔧` for claude/codex/gemini and none for opencode/ollama; Popular and Sizes screens show matching marks; Ollama stopped or old → no marks and no error.
- [ ] M.2 [manual] ai-engine-detection "Real machine": detection with Ollama, three CLIs, opencode alias and a foreign service on `:8080` behaves as before.

## Follow-ups (separate changes, not in this chain)

- [ ] F1 llama.cpp marks: after llamacpp-tools PR3 and `--jinja` support.
- [ ] F2 VRAM speed marks.
