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
