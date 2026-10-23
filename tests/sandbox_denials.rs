//! The rule the macOS real-codex test applies to the Sandbox log: a denial of
//! a jailed process that is not on the known list fails the test. Plain
//! string matching, so it runs on every system.

#[path = "support/sandbox_denials.rs"]
mod sandbox_denials;

use sandbox_denials::{Denial, normalize, parse, unexpected};

/// Lines of `log stream --style compact` on the CI runners (codex 0.153.4,
/// arm64 and Intel), one of each kind of denial codex hit there.
const REAL_LOG: &str = "\
2026-10-23 08:50:18.399 E  kernel[0:2726] (Sandbox) Sandbox: PasswordBreachAgent(2405) deny(1) file-write-create /Users/runner/Library/Caches/com.apple.Safari.PasswordBreachAgent
2026-10-23 08:50:18.411 E  kernel[0:26c6] (Sandbox) Sandbox: dataaccessd(2406) deny(1) file-read-data /private/var/db/mds/messages/501/se_SecurityMessages
2026-10-23 08:50:20.727 E  kernel[0:15d7] (Sandbox) Sandbox: proactived(2551) deny(1) mach-lookup com.apple.donotdisturb.service.non-launching
2026-10-23 08:50:21.478 E  kernel[0:29b2] (Sandbox) Sandbox: codex(2557) deny(1) file-read-data /Library/Preferences/Logging/com.apple.diagnosticd.filter.plist
2026-10-23 08:50:21.478 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) mach-lookup com.apple.diagnosticd
2026-10-23 08:50:21.629 E  kernel[0:29b8] (Sandbox) 26 duplicate reports for Sandbox: codex(2557) deny(1) mach-lookup com.apple.diagnosticd
2026-10-23 08:50:21.629 E  kernel[0:29b8] (Sandbox) Sandbox: codex(2557) deny(1) file-read-data /Users/runner/Library/Preferences/ByHost/.GlobalPreferences.CC823D3A-9E6A-5290-A6DA-4909943BD79E.plist
2026-10-23 08:50:21.630 E  kernel[0:29b8] (Sandbox) Sandbox: codex(2557) deny(1) file-read-data /Users/runner/Library/Preferences/.GlobalPreferences.plist
2026-10-23 08:50:21.630 E  kernel[0:29b8] (Sandbox) Sandbox: codex(2557) deny(1) file-read-data /Users/runner/Library/Preferences/.GlobalPreferences_m.plist
2026-10-23 08:50:21.631 E  kernel[0:29b8] (Sandbox) Sandbox: codex(2557) deny(1) file-read-data /Library/Preferences/.GlobalPreferences.plist
2026-10-23 08:50:22.202 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) mach-lookup com.apple.CoreServices.coreservicesd
2026-10-23 08:50:22.207 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) mach-lookup com.apple.DiskArbitration.diskarbitrationd
2026-10-23 08:50:22.210 E  kernel[0:15d7] (Sandbox) Sandbox: codex(2557) deny(1) mach-lookup com.apple.FSEvents
2026-10-23 08:50:22.300 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) mach-lookup com.apple.analyticsd
2026-10-23 08:50:22.301 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) mach-lookup com.apple.analyticsd.messagetracer
2026-10-23 08:50:22.302 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) mach-lookup com.apple.system.logger
2026-10-23 08:50:22.303 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) system-socket domain:32 type:2 protocol:2
2026-10-23 08:50:22.310 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) process-exec* /opt/homebrew/Cellar/git/2.55.0/bin/git
2026-10-23 08:50:22.311 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2558) deny(1) process-exec* /usr/local/Cellar/git/2.55.0/bin/git
2026-10-23 08:50:22.304 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) sysctl-read net.routetable.0.0.3.0
2026-10-23 08:50:22.305 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) sysctl-read machdep.ptrauth_enabled
2026-10-23 08:50:22.306 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) ipc-posix-shm-write-create com.apple.AppleDatabaseChanged
2026-10-23 08:50:22.307 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) file-read-data /Library/Preferences/com.apple.security.plist
2026-10-23 08:50:22.308 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) file-read-data /Library/Preferences/com.apple.networkd.plist
2026-10-23 08:50:22.309 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) file-read-data /private/var/db/mds/messages/501/se_SecurityMessages
";

#[test]
fn a_denial_line_is_read_into_process_operation_and_target() {
    let line = "2026-10-23 08:50:21.478 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) mach-lookup com.apple.diagnosticd";
    assert_eq!(
        parse(line),
        Some(Denial {
            process: "codex".into(),
            operation: "mach-lookup".into(),
            target: "com.apple.diagnosticd".into(),
        })
    );
    let duplicate = "2026-10-23 08:50:21.629 E  kernel[0:29b8] (Sandbox) 26 duplicate reports for Sandbox: codex(2557) deny(1) mach-lookup com.apple.diagnosticd";
    assert_eq!(parse(duplicate), parse(line));
}

#[test]
fn lines_that_are_not_a_denial_are_skipped() {
    for line in [
        "",
        "Filtering the log data using \"sender == \\\"Sandbox\\\"\"",
        "2026-10-23 08:50:21.478 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) allow mach-lookup x",
        "Sandbox: codex deny(1) mach-lookup x",
    ] {
        assert_eq!(parse(line), None, "{line:?}");
    }
}

#[test]
fn the_real_runs_have_no_unexpected_denial() {
    assert_eq!(unexpected(REAL_LOG), Vec::<String>::new());
}

#[test]
fn a_new_denial_of_codex_is_unexpected() {
    let log = format!(
        "{REAL_LOG}2026-10-23 08:50:23.000 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) file-read-data /Users/runner/.config/gh/hosts.yml\n\
         2026-10-23 08:50:23.001 E  kernel[0:5ff] (Sandbox) Sandbox: codex(2557) deny(1) mach-lookup com.apple.securityd\n"
    );
    assert_eq!(
        unexpected(&log),
        vec![
            "codex file-read-data ~/.config/gh/hosts.yml".to_owned(),
            "codex mach-lookup com.apple.securityd".to_owned(),
        ]
    );
}

#[test]
fn a_known_operation_on_another_target_is_unexpected() {
    let log = "Sandbox: codex(1) deny(1) file-read-data /Library/Preferences/com.apple.other.plist\n\
               Sandbox: codex(1) deny(1) system-socket domain:2\n";
    assert_eq!(unexpected(log).len(), 2, "{:?}", unexpected(log));
}

#[test]
fn a_child_of_the_jail_counts_like_codex() {
    for process in ["node", "lazarobox-shell"] {
        let log = format!("Sandbox: {process}(7) deny(1) file-write-create /Users/runner/x\n");
        assert_eq!(
            unexpected(&log),
            vec![format!("{process} file-write-create ~/x")]
        );
    }
}

#[test]
fn processes_outside_the_jail_are_ignored() {
    let log = "Sandbox: PasswordBreachAgent(2405) deny(1) file-write-create /Users/runner/Library/Caches/x\n\
               Sandbox: coreduetd(9) deny(1) mach-lookup com.apple.biome.compute.source\n";
    assert_eq!(unexpected(log), Vec::<String>::new());
}

#[test]
fn each_unexpected_denial_is_listed_once() {
    let line = "Sandbox: codex(1) deny(1) mach-lookup com.apple.securityd\n";
    assert_eq!(unexpected(&line.repeat(3)).len(), 1);
}

#[test]
fn a_target_keeps_its_spaces() {
    let line = "Sandbox: codex(1) deny(1) system-socket domain:32 type:2 protocol:2";
    assert_eq!(parse(line).unwrap().target, "domain:32 type:2 protocol:2");
}

#[test]
fn the_machine_specific_parts_of_a_target_are_normalized() {
    for (target, want) in [
        ("/Users/runner/Library/Caches/x", "~/Library/Caches/x"),
        (
            "/Users/runner/Library/Preferences/ByHost/.GlobalPreferences.CC823D3A-9E6A-5290-A6DA-4909943BD79E.plist",
            "~/Library/Preferences/ByHost/.GlobalPreferences.<uuid>.plist",
        ),
        (
            "/private/var/db/mds/messages/501/se_SecurityMessages",
            "/private/var/db/mds/messages/<uid>/se_SecurityMessages",
        ),
        (
            "/opt/homebrew/Cellar/git/2.56.1/bin/git",
            "/opt/homebrew/Cellar/git/<version>/bin/git",
        ),
        ("/usr/bin/git", "/usr/bin/git"),
    ] {
        assert_eq!(normalize(target), want, "{target}");
    }
}

#[test]
fn another_program_run_by_codex_is_unexpected() {
    let log = "Sandbox: codex(1) deny(1) process-exec* /opt/homebrew/Cellar/gh/2.80.0/bin/gh\n\
               Sandbox: codex(1) deny(1) process-exec* /usr/bin/git\n";
    assert_eq!(unexpected(log).len(), 2, "{:?}", unexpected(log));
}
