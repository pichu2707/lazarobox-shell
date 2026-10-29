use super::*;

fn argv(list: &[&str]) -> Vec<String> {
    list.iter().map(|a| a.to_string()).collect()
}

fn dir_arg(v: &str) -> StartArgs {
    StartArgs {
        dir: Some(v.into()),
        missing_value: false,
    }
}

#[test]
fn parses_both_forms_of_the_flag() {
    assert_eq!(parse_args(&argv(&["--dir", "/a"])), dir_arg("/a"));
    assert_eq!(parse_args(&argv(&["--dir=/a"])), dir_arg("/a"));
    assert_eq!(
        parse_args(&argv(&["--force", "--dir", "/a"])),
        dir_arg("/a")
    );
    assert_eq!(parse_args(&argv(&[])), StartArgs::default());
}

#[test]
fn a_flag_without_a_value_is_flagged_not_fatal() {
    let missing = StartArgs {
        dir: None,
        missing_value: true,
    };
    assert_eq!(parse_args(&argv(&["--dir"])), missing);
    assert_eq!(parse_args(&argv(&["--dir="])), missing);
    assert_eq!(parse_args(&argv(&["--dir", "--force"])), missing);
}

#[test]
fn the_flag_after_a_subcommand_is_not_read() {
    assert_eq!(
        parse_args(&argv(&["install-desktop", "--dir", "/a"])),
        StartArgs::default()
    );
}

fn kinds(path: &Path) -> Kind {
    match path.to_str().unwrap() {
        "/a" | "/b" | "/c" | "/w/proj" | "/h/code" | "/h" => Kind::Dir,
        "/etc/hostname" => Kind::File,
        _ => Kind::Missing,
    }
}

fn go(args: &StartArgs, config: Option<&str>) -> StartDir {
    resolve(
        args,
        config.map(Path::new),
        Path::new("/c"),
        Some(Path::new("/h")),
        &kinds,
    )
}

#[test]
fn the_flag_wins_over_the_config() {
    let got = go(&dir_arg("/a"), Some("/b"));
    assert_eq!((got.path, got.notice), ("/a".into(), None));
}

#[test]
fn the_config_wins_over_the_launch_cwd() {
    let got = go(&StartArgs::default(), Some("/b"));
    assert_eq!((got.path, got.notice), ("/b".into(), None));
}

#[test]
fn neither_source_is_the_launch_cwd() {
    let got = go(&StartArgs::default(), None);
    assert_eq!((got.path, got.notice), ("/c".into(), None));
}

#[test]
fn relative_and_tilde_paths_resolve() {
    let relative = resolve(&dir_arg("proj"), None, Path::new("/w"), None, &kinds);
    assert_eq!(relative.path, PathBuf::from("/w/proj"));
    assert_eq!(go(&dir_arg("~/code"), None).path, PathBuf::from("/h/code"));
    assert_eq!(go(&dir_arg("~"), None).path, PathBuf::from("/h"));
    let dotted = resolve(&dir_arg("./proj"), None, Path::new("/w"), None, &kinds);
    assert_eq!(dotted.path, PathBuf::from("/w/proj"));
}

#[test]
fn a_missing_folder_starts_in_the_cwd_with_a_notice() {
    let got = go(&dir_arg("/nope"), Some("/b"));
    assert_eq!(got.path, PathBuf::from("/c"));
    assert_eq!(
        got.notice.as_deref(),
        Some("--dir: folder not found: /nope")
    );
}

#[test]
fn a_file_and_a_missing_value_have_their_own_notice() {
    let file = go(&dir_arg("/etc/hostname"), None);
    assert_eq!(file.path, PathBuf::from("/c"));
    assert_eq!(
        file.notice.as_deref(),
        Some("--dir: not a folder: /etc/hostname")
    );
    let args = StartArgs {
        dir: None,
        missing_value: true,
    };
    let none = go(&args, Some("/b"));
    assert_eq!(none.path, PathBuf::from("/c"));
    assert_eq!(none.notice.as_deref(), Some("--dir: missing value"));
}

#[test]
fn a_config_folder_that_is_gone_falls_back_with_a_notice() {
    for bad in ["/gone", "/etc/hostname"] {
        let got = go(&StartArgs::default(), Some(bad));
        assert_eq!(got.path, PathBuf::from("/c"));
        assert_eq!(
            got.notice.as_deref(),
            Some("config: workspace.dir: folder not found")
        );
    }
}

#[test]
fn a_lossy_value_says_it_is_not_utf8() {
    let got = go(&dir_arg("/caf\u{fffd}"), Some("/b"));
    assert_eq!(got.path, PathBuf::from("/c"));
    assert_eq!(
        got.notice.as_deref(),
        Some("--dir: folder name is not valid UTF-8")
    );
}
