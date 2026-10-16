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
        loaders: Vec::new(),
        lib_dirs: Vec::new(),
        data_files: Vec::new(),
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
    assert_eq!(
        support_from_abi(2),
        JailSupport::Unavailable(JailOff::NoLandlock(2))
    );
    assert_eq!(
        support_from_abi(0),
        JailSupport::Unavailable(JailOff::NoLandlock(0))
    );
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
fn helper_exit_125_with_its_marker_is_a_jail_error() {
    let said = format!("noise\n{NOT_ENFORCED_MARKER}: ABI 2 is below the required 6\n");
    let reason = "ABI 2 is below the required 6".to_owned();
    assert_eq!(exit_error(Some(125), &said), Some(ChatError::Jail(reason)));
    assert_eq!(
        exit_error(Some(125), NOT_ENFORCED_MARKER),
        Some(ChatError::Jail(String::new()))
    );
    // Other codes, even with the marker, are not the helper's refusal.
    assert_eq!(exit_error(Some(1), &said), None);
    assert_eq!(exit_error(Some(0), &said), None);
    assert_eq!(exit_error(None, &said), None);
}

#[test]
fn a_cli_that_exits_125_by_itself_is_not_a_jail_error() {
    assert_eq!(exit_error(Some(125), ""), None);
    assert_eq!(exit_error(Some(125), "error: quota exceeded\n"), None);
    // The marker must start a line: quoting it mid-line does not count.
    let quoted = format!("the log said {NOT_ENFORCED_MARKER}\n");
    assert_eq!(exit_error(Some(125), &quoted), None);
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
}

#[test]
fn package_root_never_becomes_the_whole_node_modules_dir() {
    // The binary is the package dir (or a bare scope dir) itself: there is no
    // package folder above it, and node_modules would grant every package.
    for binary in [
        "/a/node_modules/tool",
        "/a/node_modules/@scope",
        "/a/node_modules/@scope/tool",
    ] {
        assert_eq!(package_root(&p(binary)), None, "{binary}");
    }
    assert_eq!(
        package_root(&p("/a/node_modules/@scope/tool/x")),
        Some(p("/a/node_modules/@scope/tool")),
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

/// The system folders a jailed CLI may read: libraries and data, never the
/// folders that hold programs (ro-system-dirs narrowing, decided in PR17).
const SYSTEM_RO: [&str; 5] = ["/usr/lib", "/usr/share", "/lib", "/lib64", "/etc"];

#[test]
fn the_folders_that_hold_programs_are_not_readable_so_the_loader_cannot_run_them() {
    // Landlock does not gate `mmap(PROT_EXEC)`, so a granted ELF loader can
    // run `ld.so /usr/bin/curl` if it can read the program. Not readable, not
    // runnable that way.
    for l in [nvm_codex(), native_claude(), brew_gemini()] {
        let spec = jail_spec(&l).unwrap();
        for dir in [
            "/usr",
            "/usr/bin",
            "/usr/sbin",
            "/usr/local",
            "/bin",
            "/sbin",
            "/opt",
        ] {
            assert!(!spec.ro.contains(&p(dir)), "{dir} must not be readable");
            assert!(!spec.ro_files.contains(&p(dir)), "{dir}");
        }
        assert_eq!(spec.ro[..5], SYSTEM_RO.map(p));
    }
}

#[test]
fn system_dirs_are_read_only_and_never_executable() {
    for l in [nvm_codex(), native_claude(), brew_gemini()] {
        let spec = jail_spec(&l).unwrap();
        for dir in SYSTEM_RO {
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
    assert!(kept.ro.contains(&p("/usr/lib")));
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

// ---- the ELF loader ----

/// A minimal little-endian ELF64 image: header, `phdrs` program headers
/// (type, offset, size) and the bytes placed at the given offsets.
fn elf64(phdrs: &[(u32, u64, u64)], blobs: &[(usize, &[u8])]) -> Vec<u8> {
    let mut img = vec![0u8; 512];
    img[..4].copy_from_slice(b"\x7fELF");
    img[4] = 2; // 64-bit
    img[5] = 1; // little endian
    img[0x20..0x28].copy_from_slice(&64u64.to_le_bytes()); // e_phoff
    img[0x36..0x38].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
    img[0x38..0x3a].copy_from_slice(&(phdrs.len() as u16).to_le_bytes()); // e_phnum
    for (i, (kind, offset, size)) in phdrs.iter().enumerate() {
        let at = 64 + i * 56;
        img[at..at + 4].copy_from_slice(&kind.to_le_bytes());
        img[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        img[at + 32..at + 40].copy_from_slice(&size.to_le_bytes());
    }
    for (at, bytes) in blobs {
        img[*at..*at + bytes.len()].copy_from_slice(bytes);
    }
    img
}

#[test]
fn elf_loader_is_the_pt_interp_path() {
    let glibc = b"/lib64/ld-linux-x86-64.so.2\0";
    let img = elf64(&[(1, 0, 0), (3, 300, glibc.len() as u64)], &[(300, glibc)]);
    assert_eq!(elf_loader(&img), Some(p("/lib64/ld-linux-x86-64.so.2")));

    let brew = b"/home/linuxbrew/.linuxbrew/lib/ld.so\0";
    let img = elf64(&[(3, 400, brew.len() as u64)], &[(400, brew)]);
    assert_eq!(
        elf_loader(&img),
        Some(p("/home/linuxbrew/.linuxbrew/lib/ld.so"))
    );
}

#[test]
fn the_interp_string_can_lie_far_beyond_the_head_and_is_still_located() {
    // Homebrew rewrites the loader path with patchelf, which moves `.interp`
    // to 0x8d98 in node: the program headers are at the start, the string is
    // not.
    let img = elf64(&[(1, 0, 0), (3, 0x8d98, 37)], &[]);
    assert_eq!(elf_interp_at(&img), Some((0x8d98, 37)));
    // Not an ELF64, or no PT_INTERP, or a size no path can have.
    assert_eq!(elf_interp_at(b"#!/bin/sh\n"), None);
    assert_eq!(elf_interp_at(&elf64(&[(1, 0, 0)], &[])), None);
    assert_eq!(elf_interp_at(&elf64(&[(3, 300, 4097)], &[])), None);
    assert_eq!(elf_interp_at(&elf64(&[(3, 300, 0)], &[])), None);
    // Empty and truncated headers.
    assert_eq!(elf_interp_at(b""), None);
    assert_eq!(elf_interp_at(&elf64(&[(3, 300, 6)], &[])[..70]), None);
}

#[test]
fn an_interp_string_is_an_absolute_utf8_path_up_to_its_nul() {
    assert_eq!(
        interp_path(b"/lib64/ld-linux-x86-64.so.2\0"),
        Some(p("/lib64/ld-linux-x86-64.so.2"))
    );
    assert_eq!(interp_path(b"/lib/ld.so"), Some(p("/lib/ld.so")));
    assert_eq!(interp_path(b"ld.so\0"), None);
    assert_eq!(interp_path(b"/lib/\xff\0"), None);
    assert_eq!(interp_path(b"\0"), None);
    assert_eq!(interp_path(b""), None);
}

#[test]
fn elf_loader_is_none_when_there_is_nothing_clean_to_read() {
    let name = b"/lib64/ld.so\0";
    // Static binary: no PT_INTERP.
    assert_eq!(elf_loader(&elf64(&[(1, 0, 0), (2, 0, 0)], &[])), None);
    // A script, an empty file, a truncated header.
    assert_eq!(elf_loader(b"#!/usr/bin/env node\n"), None);
    assert_eq!(elf_loader(b""), None);
    let img = elf64(&[(3, 300, name.len() as u64)], &[(300, name)]);
    assert_eq!(elf_loader(&img[..70]), None);
    // 32-bit and big-endian images are not read.
    let mut c32 = img.clone();
    c32[4] = 1;
    assert_eq!(elf_loader(&c32), None);
    let mut be = img.clone();
    be[5] = 2;
    assert_eq!(elf_loader(&be), None);
    // The string lies outside the bytes we have, or is not absolute / UTF-8.
    assert_eq!(elf_loader(&elf64(&[(3, 9000, 12)], &[])), None);
    let rel = b"ld.so\0";
    assert_eq!(elf_loader(&elf64(&[(3, 300, 6)], &[(300, rel)])), None);
    let bad = b"/lib/\xff\0";
    assert_eq!(elf_loader(&elf64(&[(3, 300, 7)], &[(300, bad)])), None);
    // A header table that claims more entries than the image holds.
    let mut many = img.clone();
    many[0x38..0x3a].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(elf_loader(&many), None);
}

#[test]
fn loaders_are_granted_as_exact_execute_files() {
    let mut l = nvm_codex();
    l.loaders = paths(&["/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2"]);
    let spec = jail_spec(&l).unwrap();
    assert_eq!(
        spec.rx,
        paths(&[
            "/home/u/.nvm/versions/node/v24.16.0/bin/node",
            "/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
            "/home/u/.nvm/versions/node/v24.16.0/lib/node_modules/@openai/codex",
        ]),
    );
    // Still no system folder is executable.
    assert!(!spec.rx.contains(&p("/usr/lib")));
}

#[test]
fn a_loader_outside_the_system_dirs_is_granted_by_its_own_path() {
    let mut l = brew_gemini();
    l.loaders = paths(&["/home/linuxbrew/.linuxbrew/lib/ld.so"]);
    let spec = jail_spec(&l).unwrap();
    assert!(spec.rx.contains(&p("/home/linuxbrew/.linuxbrew/lib/ld.so")));
    assert_eq!(spec.rx.len(), 3);
}

// ---- canonical paths and state dirs ----

#[test]
fn paths_with_dot_components_are_refused() {
    let dotted = [
        "/home/u/foo/../claude",
        "/home/u/./claude",
        "/home/u/x/../../claude",
    ];
    for binary in dotted {
        let err = jail_spec(&layout(Cli::Codex, binary, None)).unwrap_err();
        assert_eq!(err, SpecError::NotCanonical(p(binary)), "{binary}");
    }
    let mut l = nvm_codex();
    l.home = p("/home/u/../u");
    assert_eq!(
        jail_spec(&l).unwrap_err(),
        SpecError::NotCanonical(p("/home/u/../u"))
    );
    let mut l = nvm_codex();
    l.codex_home = Some(p("/srv/a/../codex"));
    assert_eq!(
        jail_spec(&l).unwrap_err(),
        SpecError::NotCanonical(p("/srv/a/../codex"))
    );
    let mut l = nvm_codex();
    l.interpreter = Some(p("/usr/bin/../bin/node"));
    assert!(matches!(jail_spec(&l), Err(SpecError::NotCanonical(_))));
    let mut l = nvm_codex();
    l.helpers = vec![p("rel/helper")];
    assert!(matches!(jail_spec(&l), Err(SpecError::NotCanonical(_))));
    // Canonical neighbours are fine.
    assert!(jail_spec(&nvm_codex()).is_ok());
}

fn resolver(links: &'static [(&'static str, &'static str)]) -> impl Fn(&Path) -> Option<PathBuf> {
    move |path| {
        links
            .iter()
            .find(|(from, _)| Path::new(from) == path)
            .map(|(_, to)| p(to))
            .or_else(|| Some(path.to_path_buf()))
    }
}

#[test]
fn a_state_dir_that_resolves_to_home_or_ssh_is_refused() {
    for target in ["/home/u", "/home/u/.ssh", "/home/u/.gnupg", "/"] {
        let links: &'static [(&str, &str)] = Box::leak(Box::new([("/home/u/.claude", target)]));
        let err = jail_spec_with(&native_claude(), &resolver(links)).unwrap_err();
        assert_eq!(err, SpecError::RootTooBroad(p(target)), "{target}");
    }
}

#[test]
fn a_state_dir_that_resolves_elsewhere_is_granted_by_its_target() {
    let links: &'static [(&str, &str)] =
        Box::leak(Box::new([("/home/u/.gemini", "/data/gemini-state")]));
    let spec = jail_spec_with(&brew_gemini(), &resolver(links)).unwrap();
    assert!(spec.rw.contains(&p("/data/gemini-state")));
    assert!(!spec.rw.contains(&p("/home/u/.gemini")));
}

#[test]
fn the_claude_config_file_is_resolved_and_checked_too() {
    let links: &'static [(&str, &str)] = Box::leak(Box::new([(
        "/home/u/.claude.json",
        "/home/u/.ssh/authorized_keys",
    )]));
    let err = jail_spec_with(&native_claude(), &resolver(links)).unwrap_err();
    assert_eq!(
        err,
        SpecError::RootTooBroad(p("/home/u/.ssh/authorized_keys"))
    );
    let links: &'static [(&str, &str)] =
        Box::leak(Box::new([("/home/u/.claude.json", "/data/claude.json")]));
    let spec = jail_spec_with(&native_claude(), &resolver(links)).unwrap();
    assert!(spec.rw_files.contains(&p("/data/claude.json")));
}

#[test]
fn a_missing_state_dir_keeps_its_nominal_path_for_the_existence_filter() {
    let spec = jail_spec_with(&native_claude(), &|_| None).unwrap();
    assert!(spec.rw.contains(&p("/home/u/.claude")));
    assert!(spec.rw_files.contains(&p("/home/u/.claude.json")));
}

#[test]
fn codex_home_must_be_a_dedicated_dir() {
    for state in [
        "/home/u/.config",
        "/home/u/.gnupg",
        "/home/u/.local",
        "/home/u/.local/share",
        "/home/u/.local/state",
        "/home/u/.cache",
        "/home/u/.gnupg/private-keys-v1.d",
        "/home/u/.config/gh/..hidden-not-dot-dot",
    ] {
        let mut l = nvm_codex();
        l.codex_home = Some(p(state));
        let result = jail_spec(&l);
        if state.ends_with("hidden-not-dot-dot") {
            assert!(result.is_ok(), "{state}");
        } else {
            assert_eq!(
                result.unwrap_err(),
                SpecError::RootTooBroad(p(state)),
                "{state}"
            );
        }
    }
    // A dedicated dir under the same parents is fine.
    let mut l = nvm_codex();
    l.codex_home = Some(p("/home/u/.config/codex"));
    assert!(jail_spec(&l).is_ok());
    l.codex_home = Some(p("/home/u/.local/share/codex"));
    assert!(jail_spec(&l).is_ok());
}

#[test]
fn more_system_and_mount_folders_are_never_a_root() {
    for dir in [
        "/run",
        "/mnt",
        "/media",
        "/srv",
        "/snap",
        "/usr/share",
        "/usr/libexec",
        "/var/lib",
    ] {
        let mut l = nvm_codex();
        l.codex_home = Some(p(dir));
        assert_eq!(
            jail_spec(&l).unwrap_err(),
            SpecError::RootTooBroad(p(dir)),
            "{dir}"
        );
        let tool = format!("{dir}/codex");
        let err = jail_spec(&layout(Cli::Codex, &tool, None)).unwrap_err();
        assert_eq!(err, SpecError::RootTooBroad(p(dir)), "{tool}");
    }
}

#[test]
fn a_home_that_is_a_symlink_is_checked_under_both_spellings() {
    // Silverblue style: /home is a link to /var/home, so $HOME has two names.
    let links: &'static [(&str, &str)] = &[
        ("/home/u", "/var/home/u"),
        ("/home/u/.claude", "/var/home/u"),
    ];
    let err = jail_spec_with(&native_claude(), &resolver(links)).unwrap_err();
    assert_eq!(err, SpecError::RootTooBroad(p("/var/home/u")));

    for (sensitive, target) in [
        ("/var/home/u/.ssh", "/var/home/u/.ssh"),
        ("/var/home/u/.gnupg", "/var/home/u/.gnupg"),
        ("/var/home/u/.config", "/var/home/u/.config"),
        ("/var/home/u/.local/share", "/var/home/u/.local/share"),
    ] {
        let leaked: &'static [(&str, &str)] = Box::leak(Box::new([
            ("/home/u", "/var/home/u"),
            ("/home/u/.claude", sensitive),
        ]));
        let err = jail_spec_with(&native_claude(), &resolver(leaked)).unwrap_err();
        assert_eq!(err, SpecError::RootTooBroad(p(target)), "{sensitive}");
    }

    // The same layout with a dedicated state dir under the real home passes.
    let fine: &'static [(&str, &str)] = &[
        ("/home/u", "/var/home/u"),
        ("/home/u/.claude", "/var/home/u/.claude"),
    ];
    let spec = jail_spec_with(&native_claude(), &resolver(fine)).unwrap();
    assert!(spec.rw.contains(&p("/var/home/u/.claude")));
}

/// Folders under `$HOME` that hold another tool's credentials or state.
const OTHER_STATE: [&str; 6] = [
    ".claude",
    ".gemini",
    ".aws",
    ".kube",
    ".docker",
    ".password-store",
];

#[test]
fn codex_home_may_not_be_another_tools_credential_folder() {
    for dir in OTHER_STATE {
        for state in [format!("/home/u/{dir}"), format!("/home/u/{dir}/sub")] {
            let mut l = nvm_codex();
            l.codex_home = Some(p(&state));
            assert_eq!(
                jail_spec(&l).unwrap_err(),
                SpecError::RootTooBroad(p(&state)),
                "{state}"
            );
        }
    }
    // The CLI's own folder, and a dedicated one elsewhere, stay fine.
    let mut l = nvm_codex();
    l.codex_home = Some(p("/home/u/.codex"));
    assert!(jail_spec(&l).is_ok());
}

#[test]
fn a_cli_may_only_have_its_own_state_folder_from_that_list() {
    // A link that makes one CLI's state folder point at another's is refused;
    // claude keeps `~/.claude` and gemini keeps `~/.gemini`.
    let to =
        |from: &'static str, target: &'static str| -> &'static [(&'static str, &'static str)] {
            Box::leak(Box::new([(from, target)]))
        };
    let err = jail_spec_with(
        &native_claude(),
        &resolver(to("/home/u/.claude", "/home/u/.aws")),
    );
    assert_eq!(err.unwrap_err(), SpecError::RootTooBroad(p("/home/u/.aws")));
    let err = jail_spec_with(
        &brew_gemini(),
        &resolver(to("/home/u/.gemini", "/home/u/.claude")),
    );
    assert_eq!(
        err.unwrap_err(),
        SpecError::RootTooBroad(p("/home/u/.claude"))
    );
    let err = jail_spec_with(
        &nvm_codex(),
        &resolver(to("/home/u/.codex", "/home/u/.gemini")),
    );
    assert_eq!(
        err.unwrap_err(),
        SpecError::RootTooBroad(p("/home/u/.gemini"))
    );
    assert!(jail_spec(&native_claude()).is_ok());
    assert!(jail_spec(&brew_gemini()).is_ok());
}

#[test]
fn a_package_root_inside_another_tools_folder_is_refused() {
    let mut l = nvm_codex();
    l.binary = p("/home/u/.aws/node_modules/@openai/codex/bin/codex.js");
    assert!(matches!(jail_spec(&l), Err(SpecError::RootTooBroad(_))));
    // claude's older installs live in its own folder: allowed for claude.
    let mut c = native_claude();
    c.binary = p("/home/u/.claude/local/claude");
    assert!(jail_spec(&c).is_ok());
}

#[test]
fn a_loader_or_helper_in_a_broad_place_is_refused() {
    for bad in [
        "/tmp/ld.so",
        "/var/tmp/x/ld.so",
        "/usr/lib",
        "/home/u/.ssh/ld.so",
        "/home/u/.aws/ld.so",
    ] {
        let mut l = nvm_codex();
        l.loaders = paths(&[bad]);
        assert_eq!(
            jail_spec(&l).unwrap_err(),
            SpecError::RootTooBroad(p(bad)),
            "{bad}"
        );
        let mut l = nvm_codex();
        l.helpers = paths(&[bad]);
        assert_eq!(
            jail_spec(&l).unwrap_err(),
            SpecError::RootTooBroad(p(bad)),
            "{bad}"
        );
    }
    let mut l = nvm_codex();
    l.loaders = paths(&["/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2"]);
    assert!(jail_spec(&l).is_ok());
}

// ---- library folders and data files the interpreter's ELF names ----

const BREW: &str = "/home/linuxbrew/.linuxbrew";

fn brew_node_gemini() -> CliLayout {
    let mut l = brew_gemini();
    l.lib_dirs = paths(&[
        "/home/linuxbrew/.linuxbrew/Cellar/node/26.8.1/lib",
        "/home/linuxbrew/.linuxbrew/Cellar/openssl@3/3.6.2/lib",
        "/home/linuxbrew/.linuxbrew/Cellar/zlib-ng-compat/2.3.3/lib",
    ]);
    l.data_files = paths(&[
        "/home/linuxbrew/.linuxbrew/etc/openssl@3/openssl.cnf",
        "/home/linuxbrew/.linuxbrew/etc/ca-certificates/cert.pem",
    ]);
    l
}

#[test]
fn the_library_folders_of_the_interpreter_are_read_only_and_nothing_else() {
    let layout = brew_node_gemini();
    let spec = jail_spec(&layout).unwrap();
    for dir in &layout.lib_dirs {
        assert!(spec.ro.contains(dir), "{dir:?}");
        assert!(!spec.rx.contains(dir) && !spec.rw.contains(dir), "{dir:?}");
    }
    // The prefix itself is never granted.
    for list in [&spec.rx, &spec.ro, &spec.rw] {
        assert!(!list.contains(&p(BREW)), "{list:?}");
        assert!(!list.contains(&p("/home/linuxbrew")), "{list:?}");
    }
}

#[test]
fn the_data_files_are_exact_read_only_files() {
    let layout = brew_node_gemini();
    let spec = jail_spec(&layout).unwrap();
    for file in &layout.data_files {
        assert!(spec.ro_files.contains(file), "{file:?}");
        assert!(!spec.rw_files.contains(file), "{file:?}");
    }
}

#[test]
fn a_layout_without_them_gets_the_same_rules_as_before() {
    let plain = jail_spec(&brew_gemini()).unwrap();
    let with = jail_spec(&brew_node_gemini()).unwrap();
    assert_eq!(plain.rx, with.rx);
    assert_eq!(plain.rw, with.rw);
    assert_eq!(plain.rw_files, with.rw_files);
    assert_eq!(plain.ro[..], with.ro[..plain.ro.len()]);
    assert_eq!(plain.ro_files[..], with.ro_files[..plain.ro_files.len()]);
}

#[test]
fn a_folder_already_covered_is_not_listed_twice() {
    let mut layout = brew_gemini();
    layout.lib_dirs = paths(&[
        "/usr/lib",
        "/usr/lib/x86_64-linux-gnu",
        "/lib64",
        "/opt/dep/lib",
    ]);
    layout.lib_dirs.push(p("/opt/dep/lib"));
    let spec = jail_spec(&layout).unwrap();
    assert_eq!(spec.ro.iter().filter(|d| **d == p("/usr/lib")).count(), 1);
    assert!(!spec.ro.contains(&p("/usr/lib/x86_64-linux-gnu")));
    assert_eq!(
        spec.ro.iter().filter(|d| **d == p("/opt/dep/lib")).count(),
        1
    );
}

#[test]
fn a_broad_library_folder_is_dropped_never_granted() {
    let mut layout = brew_gemini();
    layout.lib_dirs = paths(&[
        "/home/u",
        "/home/u/.ssh",
        "/home/u/.gnupg/lib",
        "/home/u/.claude/lib",
        "/home/u/.config",
        "/tmp/lib",
        "/",
        "/home",
        "/opt",
        "/opt/dep/lib",
    ]);
    let spec = jail_spec(&layout).unwrap();
    let plain = jail_spec(&brew_gemini()).unwrap();
    let granted: Vec<&PathBuf> = spec.ro.iter().filter(|d| !plain.ro.contains(d)).collect();
    assert_eq!(granted, [&p("/opt/dep/lib")]);
}

#[test]
fn a_data_file_in_a_credential_place_is_dropped() {
    let mut layout = brew_gemini();
    layout.data_files = paths(&[
        "/home/u/.ssh/config",
        "/home/u/.claude/.credentials.json",
        "/tmp/x.pem",
        "/home/u/.gemini/settings.json",
        "/opt/dep/etc/openssl.cnf",
    ]);
    let spec = jail_spec(&layout).unwrap();
    let plain = jail_spec(&brew_gemini()).unwrap();
    let granted: Vec<&PathBuf> = spec
        .ro_files
        .iter()
        .filter(|d| !plain.ro_files.contains(d))
        .collect();
    // gemini's own folder is already its rw state, so the file is allowed (it
    // adds nothing); the others are refused.
    assert_eq!(
        granted,
        [
            &p("/home/u/.gemini/settings.json"),
            &p("/opt/dep/etc/openssl.cnf")
        ]
    );
}

#[test]
fn a_library_folder_or_data_file_must_be_canonical() {
    for bad in ["relative/lib", "/opt/../etc/lib", "/opt/./lib"] {
        let mut layout = brew_gemini();
        layout.lib_dirs = vec![p(bad)];
        assert!(
            matches!(jail_spec(&layout), Err(SpecError::NotCanonical(_))),
            "{bad}"
        );
        let mut layout = brew_gemini();
        layout.data_files = vec![p(bad)];
        assert!(
            matches!(jail_spec(&layout), Err(SpecError::NotCanonical(_))),
            "{bad}"
        );
    }
}

// ---- refusal texts ----

fn home() -> Option<&'static Path> {
    Some(Path::new(HOME))
}

#[test]
fn the_helper_reason_is_sanitized_and_capped() {
    let said = format!(
        "{NOT_ENFORCED_MARKER}: bad \u{1b}[31mthing\u{202e}{}\n",
        "x".repeat(500)
    );
    let Some(ChatError::Jail(reason)) = exit_error(Some(125), &said) else {
        panic!("expected a jail error");
    };
    assert!(reason.starts_with("bad thing"), "{reason}");
    assert!(
        !reason.contains('\u{1b}') && !reason.contains('\u{202e}'),
        "{reason}"
    );
    assert_eq!(reason.chars().count(), 200);
}

#[test]
fn a_kernel_without_the_abi_says_what_to_upgrade_to() {
    let abi2 = JailRefusal::Unsupported(JailOff::NoLandlock(2));
    assert_eq!(
        abi2.hint(home()),
        "Landlock ABI 3+ needed (Linux 6.2); this kernel has ABI 2"
    );
    assert_eq!(abi2.short(home()), "Landlock ABI 2, needs 3+");
    let none = JailRefusal::Unsupported(JailOff::NoLandlock(0));
    assert_eq!(
        none.hint(home()),
        "Landlock is not enabled in this kernel; ABI 3+ needed (Linux 6.2)"
    );
    let other_os = JailRefusal::Unsupported(JailOff::NotLinux);
    assert_eq!(other_os.hint(home()), "the CLI jail needs Linux");
    assert_eq!(other_os.short(home()), "needs Linux");
}

#[test]
fn a_group_writable_interpreter_says_which_chmod_to_run() {
    let refusal = JailRefusal::UntrustedInterpreter {
        path: p("/home/u/.nvm/versions/node/v24/bin/node"),
        why: Untrusted::Writable(p("/home/u/.nvm/versions/node/v24/bin")),
    };
    assert_eq!(
        refusal.hint(home()),
        "~/.nvm/versions/node/v24/bin/node is writable by group or others; \
         run: chmod go-w ~/.nvm/versions/node/v24/bin"
    );
    assert_eq!(refusal.short(home()), "interpreter not trusted");
    let scratch = JailRefusal::UntrustedInterpreter {
        path: p("/tmp/x/node"),
        why: Untrusted::Scratch,
    };
    assert_eq!(
        scratch.hint(home()),
        "/tmp/x/node is under a scratch folder; install the interpreter elsewhere"
    );
}

#[test]
fn a_broad_root_names_the_folder_and_what_to_do() {
    let refusal = JailRefusal::from(SpecError::RootTooBroad(p("/usr/local")));
    assert_eq!(
        refusal.hint(home()),
        "/usr/local is too broad for the jail; keep the CLI and its state in folders of their own"
    );
    assert_eq!(refusal.short(home()), "/usr/local too broad");
    assert_eq!(
        JailRefusal::from(SpecError::NotCanonical(p("/a/../b"))),
        JailRefusal::NotCanonical(p("/a/../b"))
    );
}

#[test]
fn the_other_refusals_have_a_text_each() {
    assert_eq!(
        JailRefusal::NoHome.hint(home()),
        "$HOME is not an absolute path; set it and restart"
    );
    assert_eq!(
        JailRefusal::NoInterpreter("node".into()).hint(home()),
        "no node found to run the CLI script; install it next to the CLI or on PATH"
    );
    assert_eq!(
        JailRefusal::NoInterpreter("node".into()).short(home()),
        "no node"
    );
    assert_eq!(
        JailRefusal::BadLoader(p("/lib/ld.so")).hint(home()),
        "the program loader /lib/ld.so is not a regular file"
    );
    assert_eq!(
        JailRefusal::Unreadable(p("/home/u/bin/codex")).hint(home()),
        "cannot read ~/bin/codex; check that it exists and is readable"
    );
}

#[test]
fn paths_in_a_refusal_are_abbreviated_and_sanitized() {
    let evil = p("/home/u/\u{1b}]0;pwned\u{7}dir\u{202e}/node");
    let refusal = JailRefusal::NoInterpreter(evil.display().to_string());
    let text = refusal.hint(home());
    assert!(
        !text.chars().any(|c| c.is_control() || c == '\u{202e}'),
        "{text:?}"
    );
    let broad = JailRefusal::RootTooBroad(p("/home/u")).hint(home());
    assert!(broad.starts_with("~ is too broad"), "{broad}");
}
