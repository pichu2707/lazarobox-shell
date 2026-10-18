# Design: macOS Seatbelt jail (codex and gemini on macOS, fail-closed)

## Technical Approach

`core::ai::jail` stays the policy owner. On macOS the same `JailSpec` (CLI-specific grants) is rendered by a new pure module, `core::ai::jail::sbpl`, into a profile text plus an ordered `-D KEY=path` list. The profile text depends only on compile-time constants and on how many entries each class has. The `__jail-exec` helper (still dispatched first in `main`) parses `JailArgs` as it does today, renders the profile, and `exec`s `/usr/bin/sandbox-exec -p <text> -D … -- <program> <argv>`. The Linux `imp` is not touched. A startup probe that spawns `sandbox-exec` maps to `JailSupport`. One core constant, `SEATBELT_ENABLED`, keeps macOS refused until S4. claude is exempted through a per-(Cli, Os) policy function, so `launch()` stays CLI-agnostic.

## Architecture Decisions

| # | Topic | Options | Tradeoff | Decision |
|---|---|---|---|---|
| 1 | claude exemption | (a) `JailPolicy::BareOnMacos` plus `cfg!` inside `launch` · (b) `launch(policy, support, cli, os)` · (c) new `JailPolicy::Exempt` plus pure `jail_policy(cli, os)` | (a) puts the platform inside launch, so it cannot be tested on Linux. (b) makes the core CLI-aware at every call. (c) keeps launch policy-driven, with the platform knowledge in one pure, testable function | **(c)**. Adapters call `jail_policy(Cli::Claude, Os::CURRENT)`. `launch(Exempt, _) = Bare` |
| 2 | Gate until S4 | probe mapping · launch arm · `JailSupport::off()` across 4 call sites | The probe mapping is one choke point, and every existing `if let Unavailable(off)` site keeps working | **Probe mapping**. `SEATBELT_ENABLED=false` makes the macOS `probe()` return `NoSeatbelt(NotEnabled)` without spawning. A pure `seatbelt_support(enabled, probed)` is unit-tested on Linux |
| 3 | `JailSupport::Available{abi}` for Seatbelt | restructure to `Backend::Landlock{abi}` · `abi: 0` | Nothing reads `abi` today (grep), so restructuring is churn | `Available{backend: Seatbelt, abi: 0}`, documented as "no versioned ABI" |
| 4 | Where the macOS system grants live | in `JailSpec` (params) · SBPL constant block | They need `require-not` and `file-map-executable`, which `JailSpec` and Landlock cannot express. They are constants, not runtime data | **SBPL constant block**. D1 ("paths only as params") applies to runtime paths; constants are pinned by golden tests |
| 5 | rx/ro/rw path filters | stat for file vs dir (as Linux does) · `subpath` for every folder-or-file class | `subpath` on a file matches only that file, so the renderer stays pure | rx/ro/rw → `subpath`; ro_files/rw_files → `literal` |
| 6 | Metadata | per-ancestor literals · global `file-read-metadata` | Node's `realpathSync` lstats every ancestor. Landlock does not gate `stat` either | **Global `(allow file-read-metadata)`**: the same as Linux. No content and no directory listing |
| 7 | Unix sockets | all outbound (literal Linux parity) · IP-only plus named sockets | Global `network-outbound` reaches `docker.sock` and agent sockets | **IP `*:*` plus the mDNSResponder socket plus the `rw_files` literals (the shim socket)**. A deliberate tightening, see Risks |
| 8 | Exit-71 marker | helper fork+wait and inspect · re-exec the helper inside the sandbox to self-check · parent-side signature | fork+wait adds a relay process and stdio plumbing. A self-check needs lazarobox in rx. A parent signature costs no new code path | **Parent signature**: status 71 AND the FIRST stderr line starts `sandbox-exec: `. A CLI that forges it can only misclassify toward the safe side (a jail error), never toward a bare run |
| 9 | `import "system.sb"` | import · explicit minimum | The import grants `user-preference-read` and many mach services | Explicit minimum, trimmed with `log stream` on the tester's Mac |
| 10 | ELF readers on macOS | cfg them out · leave them | On Mach-O the ELF magic fails, so `read_loader`/`elf_search_dirs` return empty | Leave them. The SBPL system block covers dyld, the cryptex cache and Homebrew dylibs |

## Types (S1)

```rust
pub enum Backend { Landlock, Seatbelt }
pub enum SeatbeltFault { Missing, Refused, Leaked, Timeout, Setup, NoHelper, NotEnabled }
pub enum JailOff { NoLandlock(u32), NoSeatbelt(SeatbeltFault), NotLinux /* S4: NoBackend */, NotProbed }
pub enum JailPolicy { Required, Layered, Exempt }
pub enum Os { Linux, Macos, Other }      // Os::CURRENT via cfg!
pub fn jail_policy(cli: Cli, os: Os) -> JailPolicy; // (Claude,Macos)=>Exempt, (Claude,_)=>Layered, _=>Required
```

Texts:
- `NoSeatbelt(f).text()` = "the macOS jail is unavailable: {fault}". The faults read: "/usr/bin/sandbox-exec is missing", "sandbox-exec refused a test program", "the sandbox did not block a test action", "the sandbox check timed out", "the sandbox check could not prepare its folder", "this program's path cannot be resolved", "it is not enabled in this version".
- `short()` = "jail unavailable".
- `short_required()` = "jail unavailable · use claude".
- `text_required()` appends "; on this system use claude or Ollama for chat".
- All seven texts join `known_helper_reason`.
- S4 renames `NotLinux` to `NoBackend`, with the text "the CLI jail needs Linux or macOS".

The Exempt notice in `plan()` reads: "claude runs without the jail here: claude keeps its macOS Keychain login, so it never runs jailed on macOS". The `jail_note` short is "not jailed on macOS". Exempt means `runs_jailed=false`, so claude gets no MCP tools on macOS. That is the same as today, where claude falls back to bare.

## Spec platform split (S2)

`jail_spec_with` = `jail_spec_for(Os::CURRENT, …)`, and the tests call `jail_spec_for(Os::Macos, …)` on Linux.

- **Linux only:** ro `/usr/lib /usr/share /lib /lib64 /etc /proc/self /sys/fs/cgroup`, and ro_files `/proc/{cpuinfo,meminfo,stat}`.
- **Shared:** ro_files `/dev/urandom /dev/random`, and rw_files `/dev/null`.
- **macOS:** nothing more in the spec.
- **`BROAD`** gains the macOS folders that must never be a package root: `/System /Library /Applications /Users /Volumes /private /private/var /private/etc /opt/homebrew /opt/homebrew/{bin,sbin,lib,Cellar,opt} /usr/local/{Cellar,opt}`. Without them, a binary sitting directly in `/opt/homebrew/bin` would grant exec on all of Homebrew, curl included.

## SBPL renderer (S2): `src/core/ai/jail/sbpl.rs`

```rust
pub struct Profile { pub text: String, pub params: Vec<(String, PathBuf)> }
pub enum SbplError { Nul(PathBuf), Relative(PathBuf), TooLong(PathBuf), TooMany(usize) }
pub fn render(spec: &JailSpec) -> Result<Profile, SbplError>;
pub fn sandbox_args(p: &Profile) -> Vec<OsString>; // "-p", text, ("-D", "KEY=path")*
```

Keys are `RX_n RO_n RW_n ROF_n RWF_n`, using only `[A-Z0-9_]`. Each key and value is its own argv element.

Injection safety:
- The value never passes through the SBPL parser: `sandbox_init_with_parameters` binds it. So `"`, `)` and newline are inert.
- NUL is rejected explicitly.
- A path above 1024 bytes (`PATH_MAX`) is rejected, and so is a spec with more than 256 params. The worst case is then about 30 KB of profile, far below `ARG_MAX` (1 MiB).
- `=` inside a value relies on sandbox-exec splitting at the first `=`. S3b proves this; if it fails, `render` rejects `=` and the run fails closed.

The skeleton, in this order:

```scheme
(version 1)
(deny default)
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target self))
(allow file-read-metadata)
(allow file-read* file-map-executable
  (subpath "/usr/lib") (subpath "/System/Library")
  (subpath "/System/Volumes/Preboot/Cryptexes")
  (require-all (subpath "/opt/homebrew") (require-not (subpath "/opt/homebrew/var")))
  (require-all (subpath "/usr/local") (require-not (subpath "/usr/local/var"))))
(allow file-read* (subpath "/usr/share") (subpath "/private/etc") (subpath "/private/var/db/timezone"))
;; one line per entry, in spec order:
(allow file-read* file-map-executable process-exec (subpath (param "RX_0")))
(allow file-read* file-map-executable (subpath (param "RO_0")))
(allow file-read* file-write* (subpath (param "RW_0")))
(allow file-read* (literal (param "ROF_0")))
(allow file-read* file-write* (literal (param "RWF_0")))
(allow network-outbound (literal (param "RWF_0")))
(allow network-outbound (remote ip "*:*"))
(allow network-outbound (literal "/private/var/run/mDNSResponder"))
(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.notification_center") (global-name "com.apple.logd")
  (global-name "com.apple.dnssd.service") (global-name "com.apple.trustd.agent")
  (global-name "com.apple.SystemConfiguration.configd")
  (global-name "com.apple.SystemConfiguration.DNSConfiguration"))
(allow ipc-posix-shm-read-data (ipc-posix-name "apple.shm.notification_center"))
(allow sysctl-read (sysctl-name-prefix "hw.optional.") (sysctl-name-prefix "hw.perflevel")
  (sysctl-name-prefix "machdep.cpu.") (sysctl-name "hw.activecpu") (sysctl-name "hw.ncpu")
  (sysctl-name "hw.logicalcpu") (sysctl-name "hw.logicalcpu_max") (sysctl-name "hw.physicalcpu")
  (sysctl-name "hw.physicalcpu_max") (sysctl-name "hw.memsize") (sysctl-name "hw.pagesize")
  (sysctl-name "hw.pagesize_compat") (sysctl-name "hw.cachelinesize") (sysctl-name "hw.byteorder")
  (sysctl-name "hw.cputype") (sysctl-name "hw.cpusubtype") (sysctl-name "hw.cpufamily")
  (sysctl-name "hw.machine") (sysctl-name "hw.model") (sysctl-name "hw.nperflevels")
  (sysctl-name "kern.ostype") (sysctl-name "kern.osrelease") (sysctl-name "kern.osversion")
  (sysctl-name "kern.osproductversion") (sysctl-name "kern.version") (sysctl-name "kern.hostname")
  (sysctl-name "kern.boottime") (sysctl-name "kern.argmax") (sysctl-name "kern.maxfilesperproc")
  (sysctl-name "kern.usrstack64") (sysctl-name "kern.secure_kernel") (sysctl-name "vm.loadavg")
  (sysctl-name "sysctl.proc_cputype") (sysctl-name "sysctl.proc_translated")
  (sysctl-name "security.mac.lockdown_mode_state"))
```

Why each system entry is there:

| Entry | Why |
|---|---|
| `/usr/lib` | dyld, libSystem, libc++ |
| `/System/Library` | frameworks (CoreFoundation, Security), ICU |
| `Preboot/Cryptexes` | the dyld shared cache since macOS 13; without it nothing starts |
| `/private/etc` | hosts, resolv.conf, `ssl/cert.pem`, localtime (the `/etc` equivalent on Linux) |
| `/private/var/db/timezone` | the localtime target |
| `/usr/share` | zoneinfo, ICU, terminfo |
| Homebrew prefixes, minus `var/` | node, dylibs, the openssl config. `var/` holds service data (databases, logs) |

Rejected entries:
- `/Library/Preferences`: system network preferences.
- `/dev/dtracehelper`: its denial is harmless noise.

The mach services cover libinfo (getpwuid, getaddrinfo), notify, the log, DNS (dnssd plus DNSConfiguration plus the mDNSResponder socket), TLS trust (trustd), and configd. configd is there because reqwest's system-proxy lookup is known to panic when `SCDynamicStore` is unavailable.

Deliberately NOT granted:
- `com.apple.SecurityServer`: the Keychain door.
- `cfprefsd`/`user-preference-read`.
- `kern.proc.*`, `net.*`, `iokit-open`.

The sysctl entries cover CPU, memory and feature probes from V8, libuv, OpenSSL and Rust std. Process and network enumeration are excluded.

## Probe (S1; S3a upgrades it)

- **What it runs:** `/usr/bin/sandbox-exec`, by absolute path. It is checked first with `is_file`; if it is missing → `Missing`. The children get `env_clear()` plus `PATH=/usr/bin:/bin`, and all stdio goes to null.
- **Probe folder:** `<temp_dir>/lazarobox-probe-<pid>-<nanos>`, created with `DirBuilder` mode 0700, with `rw/` and `denied/` inside, then canonicalized (Seatbelt matches `/private/var/folders/...`). A Drop guard runs `remove_dir_all`. If the folder cannot be created → `Setup`.
- **S1 profile** (a constant):
  ```scheme
  (version 1)(deny default)(allow file-read*)(allow file-map-executable)(allow sysctl-read)(allow process-exec (literal (param "EXE")))(allow file-write* (subpath (param "RW")))
  ```
- **Controls**, each run as `-D EXE=/usr/bin/touch -D RW=<dir>/rw -- …`:
  1. Positive: `/usr/bin/touch <dir>/rw/ok` must exit 0 and the file must exist; otherwise `Refused`.
  2. Negative (write): `/usr/bin/touch <dir>/denied/no` must exit non-zero and the file must be absent; otherwise `Leaked`.
  3. Negative (exec): `/bin/echo x` must not exit 0 and must print nothing; otherwise `Leaked`.
- **Timeout:** 5 s per child. The probe polls `try_wait` every 10 ms; on timeout it kills the child and reports `Timeout`.
- **S3a upgrade:** the probe renders the profile with the real `sbpl::render` from a minimal spec (rx touch, rw `<dir>/rw`). A macOS release that rejects one of our ops then gives `Unavailable` at startup instead of failing turn after turn.
- **Where it runs:** `runtime::run` starts `spawn_blocking(jail::probe)` first and awaits it where `jail::probe()` is called today, before the render loop exists. It runs once per process. Nothing is cached on disk, because an OS update must re-probe.

## Helper exec on macOS (S3a)

1. `parse_args`. `--min-abi` is answered with NotEnforced ("Linux only").
2. If `/usr/bin/sandbox-exec` is missing → the NotEnforced marker and exit 125.
3. `render` → on an error, NotEnforced with a fixed text.
4. `Command::new(SANDBOX_EXEC).args(sandbox_args).arg("--").arg(program).args(argv).exec()`. On error → `Failure::Exec` (exit 127).

The environment is inherited untouched, exactly as on Linux: cli.rs already scrubs it and sets `TMPDIR` and `cwd`. SIP strips `DYLD_*` at the platform binary, which is a bonus.

`exit_error` gains a rule: code 71 plus a first stderr line starting `sandbox-exec: ` → `ChatError::Jail("the macOS sandbox could not be applied")`. The stderr is never echoed. The codes for a compile error and a denied exec are pinned by S3b; if either is not 71, the code is added. On non-Linux, `CliEnv.helper` = `current_exe()` canonicalized; if it fails → `jail = Unavailable(NoSeatbelt(NoHelper))`.

On codex's vendor binary: a global npm install keeps `@openai/codex-darwin-*/vendor/…` under `@openai/codex/node_modules`, which is inside `package_root`, which is `rx` `subpath`. The Homebrew cask's root is its `Caskroom/codex/<v>` folder. A pnpm or bun layout that hoists the binary outside the root gets an exec denial, which is an error, never a leak.

## Data Flow

```
startup ─ spawn_blocking(probe) ─→ JailSupport{Seatbelt|NoSeatbelt}
turn: plan(jail_policy, support) ─→ Exempt→bare | Required+off→refuse
   └→ jail_plan → JailSpec → JailArgs.render → helper(current_exe)
        helper: parse → sbpl::render → exec /usr/bin/sandbox-exec -p … -D … -- prog
   exit: 125+marker | 71+"sandbox-exec: " ─→ exit_error → ChatError::Jail
```

## File Changes and Slicing

| Slice | Files | Est. lines |
|---|---|---|
| **1a** probe and CI (may invalidate the plan) | `src/core/ai/jail.rs` (Backend, SeatbeltFault, NoSeatbelt, known reasons, `SEATBELT_ENABLED`, `seatbelt_support`); `src/core/ai/jail/tests.rs`; `src/runtime/jail.rs` (macOS `imp` with `seatbelt_probe`, other-OS `imp`); `src/runtime.rs` (spawn_blocking); `tests/jail_macos.rs` (probe test); `.github/workflows/ci.yml` | ~230 |
| **1b** exemption and helper path | `jail.rs` (Os, Exempt, `jail_policy`, launch arm); `adapters.rs` and its tests; `runtime/chat/cli.rs` (helper, Exempt reason, `jail_note`) and `cli/tests.rs` | ~190 |
| **2a** spec split | `jail.rs` (`jail_spec_for`, BROAD), `jail/tests.rs` | ~150 |
| **2b** renderer | `src/core/ai/jail/sbpl.rs`, `src/core/ai/jail/sbpl/tests.rs` (golden plus injection) | ~330 |
| **3a** exec and marker | `runtime/jail.rs` (macOS exec, probe through the renderer), `jail.rs` (`exit_error` rule) and tests | ~170 |
| **3b** macOS integration | `tests/jail_macos.rs` | ~280 |
| **4** enable | remove the gate; `NotLinux`→`NoBackend`; `guidance.rs`, `menu.rs`, `engine.rs`, `availability_tests.rs`, `cli/tests.rs`; `README.md` (macOS 14+, gemini "not yet verified on a real Mac", tester checklist); `CHANGELOG.md` | ~200 |

## Testing Strategy

| Slice | Test |
|---|---|
| 1a | Linux unit tests: fault texts, `seatbelt_support`, `known_helper_reason`. CI: on macOS runners, a raw shell "Seatbelt smoke" step (the three controls with the constant profile), and `LAZAROBOX_REQUIRE_JAIL=1`, so the `seatbelt_probe()` test fails instead of skipping on `macos-14` and `macos-15-intel` |
| 1b | `jail_policy` table, `launch(Exempt,*)`, the Exempt plan notice, `jail_note`, the helper path per OS |
| 2a/2b | `jail_spec_for(Macos)` has no Linux entries; the BROAD refusals. Golden: the whole profile for a representative spec. Injection: paths with `"`, `)`, `\n`, `;` leave the text byte-identical and appear only in params. NUL, relative, too-long and too-many paths are rejected. Keys match `^[A-Z]+_[0-9]+$` |
| 3a | The `exit_error` 71 rule: wrong code → None; marker not on the first line → None; reason fixed |
| 3b (`#![cfg(target_os="macos")]`) | Fake CLIs are `/bin/bash` scripts that use builtins, so no extra exec grant is needed: a `$(<secret)` read outside is denied; a write outside is denied and a write inside rw works; `/usr/bin/curl` exec is denied; `/dev/tcp/127.0.0.1/<port>` to a listener in the test reaches it, while `nc -l` (with nc in rx) cannot bind; a bad profile through raw sandbox-exec never runs touch and maps to Jail; a nested sandbox maps to Jail; a path with `= " )` and newline is granted exactly; a nested vendor script inside the root execs; the environment passes through; the runner's node (skipped if absent) starts under the profile |

## Migration / Rollout

No data migration. S1 through S3 are inert in the product because of the gate. Rollback: revert S4, which restores the gate; Linux is untouched in every slice.

## Risks and Open Questions (retired by)

- [ ] Whether runners run sandbox-exec, and whether the `--` / `-D` argv shape works: **1a CI**.
- [ ] Seatbelt symlink resolution (Homebrew `opt` links, `/etc`): **3b** plus the tester.
- [ ] `=` in a `-D` value, and the exit codes for a compile error or a denied exec: **3b**.
- [ ] Mach/sysctl completeness (DNS, TLS, configd): **3b** node test, then the S4 tester with `log stream --predicate 'sender=="Sandbox"'`.
- [ ] codex/gemini Keychain use: the tester; the gemini README note.
- [ ] The unix-socket tightening (decision 7) and the Homebrew `var/` exclusion are refinements of D3/D5 and need user confirmation.
- [ ] A `current_exe` path that vanishes after `brew upgrade` plus cleanup: an error, not a leak; accepted.
- [ ] The MSRV job checks Linux only. Keep the macOS code within 1.88; optionally add `cargo check --target aarch64-apple-darwin` to the msrv job.
- [ ] Spec scenario "Available probe, S1-S3 → refused": it is met by the gate inside the probe mapping. An injected `Available{Seatbelt}` in a test would reach the helper and fail closed with a jail error.
