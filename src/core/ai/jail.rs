//! The CLI jail, as far as the chat core needs to know it: whether this
//! system can confine a CLI, and exactly which paths a confined CLI may
//! touch. Everything here is pure; the probe and the helper that applies the
//! rules live in `runtime::jail`.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use crate::core::ai::chat::engine::ChatError;
use crate::core::ai::parse::sanitize as one_line;

pub mod rpath;

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
/// and gemini stay refused on macOS.
pub const SEATBELT_ENABLED: bool = false;

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
    NotLinux,
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
            Self::NotLinux => "the CLI jail needs Linux".to_owned(),
            Self::NotProbed => "the jail was not probed".to_owned(),
        }
    }

    /// A few words for a menu row.
    pub fn short(self) -> String {
        match self {
            Self::NoLandlock(0) => "no Landlock".to_owned(),
            Self::NoLandlock(abi) => format!("Landlock ABI {abi}, needs {MIN_ABI}+"),
            Self::NoSeatbelt(_) => "jail unavailable".to_owned(),
            Self::NotLinux => "needs Linux".to_owned(),
            Self::NotProbed => "not probed".to_owned(),
        }
    }

    /// [`Self::text`] for a CLI that does not run without the jail (codex,
    /// gemini).
    pub fn text_required(self) -> String {
        match self {
            Self::NotLinux | Self::NoSeatbelt(_) => format!(
                "{}; on this system use claude or Ollama for chat",
                self.text()
            ),
            _ => self.text(),
        }
    }

    /// [`Self::short`] for a CLI that does not run without the jail.
    pub fn short_required(self) -> String {
        match self {
            Self::NotLinux => "needs Linux · use claude".to_owned(),
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
    [JailOff::NoLandlock(0), JailOff::NotLinux]
        .into_iter()
        .chain(macos)
        .map(JailOff::text)
        .find(|text| text == said)
}

/// The jail error a finished helper stands for, if any: the status is 125 AND
/// a stderr line starts with [`NOT_ENFORCED_MARKER`]. It carries the helper's
/// reason only when it is one of the fixed texts the helper emits; any other
/// text gives an empty reason (the generic message).
pub fn exit_error(code: Option<i32>, stderr: &str) -> Option<ChatError> {
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

/// Files under `/etc` that name resolution and TLS clocks need, and that a
/// distribution may link to another place.
const ETC_LINKS: [&str; 4] = [
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
/// mount folder.
fn too_broad(path: &Path, homes: &[&Path], cli: Cli) -> bool {
    is_scratch(path)
        || BROAD.iter().any(|b| path == Path::new(b))
        || homes.iter().any(|home| too_broad_for_home(path, home, cli))
}

fn too_broad_for_home(path: &Path, home: &Path, cli: Cli) -> bool {
    home.starts_with(path)
        || path.starts_with(home.join(".ssh"))
        || path.starts_with(home.join(".gnupg"))
        || CREDENTIAL_DIRS
            .iter()
            .filter(|d| **d != cli.own_dir())
            .any(|d| path.starts_with(home.join(d)))
        || SHARED_IN_HOME.iter().any(|d| path == home.join(d))
}

/// Why a layout cannot be jailed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SpecError {
    /// A package root or state dir that would grant a whole broad folder.
    RootTooBroad(PathBuf),
    /// A path that is not absolute or holds a `.` or `..` segment. `jail_spec`
    /// takes canonical paths: a lexical check on `a/../b` would be fooled.
    NotCanonical(PathBuf),
}

fn is_canonical_form(path: &Path) -> bool {
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
    /// Read and execute: single files, or the CLI's own package folder.
    pub rx: Vec<PathBuf>,
    pub ro: Vec<PathBuf>,
    pub rw: Vec<PathBuf>,
    pub ro_files: Vec<PathBuf>,
    pub rw_files: Vec<PathBuf>,
}

fn push_unique(list: &mut Vec<PathBuf>, path: PathBuf) {
    if !list.contains(&path) {
        list.push(path);
    }
}

/// Builds the rules for one CLI from canonical paths, taking the state dirs
/// as written (`~/.claude`, `$CODEX_HOME`, …). Use [`jail_spec_with`] with a
/// real canonicalizer so a symlinked state dir cannot reach a broad folder.
#[cfg(test)]
pub(crate) fn jail_spec(layout: &CliLayout) -> Result<JailSpec, SpecError> {
    jail_spec_with(layout, &|path| Some(path.to_path_buf()))
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
    if too_broad(&root, &homes, layout.cli) {
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
        .find(|p| too_broad(p, &homes, layout.cli))
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
    let state_file = state_file.map(resolved);
    for path in [Some(&state), state_file.as_ref()].into_iter().flatten() {
        if too_broad(path, &homes, layout.cli) {
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
    // Libraries and data only. The folders that hold programs (`/usr/bin`,
    // `/usr/sbin`, `/bin`) stay unreadable: Landlock does not gate
    // `mmap(PROT_EXEC)`, so the ELF loader that some CLI needs to start could
    // otherwise run `ld.so /usr/bin/curl`. `/bin` is a link to `/usr/bin` on
    // most systems, which is why it is not listed. `/lib` and `/lib64` are
    // libraries (and links to `/usr/lib`).
    for dir in [
        "/usr/lib",
        "/usr/share",
        "/lib",
        "/lib64",
        "/etc",
        "/proc/self",
        "/sys/fs/cgroup",
    ] {
        spec.ro.push(PathBuf::from(dir));
    }
    push_unique(&mut spec.ro, layout.cwd.clone());
    // The interpreter's own library folders, read-only. A folder the system
    // rules already cover adds nothing, and a broad one is dropped rather than
    // granted: the program then reports the library it cannot load.
    let system = spec.ro.clone();
    for dir in &layout.lib_dirs {
        let covered = system.iter().any(|s| dir.starts_with(s));
        if !covered && !too_broad(dir, &homes, layout.cli) {
            push_unique(&mut spec.ro, dir.clone());
        }
    }
    push_unique(&mut spec.rw, state);
    push_unique(&mut spec.rw, layout.tmp.clone());
    for file in [
        "/proc/cpuinfo",
        "/proc/meminfo",
        "/proc/stat",
        "/dev/urandom",
        "/dev/random",
    ] {
        spec.ro_files.push(PathBuf::from(file));
    }
    // System files that are often links out of `/etc`: WSL's resolv.conf is
    // `/mnt/wsl/resolv.conf`, systemd-resolved's lives under `/run`. Landlock
    // checks the target, so `/etc` alone does not cover them and DNS fails
    // with EAI_AGAIN. Grant the exact target file, never its folder.
    let readable = spec.ro.clone();
    for file in ETC_LINKS {
        let Some(real) = resolve(Path::new(file)) else {
            continue;
        };
        let covered = readable.iter().any(|dir| real.starts_with(dir));
        if !covered && is_canonical_form(&real) && !too_broad(&real, &homes, layout.cli) {
            push_unique(&mut spec.ro_files, real);
        }
    }
    for file in &layout.data_files {
        if !too_broad(file, &homes, layout.cli) {
            push_unique(&mut spec.ro_files, file.clone());
        }
    }
    spec.rw_files.push(PathBuf::from("/dev/null"));
    spec.rw_files.extend(state_file);
    if let Some(shim) = &layout.shim {
        push_unique(&mut spec.rx, shim.binary.clone());
        if let Some(loader) = &shim.loader {
            push_unique(&mut spec.rx, loader.clone());
        }
        push_unique(&mut spec.rw_files, shim.socket.clone());
    }
    Ok(spec)
}

impl JailSpec {
    /// Keeps only the paths for which `exists` is true: a missing folder gets
    /// no rule, and the CLI then reports its own login error.
    pub fn retain_existing(mut self, exists: impl Fn(&Path) -> bool) -> Self {
        for list in [
            &mut self.rx,
            &mut self.ro,
            &mut self.rw,
            &mut self.ro_files,
            &mut self.rw_files,
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
            ("--rx", &self.spec.rx),
            ("--ro", &self.spec.ro),
            ("--rw", &self.spec.rw),
            ("--ro-file", &self.spec.ro_files),
            ("--rw-file", &self.spec.rw_files),
        ];
        for (flag, paths) in lists {
            for path in paths {
                out.extend([OsString::from(flag), path.clone().into_os_string()]);
            }
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
        let flag = it.next().ok_or(ArgError("missing `--`"))?;
        if flag == "--" {
            break;
        }
        let value = it.next().ok_or(ArgError("a flag without a value"))?;
        if flag == "--min-abi" {
            let abi = value.to_str().and_then(|v| v.parse().ok());
            min_abi = Some(abi.ok_or(ArgError("--min-abi is not a number"))?);
            continue;
        }
        let list = match flag.to_str() {
            Some("--rx") => &mut spec.rx,
            Some("--ro") => &mut spec.ro,
            Some("--rw") => &mut spec.rw,
            Some("--ro-file") => &mut spec.ro_files,
            Some("--rw-file") => &mut spec.rw_files,
            _ => return Err(ArgError("unknown flag")),
        };
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(ArgError("rule paths must be absolute"));
        }
        list.push(path);
    }
    let program = PathBuf::from(it.next().ok_or(ArgError("no program after `--`"))?);
    if !program.is_absolute() {
        return Err(ArgError("the program must be an absolute path"));
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
