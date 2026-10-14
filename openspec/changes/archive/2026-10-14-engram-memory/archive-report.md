# Archive Report: engram-memory

**Date**: 2026-10-14
**Artifact store**: hybrid (openspec files + engram `sdd/engram-memory/archive-report`)
**Branch**: `chore/openspec-archive` (from origin/main `fd8e2dc`)
**Status**: Archived, after ai-chat. Verdict ready to archive, 0 CRITICAL, 2 WARNING (manual). No gate override. 6 manual tasks accepted open.

## Change Summary

Opt-in topic memory for the chat through a local `engram serve` (v3.4.0 HTTP API). A one-time privacy warning enables it (`[ai.memory] enabled` and `acknowledged`), notes are saved by hand with a closed set of kinds, the user approves an end-of-conversation summary, a compact sanitized memory block is fetched at chat open and shown in the transcript, notes and conversations are linked (chat.db schema version 2 with two link tables), notes can be deleted, and a conversation can resume its engram session. Loopback only, off the render thread, with an offer to start `engram serve` when it is down. With memory off the pane behaves exactly as before.

## What Shipped (PRs and releases)

Merged to `main`:

- 0.9.0: #144 (spike), #145 (loopback), #147 (core), #148 (calls), #149 (config), #150 (worker), #151 (sync with main), #155 (serve offer)
- 0.10.0: #157 (sync with ai-chat), #158 (store), #159 (notes), #160 (context), #161 (summary), #162 (docs)
- 0.13.0: #177 (delete and resume, PR10c)

Release PRs carrying them: #156 (0.9.0), #164 (0.10.0), #178 (0.13.0).

## Observation IDs (traceability)

| Artifact | Topic key | Engram ID |
|----------|-----------|-----------|
| Verify report | `sdd/engram-memory/verify-report` | #3123 |
| Proposal, spec, design, tasks | `sdd/engram-memory/{proposal,spec,design,tasks}` | not re-fetched in this run; the files in this folder are canonical |

## Specs Synced

Merged after ai-chat. Every MODIFIED target existed by name in the post-ai-chat main specs (`ai-engine-detection`: 2, `chat-history`: 2, `config-menu`: 2); none was missing, so nothing was guessed.

| Capability | Action | Requirements (before -> after) | Scenarios (before -> after) |
|------------|--------|--------------------------------|-----------------------------|
| `ai-engine-detection` | 2 MODIFIED | 10 -> 10 | 44 -> 47 |
| `chat-engines` | 2 ADDED | 13 -> 15 | 94 -> 102 |
| `chat-history` | 1 ADDED, 2 MODIFIED | 11 -> 12 | 63 -> 82 |
| `chat-memory` | Created (new) | 0 -> 10 | 0 -> 77 |
| `chat-pane` | 2 ADDED | 14 -> 16 | 78 -> 86 |
| `config-menu` | 2 ADDED, 2 MODIFIED | 18 -> 20 | 95 -> 107 |
| `config-persistence` | 1 ADDED | 9 -> 10 | 57 -> 66 |
| `configuration` | 1 ADDED | 4 -> 5 | 31 -> 39 |
| `engram-client` | Created (new) | 0 -> 11 | 0 -> 51 |
| `modal-input` | 3 ADDED | 12 -> 15 | 74 -> 93 |
| `statusline` | 1 ADDED | 12 -> 13 | 62 -> 67 |

Same merge rules as ai-chat: in-place replacement for MODIFIED, append for ADDED, preambles and `(Previously: ...)` lines dropped, new capabilities written as full specs.

## Tasks

111 tasks: 105 checked, 6 accepted open. Ticked at archive time from user-reported and verified facts:

- 7.6 (serve offer), 9.9 (note keys), 10.8 (memory line), 11.5 (summary keys): `[x] (user-checked 2026-10-14)`. The user reported on 2026-10-14 that they ran the 0.10.0 manual checks in Kitty.
- 13.5 (final gate): `[x]`. Passed on `main` `fd8e2dc`: 3446 lib + 13 jail + 1 integration, fmt and clippy clean (verify #3123).

Accepted open (all `[manual]`):

- 6.9 warning readable without clipping; accept, `s`, restart shows memory on without a second warning
- 13.1 live engram 3.4.0 end to end on a throwaway instance
- 13.2 offer path with a stopped throwaway server
- 13.3 delete path with and without `ENGRAM_HTTP_TOKEN`
- 13.4 Kitty keys (Ctrl+Space n, o, s and Ctrl+S in editors) and the old 1.20 server notice
- 13.6 UNVERIFIED ASSUMPTION (PR10c): resume after End against a real engram (the re-register path 409 is covered by design and tests)

## Verification

Verify #3123 at `fd8e2dc` (0.13.0): fmt OK, clippy `-D warnings` OK, `ENGRAM_PORT=8123 cargo test` 3446 lib + 13 + 1 passed, 0 failed. 0 CRITICAL, 2 WARNING, 1 SUGGESTION.

- WARNING 1 (13.6 resume-after-End assumption unverified against a real engram): accepted open.
- WARNING 2 (Kitty key delivery of Ctrl+S, Prefix n/o/s): mostly closed by the user-checked 9.9, 10.8, 11.5; 13.4 stays open.
- SUGGESTION 1 (tick 13.5, record manual tasks as deferred): done.

## Relevant Files

- `openspec/specs/{ai-engine-detection,chat-engines,chat-history,chat-memory,chat-pane,config-menu,config-persistence,configuration,engram-client,modal-input,statusline}/spec.md`
- `openspec/changes/archive/2026-10-14-engram-memory/` (this folder)
