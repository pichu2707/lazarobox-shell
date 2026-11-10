# Proposal: Tool marks on models and engines (#430)

## Intent

Users cannot see which model or engine can use tools (shell, files, MCP). They pick a model, ask for work, and get text only. Mark tool-capable models and engines in the picker. Warn when a small model declares tools.

## Scope

### In Scope
- Ollama installed models: `🔧` if tools declared and ≥3B; `tools: unreliable` if declared and <3B; no mark if unknown.
- CLI engine rows (claude, codex, gemini): `🔧` from `MCP_VERIFIED`. Their model rows: no mark.
- Ollama download screens (Popular, Sizes): same rules from `Card.capabilities`.
- Text fallback `tools` on `TERM=linux` or non-UTF-8 locale, chosen in one function.

### Out of Scope
- llama.cpp marks (follow-up, after llamacpp-tools PR3 and `--jinja`).
- VRAM speed marks (later PR per the issue).
- Hugging Face rows (no tool data).
- Task row suffixes; lmstudio/vllm.

## Capabilities

### New Capabilities
- None

### Modified Capabilities
- `ai-engine-detection`: Ollama size and tool capability. MODIFIED requirement "Read-only and localhost-only" and its scenario "Only the allowed POST paths": detection may send a bounded `POST /api/show`, and `/api/show` joins the POST allowlist.
- `ai-task-models`: tool marks next to `✓`; CLI engine mark, shown whatever the tools setting is; no mark on the Ollama engine row.
- `config-menu`: tool marks on Ollama Popular and Sizes rows; glyph fallback.

## Approach

Exploration approach A. Keep `details.parameter_size` from `/api/tags`. Probe `/api/show` per model with a bound and a short timeout, cached in memory by digest. Store a tri-state capability (tools / no tools / unknown). Add a `Mark::DownloadedTools`-style variant; render text in one function.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/core/ai/parse.rs`, `src/core/ai/mod.rs` | Modified | Size and capability in `EngineState` |
| `src/runtime/ai_detect.rs` | Modified | Bounded POST probe, digest cache |
| `src/core/ai/picker.rs` | Modified | Mark variant; engine row mark |
| `src/ui/components/menu.rs` | Modified | Render, glyph fallback |
| `src/core/ai/picker/ollama.rs` | Modified | Popular/Sizes labels |

## Slices (feature-branch-chain, tracker `feat/model-marks`, production lines only)

1. S1 Ollama installed models (~180-220).
2. S2 CLI engine rows (~25).
3. S3 Ollama download screens (~40-60).
4. Follow-up: S4 llama.cpp (~60-100), separate change.

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Detection stops being GET-only (accepted) | High | Only `/api/show` added; loopback only; timeout, body cap, parallel and total call limits |
| Slow detection with many models | Med | Bound fan-out, timeout, digest cache; unknown means no mark |
| `🔧` width breaks alignment | Med | Width-aware `cells()`; text fallback |
| Wrong mark | Low | Tri-state; no mark when unsure |

## Rollback Plan

Revert slices in reverse order on the tracker. S1 revert restores GET-only detection. No config or data migration.

## Dependencies

- Ollama `/api/show` with `capabilities` (newer Ollama; older gives unknown).

## Decisions

1. Detection may send `POST /api/show`. It joins the POST allowlist, loopback only. It is bounded by a timeout, a body cap, a limit on parallel and total calls, and a digest cache.
2. `🔧` shows even when tools are off (`[ai.tools] enabled = false`). The mark describes the model, not the config.
3. The Ollama engine row gets no `🔧`. Only its models carry marks.

## Success Criteria

- [ ] Fixture tests: `🔧`, `tools: unreliable` and no mark for ≥3B, <3B and unknown.
- [ ] Probe timeout or old Ollama yields no mark.
- [ ] `TERM=linux` renders `tools`.
- [ ] Traffic audit: detection POSTs only `/api/show`, within the call limits; other POST paths stay refused.
- [ ] Manual: real Ollama shows correct marks; picker opens without visible delay.
