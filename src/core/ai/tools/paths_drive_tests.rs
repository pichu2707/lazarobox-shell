//! The WSL drive read policy through the path gate (openspec
//! `wsl-drive-access`, PR 1): reads reach a Windows drive, writes do not.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use super::*;
use crate::core::ai::tools::windows::WinDrives;

struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    drive: PathBuf,
    roots: SecretRoots,
}

/// A HOME and a drive `<tmp>/mnt/c` whose own profile is `ana`.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (home, drive) = (root.join("home"), root.join("mnt/c"));
    for sub in [
        "home/.ssh",
        "home/dev",
        "mnt/c/Users/ana/Documents/app",
        "mnt/c/Users/ana/AppData/Local",
        "mnt/c/Users/bob/Documents",
        "mnt/c/Windows",
    ] {
        fs::create_dir_all(root.join(sub)).unwrap();
    }
    for file in [
        "home/.ssh/id",
        "mnt/c/Users/ana/Documents/app/README.md",
        "mnt/c/Users/ana/Documents/app/.env",
        "mnt/c/Users/ana/AppData/Local/token",
        "mnt/c/Users/bob/Documents/x",
        "mnt/c/Windows/win.ini",
    ] {
        fs::write(root.join(file), "x").unwrap();
    }
    let roots = SecretRoots::load(&home, &[])
        .unwrap()
        .with_drives(WinDrives::new(
            vec![drive.clone()],
            Some(drive.join("Users/ana")),
        ));
    Fixture {
        _dir: dir,
        home,
        drive,
        roots,
    }
}

fn allowed(f: &Fixture, input: &str) -> bool {
    matches!(resolve(&f.roots, &f.home, input), PathVerdict::Allowed(_))
}

#[test]
fn a_project_on_the_drive_is_readable() {
    let f = fixture();
    let readme = f.drive.join("Users/ana/Documents/app/README.md");
    assert!(allowed(&f, readme.to_str().unwrap()));
    // A relative read from a chat opened on the drive.
    let app = f.drive.join("Users/ana/Documents/app");
    assert!(matches!(
        resolve(&f.roots, &app, "README.md"),
        PathVerdict::Allowed(real) if real == readme
    ));
}

#[test]
fn windows_folders_profiles_and_secrets_on_the_drive_are_refused() {
    let f = fixture();
    for rel in [
        "Windows/win.ini",
        "WINDOWS/win.ini",
        "Users/ana/AppData/Local/token",
        "Users/ana/Documents/app/.env",
        "Users/bob/Documents/x",
    ] {
        let path = f.drive.join(rel);
        assert!(!allowed(&f, path.to_str().unwrap()), "{rel}");
    }
}

#[test]
fn links_between_home_and_the_drive_are_judged_on_both_sides() {
    let f = fixture();
    // From HOME to a refused drive folder, and from the drive to a HOME secret.
    symlink(
        f.drive.join("Users/ana/AppData"),
        f.home.join("dev/appdata"),
    )
    .unwrap();
    symlink(
        f.home.join(".ssh"),
        f.drive.join("Users/ana/Documents/keys"),
    )
    .unwrap();
    assert!(!allowed(&f, "~/dev/appdata/Local/token"));
    let keys = f.drive.join("Users/ana/Documents/keys/id");
    assert!(!allowed(&f, keys.to_str().unwrap()));
    // From HOME to an allowed drive folder.
    symlink(f.drive.join("Users/ana/Documents"), f.home.join("dev/docs")).unwrap();
    assert!(allowed(&f, "~/dev/docs/app/README.md"));
}

#[test]
fn without_drives_the_drive_stays_outside_home() {
    let f = fixture();
    let plain = SecretRoots::load(&f.home, &[]).unwrap();
    let readme = f.drive.join("Users/ana/Documents/app/README.md");
    assert_eq!(
        resolve(&plain, &f.home, readme.to_str().unwrap()),
        PathVerdict::Refused
    );
    assert!(!plain.on_drive(&readme));
}

// Task 1.7: writes on a drive stay refused until the grant (PR 2).
#[test]
fn writes_on_the_drive_are_still_refused() {
    let f = fixture();
    let app = f.drive.join("Users/ana/Documents/app");
    assert!(resolve_write(&f.roots, &app, "main.py").is_none());
    assert!(resolve_write(&f.roots, &app, "README.md").is_none());
    assert!(
        resolve_write(
            &f.roots,
            &f.home.join("dev"),
            app.join("x").to_str().unwrap()
        )
        .is_none()
    );
}

#[test]
fn reads_open_beneath_the_drive() {
    let f = fixture();
    let readme = f.drive.join("Users/ana/Documents/app/README.md");
    assert!(f.roots.on_drive(&readme));
    assert_eq!(f.roots.base_of(&readme), f.drive.as_path());
    assert_eq!(f.roots.base_of(&f.home.join("dev")), f.home.as_path());
}

// Verifier B2: drvfs folds case and keeps the typed spelling, so the real
// target of a HOME secret root that lies on a drive is refused in any case.
#[test]
fn a_secret_root_on_the_drive_is_refused_in_any_case() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (home, drive) = (root.join("home"), root.join("mnt/c"));
    let target = drive.join("Users/ana/Documents/awsconf");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&target).unwrap();
    symlink(&target, home.join(".aws")).unwrap();
    let roots = SecretRoots::load(&home, &[])
        .unwrap()
        .with_drives(WinDrives::new(
            vec![drive.clone()],
            Some(drive.join("Users/ana")),
        ));
    for rel in [
        "Users/ana/Documents/awsconf/credentials",
        "Users/ana/Documents/AWSCONF/credentials",
        "users/ANA/documents/awsconf",
    ] {
        assert!(!roots.judge_real(&drive.join(rel)), "{rel}");
    }
    assert!(roots.holds_root(&drive.join("USERS/ana/Documents")));
}

// Verifier B3: a broad drive folder is not a root for a run's read scope.
#[test]
fn a_broad_drive_folder_is_too_broad_for_a_read_scope() {
    use crate::core::ai::orchestra::{
        RunId,
        scope::{ReadScope, TOO_BROAD},
    };
    let f = fixture();
    let no_scratch = |_: &std::path::Path| false;
    for root in [f.drive.clone(), f.drive.join("Users/ana")] {
        let input = root.to_str().unwrap().to_owned();
        let (_, dropped) = ReadScope::new(RunId(1), &f.roots, &f.home, &[&input], &no_scratch);
        assert_eq!(dropped.len(), 1, "{input}");
        assert_eq!(dropped[0].reason, TOO_BROAD, "{input}");
    }
    let app = f.drive.join("Users/ana/Documents/app");
    let (_, dropped) = ReadScope::new(
        RunId(1),
        &f.roots,
        &f.home,
        &[app.to_str().unwrap()],
        &no_scratch,
    );
    assert!(dropped.is_empty());
}

// Review R1-rest-case-fold / R3: the secret names under a profile are
// refused in any case by the real HOME rules, not only by a test stub.
#[test]
fn profile_secrets_are_refused_in_any_case() {
    let f = fixture();
    for rel in [
        "Users/ana/.SSH",
        "Users/ana/.Aws/credentials",
        "Users/ana/.CONFIG/GH/hosts.yml",
        "Users/Public/.NETRC",
        "Users/ana/Documents/app/.ENV",
    ] {
        assert!(!f.roots.judge_real(&f.drive.join(rel)), "{rel}");
    }
}

// PR 2 (code-write, Root policy, Granted drive folder): the granted folder
// takes writes below it, with the HOME deny rules; nothing else on a drive
// does, and the write primitives re-check it.
#[test]
fn a_granted_drive_folder_takes_writes_below_it_and_nowhere_else() {
    let f = fixture();
    let app = f.drive.join("Users/ana/Documents/app");
    let roots = f.roots.clone().with_grant(Some(app.clone()));
    for input in ["main.py", "src/lib.rs", ".env.example"] {
        let target = resolve_write(&roots, &app, input);
        assert_eq!(target.map(|t| t.real), Some(app.join(input)), "{input}");
    }
    for input in [
        "../x",
        ".git/config",
        "src/node_modules/x",
        ".env",
        "/etc/x",
    ] {
        assert!(resolve_write(&roots, &app, input).is_none(), "{input}");
    }
    // The primitives of an apply judge the same: inside the grant only.
    assert!(roots.judge_write(&app.join("main.py")));
    assert!(!roots.judge_write(&f.drive.join("Users/ana/Documents/x")));
    assert!(!f.roots.judge_write(&app.join("main.py")));
    // A grant of another folder, or of a broad one, opens nothing here.
    for grant in [f.drive.join("Users/ana/Documents"), f.drive.clone()] {
        let other = f.roots.clone().with_grant(Some(grant.clone()));
        assert!(
            resolve_write(&other, &app, "main.py").is_none(),
            "{grant:?}"
        );
    }
    let broad = f.roots.clone().with_grant(Some(f.drive.clone()));
    assert!(resolve_write(&broad, &f.drive, "x.txt").is_none());
    assert!(!broad.judge_write(&f.drive.join("x.txt")));
}

// The previous behaviour of `resolve_write` holds under HOME whatever the
// grant: a project there takes writes, its `.git` does not.
#[test]
fn a_grant_changes_nothing_under_home() {
    let f = fixture();
    let dev = f.home.join("dev");
    let app = f.drive.join("Users/ana/Documents/app");
    for roots in [f.roots.clone(), f.roots.clone().with_grant(Some(app))] {
        let target = resolve_write(&roots, &dev, "main.rs").map(|t| t.real);
        assert_eq!(target, Some(dev.join("main.rs")));
        assert!(resolve_write(&roots, &dev, ".git/config").is_none());
        assert!(roots.judge_write(&dev.join("main.rs")));
    }
}
