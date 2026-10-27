# Ollama Download Specification (new: Ollama downloads)

## ADDED Requirements

### Requirement: Library and registry requests
The requests to `https://ollama.com` and `https://registry.ollama.ai` MUST go through the system `curl` with the hf-download options:
- run by name from the `PATH`, with no shell;
- `-q`, `--silent`, `--show-error`, `--proto =https`, `--proto-redir =https`;
- a 15 s limit and no redirects, except the registry blob GET of a license, which follows HTTPS redirects with `--location`.

They are sent only while the user is in this flow:
- library: `https://ollama.com/library?sort=popular`, an HTML page capped at 4 MiB;
- manifest: `https://registry.ollama.ai/v2/library/<model>/manifests/<tag>`, with `Accept: application/vnd.docker.distribution.manifest.v2+json`, capped at 4 MiB;
- license: `https://registry.ollama.ai/v2/library/<model>/blobs/<digest>`, capped at 64 KiB.

`<model>` and `<tag>` MUST each be ASCII letters, digits, `.`, `_` and `-`, 1 to 64 characters, not starting with `.` or `-`. Anything else is never sent. Failures read `Ollama library request failed: <reason>` and keep the screen that asked.

#### Scenario: Requests are HTTPS curl GETs [auto, fake curl]
- GIVEN a fake curl that records its argv
- WHEN the library, a manifest and a license are asked for
- THEN each run has `--proto =https`, `--` before the URL, no shell, and the URLs above

#### Scenario: Unsafe names are never sent [auto]
- GIVEN a card named `../x` or a tag `a b`
- WHEN the page is read and the model is chosen
- THEN the card or tag is skipped and no request names it

### Requirement: Popular models that fit this PC
The library page MUST be read into model cards, each with:
- a name: the `href="/library/<name>"` link;
- a description;
- capabilities (such as `tools`, `vision`, `thinking`, `embedding`, `cloud`);
- size tags (such as `1b`, `8b`, `0.5b`, `270m`, `e2b`): a number with an optional decimal part, then `b` or `m`, optionally prefixed by `e`;
- pulls (such as `86.3M`, `512K`).

A card that cannot be read is skipped. Cards with the `embedding` capability are left out. Cards with the `cloud` capability and no size tag are left out too.
A size tag's parameter count is its number × 10^9 (`b`) or × 10^6 (`m`), and a mixture of experts `<n>x<size>` counts `n` times its size. An "effective" size (`e2b`, `e4b`) gives no count: its download is several times what the name says (`gemma3n:e2b` is 7.8 GB), so it never makes a card fit; its exact size shows on the Sizes screen. The estimated size is 5 bits per parameter, and a size fits when that estimate is at most 70% of the total RAM (the hf-popular rule).
The Popular list keeps the cards with at least one size that fits, in page order (most pulled first), at most 20. With the memory unknown, no card is left out for its size.

#### Scenario: Read the real page [auto, fixture]
- GIVEN a saved copy of `library?sort=popular`
- WHEN it is read
- THEN `llama3.2` has the description, the `tools` capability, the sizes `1b` and `3b` and the pulls `86.3M`, and `nomic-embed-text` is left out as an embedding model

#### Scenario: Only what fits [auto]
- GIVEN 8 GB of RAM and cards with sizes `1b`, `8b`, `70b` and one with only `405b`
- WHEN the Popular list is built
- THEN the first three cards are listed and the `405b` card is not

#### Scenario: Unreadable page falls back [auto]
- GIVEN a library reply that yields no card (changed HTML, or an error page)
- WHEN the Popular screen opens
- THEN it lists the curated catalog entries that fit, after the info row `could not read the Ollama library; showing the built-in list`

### Requirement: Exact sizes and license before a pull
On the Sizes screen, each size tag of the chosen model MUST show the exact size: the sum of the `size` of every layer of its manifest. A tag whose manifest cannot be read shows `size unknown` and stays choosable.
On the Confirm screen, every license layer (`application/vnd.ollama.image.license`) is read, and its first non-empty line, at most 120 characters with control and format characters drawn as `?`, is its title. The screen shows `license: <title>` for each, then `read it: https://ollama.com/library/<model>:<tag>`. With no license layer it reads `license: not stated`. Nothing is pulled before `y`.

#### Scenario: Exact size from the manifest [auto]
- GIVEN a manifest with layers of 2019377376, 1429, 7711, 6016 and 96 bytes
- WHEN the Sizes screen shows `llama3.2:3b`
- THEN its row reads `llama3.2:3b  2.0 GB`

#### Scenario: License shown before the pull [auto]
- GIVEN a license layer whose text starts with `LLAMA 3.2 COMMUNITY LICENSE AGREEMENT`
- WHEN the Confirm screen opens
- THEN it shows `license: LLAMA 3.2 COMMUNITY LICENSE AGREEMENT` and the link, and no pull starts until `y`

### Requirement: Pull through the local server
After `y`, lazarobox MUST send one `POST /api/pull` with the body `{"model":"<model>:<tag>","stream":true}` to `127.0.0.1` on the configured Ollama port, through the shared loopback client, with no credential. Only one pull or download runs at a time, shared with the Hugging Face download.
- **Progress.** The NDJSON reply is read line by line (1 MiB line cap). Progress is the sum of `completed` over the sum of `total` of the digests seen. It is shown as `↓ <percent>% <model>:<tag> · <license title>` on the row and `↓ <percent>%` in the status bar.
- **Success.** `{"status":"success"}` ends it as done and requests one forced detection.
- **Failure.** A line with `error`, a non-200 status or a closed stream ends it as `failed: <reason>`, sanitized and capped at 200 characters.
- **Cancel.** Cancelling closes the connection and ends it as cancelled. Ollama keeps the parts it has, so the next pull of the same tag resumes.

#### Scenario: Progress then success [auto, fake server]
- GIVEN a fake Ollama answering the pull with status lines, two digests with `total`/`completed`, then `success`
- WHEN the pull runs
- THEN progress events rise to 100%, the end is done, and one forced detection is requested

#### Scenario: Error line fails the pull [auto, fake server]
- GIVEN a fake Ollama answering `{"error":"pull model manifest: file does not exist"}`
- WHEN the pull runs
- THEN it ends `failed: pull model manifest: file does not exist` and no detection is requested

#### Scenario: Cancel closes the request [auto, fake server]
- GIVEN a pull in progress against a fake server
- WHEN it is cancelled
- THEN the fake sees the connection closed and the end is cancelled

#### Scenario: Real pull [manual]
- GIVEN Ollama running and 8 GB of RAM
- WHEN `qwen3:4b` is chosen from the Popular screen and confirmed
- THEN it is pulled with progress, cancel then pull again resumes, and the model is listed and chats
