# Tasks: Local project memory (chat-memory-local)

Sources: proposal.md, design.md (engram #4103), specs/{project-memory-store,memory-retrieval,chat-memory,engram-client,agent-orchestrator}. Owner decisions of 2026-11-22 are applied below.

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated production lines (total) | ~4,700 (non-test .rs, no blanks/comments, as rustfmt lays them out) |
| 400-line budget risk | High overall; every slice below is planned under 400 |
| Chained PRs recommended | Yes |
| Suggested split | S1a > S1a2 > S1a3 > S1a4 > S1b > S1b' > S1b2 > S1b2' > S1c > S1c' > S1c'' > S2a > S2a' > S2b > S3 > S4a > S4b > S5 > S5' |
| Delivery strategy | auto-chain |
| Chain strategy | stacked-to-main |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: High

Re-slice after apply batch 1: S1a measured about 743 production lines against a ~390 forecast, because rustfmt lays code out about 1.9 times longer than the compact estimates. S1a was split into S1a (350, measured) and further, after the review of #833 added checks that pushed it to 432, into S1a (350), S1a2 (gitignore edit and port), S1a3 (thread) and S1a4 (app routing, 70). The estimates of every remaining slice were multiplied by about 1.9 and the slices that would pass 400 were split again. Slices at 355-385 are tight: measure while applying and move the last work unit to a follow-up slice if one crosses 400.

| Slice | Goal | Est. prod lines | Risk |
|-------|------|-----------------|------|
| S1a | Note schema, area, ledger, `local` config switch, `.lzr` deny, `memory_root` with its checks (measured) | 350 | Low |
| S1a2 | `.gitignore` edit and the port (est.) | 215 | Low |
| S1a3 | Runtime thread: load, save, ledger, machine id (est.) | 270 | Medium |
| S1a4 | App routing and load lines (measured) | 70 | Low |
| S1b | `pii.rs` and `screen.rs`: the never-store screen, pure | 250 | Low |
| S1b' | Save pipeline in `app/project_memory.rs`, outcome lines, approved summary and compaction saves | 240 | Low |
| S1b2 | `chat/digest.rs`: cadence, history input, upsert by topic, first-save line | 200 | Low |
| S1b2' | Local-engine policy, `/cost` usage record, close-time bound | 180 | Medium |
| S1c | `rank.rs` and the budget cut (pure) | 230 | Low |
| S1c' | `local_block` with nonce, send-path injection, memory lines | 230 | Medium |
| S1c'' | `/memory why`, orchestrator and engram coexistence, docs | 220 | Low |
| S2a | `memory_view.rs` and the panel | 200 | Low |
| S2a' | Pin and delete | 200 | Low |
| S2b | Cleanup and user note editor | 360 | Medium |
| S3 | Trust prompt | 380 | Medium |
| S4a | `memory_note` tool | 380 | Medium |
| S4b | Conflict flow | 380 | Medium |
| S5 | Mirror plan (pure), gate, ledger `mirrored` | 250 | Medium |
| S5' | Mirror executor, `lzr/` slug and route, candidates | 245 | Medium |

Deviations from the first slice list: S1b of the design was split into S1b and S1b2 for the owner's digest policy, S2 and S4 into a and b, and now every slice whose 1.9x estimate passes 400 is split again, as above.

## Owner decisions (2026-11-22)

1. Engram "per project" opt-in = today's global `enabled` + `acknowledged` plus a resolved engram project (readiness `Ready(project)`). No new toggle.
2. Linked worktrees share the main checkout's `.lzr/` (D3 `store_root`).
3. The digest runs on every engine, paid CLI included; cost accepted. On local engines: small input budget, short output, only when the chat is idle, never blocks or slows a turn, skipped or postponed when the model is busy or context is tight, usage recorded so `/cost` shows it. Engram topic `sdd/chat-memory-local/decisions-digest`.
4. Design wins over spec; conflicts below.

## Spec vs design conflicts (design wins)

| # | Topic | Spec says | Design (applied) |
|---|-------|-----------|------------------|
| C1 | Block wrapper | `source="local"`, plain `</project-memory>` | `source="lzr" id="NONCE"`, closing tag carries the nonce (D6); fixed data-not-instructions line kept |
| C2 | Budget | never above the 2000-byte cap | CLI 2400; server clamp(tokens*4*3%, 600, 1500); unknown 900 (D5) |
| C3 | Cut rule | stop at first note that does not fit | greedy with skip, max 12 (D5) |
| C4 | Foreign | id not in local index | id in ledger AND content hash matches; hand-edited own note is foreign (D2) |
| C5 | Instruction-like | keep all, flag, inject as data | strong markers rejected at save and never injected at load; weak flagged and injectable (D8) |
| C6 | Command output | >5 lines or >500 chars | >6 lines with >=40% prompt/log/stack shapes, or fenced >5 lines (D8) |
| C7 | Personal data | email, phone, national ID | also IBAN mod-97, card Luhn, ES DNI/NIE letter, US SSN (D8) |
| C8 | End summary | engine call at chat close | idle digest during chat, upsert; at close only a bounded <=1 s attempt (D7) |
| C9 | Summary size | note text cap 1000 | summary note <=1500; model notes <=600/6 lines; 1000 cap for other notes |
| C10 | Cleanup | near duplicates | exact duplicates only (newest kept), superseded >30d, stale >30d (D10) |
| C11 | Mirror topic | `chat/<kind>/<slug>` | `lzr/<id>`; slug/route accept `lzr/` (D12) |
| C12 | Engram down | one line per run | failures queued in ledger, no new line (D12) |

Task rule for C12: the existing engram-down notice path stays as is; the mirror adds no line.

## Global rules (every slice)

- Worktree: `git worktree add ../lazarobox-shell-<slice> -b feat/chat-memory-<slice> main` (never under /tmp). Each PR is based on main after the previous one merges.
- MSRV 1.88: no `if let` guards in match arms.
- Strict TDD: RED test first (public interface), then GREEN, then REFACTOR. No silent failure: every outcome gets an `Entry::System` line.
- CHANGELOG entries go under `[Unreleased]` only. README.md and README.es.md change together in the same PR, same sections.
- Slice exit gate (last tasks of each slice): `cargo test`; `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check`; `rm -rf target` in the worktree; then remove the worktree after merge.

## S1a: Store and runtime port (base: main)

- [x] 1a.1 RED `src/core/ai/memory/local/note.rs` tests: JSON round trip; kind `learning` refused; text cut at 1000 with line `note cut to 1000 characters`; unknown fields ignored; missing area reads as `.`; unknown kind unreadable.
- [x] 1a.2 RED `local/area.rs` + `workspace.rs` tests: root and subfolder resolve to one store; linked worktree resolves to the main checkout's `.lzr/` with area relative to the work root; submodule has its own; Windows `C:\repo\frontend\src` gives `frontend/src`; reject prefix/root/`..`.
- [ ] (S1a2) 1a.3 RED `local/gitignore.rs` tests, one per D4 outcome: already ignored, `!.lzr` respected, no file (create), append with newline, CRLF, write error (note still saved), `.lzr/` uncreatable; `.git/info/exclude` honored; other bytes untouched; touched only on creation.
- [x] 1a.4 RED `local/ledger.rs` tests: own `{id: hash}` makes a note local; unknown id, changed hash and hand-edited own note are foreign; read-merge-write union; machine id `m-<16hex>`, note id `n-<16hex>`.
- [ ] (S1a2) 1a.5 RED `src/runtime/memory_local.rs` integration (tempfile repos): `.lzr/` absent after open, present after first save; 10 files with 2 invalid load 8 and leave the 2 byte-identical; `<<<<<<<` file and 5 MiB binary skipped and counted; read-only repo and read-only `.lzr/` write nothing but still read; no git repo writes nothing; atomic write leaves old notes intact.
- [x] 1a.6 RED `src/core/ai/tools/paths.rs` test: `.lzr` is in `WRITE_DENIED_DIRS`, and the ledger root `<data>/lazarobox/memory` is denied to tools.
- [ ] 1a.7 (config default and read done in S1a; the `local = false` sends nothing test is in S1a2) RED `config.rs` test: `[ai.memory] local` defaults true; `local = false` reads and writes nothing in `.lzr/`; engram `enabled`/`acknowledged` unchanged.
- [ ] 1a.8 (S1a: `mod`, `note`, `area`, `ledger` done; `gitignore` and `port` in S1a2) GREEN `local/{mod,note,area,gitignore,port,ledger}.rs`: schema, `LocalCmd`/`LocalReply` carrying typed outcomes (saved, skipped, foreign count, unreadable count, gitignore result, read-only, no-git).
- [x] 1a.9 GREEN `workspace::memory_root(cwd)`, `config.rs` local switch, `paths.rs` deny entries.
- [ ] (S1a2) 1a.10 GREEN runtime thread `memory_local.rs` + routing `Effect::LocalMemory` / `AppEvent::LocalMemory` in `app.rs`/`runtime.rs`; notes load once at project entry. Ledger rules (S1a3): a ledger file that is not valid UTF-8 maps to `Damaged`, and a `Damaged` or `Newer` ledger is reported, trusts no note and is never rewritten (a save is refused with a visible line).
- [ ] 1a.11 REFACTOR; CHANGELOG `[Unreleased]` entry. Run exit gate. Split by slice: S1a (store core), S1a2 (gitignore, port), S1a3 (thread), S1a4 (app routing, CHANGELOG for the load at chat open); each runs the exit gate on its own PR.

## S1b: Screen, PII, save pipeline (base: main after S1a)

- [ ] 1b.1 RED `local/pii.rs` tests: email, phone, IBAN (valid and invalid checksum), card (Luhn), DNI/NIE letter, US SSN masked as `[REDACTED]` and counted; invalid checksums and plain text such as `we use a password manager` unchanged; idempotent; no panic on arbitrary and very long input.
- [ ] 1b.2 RED `local/screen.rs` tests: secrets `sk-abcDEF1234567890`, `password=hunter2`, `ghp_...` and `<private>x</private>` never appear in any byte under `.lzr/`; 40 lines of compiler output rejected (`note not saved: looks like command output`); fenced >5 lines rejected; strong instruction markers rejected at save; weak markers flagged and kept; `N items redacted in saved note` count.
- [ ] 1b.3 RED screen-at-load tests: a hand-edited note with a secret is masked on load; a strong-marker note is never injectable; weak flag preserved.
- [ ] 1b.4 RED `app_tests/project_memory.rs`: one visible line per outcome (saved, redacted, refused kind, cut, gitignore added/failed, read-only, no git once per chat); lines are `Entry::System` and never reach the next request or any note.
- [ ] 1b.5 GREEN hand-written linear `local/pii.rs` and `local/screen.rs` (clean, redact, pii, command-output shapes, instruction markers).
- [ ] 1b.6 GREEN `app/project_memory.rs` save pipeline; approved summary and rolling compaction (`app/compact.rs`) saved locally regardless of `save_summaries`, upsert with origin `summary`, redacted, area of chat folder.
- [ ] 1b.7 REFACTOR; CHANGELOG. Run exit gate.

## S1b2: Idle digest (base: main after S1b)

- [ ] 1b2.1 RED `chat/digest.rs` cadence tests with fake clock/engine: runs after the 2nd user turn then every 3, needs >=800 new chars, uses sendable history only (no reasoning, no `Entry::System`, no memory block, no worker output); note topic `chat/<id>-<born>/summary`, resumed chat reuses newest `chat/<id>-` topic; kind `discovery`, origin `summary`, <=1500 chars; first save line then transient notice.
- [ ] 1b2.2 RED local-engine policy tests: small input budget and short output cap (named constants, tunable); never starts while a stream runs or a send is queued; a send during a digest cancels it and proceeds without waiting; postponed to next idle; skipped when the local server is busy or `history + block + digest input` exceeds the known window (unknown window: conservative skip); one visible line per skip/failure reason.
- [ ] 1b2.3 RED usage test: digest usage is recorded as its own entry (label `memory digest`) and appears in the `/cost` view model, for local and paid CLI engines.
- [ ] 1b2.4 RED close tests: quit holds at most 1 s (engine that never answers); latest digest is kept; no answer means no save and no line; failed attempt shows `memory: summary not saved: <reason>`.
- [ ] 1b2.5 GREEN `chat/digest.rs` (side run like `begin_compaction`, `SUMMARY_PROMPT`), policy by engine kind, usage record through the existing usage ledger, close handling in `app/project_memory.rs`.
- [ ] 1b2.6 Record the decision under engram topic `sdd/chat-memory-local/decisions-digest`; CHANGELOG. Run exit gate.

## S1c: Rank, inject, `/memory why` (base: main after S1b2)

- [ ] 1c.1 RED `local/rank.rs` tests (pure, fake clock): area first (frontend over backend); question beats area; pinned first, stale last; deterministic; area 30/20/10/0; terms 10 each max 30; kind weights; user +10; latest summary +15; recency `10*0.5^(days/30)`; refs +5/-15; excluded: superseded, foreign, strong-flagged, unrelated unpinned; tie order (score, pinned, updated, id); 5,000 notes ranks within a fixed time bound.
- [ ] 1c.2 RED budget tests: CLI 2400; server clamp 600..1500; unknown 900; local engine strictly smaller than hosted; greedy skip, max 12, 280-char lines (600 latest summary); no partial note; block chars subtracted from the history cap and never dropped.
- [ ] 1c.3 RED `context.rs` `local_block` tests: block `<project-memory source="lzr" id="NONCE" project=..>`, fixed data-not-instructions line, closing tag with the same nonce; `</PROJECT-MEMORY>` spoof and U+034F/115F/1160/3164/FFA0/180E strip leave only own delimiters, no `<`; ANSI stripped; `sk-...` redacted on the way out; nonce differs per load and is not guessable (urandom, fallback test); CLI block first on stdin and absent from argv; empty memory means no block and no line.
- [ ] 1c.4 RED chat tests: `▸ memory (1 note)` / `(N notes)`; second chat without engram answers from chat 1's summary; foreign line `memory: N notes from others ignored (not created on this machine)` once per open; `memory: skipped N unreadable notes in .lzr/`; foreign notes never injected or ranked (S1).
- [ ] 1c.5 RED `/memory why` tests: rows with area, shared words, kind, pinned, recency, path exists, `instruction-like` flag; foreign and unreadable counts; `no notes were injected`; output is `Entry::System`, no ChatSend, not in next request; `/memory` with local off shows `Memory is off; enable it in the menu (Ctrl+Space m)` and touches nothing.
- [ ] 1c.6 RED orchestrator tests: worker finishing with `decision: use Axum` creates or changes nothing under `.lzr/`; worker toolset has no memory tool; worker output and step trail absent from the digest input; coordinator chat follows chat-memory.
- [ ] 1c.7 RED engram coexistence: engram off means no engram request; engram ready until S5 gives a second `source="engram"` block within the shared history cap; engram-off chats produce no engram line.
- [ ] 1c.8 GREEN `local/{rank,why}.rs`, `context.rs` `local_block` (reuses neutralize/clean, adds nonce), `chat/memory.rs` `LocalMemory`, `chat/mod.rs` send path, `folder.rs` `/memory why`, `app/chat.rs` lines. Re-rank per turn against the question; injection record per turn.
- [ ] 1c.9 Docs: README.md + README.es.md (memory on by default, `/memory why`, `.lzr/` and gitignore, local switch), same sections and order; CHANGELOG `[Unreleased]`. REFACTOR. Run exit gate.

## S2a: Panel (base: main after S1c)

- [ ] 2a.1 RED `memory_view.rs` + `ui/components/chat/memory.rs`: insta snapshots at 40/80/120 columns; `no notes yet` on empty store; 10x2 and 1x1 do not panic; keys arrows, Enter, p, Esc; 40-col drops age/machine.
- [ ] 2a.2 RED pin and delete: pin raises the pinned score and ranks first next turn; `d` then `y` deletes, `d` then decline keeps; ledger updated; visible line per action.
- [ ] 2a.3 GREEN panel (CostView-like overlay), pin/unpin/delete through `LocalCmd`. CHANGELOG; README pair if keys documented. Run exit gate.

## S2b: Cleanup and user notes (base: main after S2a)

- [ ] 2b.1 RED cleanup (D10): own notes only; never pinned or foreign; exact duplicates keep newest; superseded >30d; stale >30d (refs and area gone); `memory cleanup: removed N notes`; pinned stale survives; no run without a line.
- [ ] 2b.2 RED user note editor (key `c`): origin `user`, same screen/redaction/size rules, lines on refusal.
- [ ] 2b.3 GREEN `local/cleanup.rs`, editor wiring. CHANGELOG; README pair. Run exit gate.

## S3: Trust prompt (base: main after S2b)

- [ ] 3.1 RED tests: one prompt per project when foreign notes exist; yes joins retrieval as data and is remembered (ledger `trust`, `trusted{id:hash}`); decline remembered, never injected after; new or changed foreign notes ask again, at most once per chat entry; key `t` in panel; instruction-like foreign notes follow C5.
- [ ] 3.2 GREEN prompt in app/chat + ledger fields; S1 foreign line replaced by the prompt only when a decision is pending. CHANGELOG; README pair. Run exit gate.

## S4a: Model saves (base: main after S3)

- [ ] 4a.1 RED `memory_note{kind,area?,text,refs?}` in chat-only `Family::Memory` (gh_issue_draft pattern): text <=600 chars/6 lines, refs <=5, 5 per turn, 20 per chat; missing kind gives `note not saved: missing kind`; token redacted on disk; line `saved project memory: <title> (<kind>, <area>)`; tool results are fixed text and never echo note text; content from answer text, never reasoning.
- [ ] 4a.2 RED workers and `local=false` have no `memory_note` tool.
- [ ] 4a.3 GREEN tool, limits, wiring. CHANGELOG. Run exit gate.

## S4b: Conflicts (base: main after S4a)

- [ ] 4b.1 RED D9 heuristic: same kind family, area contains, Jaccard 0.35..0.9, polarity/value signal; >=0.9 duplicate skipped with a line.
- [ ] 4b.2 RED flow: OK sets `supersedes{id,head,at}`/`replaced_by{id,at}` and shows `replaced <old> with <new>`; no keeps old and drops new with a line; unanswered at close changes nothing; pinned needs explicit confirmation naming it.
- [ ] 4b.3 GREEN `local/conflict.rs`, prompt wiring. CHANGELOG; README pair. Run exit gate.

## S5: Engram mirror (base: main after S4b)

- [ ] 5.1 RED gate tests: mirror only when `enabled` and `acknowledged` and engram readiness `Ready(project)` (owner decision 1, no new toggle); engram missing, off, unacknowledged or no project sends nothing, starts no worker, shows nothing.
- [ ] 5.2 RED mirror tests: local write first, then `MemCmd::Save` topic `lzr/<id>`, redacted; only own, trusted, non-superseded, changed notes (hash in ledger `mirrored`); foreign and untrusted never sent; failures queued in ledger, retried on next save or memory action at least 10 s apart, no duplicate, no new line (C12); delete only through `MemCmd::Delete` after explicit user confirmation, never from cleanup.
- [ ] 5.3 RED candidates: engram notes join the ranker, deduped by topic key, local wins; mirrored titles dropped; same sanitizer, cap, nonce wrapper, budget; one merged block; engram absent equals local-only.
- [ ] 5.4 GREEN mirror plan (pure) in `local/`, executor in runtime, slug/route accept `lzr/`. CHANGELOG; README pair. Run exit gate.

## Close-out

- [ ] 6.1 After the last slice merges: close #431 with a link to the merged PRs; comment on #366 that only the Drive sync remains.
- [ ] 6.2 Remove all `../lazarobox-shell-<slice>` worktrees (`scripts/clean-builds.sh`).
