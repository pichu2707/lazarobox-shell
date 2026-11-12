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
- RESOLVED after review: a reply over the cap (with or without `Content-Length`) made curl exit 63 and still print a status. `Curl::fetch` now fails with `reply too long` on exit 63 (non-cut fetches only), and `refresh` maps it to `Why::TooLarge` (`larger than 16 KiB`). A valid client padded past the cap is refused. `pinned` also needs a non-empty id part after `<pin>-`, and `Some("")` is no pin in `accept` and `refresh`.
- A fetch that succeeds but whose cache write is refused returns `Failed` with the `not saved` notice (no fallback: the cache is the thing that is broken).
- When the fetch fails and the saved copy fails the file checks, the `Failed` notice is the failure notice plus `; the saved copy is not used: <cache problem>`.
- CHANGELOG untouched: PR1 is not user-visible; the entry is task 2.10.

## Environment note

The shell umask here is 002, which makes one existing test (`runtime::mcp::presence::tests::the_client_id_follows_the_presence_and_never_the_secret`) fail on a clean main. The full suite was run with `umask 022`.

## PR2: link, planner, screen, `f` preset, README, CHANGELOG (done, 2.1-2.8, 2.10-2.12; 2.9, 2.13, 2.14 manual are owner tasks)

Branch `feat/gsc-client-link` from origin/main (after PR1, #737). Not pushed; review first.

| Task | Status | Notes |
|------|--------|-------|
| 2.1 Link fetches by value | done | `Linker::new(events, opener, Fetch)`; `answer(plan, fetch, check)` fetches only when `plan.file == cache_path(secrets)`; `AppEvent::GscLine(Result, Option<String>)`; `link_reply(reply, said, room)` leaves the fallback notice in the footer while the prompt shows |
| 2.2 nothing else fetches | done | test in `link/fetch_tests.rs`: `refresh(` is not called from `runtime.rs`, `app.rs` or the presence reader; the own path spawns no curl (empty PATH) |
| 2.3 `ClientRoute` and planner | done | `Ask.route`, `Input { client: Option, client_file, route }`, `Refused::NeedsOwnClient`; `plan_with(.., pin)` skips the own-client read on the lazarobox route and refuses it without a pin |
| 2.4 cache path plumbing | done | `mcp_secrets::cache_path(secrets)`; `Presence::with_client_cache`; reader fills it; pin `None` => plan refused with the not-configured notice |
| 2.5 route state and `u` | done | `Connectors.pin` (= `PROJECT_NUMBER`, tests set it), `Google.own` reset on `open_google`; `o` selects own; `u` toggles, or says why not |
| 2.6 screen drawing | done | route line, `CLIENT_LINE`, no checklist or own client on the lazarobox route; two new insta snapshots; existing snapshots unchanged |
| 2.7 Link failure offers own client | done | the refresh notices already carry `u uses your own client`; menu tests prove footer notice and fallback notice |
| 2.8 `f` row, Enter toggles | done | `client_target` / `client_cell` in `fields.rs`; note line `client file: Enter ...`; gsc guide text updated |
| 2.9 `PROJECT_NUMBER` | owner | still `None`; never guessed |
| 2.10 README, CHANGELOG | done | README line 41 and the network list (~1632); CHANGELOG `[Unreleased]` `### Added` |
| 2.11 chat-tool access | done | test only; the existing `.config/lazarobox` deny entry already refuses the cache, also as an extra root |
| 2.12 gate | done | fmt, clippy `-D warnings`, `cargo test` all pass (see report) |
| 2.13, 2.14 manual | owner | live checks |

## PR2 deviations

- `Connectors.pin` is a plain field set from `PROJECT_NUMBER` in `Connectors::new` (the design says the state holds the pin); a `#[cfg(test)]` setter on `MenuState` changes it.
- `plan_with` gained a `pin` argument (design 10 only named `client` and `client_file`): defence in depth so a build with no pin never writes the cache path, whatever the menu sends.
- `Refused::NeedsOwnClient` notice: `the lazarobox client is only for Search Console alone; u uses your own client`.
- The `u` key on Connect Google when `gsc` is already declared only says to use `f` on the entry (the route is by value; the plan skips a declared service).
- The `f` row cell shows `lazarobox client` / `your own client` / `not set`; the target text (`Enter writes the lazarobox client`, `Enter switches to your own client`, or the not-configured notice) is a `client file:` line under the values, because the cell cannot hold both at 62 columns.
- Hint line on Connect Google with a pin: `j/k · s/w tick · e e-mail · o own · u route · n plan · Esc` (the usual one is 58 columns already).
- Test for 2.2 is a source check plus the empty-PATH proof, not an effect-level run of every UI path.
