# Proposal: AI Chat (P2 chat pane, all engines, topic memory)

## Intent

Analysts and programmers who work in lazarobox have to leave the multiplexer to ask a model for help. P1 (ai-engines) detects the engines and ai-selector assigns them to tasks, but nothing uses those assignments yet. This change adds a chat that lives in a pane. It is ready with no setup: the user opens it and types. It uses the engine assigned to the `chat` task, applies a profile prompt (`analyst` or `programmer`), and saves every conversation by project so the user can resume it later.

## Roadmap (after this change)

| Phase | Content |
|---|---|
| P2 | **This change.** A chat pane for every detected engine: Ollama, the OpenAI-compatible servers, and the claude, codex and gemini CLIs in text-only mode. Prompt profiles, and conversations saved in SQLite and organized by topic |
| P3 | The agent loop with tools and approvals, plus terminal context. **It also absorbs CLI tool use**, so the old P4 is merged into it (D17) |
| P4 | Retired. The text-only CLIs moved to P2 and CLI tool use moved to P3. The number is not reused, so P5 and P6 keep their names |
| P5 | Cloud APIs, with reqwest |
| P6 | The orchestrator |
| Later | Shared project memory, chart explanation, encryption at rest, custom profiles, tables and full markdown |

## Decisions (user, 2026-10-12, authoritative)

- **D1 Goal.** A zero-setup chat that helps analysts and programmers. Explaining charts comes in later phases.
- **D2 Engine resolution.**
  - The chat uses `resolve(Task::Chat)`, which falls back to `general`.
  - An engine that is unassigned, undetected or has no adapter shows a guidance state.
  - The engine is NEVER switched silently.
  - `opencode` stays in the catalog, but it has no chat adapter in P2 and shows `not supported in chat yet`.
- **D3 One `ChatEngine` adapter contract.** It covers streaming text deltas, a final event, errors, and cancel. Adapters ship in this order: Ollama, then OpenAI-compatible (lmstudio, llamacpp, vllm), then claude, codex and gemini (one PR each). Each engine is usable as soon as its PR merges.
- **D4 The CLIs are text-only.**
  - Their tools are disabled, so they cannot run commands or edit files.
  - The headless flags and output formats are verified in design, never assumed.
  - If a CLI cannot be made tool-less safely, its slice is dropped.
  - Each CLI uses the user's own login. lazarobox never reads its credentials.
- **D5 The chat is a pane kind.**
  - `PaneContent::{Pty, Chat}` lives in the tile tree, with focus, zoom, tabs and several chats.
  - The pane owns the chat state, and that state survives COPY and tab switches.
  - A stream keeps running in the background.
- **D6 The seam ships first.** The first slice adds the pane kind and changes no behaviour.
- **D7 Exactly two profiles, `analyst` and `programmer`,** set in `[ai] profile`. A profile changes ONLY the prompts.
- **D8 Built-in prompts.** They are compiled in, written in English, and tell the model to reply in the user's language. A prompt is chosen per task.
- **D9 Prompt overrides.**
  - Override files live under `~/.config/lazarobox/prompts/`. Design picks the layout and the size cap.
  - Override files are untrusted. A file that is too big, not valid UTF-8 or unreadable shows a notice, and the built-in prompt is used instead.
- **D10 Switching profile.** A `Profile` row in the AI section of the menu. A switch applies to new conversations only.
- **D11 Saving conversations.**
  - Conversations are ALWAYS saved, in SQLite under `$XDG_DATA_HOME/lazarobox/`.
  - Every database file is mode 0600, including `-wal` and `-shm`.
  - `secure_delete` is on.
  - There is no encryption, and the README says so.
- **D12 Retention and delete.** The store keeps 200 conversations and evicts the oldest first. A key deletes a conversation.
- **D13 Topics.**
  - Each conversation gets a project tag, proposed from the git repo or, failing that, from the cwd of the pane the chat was opened from.
  - The tag can be edited, and more tags can be added.
- **D14 Opening the chat.**
  - It shows a list filtered by the current project, with the date, the profile and the first message.
  - The user resumes a conversation with its full history, or starts a new one.
- **D15 Memory means organizing and resuming.** Shared project memory is out of scope.
- **D16 Formatting and context.**
  - The chat shows plain text and fenced code blocks.
  - It does not see terminal output (that is P3).
- **D17 P4 is merged into P3.** Tool use is engine-agnostic: one approval model covers servers and CLIs. A separate phase would duplicate the approval UX.
- **D18 Security.**
  - Local servers are reached on 127.0.0.1 only, through the hand-written loopback client extended with POST, HTTP/1.1 and incremental chunked/NDJSON/SSE decoding. reqwest is kept for P5.
  - A stateful sanitizer cleans model output before it is stored or rendered.
  - Prompts and messages are never logged.

## Scope

### In Scope
- The pane-kind seam.
- The chat pane: composer, transcript, streaming, cancel, yank, and the guidance states.
- The `ChatEngine` contract and 5 adapter families.
- The streaming loopback HTTP client.
- The stream-aware sanitizer.
- The SQLite store, with retention, delete and tags.
- The conversation list.
- The profiles, with built-ins, overrides, the config key and the menu row.
- The README notes on privacy and the lack of encryption.

### Out of Scope
- Tools, approvals and terminal context (P3).
- An opencode adapter.
- Cloud APIs and reqwest (P5).
- Shared or summarized memory.
- Encryption.
- Custom profiles.
- Tables and full markdown.
- Charts.
- Exporting conversations.

## Capabilities

### New Capabilities
- `chat-pane`: the chat pane kind, composer, transcript, stream display, cancel, yank, guidance states, and survival across modes and tabs.
- `chat-engines`: the `ChatEngine` contract, engine resolution for chat, the Ollama, OpenAI-compatible and text-only CLI adapters, timeouts, errors, the output sanitizer, and the loopback-only streaming HTTP client.
- `chat-history`: the SQLite store, file modes, `secure_delete`, retention, delete, project tags, the list filter and resume.
- `prompt-profiles`: the two profiles, the built-in prompts per task, overrides, caps and resolution.

### Modified Capabilities
- `pane-layout`: panes gain a kind. Split, new and close work for chat panes, and closing a chat pane cancels its stream.
- `tabs`: chat state is part of the per-tab state, and a stream keeps running across tab switches.
- `modal-input`:
  - adds a prefix binding that opens the chat;
  - routes keys to the composer;
  - Ctrl+C or Esc cancels a stream, and Ctrl+C on an idle chat never quits;
  - defines COPY behaviour on a chat pane.
- `terminal-session`: the PTY registry, spawn, resize, write and the cwd poll apply only to PTY panes.
- `copy-mode-scrollback`: entering COPY on a chat pane does not touch the vt100 state. Design picks between a no-op and a transcript copy.
- `statusline`: the mode and cwd segments for a focused chat pane.
- `configuration`: adds the `[ai] profile` key.
- `config-persistence`: adds the owned key `ai.profile`.
- `config-menu`: adds the `Profile` row in the AI section.
- `ai-task-models`: the `chat` task assignment is now used at runtime, and its model is validated against detection.
- `ai-engine-detection`: the "read-only" rule now applies to detection only. The shared loopback client gains POST for chat, and stays bound to 127.0.0.1.

## Approach

- `core::ai::chat` stays pure. It holds the state machine, the sanitizer, the stream parsers, the prompt resolution and the list and tag model.
- Each pane owns a `ChatState`.
- The runtime follows the detection template: an `Effect::ChatSend` or `ChatCancel` starts or aborts a task, and that task sends `AppEvent::ChatDelta`, `ChatDone` or `ChatError` back through an mpsc channel.
- Cancelling a server stream closes the socket. Cancelling a CLI kills the child process.
- The SQLite store (rusqlite, bundled) runs off the UI thread.

## Affected Areas

| Area | Impact |
|---|---|
| `src/app.rs` (`PaneState`, split, new_tab, key routing, Effects and AppEvents) | Modified |
| `src/ui/mod.rs`, new `src/ui/components/chat.rs` | Modified/New |
| `src/core/ai/chat/` (state, sanitizer, parsers, prompts, tags) | New |
| `src/core/ai/http.rs`, `src/runtime/ai_detect.rs` → shared loopback client | Modified |
| `src/runtime.rs`, new `src/runtime/{chat,chat_store}.rs` | Modified/New |
| `src/core/ai/{config,menu}.rs`, `src/core/menu.rs` | Modified (profile) |
| `Cargo.toml` (rusqlite), `README.md` | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| The CLIs cannot be made tool-less, or their flags change | Med | Design checks each CLI's `--help` and real output. Every CLI slice has a gate: drop it if no safe flag exists. The command is spawned with no shell and the stdin prompt is closed. The CLI version is pinned in the docs |
| The pane-kind refactor breaks PTY behaviour | Med | The seam slice changes no behaviour, and every existing test and snapshot must stay unchanged |
| Hand-written streaming bugs (split chunks or UTF-8, SSE) | Med | Pure incremental decoders, with property tests at every split point |
| Escape or bidi smuggling in model output | Med | Sanitize before storing. Render through Spans only. Fuzz-style tests |
| Conversation data leaks (permissions, forensic leftovers) | Med | Files are created with umask 0600 and checked again on open. `secure_delete` is on. The README states clearly that nothing is encrypted |
| rusqlite adds build time and a C toolchain | Low | Use the `bundled` feature, and record it in the design |
| A cold model load looks like a hang | Med | Separate connect, first-byte and idle timeouts, and a `loading model…` state |
| Silent context truncation | Med | A history cap on the client side, and the system prompt is pinned |

## Rollback Plan

Revert the merge of the tracker `feat/ai-chat`. The `[ai] profile` key is ignored by the previous version. The SQLite file and the prompts directory are left alone and can be deleted by hand. Each engine adapter is its own PR, so it can be reverted alone.

## Delivery Slices

Feature-branch chain on `feat/ai-chat`, auto-chain, strict TDD, about 400 lines per PR, about 4,800 to 5,600 lines in total.

| # | Content | Size |
|---|---|---|
| C1 | The pane-kind seam, with no behaviour change | ~350 |
| C2 | Chat core: the state machine and the `ChatEngine` contract | ~400 |
| C3 | The stream-aware sanitizer and the NDJSON/SSE parsers | ~400 |
| C4 | The streaming loopback HTTP client (POST, chunked, timeouts) | ~400 |
| C5 | The runtime chat task: Effects, AppEvents, cancel | ~350 |
| C6 | SQLite store: schema, modes, `secure_delete`, retention | ~400 |
| C7 | Chat pane UI: composer, transcript, states | ~400 |
| C8 | Chat pane UI: guidance states, yank, prefix binding, snapshots | ~350 |
| C9 | The conversation list, tags, resume and delete | ~400 |
| C10 | Profiles: built-ins, overrides, config key, menu row | ~400 |
| C11 | Ollama adapter wiring | ~300 |
| C12 | OpenAI-compatible adapter (lmstudio, llamacpp, vllm) | ~300 |
| C13 | claude CLI adapter (gated) | ~300 |
| C14 | codex CLI adapter (gated) | ~250 |
| C15 | gemini CLI adapter (gated), the README and docs | ~300 |

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice. After C1 the PTY snapshots are unchanged.
- [ ] With `chat` set to `ollama › <model>`, the user opens the chat, types and sees a streamed reply. Cancel keeps the partial answer.
- [ ] Each detected engine with an adapter streams a reply. An unassigned, undetected or unsupported engine shows guidance, and the engine is never switched.
- [ ] Each CLI runs with its tools disabled, and a test proves the argv. A prompt that asks for a file write produces no file.
- [ ] The database files are mode 0600, the 201st conversation evicts the oldest one, and delete removes a conversation.
- [ ] The list shows the current project's conversations. A resumed conversation restores its full history.
- [ ] Switching the profile changes the system prompt of new conversations only. An oversized override falls back to the built-in, with a notice.
- [ ] Output with escape sequences, OSC or bidi characters is stored and rendered clean.

## Proposal question round

Resolved: the 13 user decisions of 2026-10-12 (engram `sdd/ai-chat/question-round`) are recorded above as D1 to D18. They supersede the exploration's assumptions: an Ollama-only chat, in-memory history, and profiles that might be open-ended.

Minor open points for spec and design:

1. The default keybindings: open the chat, the composer newline (Alt+Enter or Ctrl+J), delete, the tag editor, yank.
2. Retention:
   - Do pinned or tagged conversations count toward the 200?
   - Is eviction checked on save or on startup?
   - Is the user told when a conversation is evicted?
3. Tags: how the git root is detected (filesystem walk or `git`), the tag charset and length, and what happens when the cwd is not available.
4. The override layout (`prompts/<profile>/<task>.md`?) and the size cap (for example 16 KiB).
5. What COPY does on a chat pane: a no-op, or a copy of the transcript.
6. What the list shows when there is no project, or when the project has no conversations.
7. The history cap per engine, and the CLI context strategy: does every turn resend the whole history?
