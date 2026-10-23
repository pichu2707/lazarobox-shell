//! How the real-codex test reads the Sandbox log. Pure string rules, kept
//! apart from the macOS-only test so they run (and are tested) on every system.
//!
//! `log stream` reports the denials of every sandboxed process on the system,
//! so only the processes of the jail count. Of theirs, the known ones below
//! are what codex hits on a healthy run and does fine without; any other
//! denial fails the test, so a newly denied read that codex tolerates is
//! looked at instead of going unnoticed.

/// The processes that run inside the jail: codex itself, a node that starts
/// an npm install of it, and the `__jail-exec` helper before it execs codex.
pub const JAILED: [&str; 3] = ["codex", "node", "lazarobox-shell"];

/// `(operation, target)` denials of a healthy codex 0.153.4 turn on the CI
/// runners (arm64 and Intel). Targets are normalized by [`normalize`]: the
/// home folder is `~`, and the per-machine parts are `<uuid>`, `<uid>` and
/// `<version>`.
pub const KNOWN: [(&str, &str); 21] = [
    // Logging, analytics and file-system services codex's runtime asks for.
    ("mach-lookup", "com.apple.diagnosticd"),
    ("mach-lookup", "com.apple.system.logger"),
    ("mach-lookup", "com.apple.analyticsd"),
    ("mach-lookup", "com.apple.analyticsd.messagetracer"),
    ("mach-lookup", "com.apple.FSEvents"),
    ("mach-lookup", "com.apple.DiskArbitration.diskarbitrationd"),
    ("mach-lookup", "com.apple.CoreServices.coreservicesd"),
    (
        "ipc-posix-shm-write-create",
        "com.apple.AppleDatabaseChanged",
    ),
    // The network stack's own lookups (AF_SYSTEM, the routing table); the
    // request to OpenAI still goes out.
    ("system-socket", "domain:32 type:2 protocol:2"),
    ("sysctl-read", "net.routetable.0.0.3.0"),
    ("sysctl-read", "machdep.ptrauth_enabled"),
    // System and user preferences, read by the frameworks at start-up.
    (
        "file-read-data",
        "/Library/Preferences/.GlobalPreferences.plist",
    ),
    (
        "file-read-data",
        "/Library/Preferences/com.apple.security.plist",
    ),
    (
        "file-read-data",
        "/Library/Preferences/com.apple.networkd.plist",
    ),
    (
        "file-read-data",
        "/Library/Preferences/Logging/com.apple.diagnosticd.filter.plist",
    ),
    (
        "file-read-data",
        "~/Library/Preferences/.GlobalPreferences.plist",
    ),
    (
        "file-read-data",
        "~/Library/Preferences/.GlobalPreferences_m.plist",
    ),
    (
        "file-read-data",
        "~/Library/Preferences/ByHost/.GlobalPreferences.<uuid>.plist",
    ),
    (
        "file-read-data",
        "/private/var/db/mds/messages/<uid>/se_SecurityMessages",
    ),
    // codex runs `git` for the context of the working folder. The runners'
    // git is Homebrew's (Apple Silicon and Intel prefixes), outside the jail,
    // and the turn works without it.
    (
        "process-exec*",
        "/opt/homebrew/Cellar/git/<version>/bin/git",
    ),
    ("process-exec*", "/usr/local/Cellar/git/<version>/bin/git"),
];

/// One denial: who, what operation, on what.
#[derive(Debug, PartialEq, Eq)]
pub struct Denial {
    pub process: String,
    pub operation: String,
    pub target: String,
}

/// The denial in one line of `log stream --style compact`, also in a
/// `N duplicate reports for ...` line. `None` for any other line.
pub fn parse(line: &str) -> Option<Denial> {
    let (_, rest) = line.split_once("Sandbox: ")?;
    let (process, rest) = rest.split_once('(')?;
    let (pid, rest) = rest.split_once(") deny(")?;
    if process.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (_, rest) = rest.split_once(") ")?;
    let (operation, target) = rest.trim_end().split_once(' ')?;
    Some(Denial {
        process: process.to_owned(),
        operation: operation.to_owned(),
        target: target.to_owned(),
    })
}

/// `target` with the machine-specific parts replaced: `/Users/<name>/` is
/// `~/`, the ByHost preferences UUID is `<uuid>`, the user id is `<uid>`, the
/// version folder of a Homebrew formula is `<version>`.
pub fn normalize(target: &str) -> String {
    let mut target = match target.strip_prefix("/Users/") {
        Some(rest) => match rest.split_once('/') {
            Some((_, below)) => format!("~/{below}"),
            None => "~".to_owned(),
        },
        None => target.to_owned(),
    };
    const BYHOST: &str = "~/Library/Preferences/ByHost/.GlobalPreferences.";
    if target.starts_with(BYHOST) && target.ends_with(".plist") {
        target = format!("{BYHOST}<uuid>.plist");
    }
    const MDS: &str = "/private/var/db/mds/messages/";
    if let Some(rest) = target.strip_prefix(MDS)
        && let Some((uid, file)) = rest.split_once('/')
        && !uid.is_empty()
        && uid.bytes().all(|b| b.is_ascii_digit())
    {
        target = format!("{MDS}<uid>/{file}");
    }
    if let Some((prefix, rest)) = target.split_once("/Cellar/")
        && let Some((formula, rest)) = rest.split_once('/')
        && let Some((_, below)) = rest.split_once('/')
    {
        target = format!("{prefix}/Cellar/{formula}/<version>/{below}");
    }
    target
}

/// The denials of jailed processes in `log` that are not known, each once
/// as `process operation target` (normalized), in the order first seen.
pub fn unexpected(log: &str) -> Vec<String> {
    let mut found = Vec::new();
    for denial in log.lines().filter_map(parse) {
        if !JAILED.contains(&denial.process.as_str()) {
            continue;
        }
        let target = normalize(&denial.target);
        let known = KNOWN
            .iter()
            .any(|(operation, known)| *operation == denial.operation && *known == target);
        let line = format!("{} {} {target}", denial.process, denial.operation);
        if !known && !found.contains(&line) {
            found.push(line);
        }
    }
    found
}
