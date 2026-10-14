//! The jail helper, for real: `lazarobox-shell __jail-exec` applies Landlock
//! and execs a program. Every test is skipped (with a message) when this
//! system cannot jail, so CI on an old kernel stays green.

#![cfg(target_os = "linux")]

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use lazarobox_shell::{
    core::ai::jail::{ELF_HEAD_BYTES, JailArgs, JailSpec, JailSupport, elf_loader},
    runtime::jail::probe,
};

fn jail_available() -> bool {
    match probe() {
        JailSupport::Available { .. } => true,
        JailSupport::Unavailable(why) => {
            eprintln!("skipped: no Landlock jail on this system ({why})");
            false
        }
    }
}

fn canon(path: &str) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The system folders a dynamically linked program needs to load: read-only,
/// never executable (R29).
fn system_ro() -> Vec<PathBuf> {
    ["/usr", "/bin", "/lib", "/lib64", "/etc"]
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect()
}

/// The program's ELF loader, as `jail_spec` is given it: the kernel opens it
/// with execute rights, so a dynamic program cannot start without it.
fn loader_of(program: &Path) -> Option<PathBuf> {
    let mut head = vec![0u8; ELF_HEAD_BYTES];
    let read = fs::File::open(program).and_then(|mut f| std::io::Read::read(&mut f, &mut head));
    let loader = elf_loader(&head[..read.expect("the program is readable")])?;
    Some(fs::canonicalize(loader).expect("the loader exists"))
}

fn job(program: &str, argv: &[&str], tweak: impl FnOnce(&mut JailSpec)) -> JailArgs {
    let program = canon(program);
    let mut spec = JailSpec {
        ro: system_ro(),
        rx: [Some(program.clone()), loader_of(&program)]
            .into_iter()
            .flatten()
            .collect(),
        ..JailSpec::default()
    };
    tweak(&mut spec);
    JailArgs {
        min_abi: None,
        spec,
        program,
        argv: argv.iter().map(OsString::from).collect(),
    }
}

fn run_raw(args: &[OsString]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lazarobox-shell"))
        .args(args)
        .env("LC_ALL", "C")
        .env("LAZAROBOX_TEST_MARK", "marker-value")
        .stdin(Stdio::null())
        .output()
        .expect("the helper starts")
}

fn run(job: &JailArgs) -> Output {
    run_raw(&job.render())
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn denied(out: &Output) -> bool {
    stderr(out).contains("Permission denied")
}

/// A temp dir with an `outside/secret` file, none of it in any rule.
fn secret_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret"), "hunter2\n").unwrap();
    (dir, outside)
}

#[test]
fn a_read_outside_the_allow_list_fails_and_an_allowed_one_prints() {
    if !jail_available() {
        return;
    }
    let (_dir, outside) = secret_dir();
    let secret = outside.join("secret");
    let secret = secret.to_str().unwrap();

    let out = run(&job("/bin/cat", &[secret], |_| {}));
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(denied(&out), "{}", stderr(&out));
    assert!(!stdout(&out).contains("hunter2"));

    let out = run(&job("/bin/cat", &[secret], |s| s.ro.push(outside.clone())));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "hunter2\n");
}

#[test]
fn a_write_fails_in_a_read_only_dir_and_works_in_a_read_write_one() {
    if !jail_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ro = dir.path().join("ro");
    let rw = dir.path().join("rw");
    fs::create_dir(&ro).unwrap();
    fs::create_dir(&rw).unwrap();
    let in_ro = ro.join("f");
    let in_rw = rw.join("f");
    let rules = |s: &mut JailSpec| {
        s.ro.push(ro.clone());
        s.rw.push(rw.clone());
    };

    let out = run(&job("/usr/bin/touch", &[in_ro.to_str().unwrap()], rules));
    assert!(!out.status.success());
    assert!(denied(&out), "{}", stderr(&out));
    assert!(!in_ro.exists());

    let out = run(&job("/usr/bin/touch", &[in_rw.to_str().unwrap()], rules));
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(in_rw.exists());
}

#[test]
fn a_file_rule_covers_that_file_and_not_its_siblings() {
    if !jail_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let granted = dir.path().join("granted");
    let other = dir.path().join("other");
    fs::write(&granted, "g").unwrap();
    fs::write(&other, "o").unwrap();

    let out = run(&job("/bin/cat", &[granted.to_str().unwrap()], |s| {
        s.ro_files.push(granted.clone())
    }));
    assert_eq!(stdout(&out), "g", "{}", stderr(&out));

    let out = run(&job("/bin/cat", &[other.to_str().unwrap()], |s| {
        s.ro_files.push(granted.clone())
    }));
    assert!(denied(&out), "{}", stderr(&out));
}

#[test]
fn a_minimum_abi_above_the_kernels_exits_125_and_never_execs() {
    if !jail_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker");
    let mut j = job("/usr/bin/touch", &[marker.to_str().unwrap()], |s| {
        s.rw.push(dir.path().to_path_buf())
    });
    j.min_abi = Some(99);
    let out = run(&j);
    assert_eq!(out.status.code(), Some(125));
    assert!(
        stderr(&out).contains("jail not enforced"),
        "{}",
        stderr(&out)
    );
    assert!(!marker.exists(), "the program must not run");

    // The same job without the floor does run.
    j.min_abi = None;
    assert!(run(&j).status.success());
    assert!(marker.exists());
}

#[test]
fn a_rule_that_cannot_be_applied_refuses_to_run() {
    if !jail_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker");
    let missing = dir.path().join("does-not-exist");
    let out = run(&job("/usr/bin/touch", &[marker.to_str().unwrap()], |s| {
        s.rw.push(dir.path().to_path_buf());
        s.ro.push(missing.clone());
    }));
    assert_eq!(out.status.code(), Some(125), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("jail not enforced"),
        "{}",
        stderr(&out)
    );
    assert!(!marker.exists());
}

#[test]
fn bad_helper_arguments_exit_125_without_running_anything() {
    if !jail_available() {
        return;
    }
    for args in [
        vec!["__jail-exec", "--rx", "/usr/bin/touch"],
        vec!["__jail-exec", "--", "relative/program"],
        vec!["__jail-exec", "--bogus", "/x", "--", "/usr/bin/true"],
    ] {
        let args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
        let out = run_raw(&args);
        assert_eq!(out.status.code(), Some(125), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("jail not enforced"),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn the_program_runs_with_no_new_privs() {
    if !jail_available() {
        return;
    }
    let out = run(&job(
        "/usr/bin/grep",
        &["NoNewPrivs", "/proc/self/status"],
        |s| s.ro.push(PathBuf::from("/proc/self")),
    ));
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("NoNewPrivs:\t1"), "{}", stdout(&out));
}

#[test]
fn only_the_granted_programs_can_be_executed() {
    if !jail_available() {
        return;
    }
    // `env` is the one exact helper; it tries to execve each target.
    let mut tried = 0;
    for target in ["/usr/bin/ssh", "/usr/bin/curl", "/bin/sh"] {
        if !Path::new(target).exists() {
            eprintln!("note: {target} is not installed here, its EACCES check is skipped");
            continue;
        }
        tried += 1;
        let out = run(&job("/usr/bin/env", &[target, "-c", "true"], |_| {}));
        assert!(!out.status.success(), "{target} ran");
        assert!(denied(&out), "{target}: {}", stderr(&out));
    }
    assert!(tried >= 1, "/bin/sh always exists");
}

#[test]
fn the_granted_helper_runs_and_its_shared_libraries_load() {
    if !jail_available() {
        return;
    }
    let out = run(&job("/usr/bin/env", &[], |_| {}));
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("LAZAROBOX_TEST_MARK=marker-value"),
        "{}",
        stdout(&out)
    );
    assert!(
        !stderr(&out).contains("error while loading"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn an_execute_rule_on_a_folder_runs_what_lives_inside_it() {
    if !jail_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("pkg");
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir(&package).unwrap();
    fs::create_dir(&elsewhere).unwrap();
    for folder in [&package, &elsewhere] {
        fs::copy("/usr/bin/true", folder.join("tool")).unwrap();
    }
    let loader = loader_of(Path::new("/usr/bin/true")).unwrap();
    let run_tool = |inside: &Path| {
        let program = fs::canonicalize(inside.join("tool")).unwrap();
        let args = JailArgs {
            min_abi: None,
            spec: JailSpec {
                ro: system_ro(),
                rx: vec![package.clone(), loader.clone()],
                ..JailSpec::default()
            },
            program,
            argv: Vec::new(),
        };
        run(&args)
    };

    let out = run_tool(&package);
    assert!(out.status.success(), "{}", stderr(&out));
    let out = run_tool(&elsewhere);
    assert_eq!(out.status.code(), Some(127), "{}", stderr(&out));
    assert!(denied(&out), "{}", stderr(&out));
}
