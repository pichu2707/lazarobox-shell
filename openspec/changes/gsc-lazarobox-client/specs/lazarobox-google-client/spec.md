# lazarobox-google-client Specification

## Purpose

Lets a user link Search Console without a Google Cloud project, using a Desktop OAuth client owned by the `lazarobox` Google Cloud project. lazarobox fetches the client's JSON from the maintainer's site on demand, checks it against an embedded project-number pin, and keeps a private cache that Search Console's login reads. Only `webmasters.readonly` (non-sensitive) uses this client; no other service does. Refs: #733, builds on `google-connect` (#736). Scenario tags: [auto] = `cargo test` (fake `curl`, temp config folder); [manual] = real machine.

Terms: "the lazarobox client" = the OAuth client published at the URL below. "The cache" = `lazarobox-google-client.json` in the lazarobox config folder (name confirmed in design). "The pin" = the embedded project number of the `lazarobox` Google Cloud project. "Link" = the Link action of the Search Console connector (config-menu, Connect Google and the gsc entry). "Own client" = the user's `google-oauth-client.json` (mcp-secrets, OAuth client file read back).

## ADDED Requirements

### Requirement: When the fetch may run
lazarobox MUST contact the URL only when the user starts Link for Search Console and the file that link would use is the cache (the lazarobox route). It MUST NOT contact it at startup, when a menu or screen opens, when a plan is built, when a server starts, on a timer, or when Link runs with the own client or for any other service. Every Link on the lazarobox route MUST fetch, so a client the owner rotated is picked up; the cache is never trusted without trying the fetch first. The fetch MUST run off the UI thread and MUST NOT block it.

#### Scenario: Link on the lazarobox route fetches [auto]
- GIVEN Search Console on the lazarobox route and the pin configured
- WHEN Link starts, twice in a row
- THEN `curl` is run once per Link, each before any `uv` check

#### Scenario: Nothing else fetches [auto]
- GIVEN a fake `curl` that records its runs
- WHEN the app starts, the Connectors area and the Connect Google screen open, a plan is built and `y` saves it, a server starts, and Link runs on the own-client route or for Workspace
- THEN `curl` is never run

### Requirement: Request shape
The fetch MUST be one `GET https://www.javilazaro.es/lazarobox/google-client.json` run by the system `curl` child, with the host `www.javilazaro.es` (the bare domain redirects and MUST NOT be used). It MUST allow only HTTPS (`--proto =https`), MUST NOT follow redirects (a 3xx is a failure), MUST be bounded in time, and MUST cap the body at 16 KiB (`CLIENT_MAX`). The request MUST send no cookie, token, header or body of the user, and no data about the user, the machine or the project; only what `curl` sends by default plus the lazarobox user agent (name and version) is sent. The server therefore learns the IP and the version, nothing else. No new HTTP crate is added.

#### Scenario: The command line is fixed [auto]
- GIVEN a fake `curl` that records its arguments
- WHEN the fetch runs
- THEN the arguments hold the exact URL, `--proto =https`, no redirect-following option, a time limit and a size cap, and hold no cookie, header or data option carrying user data

#### Scenario: A redirect is a failure [auto]
- GIVEN a fake `curl` that reports HTTP 301
- WHEN the fetch runs
- THEN it fails with `redirected (HTTP 301)` and nothing is cached

#### Scenario: Plain HTTP is never used [auto]
- GIVEN any configuration
- WHEN the fetch URL is built
- THEN it starts with `https://` and the `curl` arguments refuse other protocols

### Requirement: Size and shape of the answer
A body larger than 16 KiB MUST be refused, whether the cap stops the transfer or the length is checked after it, and MUST NOT be parsed or cached. The body MUST be the JSON Google downloads for a Desktop client: `installed.client_id` and `installed.client_secret`, other keys ignored, read by the same reader as the own client (mcp-secrets, OAuth client file read back). A `web` client, a missing key, a non-ASCII or non-printable value, an id that does not end in `.apps.googleusercontent.com`, or invalid JSON MUST be refused and MUST NOT be cached.

#### Scenario: Oversize [auto]
- GIVEN a fake `curl` that returns 17 KiB
- WHEN the fetch runs
- THEN it fails with `larger than 16 KiB` and nothing is parsed or cached

#### Scenario: Not a Desktop client [auto]
- GIVEN an answer holding only a `web` client
- WHEN the fetch runs
- THEN it fails with `not a Desktop client file` and nothing is cached

### Requirement: Integrity pin
The pin MUST be an embedded constant, a project number, never read from the network, a file or the environment. A fetched `client_id` MUST be `<project-number>-` followed by a non-empty remainder and `.apps.googleusercontent.com`, with the number equal to the pin, compared on the whole number (a longer number that merely starts with the pin is a mismatch). A mismatch MUST be refused, MUST NOT be cached and MUST NOT replace an existing cache. While the pin is not filled in the build, the lazarobox route MUST be unavailable: no fetch runs, and the user sees the not-configured notice (`lazarobox: not configured in this build`). The number MUST NOT be guessed.

#### Scenario: Matching id [auto]
- GIVEN the pin `123456789012` and an answer whose id is `123456789012-abc.apps.googleusercontent.com`
- WHEN the fetch runs
- THEN the client is accepted

#### Scenario: Foreign project [auto]
- GIVEN the same pin and an id `999-abc.apps.googleusercontent.com`, or `1234567890123-abc...` (longer number)
- WHEN the fetch runs
- THEN it fails with `not from the lazarobox project` and nothing is cached

#### Scenario: Pin not filled [auto]
- GIVEN a build whose pin is `None`
- WHEN Link starts on the lazarobox route
- THEN `curl` is not run and the not-configured notice is shown

### Requirement: Cache file
An accepted client MUST be written to the cache, a file separate from `google-oauth-client.json` and never replacing it, in the lazarobox config folder as the real folder resolves (`$XDG_CONFIG_HOME/lazarobox/`, else `$HOME/.config/lazarobox/`). The write MUST use the secrets writer's folder checks (the folder is not a symlink, is owned by the user, is not writable by group or others; a missing folder is created as that writer creates it) and its handle checks (a regular file, owned by the user, one link, no symlink at the final component), mode 0600, atomic by rename, and MUST leave no partial file after any failure. A cache that is read back (to use after a failed fetch, or to hand to Search Console) MUST be judged on its open handle: regular, owned by the user, one link, no group or other bit, at most 16 KiB, passing the pin. A cache that fails any of these MUST NOT be used and MUST be reported. The secret in the cache MUST NEVER be drawn, logged, put in a notice or sent to a model; at most the id is shown.

#### Scenario: Written private [auto]
- GIVEN an accepted client and an empty config folder
- WHEN the cache is written
- THEN it is a regular file with mode 0600 and holds the fetched JSON, and `google-oauth-client.json` is untouched

#### Scenario: A symlink or loose file is refused [auto]
- GIVEN the cache path is a symlink, or the existing cache has mode 0644 or two links
- WHEN the cache is written or read back
- THEN nothing is written through it or used, and a notice names the problem without the secret

#### Scenario: Failure leaves nothing [auto]
- GIVEN a write that fails after the temp file exists
- WHEN the write returns
- THEN no temp or partial cache remains

### Requirement: Fallback to a valid cache
When the fetch fails (no `curl`, network, HTTP status, oversize, bad JSON or pin mismatch) and the cache reads back valid and passes the pin, Link MUST continue with the cache and MUST show a visible notice saying the fetch failed and why, and that the saved client is used. When there is no valid cache, Link on the lazarobox route MUST stop before any `uv` check, MUST show the failure notice, MUST write nothing, and the screen MUST offer the own-client route. A failed fetch MUST NOT delete or alter an existing cache.

#### Scenario: Network down, cache valid [auto]
- GIVEN a valid pinned cache and a fake `curl` that fails to connect
- WHEN Link starts
- THEN the notice `lazarobox client not refreshed (<why>); linking with the saved copy` is shown, and the `uv` checks and login line proceed with the cache path

#### Scenario: Network down, no cache [auto]
- GIVEN no cache and the same failure
- WHEN Link starts
- THEN the `not fetched` notice is shown, no `uv` check runs, and the own-client route is offered (instruction `own client: Esc, f on gsc`)

#### Scenario: Mismatch does not poison the cache [auto]
- GIVEN a valid cache and a fetched client with a foreign project number
- WHEN Link starts
- THEN the cache is byte-identical afterwards, the fallback notice names `not from the lazarobox project`, and (the cache being valid) Link continues with it

### Requirement: Failure notices
Every failure MUST be a visible, sanitized notice; none may be silent or only logged. A notice MUST carry its cause and, when there is no valid cache, the way out (use your own client). The footer is a single clipped line, so a failure notice MUST put the instruction first and the cause after it, and both MUST be drawn at the 62-column popup. No notice holds the secret or the response body. The set, as the code shows it (`<why>` is the sanitized `curl` error line, `HTTP <code>`, `redirected (HTTP 3xx)`, `larger than 16 KiB`, `not a Desktop client file` or `not from the lazarobox project`):

| Case | Notice |
|------|--------|
| Fetch failed (`curl` missing or failed, network, HTTP status, redirect), no valid cache | `own client: Esc, f on gsc · lazarobox: not fetched (<why>); link later` |
| Answer refused (over 16 KiB, not a Desktop client, foreign project), no valid cache | `own client: Esc, f on gsc · lazarobox: refused (<why>); nothing saved` |
| Pin not filled | `own client: Esc, f on gsc · lazarobox: not configured in this build` |
| Fetch failed, saved copy valid | `lazarobox client not refreshed (<why>); linking with the saved copy` |
| Fetch failed and the saved copy fails its checks | the failure notice above, then `; the saved copy is not used: lazarobox-google-client.json: <reason>` |
| Fetched client valid, cache write refused | `lazarobox client not saved: lazarobox-google-client.json: <refusal>` |
| Screen on the lazarobox route (not a failure) | `client: lazarobox (fetched from www.javilazaro.es when you link)` |

From the `gsc` entry the footer prefix is `own client: f` instead of `own client: Esc, f on gsc` (the menu rewrites it, because Esc there would leave the entry). The cause kind (`not fetched`, `refused`, `not configured`) MUST start inside the first 58 columns of the footer.

#### Scenario: Each cause has a notice [auto]
- GIVEN a fake `curl` for each cause in the table
- WHEN Link starts with no cache
- THEN exactly one notice for that cause is shown and none holds the secret or the body

#### Scenario: Missing curl [auto]
- GIVEN no `curl` on `PATH`
- WHEN Link starts on the lazarobox route
- THEN the `not fetched` notice with the curl error line is shown and no other process is started

#### Scenario: The footer draws instruction and cause [auto]
- GIVEN each of the three failure notices
- WHEN the footer is drawn at the 62-column popup
- THEN the instruction and the cause kind (`lazarobox: not fetched (`, `lazarobox: refused (`, `lazarobox: not configured`) are both visible

### Requirement: Users already linked are untouched
lazarobox MUST NOT change the config, the secrets file, the token or the link state of a user linked with their own client. A user whose `[gsc]` already holds `GSC_OAUTH_CLIENT_SECRETS_FILE` keeps that value (the existing keep-what-is-there rule of Connect Google); the lazarobox route never overwrites it. Declared entries and values written before this change keep working byte for byte.

#### Scenario: Own-client user [auto]
- GIVEN `gsc` declared and `[gsc]` holding a path to the user's file, linked
- WHEN the app starts, the screens open and Link runs on the own-client route
- THEN no request is made, `secrets.toml` and `config.toml` are byte-identical, and the cache is not created

## Out of scope for this capability
GA4, Workspace, Gmail, Drive and any scope other than `webmasters.readonly`; the web pages on javilazaro.es; app publishing; `mcp-search-console` itself.
