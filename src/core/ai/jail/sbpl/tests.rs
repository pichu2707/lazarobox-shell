use std::ffi::OsString;
use std::path::PathBuf;

use super::*;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn spec(rx: &[&str], ro: &[&str], rw: &[&str], rof: &[&str], rwf: &[&str]) -> JailSpec {
    let list = |items: &[&str]| items.iter().map(|s| p(s)).collect();
    JailSpec {
        rx: list(rx),
        ro: list(ro),
        rw: list(rw),
        ro_files: list(rof),
        rw_files: list(rwf),
    }
}

/// One path in every class.
fn one_each() -> JailSpec {
    spec(
        &["/opt/homebrew/Cellar/codex/1"],
        &["/Users/u/.codex/skills"],
        &["/Users/u/work"],
        &["/dev/urandom"],
        &["/dev/null"],
    )
}

const GOLDEN: &str = r#"(version 1)
(deny default)
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target self))
(allow file-read-metadata)
(allow file-read* file-map-executable
  (subpath "/usr/lib") (subpath "/System/Library")
  (subpath "/System/Volumes/Preboot/Cryptexes")
  (require-all (subpath "/opt/homebrew") (require-not (subpath "/opt/homebrew/var")) (require-not (subpath "/opt/homebrew/etc")))
  (require-all (subpath "/usr/local") (require-not (subpath "/usr/local/var")) (require-not (subpath "/usr/local/etc"))))
(allow file-read* (subpath "/usr/share") (subpath "/private/etc") (subpath "/private/var/db/timezone"))
(allow file-read* (literal "/private/var/run/resolv.conf"))
(allow file-read* file-map-executable process-exec (subpath (param "RX_0")))
(allow file-read* file-map-executable (subpath (param "RO_0")))
(allow file-read* file-write* (subpath (param "RW_0")))
(allow file-read* (literal (param "ROF_0")))
(allow file-read* file-write* (literal (param "RWF_0")))
(allow network-outbound (literal (param "RWF_0")))
(allow network-outbound (remote ip "*:*"))
(allow network-outbound (literal "/private/var/run/mDNSResponder"))
(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.notification_center") (global-name "com.apple.logd")
  (global-name "com.apple.dnssd.service") (global-name "com.apple.trustd.agent")
  (global-name "com.apple.SystemConfiguration.configd")
  (global-name "com.apple.SystemConfiguration.DNSConfiguration"))
(allow ipc-posix-shm-read-data (ipc-posix-name "apple.shm.notification_center"))
(allow sysctl-read (sysctl-name-prefix "hw.optional.") (sysctl-name-prefix "hw.perflevel")
  (sysctl-name-prefix "machdep.cpu.") (sysctl-name "hw.activecpu") (sysctl-name "hw.ncpu")
  (sysctl-name "hw.logicalcpu") (sysctl-name "hw.logicalcpu_max") (sysctl-name "hw.physicalcpu")
  (sysctl-name "hw.physicalcpu_max") (sysctl-name "hw.memsize") (sysctl-name "hw.pagesize")
  (sysctl-name "hw.pagesize_compat") (sysctl-name "hw.cachelinesize") (sysctl-name "hw.byteorder")
  (sysctl-name "hw.cputype") (sysctl-name "hw.cpusubtype") (sysctl-name "hw.cpufamily")
  (sysctl-name "hw.machine") (sysctl-name "hw.model") (sysctl-name "hw.nperflevels")
  (sysctl-name "kern.ostype") (sysctl-name "kern.osrelease") (sysctl-name "kern.osversion")
  (sysctl-name "kern.osproductversion") (sysctl-name "kern.version") (sysctl-name "kern.hostname")
  (sysctl-name "kern.boottime") (sysctl-name "kern.argmax") (sysctl-name "kern.maxfilesperproc")
  (sysctl-name "kern.usrstack64") (sysctl-name "kern.secure_kernel") (sysctl-name "vm.loadavg")
  (sysctl-name "sysctl.proc_cputype") (sysctl-name "sysctl.proc_translated")
  (sysctl-name "security.mac.lockdown_mode_state"))
"#;

#[test]
fn the_whole_profile_is_pinned_for_a_fixed_spec() {
    let profile = render(&one_each()).unwrap();
    assert_eq!(profile.text, GOLDEN);
}

#[test]
fn params_carry_each_path_under_its_key_in_text_order() {
    let profile = render(&one_each()).unwrap();
    let want = [
        ("RX_0", "/opt/homebrew/Cellar/codex/1"),
        ("RO_0", "/Users/u/.codex/skills"),
        ("RW_0", "/Users/u/work"),
        ("ROF_0", "/dev/urandom"),
        ("RWF_0", "/dev/null"),
    ];
    let got: Vec<(&str, &str)> = profile
        .params
        .iter()
        .map(|(k, v)| (k.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn several_entries_in_a_class_are_numbered_in_spec_order() {
    let s = spec(
        &["/a", "/b", "/c"],
        &[],
        &["/w0", "/w1"],
        &[],
        &["/s0", "/s1"],
    );
    let profile = render(&s).unwrap();
    let keys: Vec<&str> = profile.params.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        ["RX_0", "RX_1", "RX_2", "RW_0", "RW_1", "RWF_0", "RWF_1"]
    );
    assert_eq!(profile.params[2].1, p("/c"));
    // A key is in the text once per rule that names it.
    assert_eq!(profile.text.matches("(param \"RX_2\")").count(), 1);
    assert_eq!(profile.text.matches("(param \"RWF_1\")").count(), 2);
}

#[test]
fn every_key_matches_the_allowed_alphabet() {
    let s = spec(&["/a"], &["/b"], &["/c"], &["/d"], &["/e"]);
    for (key, _) in render(&s).unwrap().params {
        let (name, n) = key.split_once('_').unwrap();
        assert!(!name.is_empty() && name.bytes().all(|b| b.is_ascii_uppercase()));
        assert!(!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    }
}

#[test]
fn an_empty_spec_renders_only_the_fixed_rules() {
    let profile = render(&spec(&[], &[], &[], &[], &[])).unwrap();
    assert!(profile.params.is_empty());
    assert!(!profile.text.contains("(param "));
    assert!(profile.text.starts_with("(version 1)\n(deny default)\n"));
}

#[test]
fn hostile_paths_leave_the_text_byte_identical() {
    let plain = render(&spec(&["/a"], &["/b"], &["/c"], &["/d"], &["/e"])).unwrap();
    for hostile in [
        "/x\")(allow file-read* (subpath \"/\"))(\"",
        "/x)",
        "/x;(allow default)",
        "/x\n(allow default)",
        "/x\\\"",
        "/x=y",
    ] {
        let s = spec(&[hostile], &[hostile], &[hostile], &[hostile], &[hostile]);
        let profile = render(&s).unwrap();
        assert_eq!(
            profile.text, plain.text,
            "path {hostile:?} changed the text"
        );
        assert!(profile.params.iter().all(|(_, v)| v == &p(hostile)));
    }
}

#[test]
fn a_hostile_path_travels_as_one_argv_element() {
    let hostile = "/x\")\n;=y";
    let profile = render(&spec(&[hostile], &[], &[], &[], &[])).unwrap();
    let args = sandbox_args(&profile);
    assert_eq!(args[0], OsString::from("-p"));
    assert_eq!(args[1], OsString::from(&profile.text));
    assert_eq!(args[2], OsString::from("-D"));
    assert_eq!(args[3], OsString::from(format!("RX_0={hostile}")));
    assert_eq!(args.len(), 4);
}

#[test]
fn sandbox_args_pairs_every_param_with_its_own_flag() {
    let profile = render(&one_each()).unwrap();
    let args = sandbox_args(&profile);
    assert_eq!(args.len(), 2 + 2 * profile.params.len());
    for (i, (key, path)) in profile.params.iter().enumerate() {
        assert_eq!(args[2 + 2 * i], OsString::from("-D"));
        assert_eq!(
            args[3 + 2 * i],
            OsString::from(format!("{key}={}", path.display()))
        );
    }
}

#[test]
fn a_nul_byte_is_rejected() {
    let bad = "/a\0b";
    let err = render(&spec(&[], &[], &[], &[], &[bad])).unwrap_err();
    assert_eq!(err, SbplError::Nul(p(bad)));
}

#[test]
fn a_relative_path_is_rejected() {
    let err = render(&spec(&[], &["rel/dir"], &[], &[], &[])).unwrap_err();
    assert_eq!(err, SbplError::Relative(p("rel/dir")));
}

#[test]
fn a_path_over_1024_bytes_is_rejected_and_1024_is_not() {
    let at_limit = format!("/{}", "a".repeat(PATH_MAX - 1));
    assert_eq!(at_limit.len(), 1024);
    assert!(render(&spec(&[&at_limit], &[], &[], &[], &[])).is_ok());
    let over = format!("{at_limit}a");
    let err = render(&spec(&[], &[], &[&over], &[], &[])).unwrap_err();
    assert_eq!(err, SbplError::TooLong(p(&over)));
}

#[test]
fn more_than_256_params_are_rejected_and_256_are_not() {
    let many: Vec<String> = (0..257).map(|n| format!("/d{n}")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(render(&spec(&refs[..256], &[], &[], &[], &[])).is_ok());
    // The count is across classes, not per class.
    let err = render(&spec(
        &refs[..100],
        &refs[..100],
        &refs[..55],
        &refs[..1],
        &refs[..1],
    ))
    .unwrap_err();
    assert_eq!(err, SbplError::TooMany(257));
    // Each file class counts on its own: 256 in total is still fine.
    assert!(
        render(&spec(
            &refs[..100],
            &refs[..100],
            &refs[..54],
            &refs[..1],
            &refs[..1]
        ))
        .is_ok()
    );
}

#[test]
fn multibyte_paths_are_measured_in_bytes() {
    let long = format!("/{}", "é".repeat(600));
    assert_eq!(long.chars().count(), 601);
    assert_eq!(long.len(), 1201);
    let err = render(&spec(&[&long], &[], &[], &[], &[])).unwrap_err();
    assert_eq!(err, SbplError::TooLong(p(&long)));
    // 1 + 511 two-byte chars + 1 one-byte char = 1024 bytes: accepted.
    let at_limit = format!("/{}a", "é".repeat(511));
    assert_eq!(at_limit.len(), 1024);
    assert!(render(&spec(&[], &[&at_limit], &[], &[], &[])).is_ok());
    let over = format!("/{}", "é".repeat(512));
    assert_eq!(over.len(), 1025);
    let err = render(&spec(&[], &[], &[&over], &[], &[])).unwrap_err();
    assert_eq!(err, SbplError::TooLong(p(&over)));
}

#[test]
fn an_empty_path_is_relative() {
    let err = render(&spec(&[""], &[], &[], &[], &[])).unwrap_err();
    assert_eq!(err, SbplError::Relative(p("")));
}

#[test]
fn the_root_would_grant_everything_and_is_rejected() {
    for root in ["/", "//"] {
        for class in 0..5 {
            let mut lists: [Vec<&str>; 5] = Default::default();
            lists[class].push(root);
            let s = spec(&lists[0], &lists[1], &lists[2], &lists[3], &lists[4]);
            assert_eq!(render(&s).unwrap_err(), SbplError::Root(p(root)));
        }
    }
}

#[test]
fn dot_and_dot_dot_segments_are_rejected_but_dotted_names_are_not() {
    for bad in [
        "/a/../b",
        "/a/./b",
        "/..",
        "/a/..",
        "/a/.",
        "/./a",
        "/a/b/../..",
    ] {
        let err = render(&spec(&[bad], &[], &[], &[], &[])).unwrap_err();
        assert_eq!(err, SbplError::Unnormalized(p(bad)), "{bad}");
    }
    for ok in ["/a/.b", "/a/..b", "/a/b..", "/.hidden/x", "/a/b.c/d"] {
        assert!(render(&spec(&[ok], &[], &[], &[], &[])).is_ok(), "{ok}");
    }
}

#[cfg(unix)]
#[test]
fn sandbox_args_passes_the_raw_bytes_of_a_non_utf8_path() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let raw = b"/a\xffb";
    let path = PathBuf::from(OsStr::from_bytes(raw));
    let profile = Profile {
        text: String::new(),
        params: vec![("RX_0".to_string(), path)],
    };
    let args = sandbox_args(&profile);
    let mut want = b"RX_0=".to_vec();
    want.extend_from_slice(raw);
    assert_eq!(args[3].clone().into_vec(), want);
}

#[test]
fn the_homebrew_and_usr_local_var_folders_stay_closed() {
    let text = render(&one_each()).unwrap().text;
    assert!(text.contains(
        "(require-all (subpath \"/opt/homebrew\") (require-not (subpath \"/opt/homebrew/var\")) (require-not (subpath \"/opt/homebrew/etc\"))"
    ));
    assert!(text.contains(
        "(require-all (subpath \"/usr/local\") (require-not (subpath \"/usr/local/var\")) (require-not (subpath \"/usr/local/etc\"))"
    ));
}

#[test]
fn the_network_rules_are_outbound_only_and_name_no_other_socket() {
    let s = spec(&["/a"], &[], &[], &[], &["/s0", "/s1"]);
    let text = render(&s).unwrap().text;
    let net: Vec<&str> = text.lines().filter(|l| l.contains("network")).collect();
    assert_eq!(
        net,
        [
            "(allow network-outbound (literal (param \"RWF_0\")))",
            "(allow network-outbound (literal (param \"RWF_1\")))",
            "(allow network-outbound (remote ip \"*:*\"))",
            "(allow network-outbound (literal \"/private/var/run/mDNSResponder\"))",
        ]
    );
    for denied in [
        "network-bind",
        "network-inbound",
        "network*",
        "(remote unix",
    ] {
        assert!(!text.contains(denied), "{denied} must not appear");
    }
}

#[test]
fn network_outbound_is_never_granted_on_a_folder_or_read_only_class() {
    let s = spec(&["/a"], &["/b"], &["/c"], &["/d"], &[]);
    let text = render(&s).unwrap().text;
    assert!(!text.contains("network-outbound (literal (param"));
    assert_eq!(text.matches("network-outbound").count(), 2);
}

#[test]
fn the_system_profile_import_and_the_keychain_door_are_absent() {
    let text = render(&one_each()).unwrap().text;
    assert!(!text.contains("import"));
    assert!(!text.contains("system.sb"));
    assert!(!text.contains("SecurityServer"));
    assert!(!text.contains("user-preference-read"));
    assert!(!text.contains("cfprefsd"));
    assert!(!text.contains("allow default"));
}

#[test]
fn only_process_exec_class_is_rx() {
    let text = render(&one_each()).unwrap().text;
    assert_eq!(text.matches("process-exec").count(), 1);
    assert!(text.contains("process-exec (subpath (param \"RX_0\"))"));
}

#[test]
fn resolv_conf_is_readable_by_its_exact_macos_target_only() {
    let text = render(&one_each()).unwrap().text;
    assert!(text.contains("(allow file-read* (literal \"/private/var/run/resolv.conf\"))"));
    assert!(!text.contains("(subpath \"/private/var/run\")"));
}

#[test]
fn the_homebrew_and_usr_local_etc_folders_stay_closed() {
    let text = render(&one_each()).unwrap().text;
    assert!(text.contains("(require-not (subpath \"/opt/homebrew/etc\"))"));
    assert!(text.contains("(require-not (subpath \"/usr/local/etc\"))"));
}

#[test]
fn a_file_under_homebrew_etc_still_gets_its_own_literal_rule() {
    let cert = "/opt/homebrew/etc/openssl@3/cert.pem";
    let profile = render(&spec(&[], &[], &[], &[cert], &[])).unwrap();
    assert!(
        profile
            .text
            .contains("(allow file-read* (literal (param \"ROF_0\")))")
    );
    assert_eq!(profile.params, [("ROF_0".to_string(), p(cert))]);
    // The fixed block never names the file or an openssl folder.
    assert!(!profile.text.contains("openssl"));
    assert!(!profile.text.contains("ca-certificates"));
}
