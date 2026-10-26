# Model Download Specification (new: Hugging Face downloads)

## ADDED Requirements

### Requirement: Hub requests
Every request of this capability MUST go through the system `curl`, run by name from the `PATH` with no shell, with `-q`, `--silent`, `--show-error`, `--proto =https` and `--proto-redir =https`. It MUST be sent only to `https://huggingface.co` (API requests never follow redirects; downloads follow them with `--location`). It is sent only while the user is in the download flow of config-menu. The API requests are:
- search: `/api/models?search=<text>&filter=gguf&sort=downloads&limit=20`;
- repo: `/api/models/<owner>/<repo>`, which gives the commit (`sha`), `gated` and `cardData`;
- files: `/api/models/<owner>/<repo>/tree/<commit>`, which gives each file's `path`, `size` and `lfs.oid`.

Each API request has a 15 s limit and a reply cap of 4 MiB. Failures read:
- no `curl`: `Hugging Face needs curl on the PATH`;
- 401 or 403: `<owner>/<repo> needs a Hugging Face token: set HF_TOKEN`;
- 404: `<owner>/<repo> not found on Hugging Face`;
- anything else: `Hugging Face request failed: <reason>`.

#### Scenario: Only HTTPS to the Hub [auto]
- GIVEN a search, a repo and a files request
- WHEN their curl arguments are built
- THEN each starts with `-q`, holds `--proto =https` and `--proto-redir =https`, ends with `--` and an `https://huggingface.co/` URL, and holds no token

#### Scenario: Failures by status [auto]
- GIVEN replies 401, 403, 404 and 500 for `o/r`, and a missing curl
- WHEN they are read
- THEN the messages are the ones above, in that order

### Requirement: Token
The token MUST be read, in this order, from:
1. `HF_TOKEN` (non-empty);
2. the file named by `HF_TOKEN_PATH`;
3. `$HF_HOME/token`;
4. `$XDG_CACHE_HOME/huggingface/token`, else `~/.cache/huggingface/token`.

It is trimmed, and a blank one counts as none. It MUST NOT be read from or written to lazarobox's config, shown, logged, or put in any argv. When present, it is given to curl on stdin as a config line (`header = "Authorization: Bearer <token>"`) for requests to `huggingface.co`. curl does not forward it to a redirect host.

#### Scenario: Lookup order [auto]
- GIVEN `HF_TOKEN` empty, `HF_TOKEN_PATH` naming a file with `  t1\n`, and `$HF_HOME/token` holding `t2`
- WHEN the token is read
- THEN it is `t1`; with no `HF_TOKEN_PATH` it is `t2`; with `HF_TOKEN=t0` it is `t0`; with none of them and no file, there is none

#### Scenario: Token never in argv [auto]
- GIVEN a token `secret-123`
- WHEN any request or download is built
- THEN no argument contains `secret-123`, and the stdin config holds it once

### Requirement: GGUF choices
The files of a repo MUST be reduced to the GGUF choices:
- Only names ending in `.gguf` count (any case), and `mmproj` files are dropped.
- A split model (`<name>-00001-of-0000N.gguf`) is one choice, whose size is the sum of its parts. It is listed only when all N parts are present.
- Each choice shows its quantization, its size and its file name. The quantization is the last `Q…`, `IQ…`, `F16`, `BF16` or `F32` token of the name (case kept as written, upper-cased), or `?`.
- Sizes use decimal units, with one decimal from GB up: `415 MB`, `1.3 GB`.
- The choices are sorted by size.

#### Scenario: Choices from a tree [auto]
- GIVEN files `a-Q4_K_M.gguf` (4 GB), `a-q8_0.gguf` (8 GB), `mmproj-a-f16.gguf`, `big-Q6_K-00001-of-00002.gguf` and `big-Q6_K-00002-of-00002.gguf` (5 GB each), `README.md`
- WHEN the choices are built
- THEN they are `Q4_K_M 4.0 GB`, `Q8_0 8.0 GB`, `Q6_K 10.0 GB (2 parts)`, in that order

#### Scenario: Missing part [auto]
- GIVEN only part 1 of a two-part model
- WHEN the choices are built
- THEN it is not listed

### Requirement: License before download
Before any byte of a model is downloaded, the confirmation MUST show the license and where to read it:
- the license id, from `cardData.license`, else the first `license:` tag, else `not stated`;
- the link, from `cardData.license_link` when it is an `https://` URL, else a file of the repo named `LICENSE*` (any case) at the commit (`https://huggingface.co/<owner>/<repo>/blob/<commit>/<file>`), else the model page `https://huggingface.co/<owner>/<repo>`.

The download line keeps naming the license while it runs.

#### Scenario: License sources [auto]
- GIVEN repo info with `license: apache-2.0` and `license_link`, one with `license: gemma` and a `LICENSE` file, and one with neither
- WHEN the license is read
- THEN the first shows `apache-2.0` and its link, the second `gemma` and the blob URL of `LICENSE` at the commit, and the third `not stated` and the model page

### Requirement: Download into the Hugging Face cache
A download MUST write only under `<hub>/models--<owner>--<repo>/`. `<hub>` is `$HF_HUB_CACHE`, else `$HF_HOME/hub`, else `$XDG_CACHE_HOME/huggingface/hub`, else `~/.cache/huggingface/hub`, all absolute. For each file of the choice:
1. curl fetches `https://huggingface.co/<owner>/<repo>/resolve/<commit>/<path>` with `--location`, `--fail` and `--continue-at -` into `blobs/<lfs.oid>.incomplete`.
2. When it ends, the file size MUST equal the listed size. Otherwise it fails with `download failed: size mismatch` and the partial file is removed.
3. It is renamed to `blobs/<lfs.oid>`.
4. `snapshots/<commit>/<path>` becomes a relative link to it, replacing an older link.

When every file is done, `refs/main` holds the commit. A blob that already exists with the right size is not fetched again.

Before starting, the free space of the target filesystem MUST cover the bytes still missing. Otherwise nothing starts and it reads `not enough disk space: <need> needed, <free> free`.

Only one download runs at a time. Its progress is the bytes on disk over the total, read about every 250 ms. A cancel stops curl and keeps the `.incomplete` file, so starting the same choice again resumes it. A curl failure reads `download failed: <reason> (run it again to resume)`. The curl process group is registered with the watcher of process-watcher, so quitting stops it like any other process lazarobox starts.

When it ends, the engines are detected again and the status bar reads `downloaded <file>`.

#### Scenario: Layout after a download [auto, fake curl]
- GIVEN a fake curl that writes the listed bytes
- WHEN choice `Q4_K_M` of `o/r` at commit `c1` is downloaded
- THEN `models--o--r/blobs/<oid>` exists with that size, `snapshots/c1/a-Q4_K_M.gguf` links to `../../blobs/<oid>`, `refs/main` reads `c1`, no `.incomplete` file is left, and the local model scan lists the snapshot file

#### Scenario: Resume and cancel [auto, fake curl]
- GIVEN a download cancelled halfway
- WHEN it is started again
- THEN curl runs with `--continue-at -` on the same `.incomplete` file, and no snapshot link exists until it ends

#### Scenario: No space [auto]
- GIVEN 1 GB missing and 0.5 GB free
- WHEN the download is asked for
- THEN curl never runs and the message reads `not enough disk space: 1.0 GB needed, 500 MB free`

#### Scenario: Real download [manual]
- GIVEN a real machine with curl and llama-server
- WHEN `Qwen/Qwen2.5-0.5B-Instruct-GGUF` `Q2_K` is downloaded from the picker, cancelled at about 30 %, started again and finished
- THEN it resumes, `hf cache scan` (or `huggingface-cli scan-cache`) lists it, the picker lists the file, choosing it starts the managed llama-server, and a chat turn answers
