# Proposal: Ollama downloads

## Intent

Let a user pull an Ollama model from inside lazarobox without typing its name. The model is picked from the most popular models of the Ollama library that fit this PC's memory, and its license is shown before anything is downloaded. This mirrors the Hugging Face flow (hf-download, hf-popular). Today an Ollama model must be pulled outside lazarobox first (`ollama pull`), and the curated catalog only lists names marked `download required`.

## Scope

In scope:
- The model level of `ollama` gains a last row `download from Ollama…`, shown when the Ollama server is running (the pull goes through it).
- **Popular screen.** Enter on the row reads the Ollama library sorted by popularity (`https://ollama.com/library?sort=popular`, an HTML page).
  - Each model card gives its name, description, size tags (`1b`, `3b`, `8b`…), capabilities and pulls.
  - Models with the `embedding` capability are left out, and so are cloud-only models (a `cloud` tag and no size tag).
  - A model is listed when at least one of its sizes fits this PC: parameters from the size tag, 5 bits per parameter, at most 70% of the total RAM (the hf-popular rule).
  - At most 20 models are listed, the most pulled first, with no typing needed.
- **Sizes screen.** Enter on a model lists its size tags as `<model>:<tag>`, each with its exact size, the sum of the manifest's layers from `https://registry.ollama.ai/v2/library/<model>/manifests/<tag>`. A size too big for this PC is marked `too big for this PC`.
- **Confirm screen.** Before any byte is pulled it shows:
  - the model and its size;
  - the license title (the first non-empty line of each license layer, read from the registry) and a link to read it;
  - the folder Ollama stores models in, when known, and the free space there.

  Nothing is pulled until `y`.
- **Pull.** The pull runs in the background through the local Ollama server, `POST http://127.0.0.1:<port>/api/pull` with `{"model": "<model>:<tag>", "stream": true}`.
  - Progress is read from the streamed NDJSON (`total`, `completed`) and shown on the row and in the status bar, as the Hugging Face download is.
  - Cancelling closes the request. Ollama keeps the parts it already has, so pulling again resumes.
  - At `{"status":"success"}` the engines are detected again, so the model is listed and can be chosen.
- **Fallback.** When the library page cannot be read or parsed, the Popular screen lists the curated catalog instead, with the same fit rule, and says so in an info row.

Out of scope:
- Searching the Ollama library by text (the page is HTML; a later change can add it).
- A mark for commercial use of a license (planned for both download flows in a later change).
- Deleting models (`ollama rm`), registries other than the Ollama library, and models with a namespace (`user/model`).
- GPU memory (VRAM): only the total RAM is measured.

## Network policy

This extends the narrow policy of hf-download.
- **ollama.com:** HTTPS only, one GET of `/library?sort=popular`, through the system `curl` with the hf-download limits (15 s, 4 MiB reply cap, no redirects).
- **registry.ollama.ai:** HTTPS only, through the same `curl`:
  - one manifest GET per size tag of the chosen model, on the Sizes screen;
  - one GET per license layer, on the Confirm screen. The license text is capped at 64 KiB and only its title is shown.
- **The pull:** one loopback POST to `/api/pull` on the configured Ollama port, sent only after `y`. The loopback client's allowed paths gain `/api/pull`, for this flow only. No chat request ever asks a server to download a model.
- **Trigger:** all of these are sent only while the user is in this flow. Nothing is sent at startup, on detection or by a chat turn.

The ai-task-models rule "lazarobox MUST NOT pull or install any model through a server" (as rewritten by hf-download) changes to allow this flow and nothing else.

## Capabilities

- New `ollama-download`: the library page read, the sizes, the license, the pull, the fallback.
- Modified `ai-task-models`: the no-pull rule.
- Modified `ai-engine-detection`: the shared loopback client may POST to `/api/pull` for this flow only.
- Modified `config-menu`: the Ollama row and screens.

## Risks

- **The library page is HTML, not a supported API.** It can change at any time. The parser reads only what it needs, and every card it cannot read is skipped. A page that yields no model triggers the curated fallback with an info row, never a crash or an empty dead end. A fixture of the real page is kept in the tests.
- **A pull goes through the user's own Ollama server**, which downloads from its registry. lazarobox never writes into the Ollama model store itself.
- **The size estimate on the Popular screen can be off.** The Sizes screen shows exact sizes before the user confirms.
- **License visibility.** The Confirm screen always shows the license title and a link. With no license layer it reads `license: not stated`.
