use super::folder::*;
use crate::core::ai::tools::paths::SecretRoots;
use std::{fs, path::Path, path::PathBuf};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    roots: SecretRoots,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for sub in ["home/project", "home/.ssh", "srv/x"] {
        fs::create_dir_all(root.join(sub)).unwrap();
    }
    let roots = SecretRoots::load(&root.join("home"), &[]).unwrap();
    Fixture {
        _dir: dir,
        root,
        roots,
    }
}

#[test]
fn judges_each_kind_of_folder() {
    let f = fixture();
    let home = f.root.join("home");
    assert_eq!(
        judge(&home.join("project"), &f.roots),
        Ok(Verdict::Concrete)
    );
    assert_eq!(judge(&home, &f.roots), Ok(Verdict::Home));
    assert_eq!(judge(&f.root, &f.roots), Ok(Verdict::AboveHome));
    assert_eq!(judge(&f.root.join("srv/x"), &f.roots), Ok(Verdict::Outside));
    assert_eq!(judge(Path::new("/usr"), &f.roots), Ok(Verdict::Outside));
    assert_eq!(judge(Path::new("/"), &f.roots), Err(Refusal::Root));
    assert_eq!(
        judge(&home.join(".ssh"), &f.roots),
        Err(Refusal::NotAllowed)
    );
}

#[test]
fn only_home_ancestors_and_outside_are_broad() {
    assert!(is_broad(Verdict::Home));
    assert!(is_broad(Verdict::AboveHome));
    assert!(is_broad(Verdict::Outside));
    assert!(!is_broad(Verdict::Concrete));
}

#[test]
fn a_typed_outside_folder_is_not_allowed_and_root_has_its_own_text() {
    assert_eq!(typed(Ok(Verdict::Outside)), Err(Refusal::NotAllowed));
    assert_eq!(typed(Ok(Verdict::Home)), Ok(Verdict::Home));
    assert_eq!(typed(Ok(Verdict::AboveHome)), Ok(Verdict::AboveHome));
    assert_eq!(Refusal::NotAllowed.text(), "not allowed");
    assert_eq!(Refusal::Root.text(), "/ is not a valid chat folder");
    assert_eq!(Refusal::NotFound.text(), "folder not found");
    assert_eq!(Refusal::NotAFolder.text(), "not a folder");
}

fn cd(path: &str) -> Command {
    Command::Cd(CdArg::Path(path.to_owned()))
}

#[test]
fn the_exact_token_cd_is_the_command() {
    assert_eq!(parse_command("/cd"), Command::Cd(CdArg::Missing));
    assert_eq!(parse_command("  /cd  "), Command::Cd(CdArg::Missing));
    assert_eq!(parse_command("/cd x"), cd("x"));
    assert_eq!(parse_command("/cd  x y "), cd("x y"));
    assert_eq!(parse_command("/cd\tx"), cd("x"));
}

#[test]
fn other_slash_text_is_a_plain_message() {
    assert_eq!(parse_command("/cdx foo"), Command::Plain);
    assert_eq!(parse_command("/etc/passwd explain"), Command::Plain);
    assert_eq!(parse_command("hello /cd x"), Command::Plain);
    assert_eq!(parse_command(""), Command::Plain);
}

#[test]
fn a_double_slash_escapes_the_command() {
    assert_eq!(parse_command("//cd x"), Command::Escaped("/cd x".into()));
    assert_eq!(parse_command("//cd"), Command::Escaped("/cd".into()));
    assert_eq!(parse_command("//etc/hosts"), Command::Plain);
    assert_eq!(parse_command("//cdx"), Command::Plain);
}

#[test]
fn expand_reads_tilde_relative_and_parent_paths() {
    let (home, cur) = (Path::new("/h"), Some(Path::new("/w/proj")));
    assert_eq!(expand("~", home, cur), Some("/h".into()));
    assert_eq!(expand("~/code", home, cur), Some("/h/code".into()));
    assert_eq!(expand("/abs/x", home, cur), Some("/abs/x".into()));
    assert_eq!(expand("src", home, cur), Some("/w/proj/src".into()));
    assert_eq!(expand("..", home, cur), Some("/w".into()));
    assert_eq!(expand("../b/./c", home, cur), Some("/w/b/c".into()));
    assert_eq!(expand("src", home, None), None);
    assert_eq!(expand("~/x", home, None), Some("/h/x".into()));
}
