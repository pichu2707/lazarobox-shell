//! The OS side of the CLI jail: the startup probe and the `__jail-exec`
//! helper. The rules themselves are built by the pure `core::ai::jail`.
//!
//! The helper is a plain single-threaded process image (`main` dispatches it
//! before the tokio runtime exists), so building the ruleset may allocate
//! freely, unlike a `pre_exec` hook after `fork` in the multi-threaded app.

use std::ffi::OsString;

use std::path::{Path, PathBuf};

use crate::core::ai::jail::{JailSupport, NOT_ENFORCED_EXIT, NOT_ENFORCED_MARKER};

#[cfg(any(target_os = "macos", test))]
mod seatbelt;
#[cfg(target_os = "macos")]
pub use seatbelt::Diag;

/// Exit status of a program the helper could not execute (shell convention).
const EXEC_FAILED: i32 = 127;

/// One raw `landlock_create_ruleset(NULL, 0, VERSION)` call: whether this
/// system can jail a CLI, and with which ABI.
pub fn probe() -> JailSupport {
    imp::probe()
}

/// The macOS probe on its own, without the [`SEATBELT_ENABLED`] gate: whether
/// `/usr/bin/sandbox-exec` confines a program. CI uses it to prove the
/// binary works on a runner before the jail is turned on.
///
/// [`SEATBELT_ENABLED`]: crate::core::ai::jail::SEATBELT_ENABLED
#[cfg(target_os = "macos")]
pub fn seatbelt_probe() -> Result<(), crate::core::ai::jail::SeatbeltFault> {
    seatbelt::probe()
}

/// [`seatbelt_probe`] and, on failure, which control failed and what
/// `sandbox-exec` said first. For test and CI output only.
#[cfg(target_os = "macos")]
pub fn seatbelt_probe_diagnosed() -> Result<(), (crate::core::ai::jail::SeatbeltFault, Diag)> {
    seatbelt::probe_diagnosed()
}

/// The canonicalizer `jail_spec_with` needs: the real target of a path, or
/// `None` when it does not exist (or loops). Reads no file content.
pub fn resolve(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Runs the helper with the arguments that follow `__jail-exec`. On success
/// the process image is replaced and this never returns; it returns the exit
/// status only when the program did not run.
pub fn exec_main(args: &[OsString]) -> i32 {
    match imp::exec(args) {
        Ok(never) => match never {},
        Err(Failure::NotEnforced(why)) => {
            eprintln!("{NOT_ENFORCED_MARKER}: {why}");
            NOT_ENFORCED_EXIT
        }
        Err(Failure::Exec(program, error)) => {
            eprintln!("could not run {program}: {error}");
            EXEC_FAILED
        }
    }
}

/// Why the helper did not run the program.
#[derive(Debug)]
enum Failure {
    NotEnforced(String),
    /// The jail is in place but the program could not be executed. Only the
    /// Linux jail builds it: on macOS a failed exec of `sandbox-exec` means no
    /// jail, so it is a `NotEnforced`.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Exec(String, std::io::Error),
}

#[cfg(target_os = "linux")]
mod imp {
    use std::convert::Infallible;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use landlock::{
        ABI, Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd,
        Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope,
    };

    use super::Failure;
    use crate::core::ai::jail::JailSupport;
    use crate::core::ai::jail::{
        JailArgs, JailSpec, Limit, MIN_ABI, NetBind, Os, RLIMIT_REASON, RWX_REASON, parse_args,
        support_from_abi,
    };

    /// `landlock_create_ruleset` has the same number on every architecture.
    const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    const CREATE_RULESET_VERSION: libc::c_ulong = 1;
    /// The first ABI with `Scope` (abstract unix sockets and signals).
    const SCOPE_ABI: u32 = 6;
    /// The first ABI with TCP rules (`AccessNet`).
    const NET_ABI: u32 = 4;

    pub fn probe() -> JailSupport {
        support_from_abi(kernel_abi())
    }

    /// 0 when the kernel has no Landlock (ENOSYS, EOPNOTSUPP) or it is off.
    fn kernel_abi() -> u32 {
        // SAFETY: with a null attribute, size 0 and the VERSION flag the call
        // only returns the ABI number; it reads and writes no memory.
        let abi = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                std::ptr::null::<libc::c_void>(),
                0 as libc::size_t,
                CREATE_RULESET_VERSION,
            )
        };
        u32::try_from(abi).unwrap_or(0)
    }

    pub fn exec(args: &[OsString]) -> Result<Infallible, Failure> {
        let job = parse_args(args).map_err(|e| Failure::NotEnforced(e.0.to_owned()))?;
        let abi = kernel_abi();
        if let Some(min) = job.min_abi.filter(|min| abi < *min) {
            return Err(Failure::NotEnforced(format!(
                "ABI {abi} is below the required {min}"
            )));
        }
        if let JailSupport::Unavailable(off) = support_from_abi(abi) {
            return Err(Failure::NotEnforced(off.text()));
        }
        check_rwx(&job.spec).map_err(|_| Failure::NotEnforced(RWX_REASON.to_owned()))?;
        set_limits(&job.spec).map_err(Failure::NotEnforced)?;
        enforce(&job.spec, abi).map_err(Failure::NotEnforced)?;
        launch(&job)
    }

    /// The `rwx` root must pass `check_exec` (with `$HOME` in both of its
    /// spellings, so a broad root is refused) and be its own canonical path:
    /// a root that is a link would grant its target, not what was checked.
    fn check_rwx(spec: &JailSpec) -> Result<(), ()> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let real = home.as_deref().and_then(|h| fs::canonicalize(h).ok());
        let homes: Vec<&Path> = home.iter().chain(&real).map(PathBuf::as_path).collect();
        spec.check_exec(Os::Linux, &homes).map_err(|_| ())?;
        for root in &spec.rwx {
            if fs::canonicalize(root).ok().as_deref() != Some(root.as_path()) {
                return Err(());
            }
        }
        Ok(())
    }

    fn launch(job: &JailArgs) -> Result<Infallible, Failure> {
        // A proxy decides what to run from the name it is started as.
        let name = job.arg0.as_deref().unwrap_or(job.program.as_os_str());
        let error = Command::new(&job.program).arg0(name).args(&job.argv).exec();
        Err(Failure::Exec(job.program.display().to_string(), error))
    }

    /// Sets the spec's resource limits (soft and hard) on this process, so the
    /// program inherits them through `exec`.
    fn set_limits(spec: &JailSpec) -> Result<(), String> {
        for (limit, value) in spec.rlimits.limits() {
            let resource = match limit {
                Limit::Core => libc::RLIMIT_CORE,
                Limit::FileSize => libc::RLIMIT_FSIZE,
                Limit::CpuSeconds => libc::RLIMIT_CPU,
            };
            // A value this platform cannot hold is an error, never "no limit".
            let value = libc::rlim_t::try_from(*value).map_err(|_| RLIMIT_REASON.to_owned())?;
            let mut current = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: `current` is a valid struct that outlives the call.
            if unsafe { libc::getrlimit(resource, &mut current) } != 0 {
                return Err(RLIMIT_REASON.to_owned());
            }
            // A hard limit the parent already lowered stays: raising it is
            // EPERM, and the lower bound is the stricter one anyway.
            let value = value.min(current.rlim_max);
            let rlim = libc::rlimit {
                rlim_cur: value,
                rlim_max: value,
            };
            // SAFETY: `rlim` is a valid, initialized struct that outlives the call.
            if unsafe { libc::setrlimit(resource, &rlim) } != 0 {
                return Err(RLIMIT_REASON.to_owned());
            }
        }
        Ok(())
    }

    /// Builds the ruleset with every filesystem right of the probed ABI as a
    /// hard requirement, restricts this process and checks the outcome.
    fn enforce(spec: &JailSpec, abi: u32) -> Result<(), String> {
        debug_assert!(abi >= MIN_ABI);
        let abi_v = ABI::from(i32::try_from(abi).unwrap_or(i32::MAX));
        let all = AccessFs::from_all(abi_v);
        let mut ruleset = Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(all)
            .map_err(|e| e.to_string())?;
        if abi >= SCOPE_ABI {
            ruleset = ruleset
                .scope(Scope::from_all(abi_v))
                .map_err(|e| e.to_string())?;
        }
        if abi >= NET_ABI && spec.net_bind == NetBind::Closed {
            // Handled with no rule at all: nothing may listen on TCP, while
            // outgoing connect (not handled) stays open for the CLIs' APIs.
            // A toolchain run (`net_bind` open) leaves bind unhandled.
            ruleset = ruleset
                .handle_access(AccessNet::BindTcp)
                .map_err(|e| e.to_string())?;
        }
        let mut created = ruleset.create().map_err(|e| e.to_string())?;
        for (path, access) in rules(spec, abi_v)? {
            let fd = PathFd::new(&path).map_err(|e| e.to_string())?;
            created = created
                .add_rule(PathBeneath::new(fd, access))
                .map_err(|e| e.to_string())?;
        }
        let status = created.restrict_self().map_err(|e| e.to_string())?;
        if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
            return Err(format!(
                "ruleset {:?}, no_new_privs {}",
                status.ruleset, status.no_new_privs
            ));
        }
        Ok(())
    }

    /// Each path with the rights its rule kind grants. Execute is only ever
    /// part of an `rx` rule or the one `rwx` root (R29).
    fn rules(spec: &JailSpec, abi: ABI) -> Result<Vec<(PathBuf, BitFlags<AccessFs>)>, String> {
        let read_dir = AccessFs::ReadFile | AccessFs::ReadDir;
        let read_write_dir = AccessFs::from_all(abi) & !BitFlags::from(AccessFs::Execute);
        let read_write_file = AccessFs::from_file(abi) & !BitFlags::from(AccessFs::Execute);
        let mut out: Vec<_> = spec
            .rwx
            .iter()
            .map(|p| (p.clone(), AccessFs::from_all(abi)))
            .collect();
        for path in &spec.rx {
            let access = if is_dir(path)? {
                read_dir | AccessFs::Execute
            } else {
                AccessFs::ReadFile | AccessFs::Execute
            };
            out.push((path.clone(), access));
        }
        out.extend(spec.ro.iter().map(|p| (p.clone(), read_dir)));
        out.extend(spec.rw.iter().map(|p| (p.clone(), read_write_dir)));
        out.extend(
            spec.ro_files
                .iter()
                .map(|p| (p.clone(), AccessFs::ReadFile.into())),
        );
        out.extend(spec.rw_files.iter().map(|p| (p.clone(), read_write_file)));
        Ok(out)
    }

    fn is_dir(path: &Path) -> Result<bool, String> {
        fs::metadata(path)
            .map(|m| m.is_dir())
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::convert::Infallible;
    use std::ffi::OsString;
    #[cfg(target_os = "macos")]
    use std::path::Path;

    use super::Failure;
    #[cfg(not(target_os = "macos"))]
    use crate::core::ai::jail::JailOff;
    use crate::core::ai::jail::JailSupport;
    #[cfg(target_os = "macos")]
    use crate::core::ai::jail::{SEATBELT_ENABLED, seatbelt_support};

    #[cfg(target_os = "macos")]
    pub fn probe() -> JailSupport {
        // The gate answers before anything is spawned.
        let probed = if SEATBELT_ENABLED {
            super::seatbelt::probe()
        } else {
            Ok(())
        };
        seatbelt_support(SEATBELT_ENABLED, probed)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn probe() -> JailSupport {
        JailSupport::Unavailable(JailOff::NoBackend)
    }

    /// Replaces this process with `/usr/bin/sandbox-exec`, which applies the
    /// profile and runs the program. All the logic is in the pure
    /// `seatbelt::command`; only the `exec` call is macOS-specific.
    #[cfg(target_os = "macos")]
    pub fn exec(args: &[OsString]) -> Result<Infallible, Failure> {
        use std::os::unix::process::CommandExt;

        use super::seatbelt::{SANDBOX_EXEC, command, not_started};

        // `exec` only returns on failure; then no sandbox is in place.
        let _error = command(Path::new(SANDBOX_EXEC), args)?.exec();
        Err(not_started())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn exec(_args: &[OsString]) -> Result<Infallible, Failure> {
        Err(Failure::NotEnforced(
            "the CLI jail needs Linux or macOS".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::core::ai::jail::{Cli, CliLayout, SpecError, jail_spec_with};

    fn layout(home: &Path) -> CliLayout {
        CliLayout {
            cli: Cli::Claude,
            home: home.to_path_buf(),
            codex_home: None,
            binary: home.join(".local/share/claude/versions/1"),
            interpreter: None,
            loaders: Vec::new(),
            lib_dirs: Vec::new(),
            data_files: Vec::new(),
            helpers: Vec::new(),
            shim: None,
            cwd: PathBuf::from("/run/user/1/cwd"),
            tmp: PathBuf::from("/run/user/1/tmp"),
        }
    }

    /// A temp dir outside `/tmp`, which the broad-folder check always refuses.
    fn scratch() -> tempfile::TempDir {
        tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap()
    }

    #[test]
    fn resolve_follows_symlinks_and_reports_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir(real.join("target")).unwrap();
        symlink(real.join("target"), real.join("link")).unwrap();
        assert_eq!(resolve(&real.join("link")), Some(real.join("target")));
        assert_eq!(resolve(&real.join("nothing")), None);
    }

    #[test]
    fn a_state_symlink_to_home_or_ssh_is_refused_with_the_real_resolver() {
        let dir = scratch();
        let home = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir(home.join(".ssh")).unwrap();
        for target in [home.clone(), home.join(".ssh")] {
            let link = home.join(".claude");
            let _ = std::fs::remove_file(&link);
            symlink(&target, &link).unwrap();
            let err = jail_spec_with(&layout(&home), &resolve).unwrap_err();
            assert_eq!(err, SpecError::RootTooBroad(target));
        }
        // A real, dedicated state dir passes and is granted by its path.
        let _ = std::fs::remove_file(home.join(".claude"));
        std::fs::create_dir(home.join(".claude")).unwrap();
        let spec = jail_spec_with(&layout(&home), &resolve).unwrap();
        assert!(spec.rw.contains(&home.join(".claude")));
    }

    #[test]
    fn a_symlinked_home_is_refused_under_its_other_spelling() {
        let dir = scratch();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let real = base.join("var-home-u");
        std::fs::create_dir_all(real.join(".ssh")).unwrap();
        let alias = base.join("home-u");
        symlink(&real, &alias).unwrap();

        // $HOME is named through the link; the state dir points at the real one.
        let mut l = layout(&alias);
        l.binary = real.join(".local/share/claude/versions/1");
        for target in [real.clone(), real.join(".ssh")] {
            let _ = std::fs::remove_file(real.join(".claude"));
            symlink(&target, real.join(".claude")).unwrap();
            let err = jail_spec_with(&l, &resolve).unwrap_err();
            assert_eq!(err, SpecError::RootTooBroad(target));
        }
    }
}
