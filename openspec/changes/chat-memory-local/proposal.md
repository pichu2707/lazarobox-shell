# Proposal: Local project memory (chat-memory-local)

## Intent

Today a chat only remembers the project through engram, and memory is off by default (opt-in plus privacy warning). Without engram every chat starts blind: decisions, conventions and fixes from earlier chats are lost, and the user repeats them. The owner wants memory that is automatic at project entry, ON by default, works with local models that have small contexts, and treats engram as a plus rather than a requirement.

## Users and situations

| Situation | Need |
|---|---|
| Big repo with `frontend/` and `backend/` | One memory per repo root; the current area's notes rank first; entering from any subfolder gives the same memory |
| Local model, small context | Only relevant notes per turn, within a smaller per-model budget |
| Team sharing | `.lzr/` is gitignored by default; removing the entry shares memory through git |
| Cloned repo with a committed `.lzr/` | Foreign notes are untrusted until the user trusts them once |

## Scope

### First slice (S1): automatic context at project entry, end to end
- `.lzr/` store at the git repo root, created on first save; `.lzr/` appended to `.gitignore` with a visible line.
- Small notes, each with an `area` tag (repo-relative subfolder or path) and a `kind` (existing 7 kinds).
- End-of-chat summary auto-saved locally as a durable note (redacted, no approval).
- Ranked injection within a per-model budget, reusing `context_block()` as the sanitize/wrap stage.
- `/memory why`: which notes were injected and why (score parts).
- Local memory ON by default; the engram opt-in stays as is.
- Safe default until S3: notes not created on this machine are NOT injected, with a visible line saying so.

Each slice stays under 400 production lines; S1 may split into S1a (store + gitignore + summary save) and S1b (ranker + injection + `/memory why`).

### Later slices
- S2: `/memory` panel (see, pin, delete) and cleanup (duplicates, stale paths, superseded; pinned never removed).
- S3: trust prompt for foreign notes, once per project, remembered.
- S4: model saves tagged notes mid-chat (visible line per save) and the conflict flow (ask first; on OK replace with a visible trace; on no keep the old note).
- S5: automatic engram mirror; engram notes join the ranker as candidates, deduped by topic key, local wins ties.

### Non-goals
- Embeddings or vector search.
- Cross-project memory (engram's job).
- Syncing `.lzr/` other than through git.
- Storing reasoning, transcripts or full command output.
- Changing the engram opt-in, privacy warning or summary-approval flow for engram.

## Business rules
- One memory per git repo root; subfolders are area tags.
- No fixed note cap; cleanup removes duplicates and stale notes; pinned notes are never removed.
- The auto-saved end summary is durable at once.
- Never stored: secrets, keys, credentials, personal data, customer data, full command output. Notes are redacted and cleaned before saving.
- Notes are injected as data, never as instructions, and can never trigger tools or actions.
- Foreign notes need trust before use (S1: never used; S3: one prompt).
- Mirror to engram happens automatically only when engram memory is enabled and acknowledged (assumption, see open questions).

## Security invariants
- Model-facing text is fixed lazarobox text plus data that has been wrapped, capped and redacted.
- `Entry::System` and the step trail are display-only; they never reach the model or become notes.
- Never fail silently: every outcome (save, skip, refusal, gitignore edit, foreign notes ignored, engram down) gets a visible line.

## Edge cases
| Case | Expected behavior |
|---|---|
| No git repo | No `.lzr/`; memory keyed to the chat folder in the private chat store, or off with a visible line (design decides) |
| Read-only repo | No write; visible line; chat continues without saving |
| `.gitignore` already ignores `.lzr` | No edit, no duplicate entry |
| `.gitignore` without trailing newline | Add a newline before the entry; never rewrite other lines |
| Engram down | Local memory works unchanged; mirror retried later; one visible line |
| Notes file hand-edited or malformed | Skip bad entries, keep the rest, visible line with the count; never crash, never rewrite the file silently |
| `git pull` brings new foreign notes | Treated as foreign: not injected (S1) or prompted (S3) |
| Orchestrator workers | Display-only for notes: workers never save notes and their output never becomes a note |
| Windows paths | Area tags stored repo-relative with `/` separators; root resolution handles `\` and drive letters |

## Capabilities

### New Capabilities
- `project-memory-store`: `.lzr/` store, note format, area/kind tags, gitignore handling, foreign detection, trust, cleanup.
- `memory-retrieval`: relevance ranking, per-model budget, injection, `/memory why`.

### Modified Capabilities
- `chat-memory`: "Opt-in and the privacy warning" (local memory ON by default; engram stays opt-in), "Context injection" (source becomes the ranked provider, budget per model), "Summary with approval" (local end summary auto-saved), later the `/memory` panel and model saves.
- `engram-client`: automatic mirror of local notes (S5).
- `agent-orchestrator`: workers stay display-only for memory notes.

## Approach

Approach C from exploration, local first: a local executor answers the same `MemCmd` commands as the engram worker (`src/core/ai/memory/port.rs`), so the chat code keeps one memory port. A pure ranker scores notes (area match with the current folder, words shared with the question and files in play, kind weight, pinned, recency, path still exists) and fills the budget. `context_block()` keeps sanitizing, escaping and wrapping. `redact.rs` runs before every save.

## Affected Areas

| Area | Impact | Description |
|---|---|---|
| `src/core/ai/memory/` | Modified/New | Local store, ranker, port executor; `context.rs` reused |
| `src/core/ai/chat/memory.rs`, `chat/mod.rs` | Modified | Injection source, end summary save, `/memory why` |
| `src/core/ai/config.rs` | Modified | Local memory default ON |
| `src/runtime/engram/` | Modified (S5) | Mirror |
| `openspec/specs/chat-memory/spec.md` | Delta | Default-off scenario changes |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Prompt injection via notes (cloned/shared `.lzr/`) | High | Foreign notes not used until trusted; data wrapper with escaped `<`; instruction-like notes flagged; notes never trigger tools |
| Secret leak into a committed `.lzr/` | Med | Redaction before save; gitignored by default; never-store rules |
| Stale notes mislead the model | Med | Path-exists check, recency decay, cleanup, conflict flow (S4) |
| Budget overflow on local models | Med | Per-model budget, smaller for local engines; block counted in the history cap |
| Two sources of truth with engram | Med | Local is the truth; engram is a mirror; dedupe by topic key, local wins |
| `.gitignore` mutation surprises the user | Low | Append-only, idempotent, visible line, only when lazarobox creates `.lzr/` |

## Open design questions
- Note file format: one JSONL file vs one file per note; merge-friendliness for teams (per-note files merge cleanly, JSONL conflicts on concurrent appends).
- How "foreign" is detected: local index of note ids created here vs an author/machine id in each note; does a pull with new foreign notes ask again.
- Personal-data detection: patterns (email, phone, ID numbers) on top of `redact.rs`; false-positive handling.
- Notes that look like instructions to the model: reject at save, or keep and flag in `/memory why` and the panel.
- Relevance scoring without embeddings: term overlap vs tag-only; weights; explainability for `/memory why`.
- No-git folders: fall back to the chat folder in chat.db, or no memory.
- Does the automatic engram mirror require the existing engram privacy acknowledgment (proposed: yes).

## Rollback Plan
Each slice is one revertable PR. A config switch turns local memory off, which restores today's behavior (engram path only). `.lzr/` and the `.gitignore` entry are inert data the user can delete; no schema migration of chat.db is needed in S1.

## Dependencies
- Existing `redact.rs`, `context_block()`, `MemCmd` port.

## Success Criteria
- [ ] A second chat in the same repo, without engram, shows a memory line and answers from the first chat's end summary.
- [ ] Entering from `frontend/` ranks `frontend` notes above `backend` notes; root and subfolder resolve to the same memory.
- [ ] Injected memory never exceeds the model's budget; local engines get the smaller budget.
- [ ] `/memory why` lists every injected note with its score parts.
- [ ] Foreign notes are never injected before trust; a visible line says so.
- [ ] Redaction tests: secrets and personal-data patterns never reach `.lzr/`.
- [ ] Every outcome in the edge-case table produces a visible line; no silent failures.

## Proposal question round
Two owner rounds (2026-11-21/22) settled the product decisions above. Assumptions needing owner review:
1. In S1, foreign notes are ignored (with a visible line) until the S3 trust prompt exists.
2. The engram mirror requires engram memory to be enabled and acknowledged.
3. Outside a git repo, behavior is a design choice (chat-folder fallback or off).
