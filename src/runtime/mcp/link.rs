//! The checks behind Link with Google for `gsc` (S6b1): the client file and
//! `uv`, run by lazarobox as direct argv (no shell) with a 5-second limit,
//! then the login line built from the resolved values. The line is only
//! returned: the menu shows it at the `y`, and the user's shell runs it.
//! Nothing here spawns the server or opens a token file.

use std::{
    ffi::OsStr,
    io::{ErrorKind, Read},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use crate::core::ai::mcp_catalog::gsc::{self, Plan};

/// Why one `uv` call gave no output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fail {
    NotFound,
    Other(&'static str),
}

pub const NO_CLIENT: &str = "add your Desktop OAuth client first (o)";
const LIMIT: Duration = Duration::from_secs(5);
/// The most output read from one call.
const OUT_MAX: u64 = 64 * 1024;

/// Runs `program` with `args`, no shell, and gives its standard output.
pub fn run_with(program: &OsStr, args: &[&str], limit: Duration) -> Result<String, Fail> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => Fail::NotFound,
            _ => Fail::Other("uv failed"),
        })?;
    let end = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Fail::Other("uv did not answer in 5 s"));
            }
        }
    };
    let mut out = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(OUT_MAX).read_to_end(&mut out);
    }
    match status.success() {
        true => Ok(String::from_utf8_lossy(&out).into_owned()),
        false => Err(Fail::Other("uv failed")),
    }
}

/// The real runner: `uv` from `PATH`, five seconds at most.
pub fn uv(args: &[&str]) -> Result<String, Fail> {
    run_with(OsStr::new("uv"), args, LIMIT)
}

/// The line to show, or the notice of the first check that fails.
pub fn check(
    plan: &Plan,
    run: &impl Fn(&[&str]) -> Result<String, Fail>,
    exists: impl Fn(&Path) -> bool,
) -> Result<String, String> {
    if !exists(Path::new(&plan.file)) {
        return Err(NO_CLIENT.to_owned());
    }
    let said = |fail: Fail| match fail {
        Fail::NotFound => gsc::NO_UV.to_owned(),
        Fail::Other(why) => why.to_owned(),
    };
    gsc::pinned(&run(&["tool", "list"]).map_err(said)?)?;
    let dir = run(&["tool", "dir"]).map_err(said)?;
    let dir = gsc::tool_dir(&dir).ok_or_else(|| gsc::NO_TOOL_DIR.to_owned())?;
    gsc::login_line(&dir, plan).map_err(str::to_owned)
}

#[cfg(test)]
mod tests;
