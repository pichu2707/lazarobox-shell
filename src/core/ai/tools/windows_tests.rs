use std::path::{Path, PathBuf};

use super::*;

const MOUNTS: &str = "\
none /mnt/wsl tmpfs rw,relatime 0 0
drvfs /mnt/c 9p rw,noatime,dirsync,aname=drvfs;path=C:\\;uid=1000;gid=1000,trans=fd 0 0
D:\\134 /mnt/d drvfs rw,noatime,uid=1000 0 0
drvfs /mnt/cd 9p rw,aname=drvfs;path=X:\\ 0 0
drvfs /mnt/c/x 9p rw,aname=drvfs;path=Y:\\ 0 0
/dev/sdc / ext4 rw,relatime 0 0
tools /mnt/e 9p rw,aname=tools 0 0
";

fn drives(profile: Option<&str>) -> WinDrives {
    WinDrives::new(
        vec![PathBuf::from("/mnt/c"), PathBuf::from("/mnt/d")],
        profile.map(PathBuf::from),
    )
}

/// The `$HOME` secret rules as `paths` passes them: `.ssh` and `.env` refused.
fn rest_ok(rest: &Path) -> bool {
    !rest.components().any(|part| {
        let name = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        name == ".ssh" || name == ".env"
    })
}

fn verdict(d: &WinDrives, path: &str) -> DriveVerdict {
    d.judge(Path::new(path), rest_ok)
}

#[test]
fn mounts_keep_only_drvfs_drive_letters() {
    assert_eq!(
        parse_mounts(MOUNTS),
        vec![PathBuf::from("/mnt/c"), PathBuf::from("/mnt/d")]
    );
    assert!(parse_mounts("").is_empty());
    assert!(parse_mounts("garbage\n\n  \n").is_empty());
}

#[test]
fn profile_is_translated_to_its_drive() {
    assert_eq!(
        parse_profile("C:\\Users\\javier.lazaro\r\n"),
        Some(PathBuf::from("/mnt/c/Users/javier.lazaro"))
    );
    assert_eq!(
        parse_profile("D:\\Users\\Ana"),
        Some(PathBuf::from("/mnt/d/Users/Ana"))
    );
}

#[test]
fn unexpected_profiles_are_refused() {
    for out in [
        "",
        "%USERPROFILE%",
        "\\\\server\\share\\ana",
        "C:\\Users\\a\\b",
        "C:\\Users\\..",
        "C:\\Users\\.",
        "C:\\Users\\",
        "C:\\Windows\\ana",
        "CC:\\Users\\ana",
        "C:\\Users\\a/b",
        "C:\\Users\\a%b",
        "C:\\Users\\a\0b",
        "C:\\Users\\ana\nC:\\Users\\bob",
    ] {
        assert_eq!(parse_profile(out), None, "{out:?}");
    }
}

#[test]
fn projects_on_a_drive_are_readable() {
    let d = drives(Some("/mnt/c/Users/ana"));
    for path in [
        "/mnt/c",
        "/mnt/c/Users",
        "/mnt/c/Users/ana",
        "/mnt/c/Users/ana/Documents/app/README.md",
        "/mnt/c/users/ANA/documents",
        "/mnt/c/Users/Public/Documents/x",
        "/mnt/d/work/notes.txt",
    ] {
        assert_eq!(verdict(&d, path), DriveVerdict::Allowed, "{path}");
    }
}

#[test]
fn windows_folders_are_refused_in_any_case() {
    let d = drives(Some("/mnt/c/Users/ana"));
    for path in [
        "/mnt/c/Windows",
        "/mnt/c/WINDOWS/win.ini",
        "/mnt/c/windows/System32/config/SAM",
        "/mnt/c/Program Files/app",
        "/mnt/c/program files (x86)/app",
        "/mnt/c/ProgramData/x",
        "/mnt/c/$Recycle.Bin/x",
        "/mnt/c/System Volume Information",
        "/mnt/c/Recovery",
        "/mnt/c/pagefile.sys",
        "/mnt/d/HIBERFIL.SYS",
        "/mnt/c/swapfile.sys",
    ] {
        assert_eq!(verdict(&d, path), DriveVerdict::Refused, "{path}");
    }
}

#[test]
fn profile_secrets_and_other_profiles_are_refused() {
    let d = drives(Some("/mnt/c/Users/ana"));
    for path in [
        "/mnt/c/Users/ana/AppData/Local/x",
        "/mnt/c/Users/ana/appdata",
        "/mnt/c/Users/ana/NTUSER.DAT",
        "/mnt/c/Users/ana/ntuser.dat.LOG1",
        "/mnt/c/Users/ana/.SSH/id_rsa",
        "/mnt/c/Users/Public/AppData/x",
        "/mnt/c/Users/bob",
        "/mnt/c/Users/bob/Documents/x",
        "/mnt/d/Users/ana/Documents/x",
        "/mnt/d/work/.env",
    ] {
        assert_eq!(verdict(&d, path), DriveVerdict::Refused, "{path}");
    }
}

#[test]
fn an_unknown_profile_leaves_only_public() {
    let d = drives(None);
    assert_eq!(
        verdict(&d, "/mnt/c/Users/Public/Documents/x"),
        DriveVerdict::Allowed
    );
    assert_eq!(
        verdict(&d, "/mnt/c/Users/ana/Documents/x"),
        DriveVerdict::Refused
    );
}

#[test]
fn paths_off_the_drives_are_not_judged() {
    let d = drives(Some("/mnt/c/Users/ana"));
    for path in ["/mnt/wsl/x", "/mnt/cd/x", "/mnt", "/home/ana/x", "/tmp/x"] {
        assert_eq!(verdict(&d, path), DriveVerdict::NotOnDrive, "{path}");
    }
    let none = WinDrives::default();
    assert_eq!(verdict(&none, "/mnt/c/x"), DriveVerdict::NotOnDrive);
    assert_eq!(none.drive_of(Path::new("/mnt/c/x")), None);
}

#[test]
fn broad_drive_folders() {
    let d = drives(Some("/mnt/c/Users/ana"));
    for path in [
        "/mnt/c",
        "/mnt/d",
        "/mnt/c/Users",
        "/mnt/c/USERS",
        "/mnt/c/Users/ana",
        "/mnt/c/Users/Public",
    ] {
        assert!(d.is_broad(Path::new(path)), "{path}");
    }
    for path in [
        "/mnt/c/Users/ana/Documents",
        "/mnt/d/work",
        "/home/ana",
        "/mnt/cd",
    ] {
        assert!(!d.is_broad(Path::new(path)), "{path}");
    }
    assert_eq!(
        d.drive_of(Path::new("/mnt/d/work")),
        Some(Path::new("/mnt/d"))
    );
}

// Verifier B1: drvfs resolves DOS 8.3 short names (`PROGRA~3` is
// `ProgramData`, `SSH~1` is `.ssh`), and the real path keeps the short
// spelling, so any alias component is refused.
#[test]
fn short_names_are_refused_on_a_drive() {
    let d = drives(Some("/mnt/c/Users/ana"));
    for path in [
        "/mnt/c/PROGRA~1",
        "/mnt/c/PROGRA~3/x",
        "/mnt/c/SYSTEM~1",
        "/mnt/c/Users/ana/SSH~1/id_rsa",
        "/mnt/c/Users/ana/LOCAL~1/share/keyrings",
        "/mnt/c/Users/ana/NTUSER~1.BLF",
        "/mnt/c/Users/ana/Documents/MYPROJ~2/x",
        "/mnt/c/Users/JAVIER~1",
    ] {
        assert_eq!(verdict(&d, path), DriveVerdict::Refused, "{path}");
    }
    // A tilde that is not an alias is an ordinary name.
    assert_eq!(
        verdict(&d, "/mnt/c/Users/ana/Documents/notes~old.txt"),
        DriveVerdict::Allowed
    );
}
