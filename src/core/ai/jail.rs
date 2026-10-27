//! The CLI jail, as far as the chat core needs to know it: whether this
//! system can confine a CLI, and exactly which paths a confined CLI may
//! touch. Everything here is pure; the probe and the helper that applies the
//! rules live in `runtime::jail`.
//!
//! On macOS a program that can be run must not be writable (Seatbelt unions
//! its rules), so a layout whose binary sits in the writable state dir is
//! refused. The one exception is codex's standalone install under
//! `<state>/packages`: that folder is carved out of the state dir's write
//! grant ([`JailSpec::rw_except`]), so codex runs there but cannot update
//! itself inside the jail.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use crate::core::ai::chat::engine::ChatError;
use crate::core::ai::parse::sanitize as one_line;

pub mod rpath;
pub mod sbpl;

/// Landlock ABI 3 (Linux 6.2) is the first with TRUNCATE; below it a jail
/// would leave a hole, so it counts as no jail.
pub const MIN_ABI: u32 = 3;

/// The helper's exit status when the rules could not be fully enforced.
pub const NOT_ENFORCED_EXIT: i32 = 125;

/// The line (start of a stderr line) the helper prints with that status. A CLI
/// can exit 125 on its own, so the status alone is not proof of a jail error.
pub const NOT_ENFORCED_MARKER: &str = "lazarobox-jail: jail not enforced";

/// First argument of the re-exec'd binary that selects the helper.
pub const SUBCOMMAND: &str = "__jail-exec";

/// How many bytes of a CLI file are read to find its interpreter.
pub const SHEBANG_BYTES: usize = 256;

/// The OS mechanism that confines a CLI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Landlock,
    /// macOS `sandbox-exec`. It has no versioned ABI, so `abi` is 0.
    Seatbelt,
}

/// Whether the macOS jail is wired into the product. While `false` the probe
/// answers [`SeatbeltFault::NotEnabled`] without spawning anything, so codex
/// and gemini stay refused on macOS. It is the switch to turn the macOS jail
/// off again.
pub const SEATBELT_ENABLED: bool = true;

/// Why the macOS jail cannot be used.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeatbeltFault {
    /// `/usr/bin/sandbox-exec` is not there.
    Missing,
    /// It refused a harmless test program.
    Refused,
    /// It let a forbidden test action through.
    Leaked,
    /// A test program did not finish in time.
    Timeout,
    /// The probe's scratch folder could not be made.
    Setup,
    /// The path of this program cannot be resolved for the helper.
    NoHelper,
    /// The jail is not turned on in this version.
    NotEnabled,
}

impl SeatbeltFault {
    fn text(self) -> &'static str {
        match self {
            Self::Missing => "/usr/bin/sandbox-exec is missing",
            Self::Refused => "sandbox-exec refused a test program",
            Self::Leaked => "the sandbox did not block a test action",
            Self::Timeout => "the sandbox check timed out",
            Self::Setup => "the sandbox check could not prepare its folder",
            Self::NoHelper => "this program's path cannot be resolved",
            Self::NotEnabled => "it is not enabled in this version",
        }
    }
}

/// Whether this system can jail a CLI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JailSupport {
    Available { backend: Backend, abi: u32 },
    Unavailable(JailOff),
}

/// Why this system cannot jail a CLI at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JailOff {
    /// The kernel has no Landlock (`0`) or an ABI below [`MIN_ABI`].
    NoLandlock(u32),
    /// macOS has `sandbox-exec`, but the jail cannot be used.
    NoSeatbelt(SeatbeltFault),
    /// Neither Linux nor macOS: no jail can be built here.
    NoBackend,
    /// Nothing probed the system yet (tests, the first frame).
    NotProbed,
}

impl JailOff {
    /// The full sentence, with what to do.
    pub fn text(self) -> String {
        match self {
            Self::NoLandlock(0) => {
                "Landlock is unavailable or blocked (kernel, container or seccomp)".to_owned()
            }
            Self::NoLandlock(abi) => {
                format!("Landlock ABI {MIN_ABI}+ needed (Linux 6.2); this kernel has ABI {abi}")
            }
            Self::NoSeatbelt(fault) => format!("the macOS jail is unavailable: {}", fault.text()),
            Self::NoBackend => "the CLI jail needs Linux or macOS".to_owned(),
            Self::NotProbed => "the jail was not probed".to_owned(),
        }
    }

    /// A few words for a menu row.
    pub fn short(self) -> String {
        match self {
            Self::NoLandlock(0) => "no Landlock".to_owned(),
            Self::NoLandlock(abi) => format!("Landlock ABI {abi}, needs {MIN_ABI}+"),
            Self::NoSeatbelt(_) => "jail unavailable".to_owned(),
            Self::NoBackend => "needs Linux or macOS".to_owned(),
            Self::NotProbed => "not probed".to_owned(),
        }
    }

    /// [`Self::text`] for a CLI that does not run without the jail (codex,
    /// gemini).
    pub fn text_required(self) -> String {
        match self {
            Self::NoBackend | Self::NoSeatbelt(_) => format!(
                "{}; on this system use claude or Ollama for chat",
                self.text()
            ),
            _ => self.text(),
        }
    }

    /// [`Self::short`] for a CLI that does not run without the jail.
    pub fn short_required(self) -> String {
        match self {
            Self::NoBackend => "Linux/macOS only · use claude".to_owned(),
            Self::NoSeatbelt(_) => "jail unavailable · use claude".to_owned(),
            _ => self.short(),
        }
    }
}

impl JailSupport {
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

/// Maps the ABI the kernel reported (0 when Landlock is absent).
pub fn support_from_abi(abi: u32) -> JailSupport {
    match abi {
        0..MIN_ABI => JailSupport::Unavailable(JailOff::NoLandlock(abi)),
        _ => JailSupport::Available {
            backend: Backend::Landlock,
            abi,
        },
    }
}

/// Maps what the macOS probe found. `enabled` is [`SEATBELT_ENABLED`]; while
/// it is off the probe result does not matter.
pub fn seatbelt_support(enabled: bool, probed: Result<(), SeatbeltFault>) -> JailSupport {
    match (enabled, probed) {
        (false, _) => JailSupport::Unavailable(JailOff::NoSeatbelt(SeatbeltFault::NotEnabled)),
        (true, Ok(())) => JailSupport::Available {
            backend: Backend::Seatbelt,
            abi: 0,
        },
        (true, Err(fault)) => JailSupport::Unavailable(JailOff::NoSeatbelt(fault)),
    }
}

/// What a CLI does without a jail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JailPolicy {
    /// codex and gemini: no jail, no process.
    Required,
    /// claude: its flags alone are text-only, the jail is a second layer.
    Layered,
    /// claude on macOS: it keeps its Keychain login, so it never runs jailed.
    Exempt,
}

/// The system a policy is chosen for. Passed in, so the choice is testable
/// on any system.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Os {
    Linux,
    Macos,
    Other,
}

impl Os {
    /// The system this program was built for.
    pub const CURRENT: Self = if cfg!(target_os = "linux") {
        Self::Linux
    } else if cfg!(target_os = "macos") {
        Self::Macos
    } else {
        Self::Other
    };
}

/// What `cli` does without a jail on `os`.
pub fn jail_policy(cli: Cli, os: Os) -> JailPolicy {
    match (cli, os) {
        (Cli::Claude, Os::Macos) => JailPolicy::Exempt,
        (Cli::Claude, _) => JailPolicy::Layered,
        (Cli::Codex | Cli::Gemini, _) => JailPolicy::Required,
    }
}

/// How a turn starts a CLI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Launch {
    Jailed,
    Bare,
    Refuse,
}

pub fn launch(policy: JailPolicy, support: JailSupport) -> Launch {
    match (policy, support.is_available()) {
        (JailPolicy::Exempt, _) => Launch::Bare,
        (_, true) => Launch::Jailed,
        (JailPolicy::Layered, false) => Launch::Bare,
        (JailPolicy::Required, false) => Launch::Refuse,
    }
}

/// The fixed texts `parse_args` fails with. The helper prints them after the
/// marker, so they are on the [`known_helper_reason`] whitelist.
const ARG_NO_SEPARATOR: &str = "missing `--`";
const ARG_NO_VALUE: &str = "a flag without a value";
const ARG_BAD_ABI: &str = "--min-abi is not a number";
const ARG_UNKNOWN_FLAG: &str = "unknown flag";
const ARG_RELATIVE_RULE: &str = "rule paths must be absolute";
const ARG_NO_PROGRAM: &str = "no program after `--`";
const ARG_RELATIVE_PROGRAM: &str = "the program must be an absolute path";
const ARG_BAD_NET_BIND: &str = "--net-bind takes only `open`";
const ARG_BAD_RLIMITS: &str = "--rlimits takes only `toolchain`";
const ARG_REASONS: [&str; 9] = [
    ARG_NO_SEPARATOR,
    ARG_NO_VALUE,
    ARG_BAD_ABI,
    ARG_UNKNOWN_FLAG,
    ARG_RELATIVE_RULE,
    ARG_NO_PROGRAM,
    ARG_RELATIVE_PROGRAM,
    ARG_BAD_NET_BIND,
    ARG_BAD_RLIMITS,
];

/// The macOS helper's other fixed texts (`runtime::jail::seatbelt`).
pub const MIN_ABI_REASON: &str = "--min-abi is Linux only";
pub const PROFILE_REASON: &str = "the macOS profile could not be built";
pub const EXEC_FAILED_REASON: &str = "/usr/bin/sandbox-exec could not be started";
/// The helper's refusal of an `rwx` root that fails [`JailSpec::check_exec`].
pub const RWX_REASON: &str = "the rwx root is not allowed";
/// The helper could not set a resource limit.
pub const RLIMIT_REASON: &str = "a resource limit could not be set";

/// The reasons the helper can print that are safe to show, as the user will
/// read them. The helper builds them from numbers and fixed text; everything
/// else it might print (library errors, argument errors) can echo paths or
/// other data a CLI controls, so it is not shown.
fn known_helper_reason(said: &str) -> Option<String> {
    let number = |text: &str| text.parse::<u32>().ok().filter(|_| !text.starts_with('+'));
    if let Some(rest) = said.strip_prefix("ABI ")
        && let Some((abi, min)) = rest.split_once(" is below the required ")
        && let (Some(abi), Some(min)) = (number(abi), number(min))
    {
        return Some(format!("ABI {abi} is below the required {min}"));
    }
    let abi_prefix = format!("Landlock ABI {MIN_ABI}+ needed (Linux 6.2); this kernel has ABI ");
    if let Some(abi) = said.strip_prefix(abi_prefix.as_str()).and_then(number) {
        return Some(JailOff::NoLandlock(abi).text());
    }
    if let Some(fixed) = [
        MIN_ABI_REASON,
        PROFILE_REASON,
        EXEC_FAILED_REASON,
        RWX_REASON,
        RLIMIT_REASON,
    ]
    .into_iter()
    .chain(ARG_REASONS)
    .find(|text| *text == said)
    {
        return Some(fixed.to_owned());
    }
    let macos = [
        SeatbeltFault::Missing,
        SeatbeltFault::Refused,
        SeatbeltFault::Leaked,
        SeatbeltFault::Timeout,
        SeatbeltFault::Setup,
        SeatbeltFault::NoHelper,
        SeatbeltFault::NotEnabled,
    ]
    .map(JailOff::NoSeatbelt);
    [JailOff::NoLandlock(0), JailOff::NoBackend]
        .into_iter()
        .chain(macos)
        .map(JailOff::text)
        .find(|text| text == said)
}

/// `sandbox-exec`'s own failure status (`EX_OSERR`): it could not compile the
/// profile or apply it, or the sandbox denied the exec of the program.
pub const SANDBOX_EXEC_EXIT: i32 = 71;

/// `sandbox-exec`'s status for a profile that does not compile (`EX_DATAERR`),
/// measured on macOS 14 and 15. The helper's own profile always compiles, so
/// this is a safety net for a renderer bug.
pub const SANDBOX_EXEC_SYNTAX_EXIT: i32 = 65;

/// What `sandbox-exec` starts its first stderr line with when it fails.
pub const SANDBOX_EXEC_PREFIX: &str = "sandbox-exec: ";

/// The fixed reason for a `sandbox-exec` failure. Its stderr is never echoed.
const SANDBOX_EXEC_REASON: &str = "the macOS sandbox could not be applied";

/// The jail error a finished helper stands for, if any. Two cases:
/// - Status 125 AND a stderr line starting with [`NOT_ENFORCED_MARKER`]. It
///   carries the helper's reason only when it is one of the fixed texts the
///   helper emits; any other text gives an empty reason (the generic message).
/// - Status 71 (or 65, a profile syntax error) AND the FIRST stderr line
///   starting with [`SANDBOX_EXEC_PREFIX`] (macOS): `sandbox-exec` refused the
///   profile. The reason is fixed. A CLI that forges this can only be misread
///   as a jail error, never as a bare run.
pub fn exit_error(code: Option<i32>, stderr: &str) -> Option<ChatError> {
    if code == Some(SANDBOX_EXEC_EXIT) || code == Some(SANDBOX_EXEC_SYNTAX_EXIT) {
        let first = stderr.lines().next()?;
        return first
            .starts_with(SANDBOX_EXEC_PREFIX)
            .then(|| ChatError::Jail(SANDBOX_EXEC_REASON.to_owned()));
    }
    let marked = stderr
        .lines()
        .find(|line| line.starts_with(NOT_ENFORCED_MARKER))?;
    if code != Some(NOT_ENFORCED_EXIT) {
        return None;
    }
    let said = marked[NOT_ENFORCED_MARKER.len()..].trim_start_matches(':');
    Some(ChatError::Jail(
        known_helper_reason(said.trim()).unwrap_or_default(),
    ))
}

/// Why a layout cannot be jailed, with the text the user sees. The jail
/// rules themselves are unchanged: this only names the rule that refused.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum JailRefusal {
    /// The system cannot jail anything.
    Unsupported(JailOff),
    /// `$HOME` is not set or not an absolute path.
    NoHome,
    /// `$HOME` names a folder that does not exist.
    HomeMissing(PathBuf),
    /// The CLI file cannot be resolved or read.
    Unreadable(PathBuf),
    /// The file the kernel loads the program with is not a regular file.
    BadLoader(PathBuf),
    /// The script asks for an interpreter by name and none is found.
    NoInterpreter(String),
    /// The script's `#!` line names this interpreter path, which is gone.
    InterpreterMissing(PathBuf),
    /// The interpreter is found but is not trusted.
    UntrustedInterpreter { path: PathBuf, why: Untrusted },
    /// A package root, state folder or loader that is a broad place.
    RootTooBroad(PathBuf),
    /// A path that is not canonical.
    NotCanonical(PathBuf),
    /// A program path the CLI could also write (or the other way round).
    Overlap(PathBuf),
}

/// Why an interpreter is not trusted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Untrusted {
    /// It lives under a scratch folder (see [`SCRATCH`]).
    Scratch,
    /// The file or folder at `path` (the real one that failed the check) is
    /// writable by group or others; `yours` says the user owns it.
    Writable { path: PathBuf, yours: bool },
    /// Its real path cannot be resolved.
    Unresolvable,
}

impl From<SpecError> for JailRefusal {
    fn from(error: SpecError) -> Self {
        match error {
            SpecError::RootTooBroad(path) => Self::RootTooBroad(path),
            SpecError::NotCanonical(path) => Self::NotCanonical(path),
            SpecError::Overlap { rx, .. } => Self::Overlap(rx),
        }
    }
}

/// `path` as one shell word, single-quoted, to paste into a command. `None`
/// when it is not valid text or holds a control character: a command built
/// from a cleaned-up copy would name another path.
fn quoted(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    (one_line(text) == text).then(|| format!("'{}'", text.replace('\'', r"'\''")))
}

/// Longest path a menu row shows whole.
const BRIEF_CAP: usize = 24;

/// `path` for a menu row: whole when short, else `…/` and its last part.
fn brief(path: &Path, home: Option<&Path>) -> String {
    let whole = shown(path, home);
    if whole.chars().count() <= BRIEF_CAP {
        return whole;
    }
    match path.file_name() {
        Some(name) => format!("…/{}", one_line(&name.to_string_lossy())),
        None => whole,
    }
}

/// `path` for a message: `~` for `home`, no control or format characters.
fn shown(path: &Path, home: Option<&Path>) -> String {
    let text = match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    };
    one_line(&text)
}

impl JailRefusal {
    /// The sentence for the chat: what refused and what to do. Paths are
    /// shown with `~` for `home` and sanitized.
    pub fn hint(&self, home: Option<&Path>) -> String {
        let at = |p: &Path| shown(p, home);
        match self {
            Self::Unsupported(off) => off.text(),
            Self::NoHome => {
                "$HOME is not set or not an absolute path; set it and restart".to_owned()
            }
            Self::HomeMissing(path) => {
                format!(
                    "$HOME ({}) does not exist; create it or fix $HOME",
                    one_line(&path.display().to_string())
                )
            }
            Self::Unreadable(path) => {
                format!(
                    "cannot read {}; check that it exists and is readable",
                    at(path)
                )
            }
            Self::BadLoader(path) => {
                format!("the program loader {} is not a regular file", at(path))
            }
            Self::NoInterpreter(name) => format!(
                "no {} found to run the CLI script; install it next to the CLI or on PATH",
                one_line(name)
            ),
            Self::UntrustedInterpreter {
                path,
                why: Untrusted::Scratch,
            } => format!(
                "{} is under a scratch folder; install the interpreter elsewhere",
                at(path)
            ),
            Self::InterpreterMissing(path) => format!(
                "{} (from the script's #! line) does not exist; reinstall the CLI",
                at(path)
            ),
            Self::UntrustedInterpreter {
                path,
                why: Untrusted::Unresolvable,
            } => format!(
                "{} cannot be resolved to a real path (a broken link?)",
                at(path)
            ),
            Self::UntrustedInterpreter {
                path,
                why: Untrusted::Writable { path: bad, yours },
            } => {
                let head = if bad == path {
                    format!("{} is writable by group or others", at(bad))
                } else {
                    format!(
                        "{} is writable by group or others, so {} is not trusted",
                        at(bad),
                        at(path)
                    )
                };
                if !yours {
                    format!("{head}; it is not yours, so its owner must fix it (sudo)")
                } else {
                    match quoted(bad) {
                        Some(arg) => format!("{head}; run: chmod go-w -- {arg}"),
                        None => format!("{head}; remove the group and other write permissions"),
                    }
                }
            }
            Self::RootTooBroad(path) => format!(
                "{} is too broad for the jail; keep the CLI and its state in folders of their own",
                at(path)
            ),
            Self::NotCanonical(path) => format!("{} is not a canonical path", at(path)),
            Self::Overlap(path) => format!(
                "{} would be both run and written by the jail; keep the CLI and its state in separate folders",
                at(path)
            ),
        }
    }

    /// A few words for a menu row.
    pub fn short(&self, home: Option<&Path>) -> String {
        match self {
            Self::Unsupported(off) => off.short(),
            Self::NoHome => "no usable $HOME".to_owned(),
            Self::HomeMissing(_) => "$HOME missing".to_owned(),
            Self::Unreadable(path) => format!("cannot read {}", brief(path, home)),
            Self::InterpreterMissing(path) => format!("{} missing", brief(path, home)),
            Self::BadLoader(_) => "bad program loader".to_owned(),
            Self::NoInterpreter(name) => format!("no {}", one_line(name)),
            Self::UntrustedInterpreter { .. } => "interpreter not trusted".to_owned(),
            Self::RootTooBroad(path) => format!("{} too broad", brief(path, home)),
            Self::NotCanonical(path) => format!("{} not canonical", brief(path, home)),
            Self::Overlap(path) => format!("{} is run and written", brief(path, home)),
        }
    }
}

/// The CLIs that run jailed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cli {
    Claude,
    Codex,
    Gemini,
}

/// What a script's first line names as its interpreter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Interpreter {
    /// An absolute path, used as written.
    Path(PathBuf),
    /// A bare name that `env` would look up on `PATH`.
    OnPath(String),
}

/// Reads the interpreter from the first [`SHEBANG_BYTES`] of a file. `None`
/// for a binary, a line that does not fit, or anything unclear.
pub fn parse_shebang(head: &[u8]) -> Option<Interpreter> {
    let head = &head[..head.len().min(SHEBANG_BYTES)];
    let rest = head.strip_prefix(b"#!")?;
    let line = match rest.iter().position(|b| *b == b'\n') {
        Some(end) => &rest[..end],
        None if head.len() < SHEBANG_BYTES => rest,
        None => return None,
    };
    let line = std::str::from_utf8(line).ok()?;
    let mut words = line.split_whitespace();
    let program = Path::new(words.next()?);
    if !program.is_absolute() {
        return None;
    }
    if program.file_name() != Some(OsStr::new("env")) {
        return Some(Interpreter::Path(program.to_path_buf()));
    }
    let name = words.find(|w| !w.starts_with('-') && !w.contains('='))?;
    if name.contains('/') {
        let path = Path::new(name);
        return path
            .is_absolute()
            .then(|| Interpreter::Path(path.to_path_buf()));
    }
    Some(Interpreter::OnPath(name.to_owned()))
}

/// How many bytes of an executable are read to find its ELF loader. The
/// program headers and the `.interp` string sit at the very start of the file.
pub const ELF_HEAD_BYTES: usize = 4096;

const PT_INTERP: u32 = 3;

fn le_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn le_u64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// The longest loader path read (`PATH_MAX`).
pub const INTERP_MAX: usize = 4096;

/// Where the `PT_INTERP` string of a little-endian ELF64 executable lives:
/// its file offset and size. The program headers are at the very start of
/// the file, so `head` (the first [`ELF_HEAD_BYTES`]) is enough to find it;
/// the string itself can be anywhere (Homebrew moves it to 0x8d98 in node),
/// so the caller reads it from the file. `None` for a static binary, a script
/// and anything malformed or with a size no path can have.
pub fn elf_interp_at(head: &[u8]) -> Option<(u64, usize)> {
    const ELF64: &[u8] = b"\x7fELF\x02\x01";
    if !head.starts_with(ELF64) {
        return None;
    }
    let table = usize::try_from(le_u64(head, 0x20)?).ok()?;
    let entry = usize::from(le_u16(head, 0x36)?);
    let count = usize::from(le_u16(head, 0x38)?);
    if entry < 40 || table.checked_add(entry.checked_mul(count)?)? > head.len() {
        return None;
    }
    for i in 0..count {
        let at = table + i * entry;
        if le_u32(head, at)? != PT_INTERP {
            continue;
        }
        let offset = le_u64(head, at + 8)?;
        let size = usize::try_from(le_u64(head, at + 32)?).ok()?;
        return (1..=INTERP_MAX).contains(&size).then_some((offset, size));
    }
    None
}

/// The loader a `PT_INTERP` string names: absolute UTF-8, up to its NUL.
pub fn interp_path(raw: &[u8]) -> Option<PathBuf> {
    let name = std::str::from_utf8(raw.split(|b| *b == 0).next()?).ok()?;
    let path = Path::new(name);
    path.is_absolute().then(|| path.to_path_buf())
}

/// The dynamic loader a little-endian ELF64 executable asks for (its
/// `PT_INTERP`), when the string lies inside `head`. The kernel opens it with
/// execute rights when the program starts, so it is the one extra file a
/// dynamic program needs to run at all. `None` for static binaries, scripts
/// and anything malformed.
pub fn elf_loader(head: &[u8]) -> Option<PathBuf> {
    let (offset, size) = elf_interp_at(head)?;
    let start = usize::try_from(offset).ok()?;
    interp_path(head.get(start..start.checked_add(size)?)?)
}

/// The directory that holds a CLI's own code: the `node_modules` package
/// (`<pkg>` or `@scope/pkg`, innermost), else the folder of the binary.
pub fn package_root(binary: &Path) -> Option<PathBuf> {
    if !binary.is_absolute() {
        return None;
    }
    let parts: Vec<Component> = binary.components().collect();
    if let Some(at) = parts.iter().rposition(|c| c.as_os_str() == "node_modules") {
        let after = &parts[at + 1..];
        let scoped = after
            .first()
            .is_some_and(|c| c.as_os_str().as_encoded_bytes().starts_with(b"@"));
        let name = if scoped { 2 } else { 1 };
        // The package folder must sit above the binary. When the binary is the
        // package folder itself the parent would be the whole node_modules.
        return (after.len() > name).then(|| parts[..=at + name].iter().collect());
    }
    binary.parent().map(Path::to_path_buf)
}

/// Folders that are never a package or a state dir, whatever the layout says.
/// Checked on every system.
const BROAD: &[&str] = &[
    "/",
    "/usr",
    "/usr/bin",
    "/usr/sbin",
    "/usr/local",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/lib",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/etc",
    "/var",
    "/opt",
    "/home",
    "/root",
    "/run",
    "/mnt",
    "/media",
    "/srv",
    "/snap",
    "/usr/share",
    "/usr/libexec",
    "/var/lib",
];

/// The macOS folders that are never a package or a state dir, on top of
/// [`BROAD`]: the system, the apps, every user's home and Homebrew's prefix
/// and its package folders. Checked on macOS only.
const BROAD_MACOS: &[&str] = &[
    "/System",
    "/Library",
    "/Applications",
    "/Users",
    "/Volumes",
    "/private",
    "/private/var",
    "/private/etc",
    "/opt/homebrew",
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/opt/homebrew/lib",
    "/opt/homebrew/Cellar",
    "/opt/homebrew/opt",
    "/opt/homebrew/share",
    "/opt/homebrew/etc",
    "/opt/homebrew/include",
    "/opt/homebrew/Caskroom",
    "/opt/homebrew/Frameworks",
    "/opt/homebrew/var",
    "/usr/local/Cellar",
    "/usr/local/opt",
    "/usr/local/lib",
    "/usr/local/share",
    "/usr/local/etc",
    "/usr/local/include",
    "/usr/local/Homebrew",
    "/usr/local/Frameworks",
    "/usr/local/var",
];

/// Every system but macOS: the folders that hold libraries and data, and the
/// system state a CLI reads. Libraries and data only. The folders that hold programs
/// (`/usr/bin`, `/usr/sbin`, `/bin`) stay unreadable: Landlock does not gate
/// `mmap(PROT_EXEC)`, so the ELF loader that some CLI needs to start could
/// otherwise run `ld.so /usr/bin/curl`. `/bin` is a link to `/usr/bin` on
/// most systems, which is why it is not listed. `/lib` and `/lib64` are
/// libraries (and links to `/usr/lib`).
pub(crate) const LINUX_RO: [&str; 7] = [
    "/usr/lib",
    "/usr/share",
    "/lib",
    "/lib64",
    "/etc",
    "/proc/self",
    "/sys/fs/cgroup",
];

/// Every system but macOS: `/proc` files that runtimes read for the CPU and memory count.
pub(crate) const LINUX_RO_FILES: [&str; 3] = ["/proc/cpuinfo", "/proc/meminfo", "/proc/stat"];

/// Device files every system gives a jailed CLI: randomness, read-only.
pub(crate) const DEV_RO_FILES: [&str; 2] = ["/dev/urandom", "/dev/random"];

/// The device file every system gives a jailed CLI to write to.
pub(crate) const DEV_RW_FILE: &str = "/dev/null";

/// Files under `/etc` that name resolution and TLS clocks need, and that a
/// distribution may link to another place.
pub(crate) const ETC_LINKS: [&str; 4] = [
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/nsswitch.conf",
    "/etc/localtime",
];

/// Folders under `$HOME` that hold many programs' data: a CLI's state must be
/// a dedicated folder inside them, never one of them.
const SHARED_IN_HOME: &[&str] = &[
    ".config",
    ".local",
    ".local/share",
    ".local/state",
    ".cache",
];

/// Folders under `$HOME` (and everything below them) that hold another tool's
/// credentials or state. A CLI may only be given its own one: codex gets
/// `~/.codex`, never `~/.claude`.
pub(crate) const CREDENTIAL_DIRS: &[&str] = &[
    ".claude",
    ".gemini",
    ".codex",
    ".aws",
    ".kube",
    ".docker",
    ".password-store",
];

/// [`SHARED_IN_HOME`] on macOS: `~/Library` and the folders in it that hold
/// many apps' data. Only these folders themselves are refused here; anything
/// below `~/Library` is refused by [`LIBRARY_MACOS`] unless it is allowed.
const SHARED_IN_HOME_MACOS: &[&str] = &[
    "Library",
    "Library/Application Support",
    "Library/Caches",
    "Library/Preferences",
];

/// The folder under `$HOME` where macOS keeps every app's data. Nothing at or
/// under it is granted, except [`LIBRARY_ALLOWED_MACOS`].
const LIBRARY_MACOS: &str = "Library";

/// The only places at or under `~/Library` that may be granted, relative to
/// `$HOME`: package locations of tools that install there (fnm keeps its node
/// versions in `Application Support/fnm`). Everything else (Chrome, 1Password,
/// `Developer`, `Preferences`...) holds another app's data.
const LIBRARY_ALLOWED_MACOS: &[&str] = &["Library/Application Support/fnm"];

/// [`CREDENTIAL_DIRS`] on macOS: the Keychain and the folders of the user's
/// accounts, mail, messages, browsing and app containers. Nothing in or below
/// them is ever granted.
const CREDENTIAL_DIRS_MACOS: &[&str] = &[
    "Library/Keychains",
    "Library/Cookies",
    "Library/Mail",
    "Library/Messages",
    "Library/Safari",
    "Library/Containers",
    "Library/Group Containers",
    "Library/Mobile Documents",
    "Library/Accounts",
    "Library/Calendars",
];

impl Cli {
    /// The folder under `$HOME` that is this CLI's own by default.
    fn own_dir(self) -> &'static str {
        match self {
            Cli::Claude => ".claude",
            Cli::Codex => ".codex",
            Cli::Gemini => ".gemini",
        }
    }
}

/// The scratch folders, by every name a path can carry. macOS keeps `/tmp` and
/// `/var/tmp` as links into `/private` and its `$TMPDIR` lives under
/// `/private/var/folders`, so a canonical path there starts with `/private`.
/// `/var/folders` is the same place by its link name, listed in case a path
/// is ever compared before it is resolved.
pub(crate) const SCRATCH: [&str; 6] = [
    "/tmp",
    "/var/tmp",
    "/private/tmp",
    "/private/var/tmp",
    "/private/var/folders",
    "/var/folders",
];

/// Whether a path is under a scratch folder.
pub(crate) fn is_scratch(path: &Path) -> bool {
    SCRATCH.iter().any(|scratch| path.starts_with(scratch))
}

/// Whether granting `path` to `cli` would open far more than its own files:
/// `$HOME` or one of its parents, `/tmp`, `~/.ssh`, `~/.gnupg`, another
/// tool's credential folder, a shared data folder of `$HOME` or a system or
/// mount folder. On macOS the letter case does not count, as on its default
/// file system: `~/library/keychains` is the keychain folder.
fn too_broad(path: &Path, homes: &[&Path], cli: Cli, os: Os) -> bool {
    let macos: &[&str] = if os == Os::Macos { BROAD_MACOS } else { &[] };
    let path = &fold(path, os);
    is_scratch(path)
        || BROAD
            .iter()
            .chain(macos)
            .any(|b| *path == fold(Path::new(b), os))
        || homes
            .iter()
            .any(|home| too_broad_for_home(path, &fold(home, os), cli, os))
}

/// Whether a project root or toolchain folder is a broad place, for any CLI's
/// credential folder: `$HOME` or above it, a scratch folder, a system folder,
/// or a credential folder of any CLI. Linux rules; `homes` holds both
/// spellings of `$HOME`.
pub(crate) fn broad_place(path: &Path, homes: &[&Path]) -> bool {
    [Cli::Claude, Cli::Codex, Cli::Gemini]
        .into_iter()
        .any(|cli| too_broad(path, homes, cli, Os::Linux))
}

/// `path` in lower case on macOS, where the file system ignores letter case;
/// unchanged elsewhere. Both sides of a comparison go through it.
fn fold(path: &Path, os: Os) -> PathBuf {
    match os {
        Os::Macos => PathBuf::from(path.to_string_lossy().to_lowercase()),
        _ => path.to_path_buf(),
    }
}

/// `path` and `home` are already [`fold`]ed.
fn too_broad_for_home(path: &Path, home: &Path, cli: Cli, os: Os) -> bool {
    let (credentials, shared): (&[&str], &[&str]) = if os == Os::Macos {
        (CREDENTIAL_DIRS_MACOS, SHARED_IN_HOME_MACOS)
    } else {
        (&[], &[])
    };
    let at = |dir: &str| fold(&home.join(dir), os);
    home.starts_with(path)
        || path.starts_with(at(".ssh"))
        || path.starts_with(at(".gnupg"))
        || CREDENTIAL_DIRS
            .iter()
            .chain(credentials)
            .filter(|d| **d != cli.own_dir())
            .any(|d| path.starts_with(at(d)))
        || SHARED_IN_HOME.iter().chain(shared).any(|d| path == at(d))
        || (os == Os::Macos && in_library_not_allowed(path, home))
}

/// Whether `path` is at or under `~/Library` and not at or under an allowed
/// place, without letter case (`path` and `home` are [`fold`]ed). The
/// credential folders are checked apart, so they stay refused even if an
/// allowed place ever overlapped one.
fn in_library_not_allowed(path: &Path, home: &Path) -> bool {
    let at = |dir: &str| fold(&home.join(dir), Os::Macos);
    path.starts_with(at(LIBRARY_MACOS))
        && !LIBRARY_ALLOWED_MACOS
            .iter()
            .any(|allowed| path.starts_with(at(allowed)))
}

/// Why a layout cannot be jailed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SpecError {
    /// A package root or state dir that would grant a whole broad folder.
    RootTooBroad(PathBuf),
    /// A path that is not absolute or holds a `.` or `..` segment. `jail_spec`
    /// takes canonical paths: a lexical check on `a/../b` would be fooled.
    NotCanonical(PathBuf),
    /// A path that can be executed is equal to, under or over one that can be
    /// written. Seatbelt unions its rules, so the CLI could run what it wrote.
    Overlap { rx: PathBuf, rw: PathBuf },
}

pub(crate) fn is_canonical_form(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .as_os_str()
            .as_encoded_bytes()
            .split(|b| *b == b'/')
            .any(|segment| segment == b"." || segment == b"..")
}

/// The resolved facts about one CLI, with the paths already canonical.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CliLayout {
    pub cli: Cli,
    pub home: PathBuf,
    /// `$CODEX_HOME`, when set (codex only).
    pub codex_home: Option<PathBuf>,
    /// The canonical CLI path.
    pub binary: PathBuf,
    /// The canonical interpreter file of a script CLI.
    pub interpreter: Option<PathBuf>,
    /// The canonical ELF loaders (`PT_INTERP`) of the interpreter, or of the
    /// native CLI when it has none. Without them nothing dynamic can start.
    pub loaders: Vec<PathBuf>,
    /// The canonical library folders the interpreter's ELF names in its own
    /// search path (Homebrew's node lists its own `opt` folders), read-only.
    pub lib_dirs: Vec<PathBuf>,
    /// The canonical data files the interpreter reads at startup that are in
    /// none of the folders above (Homebrew's OpenSSL config and CA bundle),
    /// read-only.
    pub data_files: Vec<PathBuf>,
    /// Extra programs a manual check proved necessary, by exact path.
    pub helpers: Vec<PathBuf>,
    /// What the engine's MCP shim needs (tools on): see [`ShimGrant`].
    pub shim: Option<ShimGrant>,
    /// The private empty working directory.
    pub cwd: PathBuf,
    /// The private per-turn `TMPDIR`.
    pub tmp: PathBuf,
}

/// The minimum the engine needs to start `lazarobox __mcp-serve <socket>` and
/// connect: execute the lazarobox program, and read and write the one socket
/// file. No folder is granted (D11). Every path is canonical; the program is
/// the resolved executable, not `/proc/self/exe` (inside the CLI that is node).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShimGrant {
    pub binary: PathBuf,
    /// The ELF loader of `binary`; it adds a rule only when the CLI's own
    /// loaders do not already cover it.
    pub loader: Option<PathBuf>,
    pub socket: PathBuf,
}

/// The paths a jailed CLI may use.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct JailSpec {
    /// Read, write and execute: the one project root of a toolchain run
    /// (`target/`, `node_modules/` and `.venv` live in it). Empty for every
    /// CLI spec. Execute may appear only here and in `rx` (R29).
    pub rwx: Vec<PathBuf>,
    /// Read and execute: single files, or the CLI's own package folder.
    pub rx: Vec<PathBuf>,
    pub ro: Vec<PathBuf>,
    pub rw: Vec<PathBuf>,
    pub ro_files: Vec<PathBuf>,
    pub rw_files: Vec<PathBuf>,
    /// Folders inside an `rw` one that stay read-only. Only the macOS profile
    /// can carve them out (a later `deny file-write*` beats the allow);
    /// Landlock cannot, so Linux never gets one.
    pub rw_except: Vec<PathBuf>,
    pub net_bind: NetBind,
    pub rlimits: Rlimits,
}

/// Whether the jailed program may listen on TCP (Landlock ABI 4+; below that
/// nothing is mediated). Closed unless a toolchain run asks for a dev server.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NetBind {
    #[default]
    Closed,
    Open,
}

/// The resource limits the helper sets before `exec`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Rlimits {
    #[default]
    None,
    Toolchain,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Limit {
    Core,
    FileSize,
    CpuSeconds,
}

impl Rlimits {
    /// Each limit with the value used for both its soft and hard bound.
    pub fn limits(self) -> &'static [(Limit, u64)] {
        match self {
            Rlimits::None => &[],
            Rlimits::Toolchain => &[
                (Limit::Core, 0),
                (Limit::FileSize, 4 << 30),
                (Limit::CpuSeconds, 1200),
            ],
        }
    }
}

/// The folder of a codex state dir that holds the standalone installer's
/// binaries (`packages/standalone/releases/<version>/bin`).
const CODEX_PACKAGES: &str = "packages";

pub(crate) fn push_unique(list: &mut Vec<PathBuf>, path: PathBuf) {
    if !list.contains(&path) {
        list.push(path);
    }
}

/// Builds the rules for one CLI from canonical paths, taking the state dirs
/// as written (`~/.claude`, `$CODEX_HOME`, …). Use [`jail_spec_with`] with a
/// real canonicalizer so a symlinked state dir cannot reach a broad folder.
#[cfg(test)]
pub(crate) fn jail_spec(layout: &CliLayout) -> Result<JailSpec, SpecError> {
    jail_spec_linux(layout, &|path| Some(path.to_path_buf()))
}

/// The Linux rules, whatever system the tests run on.
#[cfg(test)]
pub(crate) fn jail_spec_linux(
    layout: &CliLayout,
    resolve: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Result<JailSpec, SpecError> {
    jail_spec_for(Os::Linux, layout, resolve)
}

/// Builds the rules for one CLI. Pure: nothing is checked on disk, so the
/// caller drops the paths that do not exist with [`JailSpec::retain_existing`].
///
/// Every path of the layout must be canonical (absolute, no `.`/`..`). The
/// state dirs are derived here, so `resolve` (a canonicalizer; `None` for a
/// path that does not exist) maps them to their targets and the broad-folder
/// check runs on the result.
pub fn jail_spec_with(
    layout: &CliLayout,
    resolve: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Result<JailSpec, SpecError> {
    jail_spec_for(Os::CURRENT, layout, resolve)
}

/// [`jail_spec_with`] for a given system. The CLI's own grants are the same
/// everywhere; the system folders differ. On macOS the spec names none of the
/// Linux paths (the system folders come from the profile's fixed block
/// instead); every other system gets the Linux sets.
pub fn jail_spec_for(
    os: Os,
    layout: &CliLayout,
    resolve: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Result<JailSpec, SpecError> {
    let given = [&layout.home, &layout.binary, &layout.cwd, &layout.tmp]
        .into_iter()
        .chain(&layout.codex_home)
        .chain(&layout.interpreter)
        .chain(&layout.loaders)
        .chain(&layout.lib_dirs)
        .chain(&layout.data_files)
        .chain(&layout.helpers)
        .chain(
            layout
                .shim
                .iter()
                .flat_map(|s| [&s.binary, &s.socket].into_iter().chain(s.loader.as_ref())),
        );
    if let Some(bad) = given.into_iter().find(|p| !is_canonical_form(p)) {
        return Err(SpecError::NotCanonical(bad.clone()));
    }
    // $HOME can have two spellings (/home -> /var/home): check both.
    let real_home = resolve(&layout.home).unwrap_or_else(|| layout.home.clone());
    let homes = [layout.home.as_path(), real_home.as_path()];
    let root = package_root(&layout.binary)
        .ok_or_else(|| SpecError::RootTooBroad(layout.binary.clone()))?;
    if too_broad(&root, &homes, layout.cli, os) {
        return Err(SpecError::RootTooBroad(root));
    }
    // A loader or helper is one exact file: never a broad place.
    if let Some(bad) = layout
        .loaders
        .iter()
        .chain(&layout.helpers)
        .chain(
            layout
                .shim
                .iter()
                .flat_map(|s| [&s.binary].into_iter().chain(s.loader.as_ref())),
        )
        .find(|p| too_broad(p, &homes, layout.cli, os))
    {
        return Err(SpecError::RootTooBroad(bad.clone()));
    }
    let resolved = |path: PathBuf| resolve(&path).unwrap_or(path);
    let (state, state_file) = match layout.cli {
        Cli::Claude => (
            layout.home.join(".claude"),
            Some(layout.home.join(".claude.json")),
        ),
        Cli::Codex => (
            layout
                .codex_home
                .clone()
                .unwrap_or_else(|| layout.home.join(".codex")),
            None,
        ),
        Cli::Gemini => (layout.home.join(".gemini"), None),
    };
    let state = resolved(state);
    // Only a real folder inside the state dir can be carved out: a link, or
    // one that resolves elsewhere, would put the exception outside every rw
    // dir, and its name could be replaced by the jail.
    let packages = (layout.cli == Cli::Codex)
        .then(|| state.join(CODEX_PACKAGES))
        .filter(|packages| resolved(packages.clone()) == *packages);
    let state_file = state_file.map(resolved);
    for path in [Some(&state), state_file.as_ref()].into_iter().flatten() {
        if too_broad(path, &homes, layout.cli, os) {
            return Err(SpecError::RootTooBroad(path.clone()));
        }
    }

    let mut spec = JailSpec::default();
    for path in layout
        .interpreter
        .iter()
        .chain(&layout.loaders)
        .chain([&root])
        .chain(&layout.helpers)
    {
        push_unique(&mut spec.rx, path.clone());
    }
    if os != Os::Macos {
        spec.ro.extend(LINUX_RO.map(PathBuf::from));
    }
    push_unique(&mut spec.ro, layout.cwd.clone());
    // The interpreter's own library folders, read-only. A folder the system
    // rules already cover adds nothing, and a broad one is dropped rather than
    // granted: the program then reports the library it cannot load.
    let system = spec.ro.clone();
    for dir in &layout.lib_dirs {
        let covered = system.iter().any(|s| dir.starts_with(s));
        if !covered && !too_broad(dir, &homes, layout.cli, os) {
            push_unique(&mut spec.ro, dir.clone());
        }
    }
    push_unique(&mut spec.rw, state);
    push_unique(&mut spec.rw, layout.tmp.clone());
    // Codex's standalone installer keeps the binary inside the state dir. That
    // folder stays readable and executable, but never writable: a CLI that
    // could rewrite its own binary would escape on the next run. Only added
    // when the CLI itself (not a helper) lives there, so other layouts keep the same
    // profile, and only on macOS, the one system that can carve it out.
    if os == Os::Macos
        && let Some(packages) = packages
        && layout.binary.starts_with(&packages)
    {
        push_unique(&mut spec.rw_except, packages);
    }
    if os != Os::Macos {
        spec.ro_files.extend(LINUX_RO_FILES.map(PathBuf::from));
    }
    spec.ro_files.extend(DEV_RO_FILES.map(PathBuf::from));
    // System files that are often links out of `/etc`: WSL's resolv.conf is
    // `/mnt/wsl/resolv.conf`, systemd-resolved's lives under `/run`. Landlock
    // checks the target, so `/etc` alone does not cover them and DNS fails
    // with EAI_AGAIN. Grant the exact target file, never its folder.
    let readable = spec.ro.clone();
    // macOS reads `/private/etc` from the profile's fixed block instead.
    let etc_links: &[&str] = if os == Os::Macos { &[] } else { &ETC_LINKS };
    for file in etc_links {
        let Some(real) = resolve(Path::new(file)) else {
            continue;
        };
        let covered = readable.iter().any(|dir| real.starts_with(dir));
        if !covered && is_canonical_form(&real) && !too_broad(&real, &homes, layout.cli, os) {
            push_unique(&mut spec.ro_files, real);
        }
    }
    for file in &layout.data_files {
        if !too_broad(file, &homes, layout.cli, os) {
            push_unique(&mut spec.ro_files, file.clone());
        }
    }
    spec.rw_files.push(PathBuf::from(DEV_RW_FILE));
    spec.rw_files.extend(state_file);
    if let Some(shim) = &layout.shim {
        push_unique(&mut spec.rx, shim.binary.clone());
        if let Some(loader) = &shim.loader {
            push_unique(&mut spec.rx, loader.clone());
        }
        push_unique(&mut spec.rw_files, shim.socket.clone());
    }
    if os == Os::Macos
        && let Some((rx, rw)) = writable_executable(&spec, os)
    {
        return Err(SpecError::Overlap { rx, rw });
    }
    Ok(spec)
}

/// The first write exception that is not a strict descendant of an `rw`
/// folder. The profile pins the folders above an exception, and that only makes
/// sense when an `rw` folder holds it.
#[cfg(test)]
pub(crate) fn except_outside_rw(spec: &JailSpec) -> Option<PathBuf> {
    spec.rw_except
        .iter()
        .find(|e| !spec.rw.iter().any(|w| *e != w && e.starts_with(w)))
        .cloned()
}

/// The first execute path (`rx` or the `rwx` root) that is equal to, under or
/// over a writable one, without letter case on macOS (see [`fold`]). An execute
/// path under a write exception that is itself inside that writable folder is not writable, so it
/// does not count; a writable file never gets that, and neither does a program
/// over the folder.
pub(crate) fn writable_executable(spec: &JailSpec, os: Os) -> Option<(PathBuf, PathBuf)> {
    // The exemption compares exactly: on a case-sensitive file system a case
    // variant of the exception is another, writable folder the deny misses.
    let excepted = |rx: &Path, rw: &Path| {
        spec.rw_except
            .iter()
            .any(|e| e.starts_with(rw) && rx.starts_with(e))
    };
    spec.rx.iter().chain(&spec.rwx).find_map(|rx| {
        let x = fold(rx, os);
        let in_dir = spec.rw.iter().find(|rw| {
            let w = fold(rw, os);
            (x.starts_with(&w) && !excepted(rx, rw)) || w.starts_with(&x)
        });
        let in_file = || {
            spec.rw_files.iter().find(|rw| {
                let w = fold(rw, os);
                x.starts_with(&w) || w.starts_with(&x)
            })
        };
        in_dir.or_else(in_file).map(|rw| (rx.clone(), rw.clone()))
    })
}

/// Why a spec breaks the execute rule (R29: execute only in `rx` or in the
/// single `rwx` root).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExecFault {
    /// More than one `rwx` root.
    ManyRwx(usize),
    /// The `rwx` root is not absolute or holds a `.` or `..` segment.
    NotCanonical(PathBuf),
    /// The `rwx` root is `/`, `$HOME`, a system or credential folder or
    /// another broad place: it would run and write far more than a project.
    TooBroad(PathBuf),
    /// Seatbelt does not render `rwx` yet: `run_command` is Linux only.
    Macos,
    /// An execute path (`rx` or `rwx`) is equal to, under or over a writable
    /// one: what the program writes there it could then run (R29).
    WritableExec { rx: PathBuf, rw: PathBuf },
}

impl JailSpec {
    /// Every path the jailed program may execute from: `rx`, then the `rwx`
    /// root. No other class carries the execute right.
    pub fn exec_paths(&self) -> Vec<PathBuf> {
        self.rx.iter().chain(&self.rwx).cloned().collect()
    }

    /// Checks the `rwx` class. A CLI spec has none and always passes. On macOS
    /// an `rwx` root is refused until the Seatbelt renderer grants toolchains.
    /// `homes` holds both spellings of `$HOME`, for the broad-place rule.
    pub fn check_exec(&self, os: Os, homes: &[&Path]) -> Result<(), ExecFault> {
        let Some(root) = self.rwx.first() else {
            return Ok(());
        };
        if os == Os::Macos {
            return Err(ExecFault::Macos);
        }
        if self.rwx.len() > 1 {
            return Err(ExecFault::ManyRwx(self.rwx.len()));
        }
        if !is_canonical_form(root) {
            return Err(ExecFault::NotCanonical(root.clone()));
        }
        if root.parent().is_none() || broad_place(root, homes) {
            return Err(ExecFault::TooBroad(root.clone()));
        }
        if let Some((rx, rw)) = writable_executable(self, os) {
            return Err(ExecFault::WritableExec { rx, rw });
        }
        Ok(())
    }

    /// Keeps only the paths for which `exists` is true: a missing folder gets
    /// no rule, and the CLI then reports its own login error.
    pub fn retain_existing(mut self, exists: impl Fn(&Path) -> bool) -> Self {
        for list in [
            &mut self.rwx,
            &mut self.rx,
            &mut self.ro,
            &mut self.rw,
            &mut self.ro_files,
            &mut self.rw_files,
            &mut self.rw_except,
        ] {
            list.retain(|p| exists(p));
        }
        self
    }
}

/// Everything the helper needs: the rules, the program and its argv.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JailArgs {
    /// Test-only: refuse to run below this ABI (exit 125).
    pub min_abi: Option<u32>,
    pub spec: JailSpec,
    pub program: PathBuf,
    pub argv: Vec<OsString>,
}

/// A helper command line that cannot be understood.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArgError(pub &'static str);

impl JailArgs {
    /// The arguments after the helper binary: `__jail-exec … -- program argv`.
    pub fn render(&self) -> Vec<OsString> {
        let mut out = vec![OsString::from(SUBCOMMAND)];
        if let Some(abi) = self.min_abi {
            out.extend([OsString::from("--min-abi"), OsString::from(abi.to_string())]);
        }
        let lists = [
            ("--rwx", &self.spec.rwx),
            ("--rx", &self.spec.rx),
            ("--ro", &self.spec.ro),
            ("--rw", &self.spec.rw),
            ("--ro-file", &self.spec.ro_files),
            ("--rw-file", &self.spec.rw_files),
            ("--rw-except", &self.spec.rw_except),
        ];
        for (flag, paths) in lists {
            for path in paths {
                out.extend([OsString::from(flag), path.clone().into_os_string()]);
            }
        }
        if self.spec.net_bind == NetBind::Open {
            out.extend([OsString::from("--net-bind"), OsString::from("open")]);
        }
        if self.spec.rlimits == Rlimits::Toolchain {
            out.extend([OsString::from("--rlimits"), OsString::from("toolchain")]);
        }
        out.extend([OsString::from("--"), self.program.clone().into_os_string()]);
        out.extend(self.argv.iter().cloned());
        out
    }
}

/// Parses what follows `__jail-exec`. Every rule path and the program must be
/// absolute, and everything after `--` is the program's own argv, verbatim.
pub fn parse_args(args: &[OsString]) -> Result<JailArgs, ArgError> {
    let mut spec = JailSpec::default();
    let mut min_abi = None;
    let mut it = args.iter();
    loop {
        let flag = it.next().ok_or(ArgError(ARG_NO_SEPARATOR))?;
        if flag == "--" {
            break;
        }
        let value = it.next().ok_or(ArgError(ARG_NO_VALUE))?;
        if flag == "--min-abi" {
            let abi = value.to_str().and_then(|v| v.parse().ok());
            min_abi = Some(abi.ok_or(ArgError(ARG_BAD_ABI))?);
            continue;
        }
        if flag == "--net-bind" {
            match value.to_str() {
                Some("open") => spec.net_bind = NetBind::Open,
                _ => return Err(ArgError(ARG_BAD_NET_BIND)),
            }
            continue;
        }
        if flag == "--rlimits" {
            match value.to_str() {
                Some("toolchain") => spec.rlimits = Rlimits::Toolchain,
                _ => return Err(ArgError(ARG_BAD_RLIMITS)),
            }
            continue;
        }
        let list = match flag.to_str() {
            Some("--rwx") => &mut spec.rwx,
            Some("--rx") => &mut spec.rx,
            Some("--ro") => &mut spec.ro,
            Some("--rw") => &mut spec.rw,
            Some("--ro-file") => &mut spec.ro_files,
            Some("--rw-file") => &mut spec.rw_files,
            Some("--rw-except") => &mut spec.rw_except,
            _ => return Err(ArgError(ARG_UNKNOWN_FLAG)),
        };
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(ArgError(ARG_RELATIVE_RULE));
        }
        list.push(path);
    }
    let program = PathBuf::from(it.next().ok_or(ArgError(ARG_NO_PROGRAM))?);
    if !program.is_absolute() {
        return Err(ArgError(ARG_RELATIVE_PROGRAM));
    }
    Ok(JailArgs {
        min_abi,
        spec,
        program,
        argv: it.cloned().collect(),
    })
}

#[cfg(test)]
mod tests;
