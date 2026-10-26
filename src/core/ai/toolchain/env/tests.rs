use std::ffi::OsString;
use std::path::Path;

use super::*;

const ROOT: &str = "/home/u/proj";
const TMP: &str = "/run/lb/tmp/1";

fn parent(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |name| {
        vars.iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| OsString::from(v))
    }
}

fn scrubbed(vars: &[(&str, &str)]) -> Vec<(String, String)> {
    scrub(&parent(vars), Path::new(ROOT), Path::new(TMP))
        .into_iter()
        .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
        .collect()
}

fn get<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

#[test]
fn the_listed_variables_pass_when_they_are_set() {
    let set = [
        ("HOME", "/home/u"),
        ("USER", "u"),
        ("LANG", "C.UTF-8"),
        ("LC_ALL", "C"),
        ("CARGO_HOME", "/home/u/.cargo"),
        ("RUSTUP_HOME", "/home/u/.rustup"),
        ("GOPATH", "/home/u/go"),
        ("GOROOT", "/usr/local/go"),
        ("GOCACHE", "/home/u/.cache/go-build"),
        ("GOMODCACHE", "/home/u/go/pkg/mod"),
    ];
    let env = scrubbed(&set);
    for (name, value) in set {
        assert_eq!(get(&env, name), Some(value), "{name}");
    }
    let env = scrubbed(&[("HOME", "/home/u")]);
    assert_eq!(get(&env, "HOME"), Some("/home/u"));
    for unset in ["USER", "LANG", "GOROOT", "CARGO_HOME"] {
        assert_eq!(get(&env, unset), None, "{unset}");
    }
}

#[test]
fn the_fixed_variables_win_over_the_parent() {
    let env = scrubbed(&[("TMPDIR", "/tmp/evil"), ("CI", "0"), ("TERM", "xterm")]);
    let fixed = [
        ("TMPDIR", TMP),
        ("TERM", "dumb"),
        ("CI", "1"),
        ("NO_COLOR", "1"),
        ("CARGO_TERM_COLOR", "never"),
        ("CARGO_TERM_PROGRESS_WHEN", "never"),
        ("PYTHONUNBUFFERED", "1"),
        ("GIT_TERMINAL_PROMPT", "0"),
    ];
    for (name, value) in fixed {
        assert_eq!(get(&env, name), Some(value), "{name}");
        assert_eq!(env.iter().filter(|(k, _)| k == name).count(), 1, "{name}");
    }
}

#[test]
fn nothing_else_is_passed() {
    let secrets = [
        ("LAZAROBOX_MCP_TOKEN", "s3cret"),
        ("SSH_AUTH_SOCK", "/run/user/1000/ssh"),
        ("GPG_AGENT_INFO", "x"),
        ("GPG_TTY", "/dev/pts/1"),
        ("DISPLAY", ":0"),
        ("WAYLAND_DISPLAY", "wayland-0"),
        ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
        ("GITHUB_TOKEN", "ghp_x"),
        ("AWS_SECRET_ACCESS_KEY", "k"),
        ("NPM_TOKEN", "t"),
        ("RUSTFLAGS", "-C link-arg=-x"),
        ("LD_PRELOAD", "/tmp/x.so"),
        ("PYTHONPATH", "/tmp"),
        ("NODE_OPTIONS", "--require /tmp/x.js"),
    ];
    let env = scrubbed(&secrets);
    for (name, _) in secrets {
        assert_eq!(get(&env, name), None, "{name}");
    }
    for (_, value) in secrets {
        assert!(env.iter().all(|(_, held)| held != value), "{value}");
    }
}

#[test]
fn path_loses_relative_and_project_entries() {
    let env = scrubbed(&[(
        "PATH",
        "/usr/bin::bin:./x:/home/u/proj/node_modules/.bin:/home/u/proj:/home/u/proj-x/bin:../y:/home/u/.cargo/bin",
    )]);
    assert_eq!(
        get(&env, "PATH"),
        Some("/usr/bin:/home/u/proj-x/bin:/home/u/.cargo/bin")
    );
}

#[test]
fn a_path_with_no_safe_entry_is_left_out() {
    assert_eq!(
        get(&scrubbed(&[("PATH", "bin:/home/u/proj/bin")]), "PATH"),
        None
    );
    assert_eq!(get(&scrubbed(&[("PATH", "")]), "PATH"), None);
    assert_eq!(get(&scrubbed(&[]), "PATH"), None);
}
