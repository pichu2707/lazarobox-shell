# Proposal: macOS Seatbelt jail (codex and gemini on macOS, fail-closed)

## Intent

Issue #180: a macOS tester cannot use codex or gemini. Both are `JailPolicy::Required`, and the only jail backend is Landlock, so on macOS `launch()` returns `Refuse` (`JailOff::NotLinux`). Three small fixes already shipped (#260 macOS hint, #261 outdated-option message, #262 claude 2.1.259 minimum); they explain the refusal but do not remove it.

This change gives macOS a real, proven jail built on Seatbelt, so codex (and gemini, per OQ1) run on macOS under the SAME policy data as on Linux. It does not relax the rule that these CLIs never run bare.

## Users and situations

- macOS users (arm64 and Intel) who pick codex or gemini in the chat menu or for a task, and today see "needs Linux · use claude".
- The #180 tester, who has codex only: the first real-Mac validation.
- Linux users: no behavior change.

## Decisions (user, binding)

- **D1 Approach (a).** lazarobox renders its own SBPL profile from `JailSpec`. `__jail-exec` applies it via the absolute `/usr/bin/sandbox-exec -p <fixed profile> -D Pn=<path> -- prog argv`. Paths enter ONLY as `(param "Pn")`, never interpolated. FFI `sandbox_init` is out of scope.
- **D2 Probe.** A startup probe proves enforcement with a positive control (an allowed exec succeeds) and a negative control (a write to a denied dir fails). Any failure means `Unavailable`, and `Required` refuses.
- **D3 Network = Linux.** All outbound allowed; bind and inbound denied. No host allowlist, no proxy.
- **D4 claude on macOS stays bare** (`Layered`, text-only flags), never jailed, to protect its Keychain login. `launch()` gains a per-platform exemption.
- **D5 Read scope.** Broad system reads (/usr/lib, /System/Library, /private/etc, the Homebrew prefix, dyld needs). STRICT `process-exec` only on the `rx` set (interpreter, CLI package, shim). `$HOME` denied except the CLI's own state dir and the private tmp dir. No Mach-O `LC_LOAD_DYLIB` reader.

## Business rules

- Fail-closed: no proven enforcement, no codex/gemini process.
- `Required` is NEVER launched bare, on any platform, in any slice.
- Same policy data as Linux: one `JailSpec` (refusing $HOME, scratch dirs, `~/.ssh`, `CREDENTIAL_DIRS`); macOS only adds the system-read set of D5.
- codex/gemini on macOS stay refused until slice 4 flips them on; claude is never jailed on macOS in any slice.
- A sandbox apply failure (sandbox-exec exit 71) surfaces as a jail error, never as a CLI error or a silent bare run.

## Scope

### In Scope (four PRs, each ~400 lines or less, each to `main`, child opened only after the parent merges)

| # | Slice | Notes |
|---|---|---|
| 1 | Seam (`Backend::Seatbelt`), the probe, the helper path via `current_exe` (replaces `/proc/self/exe`), the `JailOff` rework (`NotLinux` becomes e.g. `NoSeatbelt` on macOS), the claude exemption in `launch()`, and a CI job proving sandbox-exec enforces on `macos-14` and `macos-15-intel` | FIRST: may invalidate the plan |
| 2 | Pure SBPL renderer with golden and injection tests; platform split of `jail_spec_with` | No runtime effect |
| 3 | macOS helper exec, a unique failure marker, `tests/jail_macos.rs` with fake CLIs | Still refused in the product |
| 4 | Enable codex (and gemini per OQ1), copy, CHANGELOG, README, manual checklist for the tester | The user-visible switch |

### Out of Scope (non-goals)

- Windows, BSD, Linux changes.
- Network proxy or host allowlist.
- Jailing claude on macOS.
- Write tools, opencode.
- FFI `sandbox_init_with_parameters`; a Mach-O dylib reader.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `chat-engines`: "CLI jail" gains the Seatbelt backend, the probe, the SBPL profile rules (params only, deny default, strict exec, network), the macOS claude exemption, and the exit-71 jail error.
- `ai-engine-detection`: availability and refusal copy per platform; model listing runs under Seatbelt on macOS.
- `chat-pane`: the "No jail" text on macOS.

## Approach

- `core::ai::jail` stays the policy owner: `JailSpec` → pure SBPL text + ordered param list (slice 2).
- `runtime::jail` gets a macOS `imp` that execs sandbox-exec; the Linux `imp` is untouched.
- The probe runs once at startup (off the render thread) and maps to `JailSupport`.
- Fake-CLI integration tests on macOS CI mirror `tests/jail.rs`.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/jail.rs`, `src/core/ai/jail/` | Modified (Backend, JailOff, launch, spec split) |
| `src/core/ai/jail/sbpl.rs` (or similar) | New (renderer) |
| `src/runtime/jail.rs` | Modified (macOS imp, probe) |
| `src/runtime/chat/cli.rs` | Modified (helper path) |
| `src/core/ai/chat/guidance.rs`, `src/ui/components/menu.rs` | Modified (copy) |
| `tests/jail_macos.rs` | New |
| `.github/workflows/ci.yml` | Modified (macOS jobs) |
| `README.md`, `CHANGELOG.md` | Modified (slice 4) |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| mach-lookup/sysctl allowlists unverified on a real Mac | High | Start from Codex and sandbox-runtime lists; tester checklist; `log stream` denials |
| DNS/TLS mach services (opendirectoryd.libinfo, dnssd, mDNSResponder socket, trustd, SecurityServer) missing, so requests fail | High | Explicit checklist step (a real codex turn); failure is a clear error, not a leak |
| codex npm native vendor binary not in `rx` | Med | Resolve the vendor path into `rx` in slice 2; fake-CLI test of a nested binary |
| Homebrew dylibs outside the read set | Med | Read the Homebrew prefix (both /opt/homebrew and /usr/local) |
| `sandbox-exec` is deprecated and may vanish | Low/Med | Probe fails → `Unavailable` → refuse; FFI route kept as later option |
| Nested sandbox: CI runners or a CLI already sandboxed cause apply failure | Med | Slice 1 CI gate; we never use the CLIs' own Seatbelt flags; exit 71 → jail error |
| gemini/codex OAuth needs the Keychain | Med | codex uses `~/.codex` files; gemini unknown on Mac, see OQ1 |
| Tests pinning `NotLinux` break | High | Update in slice 1 |

## Rollback Plan

Revert slice 4 alone: codex/gemini return to refused on macOS, Linux untouched. Slices 1-3 are inert in the product (macOS still refuses `Required`), so they can stay or be reverted independently. No config or data migration.

## Dependencies

- GitHub-hosted `macos-14` (arm64) and `macos-15-intel` runners able to run `/usr/bin/sandbox-exec` (proved in slice 1).
- The #180 tester for the codex checklist.

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` pass on Linux and both macOS runners for every slice.
- [ ] Slice 1 CI proves positive and negative controls on both runners.
- [ ] Golden tests pin the profile; injection tests show a path with `"`, `)` or newline cannot alter the profile.
- [ ] `tests/jail_macos.rs`: reads outside the spec, writes outside rw, exec outside rx, and bind all fail; outbound connect works.
- [ ] A failed probe or exit 71 refuses codex/gemini with a jail error; no bare run.
- [ ] claude on macOS runs bare and keeps its login.
- [ ] Tester checklist (codex, real Mac): codex appears as available, a chat turn and a task turn complete with network, a jailed read of `~/.ssh` is denied, and no Keychain or login prompt breaks.
- [ ] Linux behavior and tests unchanged.

## Proposal question round

Resolved: the user made D1-D5 and the slicing before this phase.

Open questions (RESOLVED by the user, binding):

- **OQ1 gemini on macOS: RESOLVED.** gemini ships ENABLED in slice 4, with the same profile and fake-CLI CI coverage as codex. The README says it is "not yet verified on a real Mac". A failure still refuses; it never runs bare.
- **OQ2 Minimum macOS version: RESOLVED.** macOS 14+ on arm64 and Intel is supported (what CI covers). Older versions are not blocked if the probe passes, but the README lists them as unsupported.
