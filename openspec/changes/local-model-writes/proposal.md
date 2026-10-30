# Proposal: Local model writes (small local models finish a write without getting stuck)

## Intent

On 2026-10-30 the owner tried to code with `qwen3:4b` on Ollama (WSL2, an 8 GB laptop GPU). The model got there in the end, but each mechanical step went wrong:

- `write_file Descargas/fibonacci.py` returned `path not available`. The chat's folder was already `~/Descargas`, so the path meant `~/Descargas/Descargas/…`. The error did not say why.
- After staging the file, the model told the user to "Type `approve`" instead of calling `propose_batch`. The turn ended, and the batch became an orphan behind `Ctrl+G`.
- A later `edit_file` came out as JSON text in the answer, not as a tool call. Nothing happened.

The owner's words: "esto si queremos programar es una locura". The longer goal (owner, same day): "la gestión lo haga un modelo grande pero trabajos más mundanos lo haga un modelo local [...] la documentación por ejemplo del código o los readmes de github". Cheap local workers only pay off if they do not stall on the steps around the actual change.

This change removes those three stalls. It does not loosen any guard: nothing reaches the disk without the user's review and `y`, and nothing is ever run from model text.

## Decisions (owner, 2026-10-30: "si, tenemos que optimizar lo máximo posible")

### D1. The review opens by itself when a turn ends with staged changes (priority)

- **When.** A chat turn that **ended normally** (the engine's `Done`), whose pane still has a staged batch that was not proposed, opens that batch's review at once, as if the model had called `propose_batch`.
- **How.** It reuses the orphan review path as it is: `Effect::BatchReview(pane)` → `Chats::review_batch` → `BatchEvent::Review` → `open_orphan_review`. It uses the same `ApprovalAsk::Batch` view, coverage gate and 800 ms arming. `y` applies the batch, `n`/`Esc` discards it, and the model is told the outcome on its next turn with the existing texts. There is no tool call to answer.
- **Trigger point.** The batch badge changes from `staged` to `orphan` in `App::sync_batches`, for a turn whose last event was `Done`. It does not trigger at the `Done` event itself: the turn task is still alive then, so the runtime would answer "no orphan".
- **Not opened.** Nothing opens on its own when:
  - the turn was cancelled (`Ctrl+C`), timed out or failed — the README and the code-write spec promise an orphan there, and the user stopped the turn on purpose;
  - another approval is pending in that chat;
  - the batch belongs to an orchestrator worker (those go through the orchestrator's queue, code-write "Worker batches").
  
  In those cases the batch stays an orphan with ` orphan N · Ctrl+G `, as today.
- **Docs.** The definition of "orphan" in the code-write spec and the README "Huérfanos" paragraph gain this case.

### D2. Write refusals tell the model where paths start

- **Every write refusal** (`write_file`, `edit_file`, `create_dir`, `propose_batch`) keeps one text for every reason, now with the chat's folder: `path not available (paths are relative to the chat's folder ~/Descargas)`. The same text for every reason means a model still cannot probe which paths exist or are excluded. The folder shown is the chat's own folder, written with `~`, which the model already works in.
- **After the path policy passed**, a missing parent folder says so: `path not available: the folder Descargas/ does not exist (paths are relative to the chat's folder ~/Descargas)`. At that point the path is already allowed, and `list_dir` would show the same fact. This is the exact case of the `Descargas/fibonacci.py` call.
- **Reads, search, exec and git** refusals stay as they are.
- **Docs.** README (threat model of writes: "Todos los rechazos devuelven el mismo texto") and the code-write refusal requirement are amended to describe the hint and the one post-policy case.

### D3. A tool call written as text gets one reminder (Ollama and llama.cpp)

- **Detect.** When an answer has no structured tool calls, but its text holds a JSON object (bare or in a code fence) whose `name` is one of the tools offered in that turn and which has an `arguments` (or `parameters`) object, the turn does not end yet.
- **Remind.** lazarobox sends one more request in the same turn, with the model's answer and one extra message: "Your answer wrote a call to `edit_file` as text. Text does nothing. To act, call the tool." The chat shows a system line: `the model wrote a tool call as text; it was asked to call the tool`.
- **Bounds.** At most once per turn, and it counts toward the turn's request cap (8, or 24 with write tools). Nothing in the text is ever executed or parsed into a call.
- **Not covered.** The CLI engines (claude, codex, gemini) run their own loop; lazarobox only sees their stream, so it cannot answer inside the turn. Out of scope.

## Scope

- In: D1, D2, D3, their tests, README and the openspec deltas (`coding-tools/specs/code-write`, `coding-tools/specs/chat-engines` for D3).
- Out: changing what a cancel does; worker batches; CLI engines for D3; picking or downloading better models (see `model-marks`); the chat's folder following a `cd` (see `chat-folder-follow`).

## Risks

- **D1, a review the user did not expect.** It only opens after a normal end, in the chat pane itself, with the same gate as a proposed batch. `n` or `Esc` discards it.
- **D2, information.** One text for every refusal; the only specific case comes after the policy allowed the path.
- **D3, false positives.** An answer that only explains a tool (`name` without `arguments`, or a tool not offered) does not match. A match costs one extra request at most.
- **Overlap with the orchestrator chain.** `feat/agent-orchestrator-s7-2` edits `src/runtime/tools/host/files.rs`, `host.rs`, `src/runtime/chat.rs` and `src/app.rs`. D2 touches `files.rs`. Each PR here starts from `main` and is kept small, so the chain rebases over it.

## Delivery

Three work units, one PR each, in this order: D1, D2, D3. Each one carries its tests and docs.
