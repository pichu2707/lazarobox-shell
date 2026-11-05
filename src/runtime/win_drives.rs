//! The Windows drives of WSL for the tools' path policy (openspec
//! `wsl-drive-access`): the drvfs mounts, read once from `/proc/mounts`, and
//! the user's own Windows profile, asked of Windows once in the background.
//! Until that answer arrives, or when it fails, only `Public` is readable
//! among the profiles. Everywhere but WSL there are no drives.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::core::{
    ai::tools::windows::{WinDrives, parse_mounts, parse_profile},
    desktop::is_wsl,
};

/// Longest the `%USERPROFILE%` call may take.
const PROFILE_TIMEOUT: Duration = Duration::from_secs(2);

static MOUNTS: OnceLock<Vec<PathBuf>> = OnceLock::new();
static PROFILE: OnceLock<PathBuf> = OnceLock::new();

/// The drives as known now. None in tests, so they never depend on the
/// machine that runs them; a test hands its own through `ToolEnv`.
pub fn drives() -> WinDrives {
    if cfg!(test) {
        return WinDrives::default();
    }
    WinDrives::new(mounts().clone(), PROFILE.get().cloned())
}

fn mounts() -> &'static Vec<PathBuf> {
    MOUNTS.get_or_init(|| {
        let wsl = is_wsl(
            std::env::var("WSL_DISTRO_NAME").ok().as_deref(),
            std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .ok()
                .as_deref(),
        );
        if !wsl {
            return Vec::new();
        }
        std::fs::read_to_string("/proc/mounts")
            .map(|text| parse_mounts(&text))
            .unwrap_or_default()
    })
}

/// Asks Windows for `%USERPROFILE%` on a thread of its own; nothing waits for
/// it. Does nothing off WSL or when no drive is mounted.
pub fn probe_profile_in_background() {
    if cfg!(test) {
        return;
    }
    let Some(drive) = mounts().first().cloned() else {
        return;
    };
    std::thread::spawn(move || {
        if let Some(profile) = ask_profile(&drive) {
            let _ = PROFILE.set(profile);
        }
    });
}

/// Runs `cmd.exe /d /c echo %USERPROFILE%` from a drive folder (a Linux cwd
/// would make cmd.exe print a UNC warning), killed after
/// [`PROFILE_TIMEOUT`].
fn ask_profile(drive: &Path) -> Option<PathBuf> {
    let mut child = Command::new("cmd.exe")
        .args(["/d", "/c", "echo %USERPROFILE%"])
        .current_dir(drive)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < PROFILE_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut out = String::new();
    child
        .stdout
        .take()?
        .take(4096)
        .read_to_string(&mut out)
        .ok()?;
    status.success().then(|| parse_profile(&out)).flatten()
}
