use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::*;
use crate::core::ai::chat::engine::ChatError;

const HOME: &str = "/home/u";

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn paths(items: &[&str]) -> Vec<PathBuf> {
    items.iter().map(|s| p(s)).collect()
}

/// A layout whose private dirs live outside `$HOME` and `/tmp`, so the
/// "nothing under $HOME" property sees only the CLI's own paths.
fn layout(cli: Cli, binary: &str, interpreter: Option<&str>) -> CliLayout {
    CliLayout {
        cli,
        home: p(HOME),
        codex_home: None,
        binary: p(binary),
        interpreter: interpreter.map(p),
        helpers: Vec::new(),
        cwd: p("/run/user/1000/lazarobox/cli-cwd"),
        tmp: p("/run/user/1000/lazarobox/cli-tmp/1-1"),
    }
}

fn nvm_codex() -> CliLayout {
    layout(
        Cli::Codex,
        "/home/u/.nvm/versions/node/v24.16.0/lib/node_modules/@openai/codex/bin/codex.js",
        Some("/home/u/.nvm/versions/node/v24.16.0/bin/node"),
    )
}

fn native_claude() -> CliLayout {
    layout(
        Cli::Claude,
        "/home/u/.local/share/claude/versions/2.1.299",
        None,
    )
}

fn brew_gemini() -> CliLayout {
    layout(
        Cli::Gemini,
        "/home/linuxbrew/.linuxbrew/lib/node_modules/@google/gemini-cli/bundle/gemini.js",
        Some("/home/linuxbrew/.linuxbrew/Cellar/node/26.8.1/bin/node"),
    )
}

fn every_path(spec: &JailSpec) -> Vec<&Path> {
    [&spec.rx, &spec.ro, &spec.rw, &spec.ro_files, &spec.rw_files]
        .into_iter()
        .flatten()
        .map(PathBuf::as_path)
        .collect()
}

// ---- probe mapping and policy ----

#[test]
fn abi_below_three_is_unavailable() {
    for abi in [0, 1, 2] {
        assert!(!support_from_abi(abi).is_available(), "abi {abi}");
    }
    match support_from_abi(2) {
        JailSupport::Unavailable(reason) => assert!(reason.contains("ABI 3"), "{reason}"),
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn abi_three_or_more_is_available_with_its_abi() {
    for abi in [3, 6, 7, 12] {
        assert_eq!(
            support_from_abi(abi),
            JailSupport::Available {
                backend: Backend::Landlock,
                abi
            },
        );
    }
}

#[test]
fn policy_decides_how_a_cli_starts() {
    let on = support_from_abi(6);
    let off = support_from_abi(2);
    assert_eq!(launch(JailPolicy::Required, on), Launch::Jailed);
    assert_eq!(launch(JailPolicy::Required, off), Launch::Refuse);
    assert_eq!(launch(JailPolicy::Layered, on), Launch::Jailed);
    assert_eq!(launch(JailPolicy::Layered, off), Launch::Bare);
}

#[test]
fn helper_exit_125_is_a_jail_error() {
    assert_eq!(exit_error(Some(125)), Some(ChatError::Jail));
    assert_eq!(exit_error(Some(1)), None);
    assert_eq!(exit_error(Some(0)), None);
    assert_eq!(exit_error(None), None);
}

// ---- shebang ----

#[test]
fn shebang_env_node_is_looked_up_on_path() {
    assert_eq!(
        parse_shebang(b"#!/usr/bin/env node\nconsole.log(1)\n"),
        Some(Interpreter::OnPath("node".into())),
    );
    assert_eq!(
        parse_shebang(b"#!/usr/bin/env -S node --no-warnings\n"),
        Some(Interpreter::OnPath("node".into())),
    );
    assert_eq!(
        parse_shebang(b"#!/bin/env PATH=/x python3\n"),
        Some(Interpreter::OnPath("python3".into())),
    );
}

#[test]
fn shebang_direct_path_is_kept_exact() {
    assert_eq!(
        parse_shebang(b"#!/usr/bin/node --harmony\n"),
        Some(Interpreter::Path(p("/usr/bin/node"))),
    );
    assert_eq!(
        parse_shebang(b"#!/opt/x/bin/py"),
        Some(Interpreter::Path(p("/opt/x/bin/py"))),
    );
}

#[test]
fn shebang_rejects_what_is_not_a_clean_script_line() {
    assert_eq!(parse_shebang(b"\x7fELF\x02\x01\x01"), None);
    assert_eq!(parse_shebang(b"#!node\n"), None);
    assert_eq!(parse_shebang(b"#!\n"), None);
    assert_eq!(parse_shebang(b"#!/usr/bin/env\n"), None);
    assert_eq!(parse_shebang(b"#!/usr/bin/env node\xff\n"), None);
    assert_eq!(parse_shebang(b""), None);
}

#[test]
fn shebang_only_looks_at_the_first_256_bytes() {
    let mut long = b"#!/usr/bin/env ".to_vec();
    long.extend(std::iter::repeat_n(b'a', 400));
    long.extend(b"\n");
    assert_eq!(parse_shebang(&long), None);

    // The newline sits exactly at byte 255: the line fits.
    let mut fits = b"#!/usr/bin/env ".to_vec();
    fits.extend(std::iter::repeat_n(b'a', 255 - fits.len()));
    fits.extend(b"\nrest");
    let name = "a".repeat(255 - 15);
    assert_eq!(parse_shebang(&fits), Some(Interpreter::OnPath(name)));
}

// ---- package root ----

#[test]
fn package_root_is_the_node_modules_package() {
    assert_eq!(
        package_root(&p("/h/.nvm/v/lib/node_modules/@openai/codex/bin/codex.js")),
        Some(p("/h/.nvm/v/lib/node_modules/@openai/codex")),
    );
    assert_eq!(
        package_root(&p("/usr/lib/node_modules/gemini/bundle/gemini.js")),
        Some(p("/usr/lib/node_modules/gemini")),
    );
}

#[test]
fn package_root_uses_the_innermost_node_modules() {
    assert_eq!(
        package_root(&p("/a/node_modules/outer/node_modules/@s/inner/bin/x")),
        Some(p("/a/node_modules/outer/node_modules/@s/inner")),
    );
}

#[test]
fn package_root_without_node_modules_is_the_parent_dir() {
    assert_eq!(
        package_root(&p("/home/u/.local/share/claude/versions/2.1.299")),
        Some(p("/home/u/.local/share/claude/versions")),
    );
    // A file straight under node_modules has no package dir above it.
    assert_eq!(
        package_root(&p("/a/node_modules/tool")),
        Some(p("/a/node_modules")),
    );
}

#[test]
fn package_root_of_the_filesystem_root_is_none() {
    assert_eq!(package_root(&p("/")), None);
    assert_eq!(package_root(&p("tool")), None);
}

// ---- jail_spec ----

#[test]
fn nvm_node_layout_grants_execute_to_exactly_node_and_the_package() {
    let spec = jail_spec(&nvm_codex()).unwrap();
    assert_eq!(
        spec.rx,
        paths(&[
            "/home/u/.nvm/versions/node/v24.16.0/bin/node",
            "/home/u/.nvm/versions/node/v24.16.0/lib/node_modules/@openai/codex",
        ]),
    );
    assert_eq!(
        spec.rw,
        paths(&["/home/u/.codex", "/run/user/1000/lazarobox/cli-tmp/1-1"])
    );
    assert!(spec.rw_files.contains(&p("/dev/null")));
}

#[test]
fn native_claude_layout_has_no_interpreter_and_keeps_its_config_file() {
    let spec = jail_spec(&native_claude()).unwrap();
    assert_eq!(spec.rx, paths(&["/home/u/.local/share/claude/versions"]));
    assert!(spec.rw.contains(&p("/home/u/.claude")));
    assert!(spec.rw_files.contains(&p("/home/u/.claude.json")));
    assert!(!spec.rw.contains(&p("/home/u/.codex")));
}

#[test]
fn codex_home_override_replaces_the_default_state_dir() {
    let mut l = nvm_codex();
    l.codex_home = Some(p("/srv/codex-state"));
    let spec = jail_spec(&l).unwrap();
    assert!(spec.rw.contains(&p("/srv/codex-state")));
    assert!(!spec.rw.contains(&p("/home/u/.codex")));
}

#[test]
fn codex_home_only_applies_to_codex() {
    let mut l = brew_gemini();
    l.codex_home = Some(p("/srv/codex-state"));
    let spec = jail_spec(&l).unwrap();
    assert!(spec.rw.contains(&p("/home/u/.gemini")));
    assert!(!every_path(&spec).contains(&Path::new("/srv/codex-state")));
}

#[test]
fn a_package_outside_home_is_handled_by_its_own_prefix() {
    let spec = jail_spec(&brew_gemini()).unwrap();
    assert_eq!(
        spec.rx,
        paths(&[
            "/home/linuxbrew/.linuxbrew/Cellar/node/26.8.1/bin/node",
            "/home/linuxbrew/.linuxbrew/lib/node_modules/@google/gemini-cli",
        ]),
    );
    // Nothing else under /home/linuxbrew and nothing under the user's home
    // except gemini's own state.
    let brew: Vec<_> = every_path(&spec)
        .into_iter()
        .filter(|path| path.starts_with("/home/linuxbrew"))
        .collect();
    assert_eq!(brew.len(), 2, "{brew:?}");
    let home: Vec<_> = every_path(&spec)
        .into_iter()
        .filter(|path| path.starts_with(HOME))
        .collect();
    assert_eq!(home, vec![Path::new("/home/u/.gemini")]);
}

#[test]
fn system_dirs_are_read_only_and_never_executable() {
    for l in [nvm_codex(), native_claude(), brew_gemini()] {
        let spec = jail_spec(&l).unwrap();
        for dir in ["/usr", "/bin", "/lib", "/lib64", "/etc"] {
            assert!(spec.ro.contains(&p(dir)), "{dir} not read-only");
            for other in [&spec.rx, &spec.rw] {
                assert!(!other.contains(&p(dir)), "{dir} must be read-only only");
            }
        }
        assert!(spec.ro.contains(&p("/proc/self")));
        assert!(spec.ro.contains(&p("/sys/fs/cgroup")));
        assert!(spec.ro.contains(&l.cwd));
        for file in [
            "/proc/cpuinfo",
            "/proc/meminfo",
            "/proc/stat",
            "/dev/urandom",
            "/dev/random",
        ] {
            assert!(spec.ro_files.contains(&p(file)), "{file}");
        }
        assert!(
            !spec
                .rx
                .iter()
                .any(|x| x.starts_with("/usr") || x.starts_with("/bin") || x.starts_with("/lib")),
            "{:?}",
            spec.rx,
        );
    }
}

#[test]
fn no_rule_ever_reaches_home_tmp_or_ssh() {
    let mut codex_home = nvm_codex();
    codex_home.codex_home = Some(p("/srv/codex-state"));
    for l in [nvm_codex(), native_claude(), brew_gemini(), codex_home] {
        let spec = jail_spec(&l).unwrap();
        for path in every_path(&spec) {
            assert_ne!(path, Path::new(HOME), "{path:?}");
            assert_ne!(path, Path::new("/"), "{path:?}");
            assert!(!path.starts_with("/tmp"), "{path:?}");
            assert!(!path.starts_with("/home/u/.ssh"), "{path:?}");
            assert!(
                !Path::new(HOME).starts_with(path),
                "{path:?} is an ancestor of $HOME"
            );
        }
    }
}

#[test]
fn proven_helpers_are_added_by_exact_path() {
    let mut l = nvm_codex();
    l.helpers = paths(&["/usr/bin/rg"]);
    let spec = jail_spec(&l).unwrap();
    assert!(spec.rx.contains(&p("/usr/bin/rg")));
    assert!(!spec.rx.contains(&p("/usr/bin")));
    assert_eq!(spec.rx.len(), 3);
}

#[test]
fn repeated_paths_are_listed_once() {
    let mut l = nvm_codex();
    l.interpreter = Some(l.binary.clone());
    l.helpers = vec![l.binary.clone()];
    let spec = jail_spec(&l).unwrap();
    let count = spec.rx.iter().filter(|x| **x == l.binary).count();
    assert_eq!(count, 1);
}

#[test]
fn a_package_root_that_would_open_a_broad_dir_is_refused() {
    for binary in [
        "/home/u/tool",         // parent is $HOME
        "/usr/bin/gemini",      // parent is a system bin dir
        "/usr/local/bin/codex", // same
        "/tmp/x/codex",         // under /tmp
        "/home/u/.ssh/codex",   // under ~/.ssh
        "/gemini",              // parent is /
    ] {
        let err = jail_spec(&layout(Cli::Codex, binary, None)).unwrap_err();
        assert!(
            matches!(err, SpecError::RootTooBroad(_)),
            "{binary}: {err:?}"
        );
    }
    // The same executable one level down is a package of its own.
    assert!(jail_spec(&layout(Cli::Codex, "/home/u/bin/codex", None)).is_ok());
    assert!(
        jail_spec(&layout(
            Cli::Codex,
            "/usr/lib/node_modules/codex/bin/c.js",
            None
        ))
        .is_ok()
    );
}

#[test]
fn a_state_dir_that_would_open_a_broad_dir_is_refused() {
    for state in ["/home/u", "/tmp", "/tmp/codex", "/home/u/.ssh", "/"] {
        let mut l = nvm_codex();
        l.codex_home = Some(p(state));
        let err = jail_spec(&l).unwrap_err();
        assert_eq!(err, SpecError::RootTooBroad(p(state)), "{state}");
    }
}

#[test]
fn retain_existing_drops_missing_paths_only() {
    let spec = jail_spec(&nvm_codex()).unwrap();
    let kept = spec
        .clone()
        .retain_existing(|path| path != Path::new("/home/u/.codex") && path != Path::new("/lib64"));
    assert!(!kept.rw.contains(&p("/home/u/.codex")));
    assert!(!kept.ro.contains(&p("/lib64")));
    assert!(kept.ro.contains(&p("/usr")));
    assert_eq!(kept.rx, spec.rx);
    assert_eq!(kept.rw.len(), spec.rw.len() - 1);
    let none = spec.retain_existing(|_| false);
    assert!(every_path(&none).is_empty());
}

// ---- helper arguments ----

fn args(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn helper_args_round_trip() {
    let spec = jail_spec(&nvm_codex()).unwrap();
    let job = JailArgs {
        min_abi: Some(6),
        spec,
        program: p("/home/u/.nvm/versions/node/v24.16.0/bin/node"),
        argv: args(&["codex.js", "--model", "-x; $(id)", "--"]),
    };
    let rendered = job.render();
    assert_eq!(rendered[0], OsString::from("__jail-exec"));
    assert_eq!(parse_args(&rendered[1..]).unwrap(), job);

    let bare = JailArgs {
        min_abi: None,
        spec: JailSpec::default(),
        program: p("/bin/true"),
        argv: Vec::new(),
    };
    assert_eq!(parse_args(&bare.render()[1..]).unwrap(), bare);
}

#[test]
fn helper_args_keep_the_program_argv_verbatim_after_the_separator() {
    let parsed = parse_args(&args(&["--rx", "/a", "--", "/bin/x", "--rx", "/b", "--"])).unwrap();
    assert_eq!(parsed.spec.rx, paths(&["/a"]));
    assert_eq!(parsed.program, p("/bin/x"));
    assert_eq!(parsed.argv, args(&["--rx", "/b", "--"]));
}

#[test]
fn helper_args_refuse_anything_unclear() {
    for bad in [
        args(&["--rx", "/a"]),                       // no separator
        args(&["--rx", "/a", "--"]),                 // no program
        args(&["--rx", "rel/path", "--", "/bin/x"]), // relative rule
        args(&["--rx", "--", "/bin/x"]),             // missing value
        args(&["--wat", "/a", "--", "/bin/x"]),      // unknown flag
        args(&["--", "bin/x"]),                      // relative program
        args(&["--min-abi", "six", "--", "/bin/x"]), // not a number
    ] {
        assert!(parse_args(&bad).is_err(), "{bad:?}");
    }
}
