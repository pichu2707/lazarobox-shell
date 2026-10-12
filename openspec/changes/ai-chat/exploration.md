# Exploration: ai-chat (P2 Ollama chat pane with streaming + prompt profiles)

Engram: `sdd/ai-chat/explore`.

## Current state (verified in code)

- **Pane model.** In `src/app.rs`, `App.panes` maps each `PaneId` to a `PaneState` (a vt100 `Pane`, its size and `proc_cwd`), and `Tab{tree, focus, zoom}` holds the layout. Every pane is assumed to be PTY-backed:
  - `split()` and `new_tab()` always insert a `PaneState` and emit `Effect::SpawnPane`.
  - `on_terminal_key` encodes keys to `Effect::WritePty`.
  - `InputMode` is Terminal, Prefix, Copy, Group, Resize, Confirm or Menu.
  - COPY mode is bound to the vt100 `Pane` and yanks through OSC52 (`Effect::SetClipboard`).
- **Rendering.** `render()` in `src/ui/mod.rs` always draws `TerminalView::new(&Pane)`. There is no pane-kind abstraction.
- **Runtime.** In `src/runtime.rs`, AI detection is the only async side channel today: `Effect::DetectEngines` → `spawn_detection(ai_tx)` → mpsc → `AppEvent::EnginesDetected`. This is the template for a chat stream channel. `run_loop` is a `tokio::select!` over input, pty rx, ai_rx, shutdown, the cwd poll and a 16 ms tick.
- **HTTP.** `src/core/ai/http.rs` has only a pure `parse_response(raw)`. It buffers the whole response, caps the body at 1 MiB, and `dechunk` needs the terminating 0-chunk, so it **cannot stream**. In `src/runtime/ai_detect.rs`, `exchange()` is GET only, HTTP/1.0 with `Connection: close`, `read_to_end`, an 800 ms timeout, and is fixed to 127.0.0.1. reqwest 0.12 is declared in Cargo.toml but not used.
- **Config.** In `src/core/ai/config.rs`, `Task` has five values (General, Code, Chat, Review, Summarize), and `AiConfig.resolve(task)` falls back to General. `[ai]` holds the general task and `[ai.tasks.<id>]` holds the others. Writes go through toml_edit `ConfigEdit`.
- **Sanitizing.** The private `sanitize()` in `src/core/ai/parse.rs` (OSC, CSI, control and format characters) is a good base, but it is single-line and not stream-aware.
- **Bindings.** The prefix tree has no AI or chat binding yet.

## Approaches: chat surface

| Approach | Pros | Cons | Effort |
|---|---|---|---|
| A. New pane kind in the tile tree (`PaneContent::{Pty,Chat}`) | Matches the tmux model. Focus, zoom, tabs, close and multiple chats are reused. | PTY assumptions are spread through app.rs (spawn, write, resize, close, cwd poll, COPY). Needs a refactor slice. | High |
| B. Overlay or full-screen view like the Menu | Smallest blast radius. Easy to test. | Not a pane, one conversation only, hides the terminal, partly throwaway. | Low-Med |
| C. Hybrid: the seam enum now, chat only as a focused or zoomed pane at first | Same refactor as A with less layout surface. | Still needs the seam. | Med |
| D. Chat as a child PTY process rendered through vt100 | No App refactor. | Awkward ratatui-in-vt100-in-ratatui, weak state sharing, hard to test. | Not recommended |

**Recommendation: A.** The first slice adds the seam and changes no behaviour. `ChatState` stays pure in `core::ai::chat`, so B stays available as a fallback.

## Ollama chat API (to verify against a live server in design)

- **Request.** `POST /api/chat {model, messages:[{role, content}], stream:true, options:{num_ctx}, keep_alive}`. The system prompt is the first message.
- **Stream.** NDJSON lines `{message:{role,content}, done:false}`, then a final line with `done:true`.
- **Errors.**
  - An unknown model returns 404 `{"error":"model ... not found"}`.
  - Connection refused means the server is down.
  - A cold model load is a long silence before the first byte, so connect, first-byte and idle timeouts must be separate.
  - A cut stream ends with EOF and no `done:true`, or with an `{"error":...}` line. Keep the partial answer and mark it interrupted.
- **Cancel.** Close the TCP connection.
- **Context.** The default `num_ctx` is small and the server truncates silently. Cap the history on the client and pin the system prompt.

## HTTP decision

Extend the hand-written loopback client: POST, HTTP/1.1, an incremental chunked decoder, and an NDJSON splitter with a per-line cap. This keeps traffic on 127.0.0.1 by construction. reqwest honors the proxy environment by default, which would break the loopback-only guarantee. Keep it for P5 (cloud), or use it only with `no_proxy()` and no redirects.

## Task and engine resolution

The chat pane uses `resolve(Task::Chat)`, which falls back to General.

| Assignment | Chat pane shows |
|---|---|
| ollama with a model | the chat runs |
| engine unset | an empty state that points to Menu > AI |
| engine not detected | the status text and a retry |
| a CLI, lmstudio, llamacpp or vllm | an explicit "not supported in chat yet" state |

The engine is never switched silently, because that would change where the user's data goes. Models are validated against `App.engines`.

## Prompt profiles

- **Config key.** `[ai] profile = "analyst" | "programmer"`.
- **Built-ins.** A prompt set for each task is compiled in.
- **Overrides.** Files at `$XDG_CONFIG_HOME/lazarobox/prompts/<profile>/<task>.md`. Each task falls back on its own, from the user file to the built-in.
- **Bad files.** An override must be UTF-8 and stay under a size cap. A file that breaks this, or cannot be read, shows a notice and falls back to the built-in.
- **Resolution.** The task prompt is `profile[task]`, else `profile[general]`, else the built-in general prompt.
- **Switching.** A "Profile" row in the AI group of the menu. A switch applies to new conversations.
- **Spec deltas.** config-menu, config-persistence, configuration and ai-task-models.

## Conversation, UX and security

- **State.** In memory only, with a history cap and a clear key. Each chat pane owns its own state.
- **Composer.** Enter sends, and Alt+Enter or Ctrl+J inserts a newline (Shift+Enter is unreliable).
- **Transcript.** Wrapped and scrollable. It follows the stream while new text arrives.
- **Cancel.** Esc or Ctrl+C cancels the stream and keeps the partial answer, marked cancelled. Ctrl+C while idle must not quit.
- **Copy.** Yank the last answer or code block through OSC52, because COPY mode is bound to vt100.
- **Markdown.** Minimal: fenced code blocks only.
- **Security.**
  - A stateful, per-chunk sanitizer removes C0 and C1 controls, CSI, OSC, DCS and APC sequences, and bidi and format characters. It runs before the text is stored.
  - Text is rendered through ratatui Spans only.
  - Requests are built with serde_json.
  - Prompts are never logged or written to disk.
  - Override files are capped and treated as untrusted.

## Slice plan (about 400 lines each, strict TDD)

1. The pane-kind seam, with no behaviour change.
2. Pure chat core: the sanitizer, the NDJSON parser and the state machine.
3. Streaming HTTP and the runtime task, with cancel and timeouts, and the new Effects and AppEvents.
4. Chat pane UI: composer, transcript, states and snapshots.
5. Prompt profiles: built-ins, the config key, the menu row, the loader and resolution.
6. Polish: yank, clear, the trim marker and the spec archive.

## Risks

- The blast radius of the app.rs refactor.
- Correctness of the hand-written streaming HTTP: chunk boundaries and UTF-8 split across chunks.
- A cold model load that looks like a hang.
- Key ambiguities: Shift+Enter, Esc, and Ctrl+C.
- Escape or bidi smuggling through model output.
- Ollama silently truncating the context and dropping the system prompt.
- Profile scope creep into P3.
- Unbounded transcript memory.

## Open product questions

1. Profile scope: the prompt text only, or also default tasks, models and tools? Two fixed profiles, or an open list?
2. Where and when does the user switch profile, and what happens to a conversation already in progress?
3. What does the chat do when its task points to an engine other than Ollama?
4. Do conversations stay ephemeral, or can they be saved or exported?
5. Does the chat see terminal output in P2?
6. Is the chat a pane next to the shells, or a dedicated assistant view?
7. What formatting does the first release need?
8. What language are the built-in prompts in?
9. Do editable prompts need guardrails and a reset to default?
