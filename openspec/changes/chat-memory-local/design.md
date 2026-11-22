# Design: Local project memory (chat-memory-local)

## Technical Approach

The local `.lzr/` store is the only source of truth for project memory. Engram is an optional one-way mirror.

The work splits into a pure core and one runtime thread:

- **Pure core**, under `src/core/ai/memory/local/`. It holds the note schema, area tags, the never-store screen, the ranker, conflict and cleanup rules, and the mirror plan.
- **Runtime thread**, in `src/runtime/memory_local.rs`. It does all file IO: `.lzr/`, `.gitignore`, the private ledger and the machine id.

The app talks to the thread through a sibling port. Commands go out as `Effect::LocalMemory(LocalCmd)` and replies come back as `AppEvent::LocalMemory(LocalReply)`. These are plain values, like `MemCmd`.

At project entry the chat loads every note once. Each turn it ranks them again against the question. This step is pure and needs no IO. The kept notes are wrapped by a new `local_block()` in `context.rs`. That function reuses `neutralize`/`clean` and adds a nonce.

A redacted end-of-chat summary is upserted as a durable note.

Config key: `[ai.memory] local = true` is the default, and `false` restores today's behaviour. The engram opt-in (`enabled` plus `acknowledged`) is untouched.

## Architecture Decisions

| # | Decision | Choice | Rejected | Rationale |
|---|---|---|---|---|
| D0 | Port | A sibling `LocalCmd`/`LocalReply` port and its own runtime thread | Adding variants to `MemCmd` | `self.memory` (engram) is inert while engram is off, but local memory must run regardless. The engram ops (sessions, observation ids) do not map onto local ones (load, trust, gitignore). The "one memory port" goal is kept at the chat level: `ChatMemory` holds both sources. |
| D1 | Note layout | One JSON file per note: `.lzr/memory/notes/<id>.json`, pretty-printed | One `notes.jsonl` | Teammates adding notes create different files, so git never conflicts. In JSONL, two appends at EOF always conflict. A pin or supersede rewrites one small file instead of the whole log. Writes are atomic per note (`create_new` temp, `sync_all`, `rename`, which also replaces on Windows). A bad hand-edit loses one file, not a line inside a file that gets rewritten. Thousands of small files load in milliseconds. |
| D2 | Origin and foreign | Trust comes from a private ledger, not from the note. The note's `machine` field is display only. | Trusting the machine id written in the note | A committed note can copy any machine id. The ledger lives at `<data>/lazarobox/memory/<sha256(root)[..16]>.json`, a secret root already denied to tools by `paths.rs`. It records `own: {id: content_hash}`. A note is **local** only if its id is in `own` and its hash matches. Anything else is **foreign**: clones, pulls, or files written by a shell command. An own note hand-edited outside lazarobox becomes foreign too ("changed outside lazarobox"). |
| D2b | Re-ask after pull | Declining is remembered per project until it is changed in `/memory`. Trusting covers the ids and hashes present at that moment. New or changed foreign notes ask again, at most once per chat entry. | Trust once for all future notes | Trusting today must not trust a malicious commit tomorrow. The prompt only appears when the foreign set changes. |
| D3 | Root | `workspace::memory_root(cwd)` returns `(store_root, work_root)`. `store_root` is the main repository for a linked worktree, so all worktrees share one memory. A submodule keeps its own. `area` is `cwd` relative to `work_root`. | One memory per worktree | The owner chose one memory per repository. |
| D3b | No git | No memory. One line per chat: `memory: no git repository here; nothing is remembered for this folder` | A fallback keyed by folder in chat.db | A fallback means a second store and a schema v4 migration. It would also put memory on arbitrary folders such as `$HOME`. |
| D3c | Windows | Areas and refs are stored as `/`-joined `Path::components` of the repo-relative path. Prefix, root and `..` components are rejected. Matching ignores ASCII case. `canonicalize` verbatim prefixes (`\\?\`) are compared by component. | String slicing | The approach holds for drive letters, backslashes and WSL `/mnt/c`. Ignoring case only loosens ranking, which is harmless. |
| D4 | `.gitignore` | Edit only when lazarobox **creates** `.lzr/`. Check with the `ignore` crate (already a dependency) over the root `.gitignore` plus `.git/info/exclude`, then call `matched_path_or_any_parents(".lzr/memory/notes/x.json")`. | `git check-ignore` | `git check-ignore` spawns git, which may be missing. Outcomes are listed under D4 below. |
| D5 | Ranking | Integer points over explainable parts | Embeddings, or tags only | Embeddings are out of scope. Integer points are exact in tests and in `/memory why`. |
| D6 | Injection | A new `<project-memory source="lzr" id="NONCE">` block. The nonce comes from the thread's `/dev/urandom`, once per load. If no randomness is available, the fallback is time + pid + counter hashed with sha2. | Reusing the engram block unchanged | The data still has every `<` escaped. The nonce is defense in depth. |
| D7 | End summary | Sources upsert one note per conversation, in this priority: approved summary, then rolling compaction summary (saved locally whatever `save_summaries` says, because that flag stays engram-only), then the idle **digest run**. | A model run at close | Quit and pane close cannot wait for a model. |
| D8 | Instruction-like text | Strong markers are **rejected**. Weak imperatives are **saved and flagged** ⚑ and stay injectable as data. | Rejecting every imperative | Conventions such as "always run cargo fmt" are legitimate notes. |
| D9 | Conflicts | A no-model heuristic, then ask the user | A model judge | A judge means model text deciding the memory structure, plus cost. A false positive costs one question. |
| D10 | Workers | No memory block in orchestrator workers. No `memory_note` tool for them. Their output never becomes a note. | Ranked memory per worker | Workers act with write and exec tools, so untrusted-ish text in their prompt widens the blast radius. Briefs already carry the context. |
| D11 | Mirror | Local to engram only, for own trusted notes, under `lzr/<id>` keys. Engram notes are read-only ranking candidates (S5). | Two-way sync | Local is the truth, and one-way removes echo loops. |

### D2 identity details

`<data>/lazarobox/machine-id` holds `m-` plus 16 random hex digits, created once. It goes into each note's `machine` field so the panel can show "this machine" or "other". It never stores a user name, because that would be personal data. Note ids are `n-` plus 16 hex digits of `sha256(machine, nanos, counter, text)`, so no randomness is needed on non-Linux systems.

Ledger writes are read-merge-write with a union of `own`, so a second instance never drops an entry. The ledger also holds `trust`, `trusted: {id: hash}` and `mirrored: {id: (hash, observation_id)}`.

### D4 `.gitignore` outcomes (each shows one line)

| State | Action | Line |
|---|---|---|
| Already ignored | none | `memory: .lzr/ is already ignored by git` |
| Whitelisted (`!.lzr`) | none, the user's choice is respected | `memory: .lzr/ is shared through git (.gitignore un-ignores it)` |
| No `.gitignore` | create `.lzr/\n` | `memory: created .gitignore with .lzr/ (remove it to share memory)` |
| Not ignored | append `.lzr/`. Add a `\n` first if the file does not end with one. Use `\r\n` if the file uses CRLF. Open in append mode and never rewrite other lines. | `memory: added .lzr/ to .gitignore (remove it to share memory)` |
| Write error or read-only file | notes are still saved | `memory: could not add .lzr/ to .gitignore (<reason>); add it by hand to keep notes private` |
| `.lzr/` cannot be created | no save, and the chat continues | `memory: cannot write .lzr/ (<reason>); notes from this chat are not saved` |

The check is idempotent because the edit runs only when the folder was just created and the matcher said "not ignored".

### D5 Scoring (points) and budget

| Part | Points |
|---|---|
| area | 30 if the note area contains the cwd area (non-root) or holds a file in play. 20 if the note is in a sub-area of the cwd. 10 for a root note. 0 otherwise. |
| terms | 10 per shared term, at most 30. Terms are lowercase, split on non-alphanumerics, at least 3 chars, with a small EN/ES stopword list. The question is matched against the note's text, area and refs. |
| kind | decision or architecture 10, preference 9, bugfix 7, config or pattern 6, discovery 4 |
| pinned / user origin | +20 / +10 |
| latest summary | +15 for the newest summary note of the project. This guarantees the "second chat sees the first" success criterion. |
| recency | `10 × 0.5^(age_days/30)`, rounded |
| refs | +5 if every ref exists, −15 if any ref is missing (stale) |

"Files in play" are the path-like tokens in the question plus the cwd area.

**Excluded** notes: superseded, untrusted foreign, strong-flagged, and unrelated. Unrelated means area 0 and terms 0 and not pinned.

**Budget**, in bytes of rendered data:

- CLI engines: 2400.
- Local servers (`Kind::Server`), known window: `clamp(window_tokens × 4 × 3%, 600, 1500)`.
- Local servers, unknown window: 900.

**Cut:**

1. Sort by score descending. Ties go to pinned, then newer `updated`, then id.
2. Render one line per note, `- [kind · area] text (n-xxxx)`. Text is cut at 280 chars, or 600 for the latest summary.
3. Fill greedily: a line that does not fit is skipped and the next one is tried. Stop at 12 notes.

The block still comes off the history budget through `budget::budget(…, memory, …)`.

### D6 Block

```
<project-memory source="lzr" id="9f2c…" project="widget">
{DATA_LINE}
- [decision · frontend] Use Axum for the API (n-3f9a)
</project-memory id="9f2c…">
```

Each turn stores an `Injection { turn, budget, used, picked: Vec<Pick{id, parts, total, bytes}>, over_budget: Vec<(id,total)>, excluded: Counts }` in `ChatMemory`. `/memory why` renders it as `Entry::System` lines, which are display only:

```
memory why · turn 3 · 3 notes · 742 of 900 bytes (local engine)
  n-3f9a decision · frontend  61 = area 30 + terms 10 + kind 10 + recency 6 + refs 5
  left out: 4 over budget (best n-77c1, 38) · 2 not trusted · 1 replaced · 6 unrelated
```

### D7 Write paths

- **Digest run** (S1b). It follows `begin_compaction`: a side run with no stream slot, superseded by a send, on the `summarize` engine or else the `chat` engine, with `SUMMARY_PROMPT`.
  - It starts after a turn that ended well, at the 2nd user turn and then every 3 more.
  - It needs at least 800 new chars and an idle pane with no compaction or summary run.
  - Its input is the sendable history only. `Entry::System`, the tool rows and the step trail never feed it.
  - Upsert topic: `chat/<id>-<born>/summary`. A resumed chat reuses the newest loaded topic with prefix `chat/<id>-`.
  - Kind `discovery`, origin `summary`, at most 1500 chars.
  - The first save in a chat shows a line. Later updates show a transient notice. A failure shows one line per reason per chat.
- **Model** (S4). The tool is `memory_note{kind, area?, text, refs?}` in a new chat-only `Family::Memory`, not offered to workers. The pattern is `gh_issue_draft`.
  - Limits: text ≤ 600 chars and ≤ 6 lines, refs ≤ 5 repo-relative paths, 5 saves per turn, 20 per chat.
  - No approval prompt. Each save shows `memory: model saved [decision · frontend] <first 80 chars> (n-xxxx)`.
  - The tool result is fixed lazarobox text (`saved n-xxxx`, `held for the user's review`, or a refusal reason). It never echoes note text. Notes never reach a tool path or an action.
- **User** (S2). The note editor writes a local note with origin `user`.

### D8 Never-store screen (`screen.rs`, run at save AND at load)

1. `clean` (sanitize).
2. `redact` (secrets).
3. `pii.rs`, a hand-written linear scanner in the style of `redact.rs`. Each hit is masked and counted in the save line, for example `(1 email masked)`.

   | Pattern | Mask | False-positive control |
   |---|---|---|
   | Email | `[EMAIL]` | |
   | Phone (`+` prefix or grouped separators, 9–15 digits) | `[PHONE]` | Not inside identifiers, hex, paths or version strings |
   | IBAN | `[IBAN]` | mod-97 check |
   | Card | `[CARD]` | Luhn check, 13–19 digits |
   | ES DNI/NIE | `[ID]` | Check letter |
   | US SSN | `[ID]` | Excluded ranges |

4. Command output is **rejected**: more than 6 lines with at least 40% shell-prompt, log or stack-frame shapes, or a fenced block over 5 lines. The refusal says "save the conclusion in one or two sentences".
5. Instruction-like text:
   - **Strong** markers are rejected at save and never injected when found at load: role tags (`system:`, `assistant:`, `<|im_start|>`), "ignore/disregard (all|previous) instructions", "you are now", and tool-call syntax.
   - **Weak** markers (imperatives addressed to the model) are flagged ⚑.

Names and customer data cannot be found by patterns. That risk is covered by the summary prompt and the tool description.

### D9 Conflict heuristic (S4)

A conflict is possible when all of the following hold:

- The two notes have the same kind family: decision, architecture, preference, config or pattern.
- One area equals or contains the other.
- The content-term Jaccard is between 0.35 and 0.9.
- There is a polarity or value signal:
  - one side only has negation or replacement words (not, never, no longer, instead, deprecated, nunca, ya no, en lugar de), or
  - the same key has different values (`x = a` / `x = b`, the same name at a different version, "use A" / "use B").

A Jaccard of 0.9 or more is a **duplicate**: the new note is skipped with a line.

On a possible conflict the user is asked in the existing confirm-prompt pattern:

- **OK**: the new note gets `supersedes {id, head: first 120 chars, at}`, the old one gets `replaced_by {id, at}`, and a line records both.
- **No**: the new note is dropped with a line.

### D10 Cleanup (S2)

Cleanup runs on load and from the panel. It deletes only own notes and never pinned or foreign ones, because deleting foreign files would dirty a shared repo. The line is `memory: cleaned up N notes (x duplicates, y replaced, z stale)`.

It removes:

- exact duplicates (normalized text, kind and area): the newest is kept;
- superseded notes older than 30 days: the trace survives in `supersedes.head`;
- stale notes older than 30 days: every ref is gone and the area path is gone.

### D11 Panel `/memory` (S2)

An overlay like `CostView`, with a `memory_view.rs` model and a `ui/components/chat/memory.rs` view.

Rows: `★⚑◌⇄ kind · area  text-head  (age, this machine|other)`. The cwd area comes first.

| Key | Action |
|---|---|
| ↑↓ | move |
| Enter | expand |
| p | pin |
| d then y | delete |
| c | clean up |
| t | trust or decline foreign notes (S3) |
| Esc | close |

At 40 columns the view drops the age and the machine.

### D12 Mirror (S5)

The mirror runs only when `memory.active()` holds and the chat's engram readiness is `Ready(project)`. Otherwise it does nothing and shows nothing.

- It pushes `MemCmd::Save` with topic `lzr/<id>` for own, trusted, non-superseded notes whose hash differs from `mirrored`. `slug`/`route` must accept the `lzr/` prefix.
- A local delete sends `MemCmd::Delete` with the stored observation id.
- Failures are queued in the ledger and retried at the next project entry or save. They add no new line, because the existing `memory unavailable: …` line already covers engram being down.
- Echo is avoided because nothing is imported as a note. Engram context lines become ranking candidates only. Those whose normalized title matches a mirrored title are dropped, and local wins ties. In S5 the separate engram block is replaced by one merged block.

## Data Flow

```
project entry ──LocalCmd::Load{root}──▶ runtime/memory_local ──▶ .lzr/memory/notes/*.json
                                              │  ledger (data dir), machine-id, nonce
              ◀──LocalReply::Loaded{notes, foreign, unreadable, nonce}──┘
send(question) ─▶ rank(notes, area, question, budget) ─▶ local_block ─▶ ChatRequest.memory
                     └─▶ Injection (for /memory why)
turn ended ─▶ digest run ─▶ screen ─▶ LocalCmd::Upsert ─▶ atomic write + ledger ─▶ line
```

## File Changes (by slice; production lines are estimates)

| Slice | Files | ~Lines |
|---|---|---|
| **S1a** store | Create `core/ai/memory/local/{mod,note,area,gitignore,port,ledger}.rs` and `runtime/memory_local.rs`. Modify `runtime/workspace.rs` (`memory_root`), `core/ai/config.rs` (`local`), `core/ai/tools/paths.rs` (`.lzr` added to `WRITE_DENIED_DIRS`), and `app.rs`/`runtime.rs` (effect and event routing). | 390 |
| **S1b** screen + summary | Create `local/{screen,pii}.rs` and `core/ai/chat/digest.rs`. Create `app/project_memory.rs`, which holds the approved, compaction and digest upserts. Modify `app/compact.rs`. | 390 |
| **S1c** rank + inject + why | Create `local/{rank,why}.rs`. Modify `memory/context.rs` (`local_block`), `chat/memory.rs` (`LocalMemory` slot), `chat/mod.rs` (send joins the block), `chat/folder.rs` (`/memory why`), and `app/chat.rs`. Includes the "N notes not from this machine are not used" line. | 360 |
| **S2** panel, cleanup, user saves | Create `chat/memory_view.rs`, `ui/components/chat/memory.rs` and `local/cleanup.rs`. Modify `app/notes.rs`, `folder.rs` (`/memory`). Split into S2a (panel) and S2b (cleanup and user saves) if it goes over 400. | 400 |
| **S3** trust | Modify `ledger.rs`, `app/project_memory.rs` and `memory_view.rs` (prompt and `t` key) | 200 |
| **S4** model saves + conflicts | Create `tools/memory_note.rs` and `local/conflict.rs`. Modify `tools/registry.rs` (`Family::Memory`), `tools/mod.rs` and `app/tool.rs`. Split into S4a (tool) and S4b (conflict) if needed. | 400 |
| **S5** mirror | Create `local/mirror.rs`. Modify `memory/slug.rs`/`route.rs` (`lzr/`), `app/project_memory.rs` and `ledger.rs` (`mirrored`) | 260 |

Tests, fixtures and snapshots do not count toward these budgets.

## Interfaces / Contracts

```rust
pub struct LocalNote { v: u8, id: NoteId, area: String, kind: MemKind, origin: Origin,
    text: String, refs: Vec<String>, topic: Option<String>, created: i64, updated: i64,
    machine: String, pinned: bool, flags: Vec<Flag>, supersedes: Option<Trace>,
    replaced_by: Option<Trace>, #[serde(flatten)] extra: serde_json::Map<String, Value> }
pub enum Origin { User, Model, Summary }
pub enum LocalCmd { Load{pane, epoch, cwd}, Upsert{pane, root, note}, Pin{..}, Delete{..},
    Supersede{..}, Trust{root, answer}, Cleanup{root} }
pub enum LocalReply { Loaded{pane, epoch, root: Option<Root>, notes, foreign: usize,
    changed: usize, unreadable: usize, newer: usize, nonce}, Saved{pane, id, gitignore: Option<GitignoreOutcome>},
    Failed{pane, reason}, /* … */ }
pub fn rank(notes: &[LocalNote], ctx: &RankCtx, budget: usize) -> Injection;
pub fn screen(text: &str, origin: Origin) -> Result<Screened, Refusal>;
```

Notes with `v > 1` are skipped with the line `N notes made by a newer lazarobox were skipped`, and lazarobox never rewrites them. The flattened `extra` map keeps unknown fields when a note is rewritten.

## Testing Strategy (Strict TDD)

| Layer | What | How |
|---|---|---|
| Unit | Note round-trip with unknown fields kept; areas (Windows, `..`, drive letters); the gitignore decision table; every screen pattern with its checksum false positives; linear time on 1 MiB of hostile input; rank parts and budget cut; block delimiters and nonce; conflict and duplicate heuristic; cleanup never deletes pinned or foreign notes | Pure tests next to each module, in the `redact.rs` style |
| Integration | Atomic write; malformed and newer files skipped and never rewritten; read-only dir; ledger merge between two writers; foreign notes after a simulated pull; machine id | `runtime/memory_local.rs` with `tempfile` repos, never `/tmp` roots for the full suite |
| App | The second chat sees the first chat's summary; a `frontend/` chat ranks frontend notes first; `/memory why` lines; every edge-case line; workers get no block or tool | `app/app_tests/project_memory.rs` |
| Snapshot | Panel at 40, 80 and 120 columns | `insta` |

## Migration / Rollout

There is no chat.db migration. `local = false` restores today's behaviour. The `chat-memory` spec delta changes the "Default is off and silent" scenario. `.lzr/` and the `.gitignore` line are inert data the user can delete.

## Open Questions

- [ ] The "per-project engram opt-in" maps onto today's global `[ai.memory] enabled + acknowledged` plus a resolved project. Confirm that no new per-project toggle is wanted.
- [ ] Digest cadence (2nd turn, then every 3) costs model calls on paid CLI engines. Confirm, or gate digests to local engines plus compaction only.
- [ ] Linked worktrees share the main checkout's `.lzr/`. Confirm.
