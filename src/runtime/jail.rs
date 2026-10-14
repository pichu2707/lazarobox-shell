//! The OS side of the CLI jail: the startup probe and the `__jail-exec`
//! helper. The rules themselves are built by the pure `core::ai::jail`.
//!
//! The helper is a plain single-threaded process image (`main` dispatches it
//! before the tokio runtime exists), so building the ruleset may allocate
//! freely, unlike a `pre_exec` hook after `fork` in the multi-threaded app.

use std::ffi::OsString;

use crate::core::ai::jail::{JailSupport, NOT_ENFORCED_EXIT};

/// What the helper prints when the jail cannot be applied; the turn then ends
/// with a jail error.
pub const NOT_ENFORCED: &str = "jail not enforced";

/// Exit status of a program the helper could not execute (shell convention).
const EXEC_FAILED: i32 = 127;

/// One raw `landlock_create_ruleset(NULL, 0, VERSION)` call: whether this
/// system can jail a CLI, and with which ABI.
pub fn probe() -> JailSupport {
    imp::probe()
}

/// Runs the helper with the arguments that follow `__jail-exec`. On success
/// the process image is replaced and this never returns; it returns the exit
/// status only when the program did not run.
pub fn exec_main(args: &[OsString]) -> i32 {
    match imp::exec(args) {
        Ok(never) => match never {},
        Err(Failure::NotEnforced(why)) => {
            eprintln!("{NOT_ENFORCED}: {why}");
            NOT_ENFORCED_EXIT
        }
        Err(Failure::Exec(program, error)) => {
            eprintln!("could not run {program}: {error}");
            EXEC_FAILED
        }
    }
}

/// Why the helper did not run the program.
enum Failure {
    NotEnforced(String),
    /// The jail is in place but the program could not be executed.
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
        ABI, Access, AccessFs, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
        RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope,
    };

    use super::Failure;
    use crate::core::ai::jail::JailSupport;
    use crate::core::ai::jail::{JailArgs, JailSpec, MIN_ABI, parse_args, support_from_abi};

    /// `landlock_create_ruleset` has the same number on every architecture.
    const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    const CREATE_RULESET_VERSION: libc::c_ulong = 1;
    /// The first ABI with `Scope` (abstract unix sockets and signals).
    const SCOPE_ABI: u32 = 6;

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
        if let JailSupport::Unavailable(why) = support_from_abi(abi) {
            return Err(Failure::NotEnforced(why.to_owned()));
        }
        enforce(&job.spec, abi).map_err(Failure::NotEnforced)?;
        launch(&job)
    }

    fn launch(job: &JailArgs) -> Result<Infallible, Failure> {
        let error = Command::new(&job.program)
            .arg0(&job.program)
            .args(&job.argv)
            .exec();
        Err(Failure::Exec(job.program.display().to_string(), error))
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
    /// part of an `rx` rule (R29).
    fn rules(spec: &JailSpec, abi: ABI) -> Result<Vec<(PathBuf, BitFlags<AccessFs>)>, String> {
        let read_dir = AccessFs::ReadFile | AccessFs::ReadDir;
        let read_write_dir = AccessFs::from_all(abi) & !BitFlags::from(AccessFs::Execute);
        let read_write_file = AccessFs::from_file(abi) & !BitFlags::from(AccessFs::Execute);
        let mut out = Vec::new();
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

    use super::Failure;
    use crate::core::ai::jail::JailSupport;

    pub fn probe() -> JailSupport {
        JailSupport::Unavailable("the CLI jail needs Linux")
    }

    pub fn exec(_args: &[OsString]) -> Result<Infallible, Failure> {
        Err(Failure::NotEnforced("the CLI jail needs Linux".to_owned()))
    }
}
