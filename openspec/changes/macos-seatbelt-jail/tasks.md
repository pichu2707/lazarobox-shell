# Tasks: macOS Seatbelt jail (codex and gemini on macOS, fail-closed)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1550 total, 150-330 per PR |
| 400-line budget risk | Medium (no slice above 330; 2b and 3b closest) |
| Chained PRs recommended | Yes |
| Suggested split | 1a -> 1b -> 2a -> 2b -> 3a -> 3b -> 4 |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main |

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Medium

### Suggested Work Units

| Unit | Goal | PR | Est. | Risk |
|------|------|----|------|------|
| 1a | Types, probe, gate, CI smoke | PR 1 (main) | ~230 | Low |
| 1b | claude exemption, helper path | PR 2 (main, after PR 1 merges) | ~190 | Low |
| 2a | Spec split `jail_spec_for` | PR 3 | ~150 | Low |
| 2b | `sbpl.rs` renderer + golden/injection | PR 4 | ~330 | WATCH: goldens inflate; if >400 move injection tests to 2b-bis |
| 3a | macOS exec, exit-71 | PR 5 | ~170 | Low |
| 3b | `tests/jail_macos.rs` | PR 6 | ~280 | WATCH: 10 scenarios; trim to the must-have ones if >400 |
| 4 | Enable codex+gemini, docs | PR 7 | ~200 | Low |

Each child PR is opened only after its parent merges, always against main. Slices 1-3 are inert (gate off); rollback = revert slice 4.

## Confirmed decisions (user, binding)

- Network: outbound to any IP, mDNSResponder socket and the shim socket (`rw_files`) allowed. Every other unix socket denied (docker.sock, ssh/gpg agents). Inbound and bind denied.
- Homebrew: `/opt/homebrew` and `/usr/local` readable except their `var/`. Read only, never exec beyond `rx`.
- Support macOS 14+; gemini enabled but README says not verified on a real Mac.

Gate for every slice (Linux locally and CI on all 3 runners): `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`. Strict TDD: each RED task is committed failing-first, then GREEN. Files below are under `src/core/ai/` unless stated.

## Slice 1a: probe + CI (PR 1)

Files: `jail.rs`, `jail/tests.rs`, runtime entry (`runtime::run`), `.github/workflows/ci.yml`, `CHANGELOG.md`.
- [x] 1a.1 RED: tests for `seatbelt_support(enabled, probed)` (disabled gives `NoSeatbelt(NotEnabled)`, Missing/Refused/Leaked/Timeout/Setup map to faults) and texts "the macOS jail is unavailable: {fault}", "jail unavailable".
- [x] 1a.2 GREEN: add `Backend::Seatbelt`, `SeatbeltFault`, `JailOff::NoSeatbelt`, `seatbelt_support`, `SEATBELT_ENABLED=false`; texts.
- [x] 1a.3 RED: probe tests (cfg macOS): control ok, negative control denied, exec-denied `/bin/echo`, missing binary, 5 s timeout, temp dir cleanup.
- [x] 1a.4 GREEN: probe in `jail.rs` (absolute `/usr/bin/sandbox-exec`, env_clear, 0700 dir with Drop guard, constant S1 profile, `-D` and `--`); run via `spawn_blocking` in `runtime::run`; gate returns NotEnabled before spawning.
- [x] 1a.5 CI: `ci.yml` step on macos-14 and macos-15-intel runs the probe test with `LAZAROBOX_REQUIRE_JAIL=1` and a raw `/usr/bin/sandbox-exec -p ... -D K=V -- /usr/bin/true` smoke (positive and negative control).
- [x] 1a.6 STOP CHECKPOINT (MANUAL REVIEW): if the CI smoke fails on either runner (capability or argv shape), STOP and report to the orchestrator before 1b. Passed in PR #265 CI run 39162360922 on macOS 14.8.9 arm64 and 15.7.9 Intel.
- [x] 1a.7 CHANGELOG entry (Unreleased: internal probe, no behavior change).
Done: gates green on 3 runners, smoke proves sandbox-exec and `-D`/`--` shape, behavior unchanged.

## Slice 1b: claude exemption + helper path (PR 2)

Files: `jail.rs`, `jail/tests.rs`, adapters and the `CliEnv` site (grep `CliEnv`), `CHANGELOG.md`.
- [x] 1b.1 RED: `jail_policy(cli, os)` table (Claude+Macos Exempt, Claude+other Layered, rest Required); `launch(Exempt,_)=Bare`; Exempt notice and `jail_note` "not jailed on macOS"; claude text-only flags.
- [x] 1b.2 GREEN: `Os`, `JailPolicy::Exempt`, `jail_policy`, `launch`, notice copy.
- [x] 1b.3 RED: `CliEnv.helper` = canonicalized `current_exe` on non-Linux; failure gives `NoSeatbelt(NoHelper)`; Linux unchanged.
- [x] 1b.4 GREEN: wire helper path; CHANGELOG.
Done: claude bare on macOS regardless of probe, Linux tests untouched, gates green.

## Slice 2a: spec split (PR 3)

Files: `jail.rs` (or spec module), `jail/tests.rs`, `CHANGELOG.md`.
- [x] 2a.1 RED: `jail_spec_for(Os::Linux, ..)` equals current spec byte-for-byte; macOS spec has no Linux-only paths; shared `/dev/urandom`, `/dev/random`, `/dev/null`; BROAD gains /System /Library /Applications /Users /Volumes /private /private/var /private/etc /opt/homebrew(+bin,sbin,lib,Cellar,opt) /usr/local/{Cellar,opt}.
- [x] 2a.2 GREEN: `jail_spec_with = jail_spec_for(Os::CURRENT, ..)`; split constants; CHANGELOG.
Done: Linux spec unchanged (regression test), gates green.

## Slice 2b: renderer (PR 4)

Files: `jail/sbpl.rs` (new), `jail/sbpl/tests.rs` (new), `jail.rs` (mod), `CHANGELOG.md`.
- [x] 2b.1 RED: golden test of full profile text for fixed `JailSpec`; keys RX_n RO_n RW_n ROF_n RWF_n; ordered params.
- [x] 2b.2 RED: injection tests (quote, paren, semicolon, newline leave text byte-identical); errors Nul, Relative, TooLong >1024, TooMany >256.
- [x] 2b.3 RED: Homebrew `var` excluded via `require-not`; network block (remote ip `*:*`, mDNSResponder, `network-outbound` on `rw_files` literals only); no `import "system.sb"`; no bind or inbound.
- [x] 2b.4 GREEN: `render`, `sandbox_args`, `Profile`, `SbplError`, system/mach/sysctl blocks per design; CHANGELOG.
Done: pure, Linux-testable, goldens pinned, gates green. Flag: over 400 lines, split.

## Slice 3a: macOS exec + exit-71 (PR 5)

Files: `jail.rs`, helper exec module (`__jail-exec`), `jail/tests.rs`, `CHANGELOG.md`.
- [ ] 3a.1 RED: `exit_error`: status 71 plus first stderr line starting `sandbox-exec: ` gives `ChatError::Jail("the macOS sandbox could not be applied")`; stderr never echoed; forged marker only misclassifies safe.
- [ ] 3a.2 GREEN: exit-71 rule; `parse_args` (`--min-abi` gives NotEnforced), missing sandbox-exec gives NotEnforced 125.
- [ ] 3a.3 RED/GREEN: helper renders and execs `sandbox-exec -p <text> -D ... -- prog argv`, env untouched; probe now uses the real renderer.
- [ ] 3a.4 CHANGELOG.
Done: compiles on all runners, Linux imp untouched, gate still off, gates green.

## Slice 3b: macOS integration tests (PR 6)

Files: `tests/jail_macos.rs` (cfg macOS), `CHANGELOG.md`.
- [ ] 3b.1 RED/GREEN: bash-builtin fake CLIs: read allow, write deny outside rw, exec deny beyond rx, `/dev/tcp` connect ok, `nc` bind denied, other unix socket denied.
- [ ] 3b.2 RED/GREEN: bad profile (pin compile-error and denied-exec codes), nested sandbox, special-character path, `=` in values (reject if it fails), nested vendor exec, env passthrough, node start.
- [ ] 3b.4 scenario: an npm-style shim with #!/usr/bin/env node shebang starts under the profile (process-exec on /usr/bin/env, or put it in rx; check whether process-exec-interpreter is needed).
- [ ] 3b.3 Fixes found by CI go back to `sbpl.rs`; CHANGELOG.
Done: green on macos-14 and macos-15-intel with `LAZAROBOX_REQUIRE_JAIL=1`.

## Slice 4: enable + docs (PR 7)

Files: `jail.rs`, adapters, `jail/tests.rs`, `README.md`, `docs/` (checklist), `CHANGELOG.md`.
- [ ] 4.0 macOS home deny-list: add ~/Library, ~/Library/Keychains, ~/Library/Application Support (and other macOS credential/shared dirs) to CREDENTIAL_DIRS/SHARED_IN_HOME for Os::Macos before the gate flips (found in 2a review).
- [ ] 4.0b spec layer: assert spec.rw and spec.rx are disjoint in jail_spec_for (Seatbelt rules union, so overlap means write+exec), with a test.
- [ ] 4.1 RED: with Available probe, codex and gemini jailed on macOS; failure refuses; `NotLinux` becomes `NoBackend` "needs Linux or macOS"; codex model listing jailed, keeps `default`; chat-pane guidance and exit 71 jail error.
- [ ] 4.2 GREEN: `SEATBELT_ENABLED=true`, copy per spec (ai-engine-detection, chat-pane).
- [ ] 4.2b When the gate flips, bound the probe: the worst case is 3 x 5 s at startup. Give it one total deadline, or run it lazily on the first codex or gemini use.
- [ ] 4.3 Docs: README macOS 14+ support, gemini "not yet verified on a real Mac", claude never jailed on macOS; state that the jailed CLI can reach loopback, the LAN and the metadata IP (accepted risk, see design).
- [ ] 4.4 MANUAL (#180 tester, codex only) checklist doc: `which codex node`; how codex was installed (npm, brew cask); run a codex turn; run a read-tool turn; what a refusal looks like; collect `log stream --predicate 'sender == "Sandbox"'` output during the turn.
- [ ] 4.5 CHANGELOG (Added: macOS jail, codex/gemini on macOS).
- [ ] 4.6 MANUAL: send checklist to the #180 tester and record the result.
Done: gates green, checklist and README merged; tester feedback is post-merge.
