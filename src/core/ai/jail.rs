//! The CLI jail, as far as the chat core needs to know it: whether this
//! system can confine a CLI, and exactly which paths a confined CLI may
//! touch. Everything here is pure; the probe and the helper that applies the
//! rules live in `runtime::jail`.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use crate::core::ai::chat::engine::ChatError;

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
}

/// Whether this system can jail a CLI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JailSupport {
    Available { backend: Backend, abi: u32 },
    Unavailable(&'static str),
}

impl JailSupport {
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

/// Maps the ABI the kernel reported (0 when Landlock is absent).
pub fn support_from_abi(abi: u32) -> JailSupport {
    match abi {
        0 => JailSupport::Unavailable("Landlock is not enabled in this kernel"),
        1..MIN_ABI => JailSupport::Unavailable("Landlock needs ABI 3 or newer (Linux 6.2)"),
        _ => JailSupport::Available {
            backend: Backend::Landlock,
            abi,
        },
    }
}

/// What a CLI does without a jail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JailPolicy {
    /// codex and gemini: no jail, no process.
    Required,
    /// claude: its flags alone are text-only, the jail is a second layer.
    Layered,
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
        (_, true) => Launch::Jailed,
        (JailPolicy::Layered, false) => Launch::Bare,
        (JailPolicy::Required, false) => Launch::Refuse,
    }
}

/// The jail error a finished helper stands for, if any: the status is 125 AND
/// a stderr line starts with [`NOT_ENFORCED_MARKER`].
pub fn exit_error(code: Option<i32>, stderr: &str) -> Option<ChatError> {
    let marked = stderr
        .lines()
        .any(|line| line.starts_with(NOT_ENFORCED_MARKER));
    (code == Some(NOT_ENFORCED_EXIT) && marked).then_some(ChatError::Jail)
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

/// Whether granting `path` to `cli` would open far more than its own files:
/// `$HOME` or one of its parents, `/tmp`, `~/.ssh`, `~/.gnupg`, another
/// tool's credential folder, a shared data folder of `$HOME` or a system or
/// mount folder.
fn too_broad(path: &Path, homes: &[&Path], cli: Cli) -> bool {
    path.starts_with("/tmp")
        || path.starts_with("/var/tmp")
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
    /// The private empty working directory.
    pub cwd: PathBuf,
    /// The private per-turn `TMPDIR`.
    pub tmp: PathBuf,
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
        .chain(&layout.helpers);
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
    for file in &layout.data_files {
        if !too_broad(file, &homes, layout.cli) {
            push_unique(&mut spec.ro_files, file.clone());
        }
    }
    spec.rw_files.push(PathBuf::from("/dev/null"));
    spec.rw_files.extend(state_file);
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
