# Exploration: macOS Seatbelt jail for codex and gemini

Source: engram `sdd/macos-seatbelt-jail/explore` (observation 3270), copied verbatim in content.

## Recommendation

Option (a). Generate our own SBPL profile from the existing `JailSpec`. The `__jail-exec` helper applies it by exec'ing the ABSOLUTE `/usr/bin/sandbox-exec -p <fixed profile> -D P0=<path>... -- prog argv` (A1). Paths go in only as `(param "Pn")`, never interpolated (Codex pattern). A startup probe proves enforcement: a positive control (`/usr/bin/true` under allow) plus a negative one (`touch` in a dir denied by the profile must fail). Otherwise the backend is Unavailable and `Required` stays fail-closed. The FFI `sandbox_init_with_parameters` (A2) is to evaluate later.

### Rejected

- (b) The CLIs' own sandbox flags: looser, cannot be proven, silent bare runs.
- (c) Hybrid: a nested sandbox-exec fails.

## Current model (Linux)

- `jail.rs`: `Backend{Landlock}`, `JailOff{NoLandlock,NotLinux,NotProbed}`, `JailPolicy{Required(codex,gemini),Layered(claude)}`.
- `launch()` is CLI-agnostic.
- `JailSpec`: rx/ro/rw/ro_files/rw_files, built by `jail_spec_with` (L693). It refuses $HOME, the scratch dirs, ~/.ssh and the CREDENTIAL_DIRS.
- `runtime/jail.rs`: Landlock enforce. Network: outbound connect is open to all hosts, bind is denied.
- Non-portable to macOS: `CliEnv.helper=/proc/self/exe` (cli.rs:143) needs `current_exe`; ELF `PT_INTERP`/`elf_search_dirs`/`rpath.rs` need a Mach-O `LC_LOAD_DYLIB`/`LC_RPATH` reader or a coarser read grant; and /proc, /sys, /lib64 are Linux-only.

## Profile shape

- `(deny default)`.
- process-exec only on rx; exec is a separate op from read in Seatbelt, so it is stronger than Landlock.
- Reads: ro, rw, and the system paths for dyld/node (/usr/lib, /System/Library, /private/etc, the Homebrew prefix).
- Writes: rw, rw_files, /dev/null.
- `(allow network-outbound)`, deny bind and inbound.
- mach-lookup and sysctl-read allowlists from anthropic sandbox-runtime and Codex, trimmed with `log stream`. DNS/TLS candidates: opendirectoryd.libinfo, dnssd.service, the /private/var/run/mDNSResponder socket, trustd.agent, SecurityServer.
- signal limited to `(target same-sandbox)`.
- sandbox-exec exits 71 when apply fails, so it fails closed but with no unique marker.

## Slicing (about 900-1300 lines)

1. Seam plus probe plus `current_exe` plus the JailOff rework (`NoSeatbelt`) plus a CI check that sandbox-exec works on macos-14/15-intel (about 300 lines). DO FIRST: if the runners cannot run sandbox-exec, the plan changes.
2. The pure SBPL renderer with golden and injection tests, plus the platform split of `jail_spec_with` (about 400 lines).
3. macOS `imp::exec`, the failure marker, `tests/jail_macos.rs` with fake CLIs, and the Mach-O reader or a coarse grant (about 400 lines; 3b if larger).
4. Enable codex and gemini, the claude decision, copy, CHANGELOG, and a manual tester checklist (about 200 lines).

## Risks

- Nothing has been verified on a real Mac: the allowlists, DNS/TLS, the npm codex native vendor binary, Homebrew dylibs.
- claude Keychain login breaks if claude is jailed.
- gemini/codex OAuth may use the Keychain.
- Tests that pin NotLinux need updating.

## Open questions

Network all-outbound like Linux; whether claude is jailed on macOS (Keychain); read scope; minimum macOS version; behavior if sandbox-exec is removed; whether a Mac tester is available.

## Sources

- github.com/openai/codex/tree/main/codex-rs/sandboxing/src (seatbelt.rs, seatbelt_base_policy.sbpl)
- gemini-cli docs/cli/sandbox.md and sandbox-macos-*.sb
- github.com/anthropic-experimental/sandbox-runtime src/sandbox/macos-sandbox-utils.ts
- gastown.dev/docs/research/macos-sandbox-exec
