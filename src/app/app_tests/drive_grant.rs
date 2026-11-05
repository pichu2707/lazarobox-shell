//! The grant of a Windows drive folder (openspec `wsl-drive-access`,
//! chat-folder: WSL drive grant): the prompt opens only after the user chose
//! a non-broad drive folder, only an unmodified lowercase `y` grants, and the
//! grant lives in memory until the folder changes, a resume or a new chat.
//! Every path here is text the app never opens: no test touches a drive.

use super::chat_history::{ask, systems};
use super::chat_live::ollama;
use super::run_pane::{chat, one_shell_and_a_chat};
use super::*;
use crate::core::ai::chat::{
    approval::{ApprovalAsk, DRIVE_GRANT_DENIED},
    engine::{CallId, Decision, Finish, StreamEvent, Tools},
    folder::{Probed, Purpose, Refusal, Verdict},
};

const APP: &str = "/mnt/c/Users/ana/Documents/app";
const OTHER: &str = "/mnt/c/Users/ana/Documents/other";

/// A drive folder as the runtime judges it.
fn drive(path: &str) -> Result<Probed, Refusal> {
    Ok(Probed {
        path: path.into(),
        verdict: Verdict::Concrete,
        drive: true,
    })
}

fn judged(path: &str, verdict: Verdict, drive: bool) -> Result<Probed, Refusal> {
    Ok(Probed {
        path: path.into(),
        verdict,
        drive,
    })
}

fn pending(a: &App) -> Option<ApprovalAsk> {
    chat(a, id2())
        .approvals()
        .pending()
        .map(|p| p.ask().clone())
}

fn asked_for(a: &App) -> Option<PathBuf> {
    match pending(a) {
        Some(ApprovalAsk::DriveGrant { folder }) => Some(folder),
        _ => None,
    }
}

fn grant_of(a: &App) -> Option<PathBuf> {
    chat(a, id2()).drive_grant().map(Path::to_path_buf)
}

fn folder_of(a: &App) -> Option<PathBuf> {
    chat(a, id2()).origin_cwd().map(Path::to_path_buf)
}

fn lines(a: &App) -> Vec<String> {
    systems(chat(a, id2()))
}

fn wait_for_arming() {
    NOW_MS.with(|n| n.set(n.get() + 1_000));
}

fn chat_app() -> App {
    NOW_MS.with(|n| n.set(0));
    one_shell_and_a_chat("hello").with_clock(mock_now)
}

/// A chat that is in `folder`, idle, with no grant asked.
fn in_folder(folder: &str) -> App {
    let mut a = chat_app();
    a.apply_folder(id2(), folder.into());
    a
}

fn probe_purposes(effects: &[Effect]) -> Vec<(PathBuf, Purpose)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ProbeFolder { path, purpose, .. } => Some((path.clone(), *purpose)),
            _ => None,
        })
        .collect()
}

/// `/cd <target>` typed and its probe answered with `result`.
fn cd(a: &mut App, target: &str, result: Result<Probed, Refusal>) -> Vec<Effect> {
    let effects = ask(a, &format!("/cd {target}"));
    let Some((_, purpose)) = probe_purposes(&effects).into_iter().next() else {
        return effects;
    };
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose,
        result,
    })
}

/// `/cd <target>` confirmed with `y`, its last check answered with `result`.
fn cd_confirmed(a: &mut App, target: &str, result: Result<Probed, Refusal>) {
    cd(a, target, result.clone());
    assert!(matches!(pending(a), Some(ApprovalAsk::Folder { .. })));
    let effects = a.update(key('y'));
    let Some((_, purpose @ Purpose::Confirm(_))) = probe_purposes(&effects).into_iter().next()
    else {
        panic!("expected the confirm probe, got {effects:?}");
    };
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose,
        result,
    });
}

/// A chat whose folder is `/w/a` resumes conversation 30 stored in `stored`,
/// and the probe of that folder answers `result`.
fn resume(stored: &str, result: Result<Probed, Refusal>) -> App {
    let mut a = super::chat_history::listing().with_clock(mock_now);
    a.apply_folder(id2(), "/w/a".into());
    a.update(super::chat_history::prompt_for(id2()));
    resume_again(&mut a, 30, stored, result);
    a
}

fn resume_again(a: &mut App, id: i64, stored: &str, result: Result<Probed, Refusal>) {
    resume_reply(a, id, stored);
    resume_answer(a, result);
}

/// The store hands back conversation `id`, stored in `stored`.
fn resume_reply(a: &mut App, id: i64, stored: &str) {
    let AppEvent::Store(StoreReply::Resumed {
        pane,
        conversation,
        profile,
        tag,
        extras,
        messages,
        session,
        ..
    }) = super::chat_history::resumed(id2(), id, "programmer")
    else {
        unreachable!("resumed builds a Resumed reply");
    };
    a.update(AppEvent::Store(StoreReply::Resumed {
        pane,
        conversation,
        profile,
        tag,
        extras,
        messages,
        session,
        folder: Some(stored.into()),
        skipped: 0,
        github_read: false,
    }));
}

/// The probe of the stored folder answers `result`.
fn resume_answer(a: &mut App, result: Result<Probed, Refusal>) {
    let token = a.resume_folders.get(&id2()).map_or(0, |entry| entry.0);
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose: Purpose::Resume(token),
        result,
    });
}

/// The prompt is open for `APP` and armed.
fn prompted() -> App {
    let mut a = in_folder("/w/a");
    cd_confirmed(&mut a, APP, drive(APP));
    assert_eq!(asked_for(&a), Some(PathBuf::from(APP)));
    wait_for_arming();
    a
}

/// Granted in `APP`.
fn granted() -> App {
    let mut a = prompted();
    a.update(key('y'));
    assert_eq!(grant_of(&a), Some(PathBuf::from(APP)));
    a
}

// ---- when the prompt opens (2.2) -------------------------------------------

#[test]
fn a_confirmed_cd_to_a_drive_folder_asks_for_writes() {
    let a = prompted();
    assert_eq!(folder_of(&a), Some(PathBuf::from(APP)));
    assert_eq!(grant_of(&a), None);
    let prompt = a.approval_prompt(id2()).expect("the grant prompt is drawn");
    assert_eq!(prompt.target, format!("Allow writes in {APP}? (y/n)"));
    assert_eq!(prompt.command, APP);
}

#[test]
fn a_picker_accept_of_a_drive_folder_asks_for_writes() {
    let mut a = launched(80, 25).with_ai(ollama()).with_clock(mock_now);
    a.update(ctrl_space());
    a.update(key('c'));
    // A broad folder opens the picker; the user types the drive folder.
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose: Purpose::Open,
        result: judged("/h", Verdict::Home, false),
    });
    super::chat_history::type_text(&mut a, APP);
    let effects = a.update(super::chat_history::plain(KeyCode::Enter));
    let Some((_, purpose @ Purpose::Pick(_))) = probe_purposes(&effects).into_iter().next() else {
        panic!("expected the pick probe, got {effects:?}");
    };
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose,
        result: drive(APP),
    });
    assert_eq!(folder_of(&a), Some(PathBuf::from(APP)));
    assert_eq!(asked_for(&a), Some(PathBuf::from(APP)));
}

#[test]
fn a_new_chat_opened_in_a_drive_folder_asks_and_inherits_no_grant() {
    let mut a = launched(80, 25).with_ai(ollama()).with_clock(mock_now);
    a.update(ctrl_space());
    let effects = a.update(key('c'));
    // The chat opens in its source pane's folder, here a drive folder.
    let Some((folder, Purpose::Open)) = probe_purposes(&effects).into_iter().next() else {
        panic!("expected the open probe, got {effects:?}");
    };
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose: Purpose::Open,
        result: drive(&folder.to_string_lossy()),
    });
    assert_eq!(asked_for(&a), Some(folder));
    assert_eq!(grant_of(&a), None);
}

#[test]
fn a_resume_in_a_drive_folder_asks_again() {
    let a = resume(APP, drive(APP));
    assert_eq!(folder_of(&a), Some(PathBuf::from(APP)));
    assert_eq!(asked_for(&a), Some(PathBuf::from(APP)));
}

#[test]
fn a_broad_a_home_or_a_refused_folder_never_asks() {
    let mut a = in_folder("/w/a");
    cd_confirmed(
        &mut a,
        "/mnt/c/Users/ana",
        judged("/mnt/c/Users/ana", Verdict::BroadDrive, true),
    );
    assert_eq!(folder_of(&a), Some(PathBuf::from("/mnt/c/Users/ana")));
    assert_eq!(pending(&a), None);
    let mut a = in_folder("/w/a");
    cd_confirmed(&mut a, "/h/dev", judged("/h/dev", Verdict::Concrete, false));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/h/dev")));
    assert_eq!(pending(&a), None);
    let mut a = in_folder("/w/a");
    cd(&mut a, "/mnt/c/Windows", Err(Refusal::NotAllowed));
    assert_eq!(pending(&a), None);
    assert!(lines(&a).contains(&"/cd refused: not allowed".to_owned()));
}

#[test]
fn a_busy_chat_is_not_asked() {
    // The resume's probe answers while the model answers: no ask.
    let mut a = resume("/w/a", judged("/w/a", Verdict::Concrete, false));
    a.apply_folder(id2(), APP.into());
    resume_reply(&mut a, 31, APP);
    let ready = crate::core::ai::chat::engine::Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    };
    let chat = a.panes.get_mut(&id2()).and_then(PaneState::chat_mut);
    chat.unwrap().send("go", &ready, "system");
    resume_answer(&mut a, drive(APP));
    assert_eq!(pending(&a), None);
    // And `/cd` to the current folder is refused like any `/cd`.
    let mut a = in_folder(APP);
    a.update(super::chat_history::prompt_for(id2()));
    ask(&mut a, "again");
    cd(&mut a, APP, drive(APP));
    assert_eq!(pending(&a), None);
    assert!(
        lines(&a).contains(&"/cd refused: the model is answering".to_owned()),
        "{:?}",
        lines(&a)
    );
}

// ---- the keys (2.2) --------------------------------------------------------

#[test]
fn only_an_unmodified_lowercase_y_grants() {
    let a = granted();
    assert_eq!(pending(&a), None);
    assert!(
        lines(&a).contains(&format!("writes allowed in {APP} until you quit")),
        "{:?}",
        lines(&a)
    );
    for answer in [
        AppEvent::Key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT)),
        AppEvent::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::ALT)),
        key('n'),
        AppEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    ] {
        let mut a = prompted();
        a.update(answer.clone());
        assert_eq!(grant_of(&a), None, "{answer:?}");
    }
}

#[test]
fn a_y_before_the_prompt_is_armed_grants_nothing() {
    let mut a = in_folder("/w/a");
    cd_confirmed(&mut a, APP, drive(APP));
    a.update(key('y'));
    assert_eq!(grant_of(&a), None);
    assert_eq!(asked_for(&a), Some(PathBuf::from(APP)));
}

#[test]
fn n_leaves_the_chat_read_only_and_says_how_to_ask_again() {
    let mut a = prompted();
    a.update(key('n'));
    assert_eq!((pending(&a), grant_of(&a)), (None, None));
    assert!(
        lines(&a).contains(&format!(
            "the chat stays read-only in {APP}; /cd {APP} asks again"
        )),
        "{:?}",
        lines(&a)
    );
}

// ---- how long it lasts (2.3) -----------------------------------------------

#[test]
fn a_folder_change_clears_the_grant_and_coming_back_asks_again() {
    let mut a = granted();
    cd_confirmed(&mut a, OTHER, drive(OTHER));
    assert_eq!(grant_of(&a), None);
    assert_eq!(asked_for(&a), Some(PathBuf::from(OTHER)));
    a.update(key('n'));
    cd_confirmed(&mut a, APP, drive(APP));
    assert_eq!(grant_of(&a), None);
    assert_eq!(asked_for(&a), Some(PathBuf::from(APP)));
}

#[test]
fn a_resume_clears_the_grant_and_asks_again() {
    let mut a = resume(APP, drive(APP));
    wait_for_arming();
    a.update(key('y'));
    assert_eq!(grant_of(&a), Some(PathBuf::from(APP)));
    // Another conversation stored in the same folder, from the list: the
    // folder does not change, and still the grant is gone and asked again.
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .open_list();
    resume_again(&mut a, 31, APP, drive(APP));
    assert_eq!(grant_of(&a), None);
    assert_eq!(asked_for(&a), Some(PathBuf::from(APP)));
}

#[test]
fn cd_to_the_current_folder_reopens_the_prompt_until_granted() {
    let mut a = prompted();
    a.update(key('n'));
    cd(&mut a, APP, drive(APP));
    assert_eq!(asked_for(&a), Some(PathBuf::from(APP)));
    assert!(!lines(&a).iter().any(|l| l.starts_with("already in")));
    wait_for_arming();
    a.update(key('y'));
    assert_eq!(grant_of(&a), Some(PathBuf::from(APP)));
    cd(&mut a, APP, drive(APP));
    assert_eq!(pending(&a), None);
    assert!(lines(&a).contains(&format!("already in {APP}")));
}

// The previous behaviour of `/cd <current>` outside a drive holds.
#[test]
fn cd_to_the_current_home_folder_still_says_already_in() {
    let mut a = in_folder("/h/dev");
    cd(&mut a, "/h/dev", judged("/h/dev", Verdict::Concrete, false));
    assert_eq!(pending(&a), None);
    assert!(lines(&a).contains(&"already in /h/dev".to_owned()));
}

// ---- the model cannot reach it (2.4) ---------------------------------------

#[test]
fn a_tool_call_cannot_open_the_prompt() {
    let (mut a, turn) = super::tool_approval::live();
    a.apply_folder(id2(), APP.into());
    let effects = super::tool_approval::announce(
        &mut a,
        turn,
        7,
        ApprovalAsk::DriveGrant { folder: APP.into() },
    );
    assert_eq!(pending(&a), None);
    assert!(effects.contains(&Effect::ToolDecision {
        pane: id2(),
        worker: None,
        turn,
        call: CallId(7),
        decision: Decision::Denied(DRIVE_GRANT_DENIED),
    }));
}

#[test]
fn a_model_answer_that_says_cd_changes_nothing() {
    let (mut a, turn) = super::tool_approval::live();
    a.apply_folder(id2(), "/w/a".into());
    for text in [format!("/cd {APP}"), "\ny".to_owned()] {
        a.update(AppEvent::Chat(id2(), turn, StreamEvent::Delta(text)));
    }
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
    assert_eq!((pending(&a), grant_of(&a)), (None, None));
}

// ---- the grant reaches the turn (2.7) --------------------------------------

/// The turn the chat is streaming.
fn turn_of(a: &App) -> u64 {
    match a.pane_view(id2()) {
        Some(PaneView::Chat(chat)) => chat.turn(),
        _ => panic!("pane 2 is not a chat"),
    }
}

/// The request a send produced.
fn sent_request(effects: &[Effect]) -> Option<crate::core::ai::chat::engine::ChatRequest> {
    effects.iter().find_map(|effect| match effect {
        Effect::ChatSend { request, .. } => Some(request.clone()),
        _ => None,
    })
}

// A granted chat's turn carries its folder to the tool host; an ungranted
// one, and a turn without tools, carry none.
#[test]
fn a_turn_carries_the_grant_of_its_chat() {
    NOW_MS.with(|n| n.set(0));
    let mut a = launched(80, 25)
        .with_ai(ollama())
        .with_tool_engines(&["ollama"])
        .with_clock(mock_now);
    press(&mut a, "c");
    super::run_pane::answer(&mut a, id2(), "hi");
    a.apply_folder(id2(), "/w/a".into());
    cd_confirmed(&mut a, APP, drive(APP));
    a.update(key('n'));
    assert_eq!(pending(&a), None);
    // Each folder change asked for the system prompt again; a stale answer
    // asks once more.
    while !a.update(super::chat_history::prompt_for(id2())).is_empty() {}
    assert!(super::run_pane::chat(&a, id2()).prompt().is_some());
    // A second Enter at the same instant is a key repeat.
    wait_for_arming();
    let ungranted = sent_request(&ask(&mut a, "first")).expect("a send");
    assert!(ungranted.tools != Tools::Off);
    assert_eq!(ungranted.drive_grant, None);
    a.update(AppEvent::Chat(
        id2(),
        turn_of(&a),
        StreamEvent::Done(Finish::Stop),
    ));
    cd(&mut a, APP, drive(APP));
    wait_for_arming();
    a.update(key('y'));
    assert_eq!(grant_of(&a), Some(PathBuf::from(APP)));
    wait_for_arming();
    let granted = sent_request(&ask(&mut a, "second")).expect("a send");
    assert_eq!(granted.drive_grant, Some(PathBuf::from(APP)));
    a.ai.set_tools_enabled(false);
    a.update(AppEvent::Chat(
        id2(),
        turn_of(&a),
        StreamEvent::Done(Finish::Stop),
    ));
    wait_for_arming();
    let off = sent_request(&ask(&mut a, "third")).expect("a send");
    assert_eq!(off.tools, Tools::Off);
    assert_eq!(off.drive_grant, None);
}
