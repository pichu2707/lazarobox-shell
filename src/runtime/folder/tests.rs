use super::*;
use crate::core::ai::chat::folder::Verdict;
use crate::runtime::tools::ToolEnv;
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    env: ToolEnv,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for sub in ["home/project/src", "home/.ssh", "srv/x"] {
        fs::create_dir_all(root.join(sub)).unwrap();
    }
    fs::write(root.join("home/note.txt"), "x").unwrap();
    symlink(root.join("home/project"), root.join("home/link")).unwrap();
    let env = ToolEnv {
        home: Some(root.join("home").into_os_string()),
        xdg_config_home: None,
        xdg_data_home: None,
        origin: PathBuf::from("/"),
    };
    Fixture {
        _dir: dir,
        root,
        env,
    }
}

#[test]
fn a_symlink_is_resolved_to_its_target() {
    let f = fixture();
    let got = probe(&f.env, &f.root.join("home/link")).unwrap();
    assert_eq!(got.path, f.root.join("home/project"));
    assert_eq!(got.verdict, Verdict::Concrete);
}

#[test]
fn a_bad_path_is_refused_with_its_reason() {
    let f = fixture();
    let home = f.root.join("home");
    let refused = |path: &Path| probe(&f.env, path).unwrap_err();
    assert_eq!(refused(&home.join("nope")), Refusal::NotFound);
    assert_eq!(refused(&home.join("note.txt")), Refusal::NotAFolder);
    assert_eq!(refused(&home.join(".ssh")), Refusal::NotAllowed);
    assert_eq!(refused(Path::new("/")), Refusal::Root);
}

#[test]
fn broad_folders_keep_their_verdict() {
    let f = fixture();
    let verdict = |path: &Path| probe(&f.env, path).unwrap().verdict;
    assert_eq!(verdict(&f.root.join("home")), Verdict::Home);
    assert_eq!(verdict(&f.root), Verdict::AboveHome);
    assert_eq!(verdict(&f.root.join("srv/x")), Verdict::Outside);
}

#[test]
fn no_home_refuses_everything_like_the_tools() {
    let f = fixture();
    let env = ToolEnv {
        home: None,
        ..f.env.clone()
    };
    assert_eq!(
        probe(&env, &f.root.join("home/project")).unwrap_err(),
        Refusal::NotAllowed
    );
}

// The verdict must match the building blocks the tool host judges a root by
// (`root_of`: a directory that is not HOME and passes `judge_real`;
// `concrete_root`: a folder that does not hold HOME). Those two are private
// to the host, so their expressions are repeated here on purpose.
#[test]
fn the_verdict_matches_the_tool_root_policy() {
    let f = fixture();
    let roots = crate::runtime::tools::fs::load_roots(&f.env).unwrap();
    for sub in [
        "home/project",
        "home/project/src",
        "home/.ssh",
        "home",
        "srv/x",
        "",
    ] {
        let dir = f.root.join(sub);
        let real = dir.canonicalize().unwrap();
        let tool_root = real.is_dir() && real != roots.home() && roots.judge_real(&real);
        let concrete = !roots.home().starts_with(&real);
        let got = probe(&f.env, &dir);
        let is_concrete = matches!(&got, Ok(p) if p.verdict == Verdict::Concrete);
        assert_eq!(is_concrete, tool_root, "{sub}");
        let broad_home =
            matches!(&got, Ok(p) if matches!(p.verdict, Verdict::Home | Verdict::AboveHome));
        assert_eq!(broad_home, !concrete, "{sub}");
    }
}
