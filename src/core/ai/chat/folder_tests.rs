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
