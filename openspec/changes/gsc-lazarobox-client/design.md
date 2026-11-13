# Design: Link Search Console with the lazarobox OAuth Client

## Technical Approach

A pure core module decides everything about the fetched client: pin, parsing, the canonical cache text, and the notices. A runtime module runs one `curl` GET through the existing `hf::Curl` (as the update check and the Hub do) and writes the cache through the secrets writer's handle checks. The route is chosen by **value**: Link fetches only when the plan's `GSC_OAUTH_CLIENT_SECRETS_FILE` equals the cache path. Any other path takes today's code, so own-client users see no change.

Delivery: three chained PRs (see Slicing).

## Architecture Decisions

| # | Topic | Options | Decision |
|---|---|---|---|
| 1 | Module layout | one runtime module · core + runtime | **Core** `src/core/ai/mcp_secrets/lazarobox_client.rs` (no I/O). **Runtime** `src/runtime/google_client.rs` (fetch, cache read and write, fallback) |
| 2 | Pin | `Option<u64>` · `Option<&str>` · `option_env!` | **`pub const PROJECT_NUMBER: Option<&str> = None`** in the core module. A string avoids formatting. `option_env!` rejected: a pin set outside the source cannot be reviewed. `pinned(id, pin)` is true only when `id` starts with `<pin>-` and passes `valid_id`. A unit test asserts that a `Some` pin is ASCII digits only. Every function takes `pin: Option<&str>` as a parameter. Production passes the constant; tests pass `Some("123")`. `Linker` and the `Connectors` state hold the pin they were built with, and tests set it |
| 3 | What is cached | fetched bytes · rebuilt text | **Rebuilt**: `client_json(id, secret)`, which writes fixed Google `auth_uri`/`token_uri`/`redirect_uris`. Rejected: verbatim bytes, because a JSON that passes the pin could still point `token_uri` at another host |
| 4 | curl | new fetcher · `hf::Curl` | **`Curl::google_client(path)`**: label `javilazaro.es`, base `https://www.javilazaro.es`, no token. `Fetch { accept: "application/json", cap: CLIENT_MAX, cut: false, follow: false }`. Exact args: `-q --silent --show-error --user-agent <ua> --proto =https --proto-redir =https --max-time 15 --connect-timeout 15 --max-filesize 16384 --header "Accept: application/json" --write-out "\n%{http_code}" -- https://www.javilazaro.es/lazarobox/google-client.json`. Without `-L`, a 301 comes back as status 301 and is refused. No `--fail`: every non-200 status is mapped to a notice. The read stops at cap+8 bytes, so a reply without a `Content-Length` is still capped. `-q` skips `~/.curlrc`. No cookies, no token |
| 5 | Cache path | `$HOME/.config/lazarobox/…` · next to `secrets.toml` | **`<secrets_path().parent()>/lazarobox-google-client.json`**, which honours `XDG_CONFIG_HOME`. The planner and the `f` preset write this **absolute** path. They do not use the home-relative `preset_for`. `Presence::with_client_cache(Option<String>)` carries the path to the menu |
| 6 | Cache write | `std::fs` · `replace_with` | **`replace_with`** with a new `Target { file: CACHE_FILE, backup: None, max: CLIENT_MAX, keep_mode: false }`. `Target.backup` becomes `Option<&'static str>`. With `None`, `swap` skips the backup, so no stale secret copy is kept. The write is skipped when the existing bytes are identical. The cache is read back with `read_client_with`, generalised to take a file name (0600, not a symlink, not hard-linked, owned by the user) |
| 7 | Chat-tool access | new deny entry · existing | **Existing**. `.config/lazarobox` is in the deny list (`paths.rs:34`), and the tool runtime passes the real XDG config folder as an extra root (`host.rs:260`). Confirmed by a test that reads the cache file and is refused |
| 8 | Refetch | once · every Link | **Every Link** (owner decision 1). On any failure (network, HTTP, JSON, pin, size), a cache that passes the pin is used, nothing is written, and a notice is shown. With no valid cache, Link stops with a notice |
| 9 | Link trigger | menu effect · inside `Linker::check` | **Inside `Linker::check`**, async, before the existing `spawn_blocking`. `AppEvent::GscLine` gains `Option<String>` (the fallback notice). `link_reply` puts it in `said` while the prompt shows |
| 10 | Screen route | infer · explicit state | **`ClientRoute { Lazarobox, Own }`** in `connect::Ask` and in `Google` state. The default is `Lazarobox` when the pin is `Some`. `u` toggles the route. With the pin `None`, the route is shown disabled. `Lazarobox` applies only when the chosen services are exactly `[gsc]`. With Workspace ticked, the screen behaves as in #736 |
| 11 | Planner | relax `NO_CLIENT` globally · per route | **Per route**. `Input.client: Option<&Client>` plus `Input.client_file: Option<&str>`. `plan_with` skips `read_client_with` on the `Lazarobox` route. `source()` fills `GSC_OAUTH_CLIENT_SECRETS_FILE` from `client_file`. On the `Lazarobox` route, a request that includes `google` is refused (`Refused::NeedsOwnClient`) |
| 12 | `f` preset | new `Way` · Enter toggles · extra key | **Enter toggles**. On that row, Enter writes the other route's path: unset or own path → cache path, cache path → own path. The row text names the target (`Enter writes the lazarobox client` / `Enter switches to your own client`). The existing replace prompt confirms before a set value changes. A new `Way` was rejected because `way_for` picks a way from the names that are set, not their values, and the two OAuth ways have the same names |
| 13 | "Already linked" | detect and skip · route by value | **Route by value**. Connect Google already leaves a declared `gsc` alone. `f` changes a value only on an explicit key plus a confirm. Link fetches only for the cache path. No token or `Presence::linked()` check is needed |

## Notices (exact)

Superseded in wording by the final code and by `specs/lazarobox-google-client/spec.md` (Failure notices), which is the reference. The footer is one clipped line, so the failure notices put the instruction first and the cause after it. The first draft of this table (`lazarobox client not fetched (<why>); link again later, or u uses your own client` and similar) is replaced by:

| Case | Text |
|---|---|
| Pin `None` (Link) | `own client: Esc, f on gsc · lazarobox: not configured in this build` |
| Pin `None` (screen, `u`, plan refusal) | `lazarobox client not configured in this build; this screen uses your own client` |
| Fetch failed, no cache | `own client: Esc, f on gsc · lazarobox: not fetched (<why>); link later` |
| Refused, no cache | `own client: Esc, f on gsc · lazarobox: refused (<why>); nothing saved` |
| Fallback to cache | `lazarobox client not refreshed (<why>); linking with the saved copy` |
| Fetch failed, saved copy fails its checks | the failure notice, then `; the saved copy is not used: lazarobox-google-client.json: <reason>` |
| Cache write refused | `lazarobox client not saved: lazarobox-google-client.json: <refusal>` |
| Screen client line | `client: lazarobox (fetched from www.javilazaro.es when you link)` |

From the `gsc` entry the prefix is `own client: f`. Rationale for the change: with the first-draft texts the footer clipped the instruction (see apply-progress, PR2 review fixes).

`<why>` is one of: the curl error line (sanitised), `HTTP <code>`, `redirected (HTTP 3xx)`, `larger than 16 KiB`, `not a Desktop client file`, `not from the lazarobox project`. The text never includes the secret.

## Data Flow

```
g ─► gsc::plan ─► Effect::LinkGsc(plan) ─► Linker::check
        plan.file == cache? ──no──► existing uv checks ─► GscLine(line, None)
              │yes
              ▼
   Curl GET ─► core::accept(status, body, pin) ─ok─► replace_with(cache) ─► uv checks
              │err                                                      ─► GscLine(line, None)
              ▼
   read cache + pin ─ok─► uv checks ─► GscLine(line, Some(fallback notice))
              │err
              ▼
   GscLine(Err(notice))
```

## File Changes

| File | PR | Change |
|---|---|---|
| `src/core/ai/mcp_secrets/lazarobox_client.rs` (new) | 1 | pin, `pinned`, `accept`, `CACHE_FILE`, `URL_PATH`, notices |
| `src/runtime/hf.rs` | 1 | `Curl::google_client` constructor |
| `src/runtime/google_client.rs` (new) | 1 | `refresh(curl, pin, secrets) -> Fresh / Stale(notice) / Failed(notice)` |
| `src/runtime/secrets_write.rs`, `secrets_write/google.rs` | 1 | `Target.backup: Option`, cache target, save, read by file name |
| `src/runtime/mcp/link.rs`, `src/runtime.rs`, `src/app.rs` | 2 | fetch before checks, `GscLine` notice, wiring (PATH, secrets, pin) |
| `src/core/ai/mcp_catalog/connect.rs`, `presence.rs`, `entries.rs` | 2 | `ClientRoute`, `Input` fields, cache path, gsc guide text |
| `src/core/menu/connectors/connect.rs`, `fields.rs`, `connectors.rs` | 2 | `u`, client line, checklist hidden on the `Lazarobox` route, Enter toggle |
| `README.md` (line 41, line 1632), `CHANGELOG.md` | 2 | Sin telemetría: one GET to `www.javilazaro.es`, only when you link Search Console with the lazarobox client. It reveals the IP address and the version (User-Agent `lazarobox-shell/<v>`) |
| `docs/conectores.md`, `openspec/specs/*` | 3 | guide and spec sync |

## Testing Strategy (strict TDD)

| Layer | What | How |
|---|---|---|
| Unit (core) | pin prefix, `-` boundary, foreign number, `None`; `accept` for 200/301/404/bad JSON/`web` key/oversize; canonical text drops a foreign `token_uri`; notice texts | pure functions, injected pin |
| Unit (runtime) | args exactly as decision 4; 200 → cache 0600 written; 301 or foreign pin → nothing written; failure plus a valid cache → `Stale`; symlinked or 0644 cache → not used | the local `serve` from `hf/tests.rs` plus `Curl::local`, temp config folder, the `SaveFs` fakes |
| Unit (link) | cache path fetches, other path never spawns curl | a curl with an empty PATH proves no spawn |
| Unit (menu) | route default, `u`, pin `None` disabled line, checklist hidden, Enter toggle and confirm, Workspace ticked unchanged | `TestBackend` key tests |
| Unit (tools) | read of the cache refused | `paths.rs` fixture |

## Slicing (production lines; tests excluded)

| PR | Scope | Estimate |
|---|---|---|
| 1 | core, fetch, cache; not reachable | ~220 |
| 2 | link, planner, screen, `f`, README network line, CHANGELOG | ~300 |
| 3 | `docs/conectores.md`, spec sync | ~100 docs |

## Migration / Rollout

No migration. The pin stays `None` until the owner supplies it, so the route ships disabled with its notice. Do not announce the feature until the Google app is published.

## Open Questions

- [ ] The project number of the `lazarobox` project (owner). This blocks only the real use, not PR 1 or PR 2.
