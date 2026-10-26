# Proposal: Hugging Face downloads (issue #282)

## Intent

Let a user find a GGUF model on the Hugging Face Hub from inside lazarobox, see its license, download it with progress into the standard Hugging Face cache, and run it with the managed llama-server. Today a model must be downloaded outside lazarobox first.

## Scope

In scope:
- The model picker of `llamacpp` gains a `download from Hugging Face…` row.
  - It opens a field that takes search text or an `owner/repo` id, then a list of repos, then the `.gguf` files of the chosen repo with their quantization and size.
  - A split model (`-00001-of-0000N.gguf`) is one choice, with the size of all its parts.
- A confirmation before any byte is downloaded. It names the file, its size, the license and a link to read it, the target folder and the free space. Nothing downloads until `y`.
- The download runs in the background with the system `curl`, one at a time.
  - Progress shows in the picker and the status bar. It can be cancelled, and running it again resumes it.
  - It goes into the Hugging Face hub cache (`models--<owner>--<repo>/blobs`, `snapshots/<commit>`, `refs/main`), so other tools reuse it.
  - When it ends, the engines are detected again, so the model is listed and choosing it starts the managed llama-server.
- An optional token for gated and private repos.
  - It is read from `HF_TOKEN`, `$HF_TOKEN_PATH`, `$HF_HOME/token` or `~/.cache/huggingface/token`, never from or into lazarobox's config.
  - It is never shown and never in a command line, and it is sent only to `huggingface.co`.

Out of scope:
- `.safetensors` and other formats: llama-server needs GGUF.
- Ollama pulls, candle, in-process inference, the HF Inference API.
- Deleting downloaded models (the README says where they are).
- Downloading at startup or in the background without a user action.

## Network policy

This is the first request lazarobox sends to the internet for a model. It is narrow:
- Hosts and protocol: HTTPS only, to `huggingface.co`, plus the download redirects it gives (HTTPS only after redirects). The token is never sent to a redirect host.
- Trigger: only while the user is in this flow (search, list files, download). Nothing is sent at startup, on detection or by a chat turn.
- Transport: the system `curl`, as for the update check. No TLS stack is added. The proxy and CA settings come from the environment.

The ai-task-models rule "lazarobox MUST NOT download, pull or install any model" changes to allow this flow and nothing else (delta below). When the change is archived, the Purpose line of that spec ("Nothing is downloaded and nothing is fetched") is updated to point at model-download.

## Capabilities

- New `model-download`: Hub requests, the token, the license, the download, the cache layout.
- Modified `ai-task-models`: the no-download rule.
- Modified `config-menu`: the picker rows and screens.

## Risks

- Credentials: the token must never leak. It is passed to curl on stdin (`--config -`), never in argv. curl drops the `Authorization` header on a redirect to another host.
- A shared cache: it is written only under the chosen repo's folder, with a partial file named `.incomplete` that the model scan never lists. The final size and the LFS sha256 are checked before the file is linked into the snapshot (the `sha2` crate, already in the lock file, becomes a direct dependency).
- Disk space: checked before the download starts.
