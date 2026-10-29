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

// the change -----------------------------------------------------------------

mod change {
    use super::super::{
        ChatState, Entry,
        prompts::Resolved,
        state_tests::{SYSTEM, sent},
    };
    use std::path::PathBuf;

    fn chat_in(folder: &str) -> ChatState {
        ChatState::new(Some(PathBuf::from(folder)))
    }

    fn prompt() -> Resolved {
        Resolved {
            text: "system".into(),
            notices: Vec::new(),
        }
    }

    #[test]
    fn the_folder_is_replaced_and_the_old_one_returned() {
        let mut state = chat_in("/w/a");
        let change = state.set_folder("/w/b".into());
        assert_eq!(change.old, Some(PathBuf::from("/w/a")));
        assert_eq!(state.origin_cwd(), Some(std::path::Path::new("/w/b")));
    }

    #[test]
    fn a_visible_line_names_both_folders() {
        let mut state = chat_in("/w/a");
        state.set_folder("/w/b".into());
        let last = state.entries().last();
        assert_eq!(
            last,
            Some(&Entry::System("folder changed: /w/a -> /w/b".into()))
        );
    }

    #[test]
    fn the_model_hears_of_it_once_and_nothing_is_stored() {
        let mut state = chat_in("/w/a");
        state.set_folder("/w/b".into());
        let (turn, first) = sent(&mut state, "one");
        assert!(first.system.starts_with(SYSTEM), "{}", first.system);
        assert!(
            first.system.contains("from /w/a to /w/b"),
            "{}",
            first.system
        );
        assert!(first.system.contains("tool paths are now relative to /w/b"));
        state.apply(turn, super::super::engine::StreamEvent::Delta("hi".into()));
        state.apply(
            turn,
            super::super::engine::StreamEvent::Done(super::super::engine::Finish::Stop),
        );
        let (_, second) = sent(&mut state, "two");
        assert_eq!(second.system, SYSTEM);
        assert!(second.messages.iter().all(|m| !m.content.contains("/w/b")));
    }

    #[test]
    fn a_prepared_prompt_is_dropped_and_asked_again() {
        let mut state = chat_in("/w/a");
        state.set_prompt(prompt());
        assert!(state.set_folder("/w/b".into()).prompt_now);
        assert!(state.prompt.is_none());
    }

    #[test]
    fn a_prompt_still_being_prepared_is_dropped_when_it_arrives() {
        let mut state = chat_in("/w/a");
        assert!(!state.set_folder("/w/b".into()).prompt_now);
        assert!(state.set_prompt_or_reprepare(prompt()), "prepare it again");
        assert!(!state.set_prompt_or_reprepare(prompt()));
    }

    #[test]
    fn a_fresh_untouched_conversation_proposes_its_tag_again() {
        let mut state = chat_in("/w/a");
        state.set_tag(Some("a".into()));
        state.set_folder("/w/b".into());
        assert_eq!(state.tag(), None);
    }

    #[test]
    fn a_tag_the_user_edited_stays() {
        let mut state = chat_in("/w/a");
        state.set_tag(Some("mine".into()));
        state.tags_edited = true;
        state.set_folder("/w/b".into());
        assert_eq!(state.tag(), Some("mine"));
    }
}

// busy ----------------------------------------------------------------------

mod busy {
    use super::super::{
        ChatState,
        approval::{ApprovalAsk, Origin},
        review::Badge,
        state_tests::{chat, sent},
    };

    fn ask(state: &mut ChatState) {
        let run = ApprovalAsk::Run {
            command: "ls".into(),
        };
        state.approvals_mut().request(run, Origin::Tool).unwrap();
    }

    #[test]
    fn an_idle_chat_is_not_busy() {
        assert_eq!(chat().folder_busy(false), None);
    }

    #[test]
    fn each_reason_has_its_text() {
        let mut state = chat();
        ask(&mut state);
        assert_eq!(state.folder_busy(false), Some("an approval is pending"));

        assert_eq!(chat().folder_busy(true), Some("a command is running"));

        let mut state = chat();
        sent(&mut state, "q");
        assert_eq!(state.folder_busy(false), Some("the model is answering"));

        for badge in [Badge::Staged(1), Badge::Orphan(1)] {
            let mut state = chat();
            state.set_batch_badge(Some(badge));
            assert_eq!(
                state.folder_busy(false),
                Some("writes are staged or proposed")
            );
        }

        let mut state = chat();
        state.save_sent("proj");
        assert_eq!(
            state.folder_busy(false),
            Some("a note is still being saved")
        );
    }

    #[test]
    fn reasons_are_reported_in_order() {
        let mut state = chat();
        ask(&mut state);
        sent(&mut state, "q");
        state.set_batch_badge(Some(Badge::Staged(1)));
        state.save_sent("proj");
        assert_eq!(state.folder_busy(true), Some("an approval is pending"));

        let mut state = chat();
        sent(&mut state, "q");
        state.set_batch_badge(Some(Badge::Staged(1)));
        state.save_sent("proj");
        assert_eq!(state.folder_busy(true), Some("a command is running"));
        assert_eq!(state.folder_busy(false), Some("the model is answering"));

        let mut state = chat();
        state.set_batch_badge(Some(Badge::Staged(1)));
        state.save_sent("proj");
        assert_eq!(
            state.folder_busy(false),
            Some("writes are staged or proposed")
        );
    }
}
