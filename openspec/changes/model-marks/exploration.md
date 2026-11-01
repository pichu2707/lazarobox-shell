# Exploration: Tool marks on models and engines (issue #430)

## Goal

Show which models and engines can use tools, so the user picks a model that
can work with the shell and the code. Small models that declare tools are
marked as unreliable. New scope from the issue: CLI engines (claude, codex,
gemini) and llama.cpp.

Note: `gh issue view 430` could not run. The issue was mapped against the code.

## Agreed premises (2026-11-01)

1. UI text stays English. A model that declares tools and has 3B or more
   params shows `🔧`. A model that declares tools and has fewer than 3B params
   shows `tools: unreliable`.
2. CLI engines claude, codex and gemini show `🔧` on the engine row only. It
   means "lazarobox gives this engine its tools" and follows `MCP_VERIFIED`.
   Their model rows carry no mark.
3. Unknown capability (timeout, old Ollama, missing data): no mark. Never a
   wrong mark.
4. llama.cpp is a later slice. It waits until llamacpp-tools PR3 merges
   (manual pass P15) and needs `--jinja`.
5. `🔧` falls back to the text `tools` when `TERM=linux` or the locale is not
   UTF-8. The choice lives in one function.
6. VRAM speed marks are out of scope (a later PR, per the issue).

## Current state

| Area | Location | Finding |
|------|----------|---------|
| Engine state | `src/core/ai/mod.rs:224` | `EngineState` has no capability or size field. |
| Server detection | `src/core/ai/mod.rs:405`, `:414` | Uses `parse::ollama_tags`. |
| Tags parser | `src/core/ai/parse.rs:13` | Returns names only. Drops `details.parameter_size`. `MAX_MODELS = 256`. |
| HTTP client | `src/runtime/ai_detect.rs:50` | `http_get` is GET-only. `:548` notes `/api/show` needs POST; not made. |
| POST helper | `loopback.rs` `post_stream` | Chat-only, `CHAT_PATHS` allowlist. No probe helper. |
| Picker marks | `src/core/ai/picker.rs:93` | `enum Mark`: Plain, Downloaded, DownloadRequired, Checking, NotInList, Status(String). One mark per row. |
| Engine rows | `src/core/ai/picker.rs:201` | Engine mark is Status text. |
| Model rows | `src/core/ai/picker.rs:324` | Installed rows `Mark::Downloaded`; CLI rows Plain. |
| Render | `src/ui/components/menu.rs:88-124`, `:147` | `mark_text`; `✓` for Downloaded; mark clipped by `cells()`. |
| Disabled rows | `src/core/menu.rs:547` | `disabled_reason`. |
| Ollama library | `src/core/ai/ollama_lib.rs:42`, `:164` | `Card{capabilities, sizes}`; `size_params` parses `8b` (reuse for `8.0B` after lowercase). |
| Ollama screens | `src/core/ai/picker/ollama.rs:142`, `:153` | `pick_label` drops `Card.capabilities`; Sizes screen lost them. |
| Hugging Face | `hf.rs:37`, `picker/hub.rs:262` | `RepoHit.params` from `gguf.total`; no tool info. |
| Tool gate | `src/core/ai/chat/engine.rs:82` | `MCP_VERIFIED = ["claude","codex","gemini","ollama"]`. llamacpp not in it. |
| llama.cpp tools | `runtime/chat.rs:425`, `:632` | PR1/PR2 merged; inert until PR3 adds llamacpp to `MCP_VERIFIED`. |
| `--jinja` | `config.rs:1337` | Users pass it via managed args. Fallback notice at `runtime/chat/ollama.rs:84`. |

## Spec constraints found

- `ai-engine-detection` "Read-only and localhost-only": detection sends only
  `GET`. POST is refused except chat paths and `/api/pull`. A `/api/show`
  probe needs this requirement changed.
- `ai-task-models` "Server model list and marks": installed rows are marked
  `✓`. The tool mark must sit next to it.
- `config-menu` "Ollama screens": fixed row formats for Popular and Sizes.

## Approaches

| Approach | Pros | Cons |
|----------|------|------|
| A. Probe at detection: `parameter_size` from `/api/tags`, bounded `POST /api/show` per model, cached by digest in memory | Marks ready when the picker opens; one place | Widens detection traffic; needs POST allowlist entry |
| B. Probe lazily when the picker opens | Detection stays GET-only | Marks appear late; picker state gets async |
| C. Name heuristics (family list) | No requests | Wrong marks; breaks premise 3 |

Recommended: A, with a `Mark::DownloadedTools`-style variant.

## Slices (production lines only)

- S1 Ollama installed models: parser, probe, cache, mark, render, fallback text (~180-220).
- S2 CLI engine rows from `MCP_VERIFIED` (~25).
- S3 Ollama download screens from `Card.capabilities` (~40-60).
- S4 llama.cpp after PR3 (~60-100). Follow-up.
- Hugging Face: none (no tool data).

## Gotchas

- No emoji exists in the UI today; `✓ ● ↓ · …` are used. `🔧` is two cells wide.
- Mark text is clipped by `cells()`; `✓ tools: unreliable` is long.
- Ollama also falls back when a model rejects tools (400 "does not support
  tools", `ollama_tests.rs:493`), so a missing mark is not a hard block.
