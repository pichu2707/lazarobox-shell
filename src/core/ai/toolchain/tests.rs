use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::*;
use crate::core::ai::jail::Os;
use crate::core::ai::tools::allow::Program;

const HOME: &str = "/home/u";
const ROOT: &str = "/home/u/proj";
const TMP: &str = "/run/lb/tmp/1";

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn paths(items: &[&str]) -> Vec<PathBuf> {
    items.iter().map(|s| p(s)).collect()
}

/// A fake file system: only the listed paths exist and none is a link.
fn fs(existing: &[&str]) -> impl Fn(&Path) -> Option<PathBuf> {
    let set: HashSet<PathBuf> = existing.iter().map(|s| p(s)).collect();
    move |path| set.contains(path).then(|| path.to_path_buf())
}

fn layout(tool: Program, program: &str) -> Layout {
    Layout {
        tool,
        home: p(HOME),
        root: p(ROOT),
        program: p(program),
        tmp: p(TMP),
        vars: Vars::default(),
    }
}

const SYSTEM: [&str; 5] = ["/usr", "/bin", "/lib", "/lib64", "/usr/local"];

fn build(layout: &Layout, existing: &[&str]) -> Result<JailSpec, Refusal> {
    toolchain_spec(layout, &fs(existing))
}

fn system_plus(extra: &[&str]) -> Vec<PathBuf> {
    paths(&SYSTEM).into_iter().chain(paths(extra)).collect()
}

fn every_path(spec: &JailSpec) -> Vec<&PathBuf> {
    [
        &spec.rwx,
        &spec.rx,
        &spec.ro,
        &spec.rw,
        &spec.ro_files,
        &spec.rw_files,
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[test]
fn cargo_gets_its_bin_and_rustup_home_and_the_registry_caches() {
    let l = layout(Program::Cargo, "/home/u/.cargo/bin/rustup");
    let exist = [
        "/home/u/.cargo/registry",
        "/home/u/.cargo/git",
        "/home/u/.npm",
    ];
    let spec = build(&l, &exist).unwrap();
    assert_eq!(spec.rwx, paths(&[ROOT]));
    assert_eq!(
        spec.rx,
        system_plus(&["/home/u/.cargo/bin", "/home/u/.rustup"])
    );
    assert_eq!(
        spec.rw,
        paths(&[
            TMP,
            "/home/u/.npm",
            "/home/u/.cargo/registry",
            "/home/u/.cargo/git"
        ])
    );
    assert_eq!(spec.check_exec(Os::Linux, &[]), Ok(()));
}

#[test]
fn cargo_home_and_rustup_home_follow_the_environment() {
    let mut l = layout(Program::Cargo, "/opt/c/bin/rustup");
    l.vars.cargo_home = Some(p("/opt/c"));
    l.vars.rustup_home = Some(p("/opt/r"));
    let spec = build(&l, &["/opt/c/registry"]).unwrap();
    assert_eq!(spec.rx, system_plus(&["/opt/c/bin", "/opt/r"]));
    assert_eq!(spec.rw, paths(&[TMP, "/opt/c/registry"]));
}

#[test]
fn a_system_cargo_adds_no_toolchain_root() {
    let spec = build(&layout(Program::Cargo, "/usr/bin/cargo"), &[]).unwrap();
    assert_eq!(spec.rx, system_plus(&[]));
}

#[test]
fn the_node_root_of_nvm_volta_and_fnm_is_executable() {
    let cases = [
        (
            "/home/u/.nvm/versions/node/v20.1.0/lib/node_modules/npm/bin/npm-cli.js",
            "/home/u/.nvm/versions/node/v20.1.0",
        ),
        (
            "/home/u/.volta/tools/image/node/20.1.0/bin/node",
            "/home/u/.volta",
        ),
        (
            "/home/u/.local/share/fnm/node-versions/v20.1.0/installation/lib/node_modules/pnpm/bin/pnpm.cjs",
            "/home/u/.local/share/fnm/node-versions/v20.1.0/installation",
        ),
    ];
    for (program, root) in cases {
        let spec = build(&layout(Program::Pnpm, program), &["/home/u/.npm"]).unwrap();
        assert_eq!(spec.rx, system_plus(&[root]), "{program}");
        assert_eq!(spec.rw, paths(&[TMP, "/home/u/.npm"]), "{program}");
    }
}

#[test]
fn a_node_outside_the_known_managers_is_refused() {
    let program = "/home/u/Downloads/node/bin/npm";
    assert_eq!(
        build(&layout(Program::Npm, program), &[]),
        Err(Refusal::Program(p(program)))
    );
}

#[test]
fn go_runs_from_goroot_or_from_the_folder_above_its_bin() {
    let mut l = layout(Program::Go, "/home/u/sdk/go1.22/bin/go");
    let exist = ["/home/u/go/pkg/mod", "/home/u/.cache/go-build"];
    let spec = build(&l, &exist).unwrap();
    assert_eq!(spec.rx, system_plus(&["/home/u/sdk/go1.22"]));
    assert_eq!(
        spec.rw,
        paths(&[TMP, "/home/u/.cache/go-build", "/home/u/go/pkg/mod"])
    );
    l.vars.goroot = Some(p("/home/u/sdk/go1.22"));
    let spec = build(&l, &[]).unwrap();
    assert_eq!(spec.rx, system_plus(&["/home/u/sdk/go1.22"]));
    l.program = p("/home/u/sdk/go1.22/bin/go");
    l.vars.goroot = Some(p("/home/u/sdk/other"));
    assert_eq!(build(&l, &[]), Err(Refusal::Program(l.program.clone())));
    // Under the system rx already: no root of its own.
    let usr = build(&layout(Program::Go, "/usr/local/go/bin/go"), &[]).unwrap();
    assert_eq!(usr.rx, system_plus(&[]));
}

#[test]
fn pytest_runs_from_a_venv_outside_the_root_or_its_own_file() {
    let venv = "/home/u/.local/pipx/venvs/pytest";
    let l = layout(
        Program::Pytest,
        "/home/u/.local/pipx/venvs/pytest/bin/pytest",
    );
    let spec = build(&l, &["/home/u/.local/pipx/venvs/pytest/pyvenv.cfg"]).unwrap();
    assert_eq!(spec.rx, system_plus(&[venv]));
    let user = layout(Program::Pytest, "/home/u/.local/bin/pytest");
    assert_eq!(
        build(&user, &[]).unwrap().rx,
        system_plus(&["/home/u/.local/bin/pytest"])
    );
    let system = build(&layout(Program::Pytest, "/usr/bin/pytest"), &[]).unwrap();
    assert_eq!(system.rx, system_plus(&[]));
}

// #775: the python3 the PATH resolves to gets pytest's rules and no venv
// handling (`find` canonicalizes, so a venv `bin/python3` never reaches here as
// a venv path). Outside the system rx paths only a self-contained install
// works (see below).
#[test]
fn python3_gets_the_system_rules_and_no_venv_handling() {
    let system = build(&layout(Program::Python3, "/usr/bin/python3"), &[]).unwrap();
    assert_eq!(system.rx, system_plus(&[]));
    // Outside the system paths, with no standard library found beside it, it
    // is refused up front, naming the interpreter (#794). Pytest keeps its
    // own rule.
    for program in [
        "/home/u/.pyenv/versions/3.12/bin/python3",
        "/home/linuxbrew/.linuxbrew/bin/python3",
        "/opt/python/bin/python3",
    ] {
        assert_eq!(
            build(&layout(Program::Python3, program), &[]),
            Err(Refusal::Interpreter(p(program))),
            "{program}"
        );
    }
    for program in ["/home/u/proj/.venv/bin/python3", "/home/u/proj/bin/python3"] {
        assert_eq!(
            build(&layout(Program::Python3, program), &[]),
            Err(Refusal::InRoot(p(program))),
            "{program}"
        );
    }
}

// #794 (option A, self-contained installs only): the folder above the `bin`
// that holds `python3.X` is granted when its own standard library sits in
// `lib/python3.X`, as pyenv, uv and python.org builds have it.
#[test]
fn a_self_contained_python3_gets_its_prefix() {
    for (program, prefix) in [
        (
            "/home/u/.pyenv/versions/3.12.1/bin/python3.12",
            "/home/u/.pyenv/versions/3.12.1",
        ),
        (
            "/home/u/.local/share/uv/python/cpython-3.12.4-linux-x86_64-gnu/bin/python3.12",
            "/home/u/.local/share/uv/python/cpython-3.12.4-linux-x86_64-gnu",
        ),
        ("/opt/python/3.13/bin/python3.13t", "/opt/python/3.13"),
    ] {
        let name = Path::new(program).file_name().unwrap().to_str().unwrap();
        let landmark = format!("{prefix}/lib/{name}/os.py");
        let spec = build(&layout(Program::Python3, program), &[&landmark]).unwrap();
        assert_eq!(spec.rx, system_plus(&[prefix]), "{program}");
        assert!(
            spec.rw.iter().all(|rw| !rw.starts_with(prefix)),
            "{program}"
        );
    }
}

#[test]
fn a_python3_that_is_not_self_contained_keeps_the_refusal() {
    let refused = |program: &str, existing: &[&str]| {
        assert_eq!(
            build(&layout(Program::Python3, program), existing),
            Err(Refusal::Interpreter(p(program))),
            "{program}"
        );
    };
    // No standard library beside it.
    refused("/home/u/.pyenv/versions/3.12.1/bin/python3.12", &[]);
    // A name without its version: no landmark to look for.
    refused(
        "/opt/python/bin/python3",
        &["/opt/python/lib/python3/os.py"],
    );
    // Not in a `bin` folder.
    refused("/opt/python/python3.12", &["/opt/lib/python3.12/os.py"]);
    // A Homebrew keg holds its stdlib but not its site-packages (#794).
    let keg = "/home/linuxbrew/.linuxbrew/Cellar/python@3.13/3.13.1";
    refused(
        &format!("{keg}/bin/python3.13"),
        &[&format!("{keg}/lib/python3.13/os.py")],
    );
    // A prefix too broad to grant: the home folder itself.
    refused("/home/u/bin/python3.12", &["/home/u/lib/python3.12/os.py"]);
}

#[test]
fn only_the_root_venv_pytest_may_live_in_the_root() {
    let spec = build(
        &layout(Program::Pytest, "/home/u/proj/.venv/bin/pytest"),
        &[],
    )
    .unwrap();
    assert_eq!(spec.rx, system_plus(&[]));
    for (tool, program) in [
        (Program::Pytest, "/home/u/proj/bin/pytest"),
        (Program::Pytest, "/home/u/proj/.venv/bin/other"),
        (Program::Npm, "/home/u/proj/node_modules/.bin/npm"),
        (Program::Cargo, "/home/u/proj/target/debug/cargo"),
    ] {
        assert_eq!(
            build(&layout(tool, program), &[]),
            Err(Refusal::InRoot(p(program))),
            "{program}"
        );
    }
}

#[test]
fn a_broad_or_relative_root_is_refused() {
    for root in [
        "/home/u",
        "/tmp/proj",
        "/",
        "/home/u/.ssh",
        "/home/u/.claude/x",
    ] {
        let mut l = layout(Program::Cargo, "/usr/bin/cargo");
        l.root = p(root);
        assert_eq!(build(&l, &[]), Err(Refusal::Root(p(root))), "{root}");
    }
    let mut l = layout(Program::Cargo, "/usr/bin/cargo");
    l.root = p("proj");
    assert_eq!(build(&l, &[]), Err(Refusal::NotCanonical(p("proj"))));
}

#[test]
fn a_root_over_a_credential_file_is_refused() {
    let mut l = layout(Program::Cargo, "/usr/bin/cargo");
    for root in ["/home/u/.cargo", "/home/u/.cargo/credentials.toml"] {
        l.root = p(root);
        assert_eq!(build(&l, &[]), Err(Refusal::Never(p(root))), "{root}");
    }
}

#[test]
fn no_class_ever_reaches_a_credential_place_or_home() {
    let l = layout(Program::Cargo, "/home/u/.cargo/bin/rustup");
    let exist = ["/home/u/.cargo/registry", "/home/u/.cargo/git"];
    let spec = build(&l, &exist).unwrap();
    let never = [
        "/home/u/.ssh",
        "/home/u/.gnupg",
        "/home/u/.aws",
        "/home/u/.cargo/credentials",
        "/home/u/.cargo/credentials.toml",
        "/home/u/.gitconfig",
    ];
    for granted in every_path(&spec) {
        assert!(!Path::new(HOME).starts_with(granted), "{granted:?}");
        for bad in never {
            let bad = Path::new(bad);
            assert!(
                !granted.starts_with(bad) && !bad.starts_with(granted),
                "{granted:?} vs {bad:?}"
            );
        }
    }
}

#[test]
fn the_system_files_match_the_cli_jail() {
    let spec = build(&layout(Program::Cargo, "/usr/bin/cargo"), &[]).unwrap();
    assert!(spec.ro.contains(&p("/etc")));
    assert!(spec.ro_files.contains(&p("/dev/urandom")));
    assert_eq!(spec.rw_files, paths(&["/dev/null"]));
}

#[test]
fn a_cache_inside_an_execute_place_is_dropped_not_made_writable() {
    // The Docker rust layout: `/usr/local` is in the system rx and the cargo
    // home lives under it, so its registry would be writable AND executable.
    let mut l = layout(Program::Cargo, "/usr/local/cargo/bin/cargo");
    l.vars.cargo_home = Some(p("/usr/local/cargo"));
    l.vars.rustup_home = Some(p("/usr/local/rustup"));
    // The credential files of that home sit inside the system rx, which
    // Landlock cannot carve out: refused, never a writable+executable cache.
    let exist = ["/usr/local/cargo/registry", "/usr/local/cargo/git"];
    assert_eq!(build(&l, &exist), Err(Refusal::Never(p("/usr"))));
    // Go: `~/go` is the goroot of a go in `~/go/bin`, and holds `pkg/mod`.
    let l = layout(Program::Go, "/home/u/go/bin/go");
    let spec = build(&l, &["/home/u/go/pkg/mod", "/home/u/.cache/go-build"]).unwrap();
    assert_eq!(spec.rx, system_plus(&["/home/u/go"]));
    assert_eq!(spec.rw, paths(&[TMP, "/home/u/.cache/go-build"]));
    // An rwx root over a cache already writes it: nothing to grant twice.
    let mut l = layout(Program::Go, "/usr/bin/go");
    l.root = p("/home/u/go");
    let spec = build(&l, &["/home/u/go/pkg/mod"]).unwrap();
    assert_eq!(spec.rw, paths(&[TMP]));
}

#[test]
fn a_private_tmp_inside_an_execute_place_is_refused() {
    let mut l = layout(Program::Cargo, "/usr/bin/cargo");
    l.tmp = p("/usr/local/tmp/1");
    assert_eq!(
        build(&l, &[]),
        Err(Refusal::WritableExec {
            rx: p("/usr"),
            rw: p("/usr/local/tmp/1"),
        })
    );
}

#[test]
fn a_system_go_is_covered_by_the_system_rx() {
    let spec = build(&layout(Program::Go, "/usr/bin/go"), &[]).unwrap();
    assert_eq!(spec.rx, system_plus(&[]));
    let spec = build(&layout(Program::Go, "/usr/local/go/bin/go"), &[]).unwrap();
    assert_eq!(spec.rx, system_plus(&[]));
}
