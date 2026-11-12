# Apply progress: gsc-lazarobox-client

Mode: Strict TDD (`cargo test`). Delivery: 3 chained PRs, `stacked-to-main`. Branch `feat/gsc-client-core`.

## PR1: core, fetch, cache (done, 9/9 tasks)

| Task | Status | Notes |
|------|--------|-------|
| 1.1 pin and `pinned` | done | `PROJECT_NUMBER: Option<&str> = None`; empty pin counts as no pin |
| 1.2 `accept` | done | returns `Result<Client, Why>`; `Why` carries the `<why>` text |
| 1.3 notices, constants | done | `client_json` reused from the own-client code (already rebuilds Google's endpoints) |
| 1.4 `Curl::google_client` | done | `hf::CLIENT` fetch spec; args test equals design 4 exactly |
| 1.5 `Target.backup: Option` | done | `CACHE` target; `replace_with` reports no backup when none |
| 1.6 save the cache | done | `save_cache_with`; identical bytes skip the write |
| 1.7 read back with the pin | done | `read_client_file` (file name) and `read_cache`; `read_client_with` kept as a wrapper so its callers and tests are unchanged |
| 1.8 `refresh` | done | `Fresh / Stale(notice) / Failed(notice)` in `src/runtime/google_client.rs` |
| 1.9 gate | done | see counts in the report; `rm -rf target` run after the checks |

## Deviations

- `read_client_with` keeps its signature and delegates to `read_client_file(fs, secrets, file)`, instead of changing its signature (task 1.7 wording). Existing tests pass unchanged.
- A no-`Content-Length` reply over the cap is cut by curl at 16 KiB, so it reaches `accept` as a truncated body and is reported as `not a Desktop client file`, not `larger than 16 KiB`. It is still refused and never read beyond the cap.
- A fetch that succeeds but whose cache write is refused returns `Failed` with the `not saved` notice (no fallback: the cache is the thing that is broken).
- When the fetch fails and the saved copy fails the file checks, the `Failed` notice is the failure notice plus `; the saved copy is not used: <cache problem>`.
- CHANGELOG untouched: PR1 is not user-visible; the entry is task 2.10.

## Environment note

The shell umask here is 002, which makes one existing test (`runtime::mcp::presence::tests::the_client_id_follows_the_presence_and_never_the_secret`) fail on a clean main. The full suite was run with `umask 022`.
