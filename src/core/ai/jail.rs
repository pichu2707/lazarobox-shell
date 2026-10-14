//! The CLI jail, as far as the chat core needs to know it: whether this
//! system can confine a CLI, and exactly which paths a confined CLI may
//! touch. Everything here is pure; the probe and the helper that applies the
//! rules live in `runtime::jail`.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use crate::core::ai::chat::engine::ChatError;

/// Landlock ABI 3 (Linux 6.2) is the first with TRUNCATE; below it a jail
/// would leave a hole, so it counts as no jail.
pub const MIN_ABI: u32 = 3;

/// The helper's exit status when the rules could not be fully enforced.
pub const NOT_ENFORCED_EXIT: i32 = 125;

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

/// The jail error a helper exit status stands for, if any.
pub fn exit_error(code: Option<i32>) -> Option<ChatError> {
    (code == Some(NOT_ENFORCED_EXIT)).then_some(ChatError::Jail)
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
        // The package folder must sit above the binary, not be the binary.
        if after.len() > name {
            return Some(parts[..=at + name].iter().collect());
        }
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
];

/// Whether granting `path` would open far more than one CLI's own files:
/// `$HOME` or one of its parents, `/tmp`, `~/.ssh` or a system folder.
fn too_broad(path: &Path, home: &Path) -> bool {
    home.starts_with(path)
        || path.starts_with("/tmp")
        || path.starts_with("/var/tmp")
        || path.starts_with(home.join(".ssh"))
        || BROAD.iter().any(|b| path == Path::new(b))
}

/// Why a layout cannot be jailed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SpecError {
    /// A package root or state dir that would grant a whole broad folder.
    RootTooBroad(PathBuf),
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

/// Builds the rules for one CLI. Pure: nothing is checked on disk, so the
/// caller drops the paths that do not exist with [`JailSpec::retain_existing`].
pub fn jail_spec(layout: &CliLayout) -> Result<JailSpec, SpecError> {
    let root = package_root(&layout.binary)
        .ok_or_else(|| SpecError::RootTooBroad(layout.binary.clone()))?;
    if too_broad(&root, &layout.home) {
        return Err(SpecError::RootTooBroad(root));
    }
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
    if too_broad(&state, &layout.home) {
        return Err(SpecError::RootTooBroad(state));
    }

    let mut spec = JailSpec::default();
    for path in layout
        .interpreter
        .iter()
        .chain([&root])
        .chain(&layout.helpers)
    {
        push_unique(&mut spec.rx, path.clone());
    }
    for dir in [
        "/usr",
        "/bin",
        "/lib",
        "/lib64",
        "/etc",
        "/proc/self",
        "/sys/fs/cgroup",
    ] {
        spec.ro.push(PathBuf::from(dir));
    }
    push_unique(&mut spec.ro, layout.cwd.clone());
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
