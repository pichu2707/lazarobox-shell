//! The macOS Seatbelt profile (SBPL) for a [`JailSpec`]. Pure: it builds the
//! profile text and the `-D KEY=path` list, and never touches the system, so
//! it is tested on every OS. `runtime::jail` hands the result to
//! `/usr/bin/sandbox-exec`.
//!
//! Paths never enter the profile text. Each one is a `(param "KEY")` whose
//! value travels as its own `-D` argument and is bound by Seatbelt itself, so
//! a quote, a parenthesis or a newline in a path cannot change the rules. The
//! text depends only on constants and on how many entries each class has.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::JailSpec;

/// The longest path accepted, in bytes (`PATH_MAX`).
pub const PATH_MAX: usize = 1024;

/// The most `-D` parameters in one profile. With the longest paths this keeps
/// the arguments near 30 KB, far below `ARG_MAX`.
pub const MAX_PARAMS: usize = 256;

/// A rendered profile: the text, and the values its `(param ..)` keys name.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Profile {
    pub text: String,
    /// `(KEY, path)` in the order the keys appear in the text.
    pub params: Vec<(String, PathBuf)>,
}

/// Why a spec cannot be rendered. Every case fails closed: no profile.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SbplError {
    /// The path holds a NUL byte, which no argv element can carry.
    Nul(PathBuf),
    Relative(PathBuf),
    /// The path is longer than [`PATH_MAX`] bytes.
    TooLong(PathBuf),
    /// The path is `/`, and `(subpath "/")` would grant the whole disk.
    Root(PathBuf),
    /// The path has a `.` or `..` segment. The spec layer never builds one;
    /// this is the last check before Seatbelt, which does not normalize.
    Unnormalized(PathBuf),
    /// The spec holds more than [`MAX_PARAMS`] paths.
    TooMany(usize),
}

/// The rules every jailed CLI gets, before its own paths: what a process needs
/// to start, resolve names and read system files (design, "SBPL renderer").
/// There is no `import "system.sb"`: it grants far more than this.
const HEAD: &str = r#"(version 1)
(deny default)
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target self))
(allow file-read-metadata)
(allow file-read* file-map-executable
  (subpath "/usr/lib") (subpath "/System/Library")
  (subpath "/System/Volumes/Preboot/Cryptexes")
  (require-all (subpath "/opt/homebrew") (require-not (subpath "/opt/homebrew/var")) (require-not (subpath "/opt/homebrew/etc")))
  (require-all (subpath "/usr/local") (require-not (subpath "/usr/local/var")) (require-not (subpath "/usr/local/etc"))))
(allow file-read* (subpath "/usr/share") (subpath "/private/etc") (subpath "/private/var/db/timezone"))
(allow file-read* (literal "/private/var/run/resolv.conf"))
(allow file-read* (literal "/"))
"#;

/// Network, mach services and sysctls. Outbound is IP only, plus the DNS
/// socket (and, above, the `rw_files` literals). There is no bind and no
/// inbound rule.
const TAIL: &str = r#"(allow network-outbound (remote ip "*:*"))
(allow network-outbound (literal "/private/var/run/mDNSResponder"))
(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.notification_center") (global-name "com.apple.logd")
  (global-name "com.apple.dnssd.service") (global-name "com.apple.trustd.agent")
  (global-name "com.apple.SecurityServer")
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
  (sysctl-name "kern.boottime") (sysctl-name "kern.bootargs") (sysctl-name "kern.argmax") (sysctl-name "kern.maxfilesperproc")
  (sysctl-name "kern.usrstack64") (sysctl-name "kern.secure_kernel") (sysctl-name "vm.loadavg")
  (sysctl-name "sysctl.proc_cputype") (sysctl-name "sysctl.proc_translated")
  (sysctl-name "security.mac.lockdown_mode_state"))
"#;

/// Checks one path before it becomes a `-D` value.
fn check(path: &Path) -> Result<(), SbplError> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.contains(&0) {
        return Err(SbplError::Nul(path.to_path_buf()));
    }
    if !path.is_absolute() {
        return Err(SbplError::Relative(path.to_path_buf()));
    }
    if bytes.len() > PATH_MAX {
        return Err(SbplError::TooLong(path.to_path_buf()));
    }
    if bytes.iter().all(|&b| b == b'/') {
        return Err(SbplError::Root(path.to_path_buf()));
    }
    if bytes
        .split(|&b| b == b'/')
        .any(|seg| seg == b"." || seg == b"..")
    {
        return Err(SbplError::Unnormalized(path.to_path_buf()));
    }
    Ok(())
}

/// Renders the profile for a spec. Folders (`rx`, `ro`, `rw`) match by
/// `subpath`, which on a file matches just that file; `ro_files` and
/// `rw_files` match by `literal`. `rw_except` folders are denied writing after
/// all the allows.
pub fn render(spec: &JailSpec) -> Result<Profile, SbplError> {
    let count = spec.rx.len()
        + spec.ro.len()
        + spec.rw.len()
        + spec.ro_files.len()
        + spec.rw_files.len()
        + spec.rw_except.len();
    if count > MAX_PARAMS {
        return Err(SbplError::TooMany(count));
    }
    let mut text = String::from(HEAD);
    let mut params = Vec::with_capacity(count);
    let classes: [(&str, &str, &str, &[PathBuf]); 5] = [
        (
            "RX",
            "subpath",
            "file-read* file-map-executable process-exec",
            &spec.rx,
        ),
        ("RO", "subpath", "file-read* file-map-executable", &spec.ro),
        ("RW", "subpath", "file-read* file-write*", &spec.rw),
        ("ROF", "literal", "file-read*", &spec.ro_files),
        ("RWF", "literal", "file-read* file-write*", &spec.rw_files),
    ];
    for (prefix, filter, ops, paths) in classes {
        for (n, path) in paths.iter().enumerate() {
            check(path)?;
            let key = format!("{prefix}_{n}");
            text.push_str(&format!("(allow {ops} ({filter} (param \"{key}\")))\n"));
            params.push((key, path.clone()));
        }
    }
    // Seatbelt takes the last matching rule, so the write exceptions come after
    // every allow that could grant them. They keep their read access.
    for (n, path) in spec.rw_except.iter().enumerate() {
        check(path)?;
        let key = format!("WX_{n}");
        text.push_str(&format!("(deny file-write* (subpath (param \"{key}\")))\n"));
        params.push((key, path.clone()));
    }
    // The shim socket is an `rw_files` literal; this is the only unix socket
    // a CLI may connect to besides the DNS one.
    for (n, _) in spec.rw_files.iter().enumerate() {
        text.push_str(&format!(
            "(allow network-outbound (literal (param \"RWF_{n}\")))\n"
        ));
    }
    text.push_str(TAIL);
    Ok(Profile { text, params })
}

/// The arguments for `sandbox-exec`, before `--` and the program: `-p`, the
/// text, then one `-D` and one `KEY=path` element per parameter. Path bytes
/// pass through raw. `sandbox-exec` splits a `-D` on its first `=`, so a value
/// holding `=` is safe (proved on macOS 14 and 15 by `tests/jail_macos.rs`).
pub fn sandbox_args(profile: &Profile) -> Vec<OsString> {
    let mut args = vec![OsString::from("-p"), OsString::from(&profile.text)];
    for (key, path) in &profile.params {
        let mut pair = OsString::from(key);
        pair.push("=");
        pair.push(path);
        args.push(OsString::from("-D"));
        args.push(pair);
    }
    args
}

#[cfg(test)]
mod tests;
