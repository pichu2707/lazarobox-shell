//! The jail of a toolchain run (`run_command`): which folders the program may
//! execute from, which it may write, and which are never reachable. Pure:
//! `resolve` (a canonicalizer; `None` for a path that does not exist) stands
//! in for the file system, so every rule is tested with a fake one.
//!
//! The result is a [`JailSpec`] with one `rwx` root (the project) that the
//! helper learns to render in a later slice; Landlock is an allow-list, so
//! the credential places are unreachable by omission, and [`toolchain_spec`]
//! also refuses any grant that would overlap one.

use std::path::{Component, Path, PathBuf};

use super::jail::{
    DEV_RO_FILES, DEV_RW_FILE, ETC_LINKS, JailSpec, LINUX_RO, LINUX_RO_FILES, Os, broad_place,
    is_canonical_form, push_unique, writable_executable,
};
use super::tools::allow::Program;

/// Read and execute: the system programs and libraries a toolchain starts.
const SYSTEM_RX: [&str; 5] = ["/usr", "/bin", "/lib", "/lib64", "/usr/local"];

/// Package caches under `$HOME` that are written when they exist.
const HOME_CACHES: [&str; 7] = [
    ".npm",
    ".cache/pip",
    ".cache/yarn",
    ".cache/go-build",
    ".cache/pre-commit",
    "go/pkg/mod",
    ".local/share/pnpm",
];

/// Caches under `$CARGO_HOME`.
const CARGO_CACHES: [&str; 2] = ["registry", "git"];

/// Places no run may touch, under `$HOME`.
const NEVER_IN_HOME: [&str; 4] = [".ssh", ".gnupg", ".aws", ".gitconfig"];

/// Cargo's token files, under `$CARGO_HOME`.
const NEVER_IN_CARGO: [&str; 2] = ["credentials", "credentials.toml"];

/// Where a node version manager keeps one node: the folder names to find in
/// the program's path, and how many components after them belong to the node.
const NODE_MANAGERS: [(&[&str], usize); 3] = [
    (&[".nvm", "versions", "node"], 1),
    (&[".volta"], 0),
    (&["fnm", "node-versions"], 2),
];

/// The environment variables that move a toolchain. Paths are canonical.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Vars {
    pub cargo_home: Option<PathBuf>,
    pub rustup_home: Option<PathBuf>,
    pub goroot: Option<PathBuf>,
}

/// The resolved facts about one run. Every path is canonical.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Layout {
    pub tool: Program,
    pub home: PathBuf,
    /// The project root: the one `rwx` folder.
    pub root: PathBuf,
    /// The canonical path of the program found on the scrubbed `PATH`.
    pub program: PathBuf,
    /// The private `TMPDIR` of this run.
    pub tmp: PathBuf,
    pub vars: Vars,
}

/// Why a run cannot be jailed. Every case fails closed: no spec.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Refusal {
    NotCanonical(PathBuf),
    /// The project root is `$HOME`, above it, a scratch or system folder, or a
    /// credential folder.
    Root(PathBuf),
    /// The program lives in the project, other than the pytest venv rule.
    InRoot(PathBuf),
    /// The program is outside every known toolchain place, or its place is too
    /// broad to grant.
    Program(PathBuf),
    /// A granted path equals, contains or is inside a credential place.
    Never(PathBuf),
    /// A writable path equals, contains or is inside an execute path (R29).
    WritableExec {
        rx: PathBuf,
        rw: PathBuf,
    },
}

/// Builds the rules for one run. `resolve` canonicalizes a path and is `None`
/// for one that does not exist; caches that do not exist get no rule.
pub fn toolchain_spec(
    layout: &Layout,
    resolve: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Result<JailSpec, Refusal> {
    let vars = &layout.vars;
    let given = [&layout.home, &layout.root, &layout.program, &layout.tmp]
        .into_iter()
        .chain(
            [&vars.cargo_home, &vars.rustup_home, &vars.goroot]
                .into_iter()
                .flatten(),
        );
    if let Some(bad) = given.into_iter().find(|p| !is_canonical_form(p)) {
        return Err(Refusal::NotCanonical(bad.clone()));
    }
    let real_home = resolve(&layout.home).unwrap_or_else(|| layout.home.clone());
    let homes = [layout.home.as_path(), real_home.as_path()];
    if broad_place(&layout.root, &homes) {
        return Err(Refusal::Root(layout.root.clone()));
    }
    let venv_pytest = layout.root.join(".venv/bin/pytest");
    if layout.program.starts_with(&layout.root)
        && !(layout.tool == Program::Pytest && layout.program == venv_pytest)
    {
        return Err(Refusal::InRoot(layout.program.clone()));
    }
    let cargo_home = vars
        .cargo_home
        .clone()
        .unwrap_or_else(|| layout.home.join(".cargo"));

    let mut spec = JailSpec {
        rwx: vec![layout.root.clone()],
        rx: SYSTEM_RX.map(PathBuf::from).to_vec(),
        ..JailSpec::default()
    };
    let covered =
        |spec: &JailSpec, path: &Path| spec.exec_paths().iter().any(|x| path.starts_with(x));
    let mut roots = Vec::new();
    match layout.tool {
        Program::Cargo if layout.program.starts_with(cargo_home.join("bin")) => {
            roots.push(cargo_home.join("bin"));
            roots.push(
                vars.rustup_home
                    .clone()
                    .unwrap_or_else(|| layout.home.join(".rustup")),
            );
        }
        Program::Npm | Program::Pnpm | Program::Yarn => {
            roots.extend(node_root(&layout.program, &layout.home));
        }
        // A go the system rx already covers (`/usr/bin/go`) needs no root; the
        // folder above its `bin` would be `/usr`.
        Program::Go if !covered(&spec, &layout.program) => {
            roots.extend(go_root(&layout.program, vars));
        }
        Program::Pytest if !covered(&spec, &layout.program) => {
            roots.push(pytest_root(&layout.program, resolve));
        }
        _ => {}
    }
    for root in roots {
        if broad_place(&root, &homes) {
            return Err(Refusal::Program(layout.program.clone()));
        }
        push_unique(&mut spec.rx, root);
    }
    if !covered(&spec, &layout.program) {
        return Err(Refusal::Program(layout.program.clone()));
    }

    spec.ro.extend(LINUX_RO.map(PathBuf::from));
    spec.ro_files.extend(LINUX_RO_FILES.map(PathBuf::from));
    spec.ro_files.extend(DEV_RO_FILES.map(PathBuf::from));
    for file in ETC_LINKS {
        let Some(real) = resolve(Path::new(file)) else {
            continue;
        };
        if !spec.ro.iter().any(|dir| real.starts_with(dir)) && is_canonical_form(&real) {
            push_unique(&mut spec.ro_files, real);
        }
    }
    spec.rw_files.push(PathBuf::from(DEV_RW_FILE));
    spec.rw.push(layout.tmp.clone());
    let caches = HOME_CACHES
        .map(|c| layout.home.join(c))
        .into_iter()
        .chain(CARGO_CACHES.map(|c| cargo_home.join(c)));
    for cache in caches {
        // A cache in, over or equal to an execute place would be writable and
        // executable at once (R29, as on macOS): it is dropped, and the tool
        // then runs without that cache.
        if let Some(real) = resolve(&cache)
            && !spec
                .exec_paths()
                .iter()
                .any(|x| x.starts_with(&real) || real.starts_with(x))
        {
            push_unique(&mut spec.rw, real);
        }
    }
    if let Some((rx, rw)) = writable_executable(&spec, Os::Linux) {
        return Err(Refusal::WritableExec { rx, rw });
    }

    let never: Vec<PathBuf> = NEVER_IN_HOME
        .map(|n| layout.home.join(n))
        .into_iter()
        .chain(NEVER_IN_CARGO.map(|n| cargo_home.join(n)))
        .collect();
    let granted = spec.rwx.iter().chain(&spec.rx).chain(&spec.rw);
    if let Some(bad) = granted
        .into_iter()
        .find(|g| never.iter().any(|n| g.starts_with(n) || n.starts_with(g)))
    {
        return Err(Refusal::Never(bad.clone()));
    }
    Ok(spec)
}

/// The node folder of a version manager that holds `program`, when it sits in
/// `home`.
fn node_root(program: &Path, home: &Path) -> Option<PathBuf> {
    if !program.starts_with(home) {
        return None;
    }
    let parts: Vec<Component> = program.components().collect();
    NODE_MANAGERS.iter().find_map(|(marker, extra)| {
        let at = parts
            .windows(marker.len())
            .position(|w| w.iter().zip(*marker).all(|(c, m)| c.as_os_str() == *m))?;
        let end = at + marker.len() + extra;
        (parts.len() > end).then(|| parts[..end].iter().collect())
    })
}

/// `GOROOT`, else the folder above the `bin` that holds `go`.
fn go_root(program: &Path, vars: &Vars) -> Option<PathBuf> {
    if vars.goroot.is_some() {
        return vars.goroot.clone();
    }
    let bin = program.parent()?;
    (bin.file_name()? == "bin")
        .then(|| bin.parent().map(Path::to_path_buf))
        .flatten()
}

/// The virtual environment that holds `program` when `pyvenv.cfg` sits next to
/// its `bin`, else the program file alone.
fn pytest_root(program: &Path, resolve: &dyn Fn(&Path) -> Option<PathBuf>) -> PathBuf {
    let venv = program
        .parent()
        .filter(|bin| bin.file_name().is_some_and(|n| n == "bin"))
        .and_then(Path::parent)
        .filter(|venv| resolve(&venv.join("pyvenv.cfg")).is_some());
    venv.unwrap_or(program).to_path_buf()
}

pub mod env;

#[cfg(test)]
mod tests;
