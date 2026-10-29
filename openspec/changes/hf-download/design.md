# Design: Hugging Face downloads (issue #282)

> **Retrospective design: documents the implementation as shipped in v0.27.0.** It proposes nothing new. It records the decisions in the code and its comments, so the change can be archived.

Inputs: `proposal.md`, `specs/{model-download,ai-task-models,config-menu}/spec.md`, `tasks.md`, and the code on `main` (v0.27.0, including the hf-popular follow-up).

## Technical Approach

The change follows the repo's split between a pure core, IO in the runtime, and app glue:

- `src/core/ai/hf.rs` (pure). It parses the Hub JSON, builds the GGUF choices, the license, the token lookup, the cache paths, the URL paths and the fit heuristic. Every value from the Hub that becomes a path is validated here.
- `src/core/ai/picker/hub.rs` (pure). The UI state machine of the picker. Requests leave as `Outcome::Ask` and `Outcome::Download`, and replies come back through `Picker::hub_reply`.
- `src/runtime/hf.rs` (IO). Runs the system `curl` for API GETs and downloads, writes the cache and the token file, and owns the `Hub` worker (one download at a time).
- `src/app/hub.rs` (glue). Turns the effects (`Download`, `CancelDownload`, `HfAccount`) into events (`HubReply`, `DownloadProgress`, `DownloadEnd`, `HfAccount`), shows status bar progress and notices, and forces a redetect on success.

## Architecture Decisions

| # | Topic | Options / tradeoff | Decision |
|---|---|---|---|
| D1 | Transport | reqwest or hyper with rustls would add a TLS stack and many deps. The system `curl` is already used by the update check. | **`curl`, shelled out.** `-q` comes first so `~/.curlrc` is ignored. `--proto =https --proto-redir =https`. `--` goes before the URL. API calls get `--max-time 15`, `--max-filesize 4 MiB` and the status via `--write-out`. The process runs in its own group with `GroupKill`. The proxy and CA settings come from the environment. |
| D2 | Binary lookup | Inherit PATH, or require absolute entries. | `PATH` is refused (`NO_CURL`) when it is empty or has a relative entry. Otherwise a `curl` planted in the cwd could run. |
| D3 | Token transport | argv or env (visible in `ps`), or stdin config. | **`--config -` on stdin** as `header = "Authorization: Bearer …"`. A token with non-graphic chars, `"` or `\` is not sent. curl does not forward the header to a redirect host. `Secret` hides its value in `Debug` and zeroes its bytes on drop. |
| D4 | Token source | Store it in the lazarobox config, or use HF's own files. | `HF_TOKEN`, then `$HF_TOKEN_PATH`, then `$HF_HOME/token`, then `<cache>/huggingface/token`. Only absolute paths count. Blank means none. The token is read for each request and never cached. Connect checks `whoami-v2` first, then writes the file as 0600 through a temp file and a rename (folder 0700, symlinks followed). Disconnect deletes the file. |
| D5 | Resume | Restart from zero, or use HTTP Range. | **`--continue-at -`** writes into `blobs/<oid>.incomplete`. Cancel keeps the partial file. A partial longer than the expected size is deleted. |
| D6 | Integrity | Trust curl's exit code, or verify. | `keep()` runs in `spawn_blocking` and checks the exact size, then the sha256 (`sha2`) when the oid is a 64-hex LFS id. A mismatch deletes the partial file ("resuming it would only grow the mistake"). Only then is the file renamed to `blobs/<oid>`. |
| D7 | Cache layout | A private folder, or the HF hub cache. | **The HF hub layout**: `models--<owner>--<repo>/{blobs,snapshots/<commit>/<path>,refs/main}`, with a relative symlink from the snapshot to the blob, so other tools reuse the files. The hub folder is `gguf::hub_dir`, the same resolver the model scan uses. A `.incomplete` file is never listed, because it is not `.gguf`. |
| D8 | Path safety | | `is_repo_id` (no `..`, no `--`, limited charset), a commit of 40 hex chars, an oid of 40 or 64 hex chars, and `is_safe_path` (relative, normal components, no `\` or control chars). A reply can never name a file outside its repo folder. |
| D9 | Popular fit heuristic | Show everything, or filter by RAM. | The size estimate is `params / 8 * 5` (about Q4_K_M). A model fits when it is ≤ 70% of RAM (`sysconf`). The popular list asks for 100 repos, keeps those that fit, then the top 20 by downloads. With RAM unknown, all are kept. Searches and files are marked `too big for this PC`, never hidden. |
| D10 | Concurrency | | One download at a time, in both `Hub::start` (open cancel channel) and `App::start_download`. Progress events are `try_send` and may be dropped. The end event is always sent. |
| D11 | Space | | `missing_bytes` (a blob that is already there counts as 0, a partial counts as far as it goes) is compared with `statvfs` `f_bavail`. Confirm disables `y` when space is short, and `download()` checks again. If free space is unknown, the download is not blocked. |

## UI State Machine (`picker/hub.rs`)

```
HfDownload row ──Enter──▶ (download running? ─▶ cancel prompt y/n ─▶ CancelDownload)
      │
      ▼ open_hub: Query("") + asking=Popular
  Repos{popular} ──Esc──▶ Query ──Enter──▶ repo id? Ask::Files : Ask::Search
      │Enter                                    │
      ▼                                         ▼
  Ask::Files ──▶ Files(license row + choices) ──Enter──▶ Confirm ──y──▶ Download(job), leave hub
                                                         (n/Esc back; y ignored if `short`)
```

- `frames` is a stack. Esc pops a frame and restores its cursor, and popping the last frame leaves the hub.
- While `asking` is set, only Esc acts. A reply is kept only when it matches `asking`, so a late reply is dropped.
- An error stays on the screen that asked and comes back as a message.

## Data Flow

```
Picker ─Outcome::Ask─▶ Menu ─▶ App ─Effect─▶ runtime::Hub.ask ─curl─▶ huggingface.co
   ▲                                                         │
   └── hub_reply ◀── App ◀── AppEvent::HubReply(ask, answer) ┘
Confirm y ─▶ Effect::Download(job) ─▶ Hub.start ─▶ download(): space → fetch(curl, 250 ms ticks)
   → keep(size, sha256) → link snapshot → refs/main ─▶ DownloadProgress* / DownloadEnd
DownloadEnd(Done) ─▶ notice + forced redetect ─▶ the model is listed in the llamacpp picker
```

## Error Handling

- HTTP 401/403 reads `<id> needs a Hugging Face token: set HF_TOKEN`. 404 reads `not found`. Any other status reads `HTTP <n>`.
- Bad JSON, a reply that is too long or non-UTF-8 reads `Hugging Face request failed: …`.
- A curl failure shows the last line of its stderr, sanitized. A failed download ends with "(run it again to resume)".
- Other messages: `NO_HUB` (no cache folder), `NO_CURL`, and `not enough disk space: X needed, Y free`.

## File Map

| File | Role |
|---|---|
| `src/core/ai/hf.rs` (+`hf/tests.rs`) | Pure types, parsing, validation, paths, token lookup, fit heuristic |
| `src/core/ai/gguf.rs` | `hub_dir`, `split_part` (shared with the model scan) |
| `src/core/ai/picker/hub.rs`, `src/core/ai/picker.rs` (`HfDownload` row, `Outcome`) | Picker screens |
| `src/core/menu.rs` (+`menu/hub_tests.rs`, `menu/account_tests.rs`) | Commands, `Hugging Face` account row |
| `src/runtime/hf.rs` (+`hf/tests.rs`) | curl, download, token file, `Hub` worker |
| `src/runtime.rs` (`with_hub`, `hub_dir`, effects) | Wiring |
| `src/app.rs`, `src/app/hub.rs` (+`app_tests/hf_download.rs`, `hf_account.rs`) | Events, status bar, redetect |
| `Cargo.toml` | `sha2` as a direct dependency |

## Testing Strategy

| Layer | What | Approach |
|---|---|---|
| Unit (pure) | Parsing, choices/splits, quant, license, token order, paths, fit | Table tests in `core/ai/hf/tests.rs` |
| Integration | API args, token on stdin only, layout, resume, cancel, checksum, connect/disconnect | Real `curl` against a local `TcpListener` HTTP server (`noproxy`, `https_only=false`); skipped with a notice when curl is missing |
| UI/app | Screens, late replies, nothing before `y`, progress, one redetect | Picker, menu and app event tests |
| Manual | Real download plus resume; gated repo (tasks 5.2, 5.3, still open) | Live |

Note: the spec scenarios say "fake curl", but the tests run the real curl against a fake Hub.

## Migration / Rollout

No migration is needed. Nothing is written to the lazarobox config. Shipped in v0.27.0.

## Known Gaps

- The `download from Hugging Face…` row only appears when llama-server is installed (`src/core/ai/picker.rs:252-267`, `status != NotInstalled`). A user without llama-server cannot reach the download flow. The follow-up change `llama-server-install` will change this.
- The manual live checks (tasks 5.2 and 5.3) are not ticked.
- Linux and Unix only: it uses `statvfs`, `symlink` and `sysconf`.
