# Design: Tool marks on models and engines (#430)

## Technical Approach

Exploration approach A. Detection keeps what `/api/tags` already sends (size, digest). After `reduce`, `detect_all` runs one bounded `POST /api/show` phase for Ollama, cached by digest. A pure rule turns (tools, params) into a mark. The picker carries the mark as a badge next to the existing `Mark`. The UI picks `🔧` or `tools` with one function.

## Verified facts vs assumptions

| Fact | Status |
|------|--------|
| `/api/tags` entries carry `digest`, `details.parameter_size` (`"70.6B"`, `"137M"`) and a `capabilities` array | VERIFIED live 2026-11-01, Ollama 0.30.10 (engram `ollama/api-show-shape`). Tags `capabilities` can OMIT `tools` while show reports it (deepseek-r1:70b: tags `[completion,thinking]`, show `[tools,thinking,completion]`), so tags capabilities are never used. |
| `size_params` (`ollama_lib.rs:164`) parses `8.0b`, `137m`, `8x7b` after lowercasing | Verified (code) |
| `/api/show` is POST, body `{"model": "<name>"}`, reply has `capabilities: ["completion","tools",…]` | VERIFIED live 2026-11-01 (Ollama 0.30.10): request key is `model`; `/api/show` is the source of truth for tools. Older Ollama may omit `capabilities` (then Unknown). Task S1a.1 still records real fixtures (tags + show for 2-3 models incl. a small one) before the parser. |
| `🔧` (U+1F527) is 2 cells in `unicode-width` | Expected (Emoji_Presentation, EAW=W). Pin with a test. |
| Locale check exists: `approval::names_utf8` (`chat/approval.rs:533`), read at `runtime.rs:263` | Verified |

## Architecture Decisions

| # | Decision | Rejected | Why |
|---|----------|----------|-----|
| D1 | Facts on `Detection` (`model_facts: Vec<ModelFacts>`), next to `windows` (`mod.rs:269`) | Field on `EngineState` (`mod.rs:224`) | ~38 full `EngineState` literals in tests would break; `Detection` literals all use `..Default`. Same post-`reduce` enrichment as `windows`. |
| D2 | `reduce` (`mod.rs:309`) fills name, params, digest from a new `parse::ollama_tag_facts` beside `ollama_tags` (`parse.rs:13`); `tools` starts `Unknown` | Change `ollama_tags` return type | Keeps `server_state` (`mod.rs:405`) and its tests unchanged. |
| D3 | New helper `http_post_show(port, model)` in `ai_detect.rs`; path is a const `SHOW_PATH = "/api/show"`, never a parameter; host fixed `127.0.0.1`; `exchange` (`:54`) takes the request text | Reuse `loopback::request_to` (engram `METHOD_ROUTES`, `loopback.rs:672`) or `post_stream` (`CHAT_PATHS`, `:35`) | Each POST user already has its own fixed guard (`pull.rs:20`, `:56`). A const path cannot be widened by a caller. Shares the 1 MiB `MAX_BODY` cap and `parse_response`. Spec delta: `ai-engine-detection` spec.md:333-366, rule text, "Only the allowed POST paths" and "Detection stays GET-only with chat present". |
| D4 | Bounds: 4 in parallel, 1 s per call, 2 s total deadline, at most 32 uncached calls per run; Unknown past the deadline | Unbounded `join_all` | Picker must not wait. 2 s equals `VERSION_TIMEOUT`, so detection is not slower in practice. |
| D5 | `ShowCache`: in memory, `Mutex<HashMap<digest, bool>>`, capped at 256 (cleared when full), created at `runtime.rs:304` like `ListingCache` (`ai_detect.rs:336`). Only Yes/No are cached; Unknown is retried. `r` does not bypass it. | TTL cache; file cache | Digest is content-addressed, so a hit never goes stale. |
| D6 | Show phase joins the `tokio::join!` at `ai_detect.rs:536`; runs only when ollama is `Available` | Lazy probe at picker open (approach B) | Marks ready with the rows; picker stays sync. |
| D7 | Badge field `tools: Option<ToolMark>` on `PickRow` (`picker.rs:109`) and `Row::Pick` (`core/menu.rs:578`) | Combined `Mark::DownloadedTools` variants (proposal wording) | The badge sits beside `Downloaded`, `Status` and `Plain`. Wrapping `Mark` would break `disabled_reason` (`menu.rs:547`) and the `Status` checks at `menu.rs:2211-2235` on CLI rows. |
| D8 | Glyph: `ToolGlyph::{Emoji, Text}` from `marks::tool_glyph(term, utf8)`: `Text` when `TERM == "linux"` or not UTF-8. Computed once at `runtime.rs:263`, stored by `App::with_tool_glyph` (near `app.rs:1112`), passed by `MenuPopup::with_glyph` at `ui/mod.rs:158`. Default `Text`. | Read env in the widget | Pure and testable; one function decides. |
| D9 | CLI badge: engine rows of `Kind::Cli` ids in `MCP_VERIFIED` (`chat/engine.rs:82`), set in the `push` closure (`picker.rs:220`). Ollama is a server, so it never gets one. Ignores `[ai.tools]`. | Gate on tools config | Premise 2, decision 2. |
| D10 | Rule: Yes and params ≥ 3_000_000_000 → `Capable`; Yes and less → `Unreliable`; anything else (No, Unknown, params unknown) → none | Mark `Capable` when params are unknown | Premise 3: never a wrong mark. |

## Data Flow

    GET /api/tags ─→ reduce ─→ Detection.model_facts (tools: Unknown)
                                     │
    detect_all join! ── show phase ──┤  cache hit? use it
                       POST /api/show (≤4 ∥, 1 s, 2 s total, ≤32)
                                     ↓
    picker listed_models (:500) ─→ tool_mark() ─→ PickRow.tools
    core/menu.rs:1343 ─→ Row::Pick.tools ─→ ui menu.rs tail: " ✓ 🔧 ●"

Render (`ui/components/menu.rs:88-124`): tail = mark + badge + dot. The badge is never clipped, like `●`; `mark_cap` subtracts its cells. Badge text: `Capable` → `🔧` or `tools`; `Unreliable` → `tools: unreliable` in both modes.

## Interfaces

```rust
// src/core/ai/marks.rs (new, pure)
pub enum Tools { Yes, No, Unknown }
pub enum ToolMark { Capable, Unreliable }
pub enum ToolGlyph { Emoji, Text }
pub fn tool_mark(tools: Tools, params: Option<u64>) -> Option<ToolMark>;
pub fn tool_glyph(term: Option<&str>, utf8: bool) -> ToolGlyph;
pub fn params_of(parameter_size: &str) -> Option<u64>; // lowercase + size_params

// src/core/ai/mod.rs
pub struct ModelFacts { pub engine: &'static str, pub name: String,
    pub params: Option<u64>, pub digest: Option<String>, pub tools: Tools }
impl Detection { pub fn facts(&self, engine: &str, name: &str) -> Option<&ModelFacts>; }

// src/core/ai/parse.rs
pub fn show_tools(body: &[u8]) -> Tools; // No `capabilities` → Unknown
```

## Download screens (S3)

`Screen::Sizes` (`picker/ollama.rs:39`) gains `tools: bool`. It is set at `:381` from the Popular pick with the same card name in the current screen. Popular rows (`:114-119`): badge from `card.capabilities` and the largest fitting size. Sizes rows (`:126-130`): badge per tag via `size_params`. Rows still come from `item()` (`hub.rs:292`) with `tools` set after.

## File Changes

| File | Action |
|------|--------|
| `src/core/ai/marks.rs` | Create: rule, glyph, params |
| `src/core/ai/{mod.rs,parse.rs}` | Modify: `ModelFacts`, `reduce` fill, tags facts, show parser |
| `src/runtime/ai_detect.rs` | Modify: POST helper, `ShowCache`, show phase, comment at `:545-547` |
| `src/runtime.rs`, `src/app.rs`, `src/ui/mod.rs` | Modify: cache, glyph wiring |
| `src/core/ai/picker.rs`, `picker/{hub,manage,ollama}.rs`, `src/core/menu.rs` | Modify: badge field and values |
| `src/ui/components/menu.rs` | Modify: tail render |
| Recorded `/api/tags` and `/api/show` fixtures (folder chosen by tasks) | Create |

## Testing Strategy

| Layer | What | How |
|-------|------|-----|
| Unit | `tool_mark` table, `tool_glyph`, `params_of`, `show_tools` | Pure tests; recorded fixtures (with tools, without, old shape) |
| Unit | Picker badges: Ollama ≥3B, <3B, unknown; CLI rows; no badge on ollama row | `picker.rs` tests |
| Integration | Traffic audit: only `GET /api/tags` and `POST /api/show`; limits; timeout → none; cache hit sends nothing | Fake loopback server in `ai_detect.rs` tests, paused clock |
| Render | `✓ 🔧`, `tools`, width 2, badge kept when narrow | `TestBackend` in `ui/components/menu.rs` |

## Slices (feature-branch-chain, production lines)

| Slice | Content | Est. |
|-------|---------|------|
| S1 | marks.rs, facts, show probe, cache, badge field, render, glyph, Ollama installed rows | ~250-280 |
| S2 | CLI engine badge | ~10-20 |
| S3 | Popular and Sizes badges | ~35-50 |
| Later | llama.cpp (after PR3, `--jinja`); VRAM marks | separate |

S1 is split into two chained PRs: S1a (probe + facts, no UI, ~170) then S1b (badge + render, ~100).

## Migration / Rollout

No migration. Revert slices in reverse order; reverting S1 restores GET-only detection.

## Open Questions

- [x] Exact `/api/show` request key: `model` (verified 2026-11-01). Fixtures recorded in S1a.1.
- [x] Popular row with mixed sizes (`1b 3b`): largest fitting size; spec aligned.
