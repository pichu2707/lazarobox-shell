//! Reads on a WSL drive through the read tools (openspec `wsl-drive-access`,
//! PR 1): the drive is passed in through `ToolEnv`, never read from the
//! machine.

use std::fs;
use std::path::{Path, PathBuf};

use super::fs::{Failure, Limits, ToolEnv, execute};
use crate::core::ai::tools::{Call, windows::WinDrives};

struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    drive: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for sub in [
        "home",
        "mnt/c/Users/ana/Documents/app",
        "mnt/c/Users/ana/AppData",
        "mnt/c/Users/bob",
        "mnt/c/Users/Public",
    ] {
        fs::create_dir_all(root.join(sub)).unwrap();
    }
    for (file, text) in [
        ("mnt/c/Users/ana/Documents/app/README.md", "hello from C:\n"),
        ("mnt/c/Users/ana/Documents/app/.env", "TOKEN=1\n"),
        ("mnt/c/Users/ana/AppData/token", "secret\n"),
    ] {
        fs::write(root.join(file), text).unwrap();
    }
    Fixture {
        _dir: dir,
        home: root.join("home"),
        drive: root.join("mnt/c"),
    }
}

fn env(f: &Fixture, origin: &Path) -> ToolEnv {
    ToolEnv {
        home: Some(f.home.clone().into()),
        xdg_config_home: None,
        xdg_data_home: None,
        origin: origin.to_path_buf(),
        drives: Some(WinDrives::new(
            vec![f.drive.clone()],
            Some(f.drive.join("Users/ana")),
        )),
    }
}

fn read(env: &ToolEnv, path: &str) -> Result<String, Failure> {
    let call = Call::ReadFile {
        path: path.into(),
        lines: None,
    };
    execute(&call, env, &Limits::default())
}

fn list(env: &ToolEnv, path: &str) -> Result<String, Failure> {
    execute(
        &Call::ListDir { path: path.into() },
        env,
        &Limits::default(),
    )
}

#[test]
fn a_chat_on_the_drive_reads_its_project() {
    let f = fixture();
    let app = f.drive.join("Users/ana/Documents/app");
    let out = read(&env(&f, &app), "README.md").unwrap();
    assert!(out.contains("hello from C:"), "{out}");
}

#[test]
fn refused_drive_files_are_not_read() {
    let f = fixture();
    let app = f.drive.join("Users/ana/Documents/app");
    let env = env(&f, &app);
    for path in [".env", "../../AppData/token"] {
        assert_eq!(read(&env, path), Err(Failure::Refused), "{path}");
    }
}

#[test]
fn listings_on_the_drive_hide_refused_entries() {
    let f = fixture();
    let profile = f.drive.join("Users/ana");
    let out = list(&env(&f, &profile), ".").unwrap();
    assert!(out.contains("Documents"), "{out}");
    assert!(!out.contains("AppData"), "{out}");
    let users = list(&env(&f, &f.drive), "Users").unwrap();
    assert!(users.contains("ana") && users.contains("Public"), "{users}");
    assert!(!users.contains("bob"), "{users}");
    let app = list(&env(&f, &profile), "Documents/app").unwrap();
    assert!(app.contains("README.md") && !app.contains(".env"), "{app}");
}

#[test]
fn without_drives_nothing_on_the_drive_is_read() {
    let f = fixture();
    let app = f.drive.join("Users/ana/Documents/app");
    let env = ToolEnv {
        drives: None,
        ..env(&f, &app)
    };
    assert_eq!(read(&env, "README.md"), Err(Failure::Refused));
}
