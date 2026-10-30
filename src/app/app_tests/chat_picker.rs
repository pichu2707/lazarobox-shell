//! The folder picker in the app (chat-folder B2): the open flow, its keys,
//! and what accepting or dropping it does.

use super::chat_live::ollama;
use super::run_pane::chat;
use super::*;
use crate::core::ai::chat::{
    folder::{Probed, Purpose, Refusal, Verdict},
    folder_picker::{Row, Source},
};

/// Opens a chat from the shell pane without answering the folder probe.
fn open(a: &mut App) -> Vec<Effect> {
    a.update(ctrl_space());
    a.update(key('c'))
}

fn app() -> App {
    launched(80, 25).with_ai(ollama())
}

fn probes(effects: &[Effect]) -> Vec<(PathBuf, Purpose)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ProbeFolder { path, purpose, .. } => Some((path.clone(), *purpose)),
            _ => None,
        })
        .collect()
}

fn prepares(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::PreparePrompt { .. }))
        .count()
}

fn lists(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Store(StoreCmd::List { .. })))
        .count()
}

type Gathering = (Vec<(Source, PathBuf)>, bool);

fn gathers(effects: &[Effect]) -> Vec<Gathering> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ProbeFolders { paths, git, .. } => Some((paths.clone(), *git)),
            _ => None,
        })
        .collect()
}

fn answer(a: &mut App, purpose: Purpose, result: Result<Probed, Refusal>) -> Vec<Effect> {
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose,
        result,
    })
}

fn judged(path: &str, verdict: Verdict) -> Result<Probed, Refusal> {
    Ok(Probed {
        path: path.into(),
        verdict,
    })
}

/// The purpose of the probe the open picker asked for.
fn pick(a: &App) -> Purpose {
    Purpose::Pick(chat(a, id2()).folder_picker().expect("a picker").id())
}

fn picker_open(a: &App) -> bool {
    chat(a, id2()).folder_picker().is_some()
}

fn answer_pick(a: &mut App, result: Result<Probed, Refusal>) -> Vec<Effect> {
    let purpose = pick(a);
    answer(a, purpose, result)
}

fn picker_id(a: &App) -> u64 {
    chat(a, id2()).folder_picker().map_or(0, |p| p.id())
}

fn broad(a: &mut App) -> Vec<Effect> {
    open(a);
    answer(a, Purpose::Open, judged("/h", Verdict::Home))
}

#[test]
fn opening_defers_the_steps_behind_the_folder_probe() {
    let mut a = app();
    a.history = true;
    let effects = open(&mut a);
    assert_eq!(
        probes(&effects),
        vec![(PathBuf::from(LAUNCH), Purpose::Open)]
    );
    assert!(effects.contains(&Effect::DetectEngines));
    assert_eq!(prepares(&effects), 0, "{effects:?}");
    assert_eq!(lists(&effects), 0, "{effects:?}");
    assert!(!picker_open(&a));
}

#[test]
fn a_concrete_folder_opens_as_before() {
    let mut a = app();
    a.history = true;
    open(&mut a);
    let effects = answer(&mut a, Purpose::Open, judged(LAUNCH, Verdict::Concrete));
    assert_eq!((prepares(&effects), lists(&effects)), (1, 1), "{effects:?}");
    assert!(!picker_open(&a));
}

#[test]
fn a_broad_or_unknown_folder_opens_the_picker_and_keeps_the_steps_back() {
    let cases = [
        judged("/h", Verdict::Home),
        judged("/home", Verdict::AboveHome),
        judged("/usr", Verdict::Outside),
        Err(Refusal::NotFound),
        Err(Refusal::Root),
    ];
    for result in cases {
        let mut a = app();
        a.history = true;
        open(&mut a);
        let effects = answer(&mut a, Purpose::Open, result.clone());
        assert!(picker_open(&a), "{result:?}");
        assert_eq!((prepares(&effects), lists(&effects)), (0, 0), "{result:?}");
        assert!(chat(&a, id2()).folder_picker().unwrap().gathering());
    }
}

#[test]
fn the_candidates_are_gathered_from_the_tab_and_the_store() {
    let mut a = app();
    a.history = true;
    open(&mut a);
    let effects = answer(&mut a, Purpose::Open, judged("/h", Verdict::Home));
    // The store is asked first; the probe waits for its answer.
    assert!(gathers(&effects).is_empty(), "{effects:?}");
    assert!(effects.iter().any(
        |e| matches!(e, Effect::Store(StoreCmd::RecentFolders { pane, .. }) if *pane == id2())
    ));
    let effects = a.update(AppEvent::Store(StoreReply::RecentFolders {
        pane: id2(),
        folders: vec!["/w/a".into(), "/w/b".into()],
    }));
    let [(paths, git)] = gathers(&effects).try_into().expect("one gathering");
    assert!(git);
    let of = |source| {
        paths
            .iter()
            .filter(|(s, _)| *s == source)
            .map(|(_, p)| p.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(of(Source::LastUsed), vec![PathBuf::from("/w/a")]);
    assert_eq!(of(Source::Recent), vec![PathBuf::from("/w/b")]);
    assert_eq!(of(Source::PaneRoot), vec![PathBuf::from(LAUNCH)]);
}

#[test]
fn without_history_the_candidates_are_probed_at_once() {
    let mut a = app();
    open(&mut a);
    let effects = answer(&mut a, Purpose::Open, judged("/h", Verdict::Home));
    assert_eq!(gathers(&effects).len(), 1, "{effects:?}");
}

#[test]
fn the_rows_arrive_and_nothing_is_chosen() {
    let mut a = app();
    broad(&mut a);
    a.update(AppEvent::FoldersProbed {
        pane: id2(),
        id: picker_id(&a),
        busy: false,
        found: vec![(Source::PaneRoot, judged("/w/a", Verdict::Concrete))],
    });
    let picker = chat(&a, id2()).folder_picker().unwrap();
    assert_eq!(
        picker.rows(),
        [Row {
            path: "/w/a".into(),
            source: Source::PaneRoot
        }]
    );
    assert!(!picker.gathering());
    // No key was pressed: the folder is the one the pane had.
    assert_eq!(chat(&a, id2()).origin_cwd(), Some(Path::new(LAUNCH)));
}

#[test]
fn keys_and_pastes_edit_the_path_and_never_the_composer() {
    let mut a = app();
    broad(&mut a);
    a.update(key('x'));
    a.update(AppEvent::Paste("yz".into()));
    let chat = chat(&a, id2());
    assert_eq!(chat.folder_picker().unwrap().field().text(), "xyz");
    assert_eq!(chat.composer().text(), "");
}

#[test]
fn keys_do_nothing_while_the_open_probe_is_pending() {
    let mut a = app();
    open(&mut a);
    a.update(key('x'));
    assert_eq!(chat(&a, id2()).composer().text(), "");
}

#[test]
fn a_late_candidate_list_for_a_closed_picker_is_dropped() {
    let mut a = app();
    open(&mut a);
    answer(&mut a, Purpose::Open, judged(LAUNCH, Verdict::Concrete));
    a.update(AppEvent::FoldersProbed {
        pane: id2(),
        id: picker_id(&a),
        busy: false,
        found: Vec::new(),
    });
    assert!(!picker_open(&a));
}

// ---- accept and drop ------------------------------------------------------

fn with_rows(a: &mut App) {
    a.update(AppEvent::FoldersProbed {
        pane: id2(),
        id: picker_id(a),
        busy: false,
        found: vec![
            (Source::PaneRoot, judged("/w/a", Verdict::Concrete)),
            (Source::Recent, judged("/w/b", Verdict::Concrete)),
        ],
    });
}

fn enter() -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

fn systems(a: &App) -> Vec<String> {
    super::chat_history::systems(chat(a, id2()))
}

fn origins_of_prepares(effects: &[Effect]) -> Vec<Option<PathBuf>> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::PreparePrompt { origin, .. } => Some(origin.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn enter_probes_the_selected_row_and_the_answer_opens_the_chat_there() {
    let mut a = app();
    a.history = true;
    broad(&mut a);
    with_rows(&mut a);
    a.update(key_code(KeyCode::Down));
    let effects = a.update(enter());
    assert_eq!(probes(&effects), vec![(PathBuf::from("/w/b"), pick(&a))]);
    // Nothing changed yet: the folder is judged again first.
    assert_eq!(chat(&a, id2()).origin_cwd(), Some(Path::new(LAUNCH)));
    let effects = answer_pick(&mut a, judged("/w/b", Verdict::Concrete));
    assert!(!picker_open(&a));
    assert_eq!(chat(&a, id2()).origin_cwd(), Some(Path::new("/w/b")));
    // The held-back steps ran, for the new folder.
    assert!(origins_of_prepares(&effects).contains(&Some(PathBuf::from("/w/b"))));
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Store(StoreCmd::List { origin: Some(o), .. }) if o == Path::new("/w/b")
    )));
}

fn key_code(code: KeyCode) -> AppEvent {
    AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn esc_keeps_the_folder_says_so_and_runs_the_steps() {
    let mut a = app();
    a.history = true;
    broad(&mut a);
    let effects = a.update(key_code(KeyCode::Esc));
    assert!(!picker_open(&a));
    assert_eq!(chat(&a, id2()).origin_cwd(), Some(Path::new(LAUNCH)));
    assert!(systems(&a).contains(&format!(
        "chat folder is {LAUNCH}; use /cd <folder> to change it"
    )));
    assert_eq!((prepares(&effects), lists(&effects)), (1, 1), "{effects:?}");
}

#[test]
fn a_late_answer_after_esc_changes_nothing() {
    let mut a = app();
    broad(&mut a);
    with_rows(&mut a);
    a.update(enter());
    let purpose = pick(&a);
    a.update(key_code(KeyCode::Esc));
    answer(&mut a, purpose, judged("/w/a", Verdict::Concrete));
    assert_eq!(chat(&a, id2()).origin_cwd(), Some(Path::new(LAUNCH)));
}

#[test]
fn a_held_enter_never_accepts() {
    let mut a = app();
    broad(&mut a);
    with_rows(&mut a);
    let effects = a.update(super::chat_live::repeat(KeyCode::Enter, KeyModifiers::NONE));
    assert!(probes(&effects).is_empty());
}

#[test]
fn a_typed_tilde_gives_home_and_is_accepted() {
    let mut a = app();
    a.home = Some("/h".into());
    broad(&mut a);
    super::chat_history::type_text(&mut a, "~");
    let effects = a.update(enter());
    assert_eq!(probes(&effects), vec![(PathBuf::from("/h"), pick(&a))]);
    answer_pick(&mut a, judged("/h", Verdict::Home));
    assert_eq!(chat(&a, id2()).origin_cwd(), Some(Path::new("/h")));
    assert!(!picker_open(&a));
}

#[test]
fn typed_system_folders_and_root_are_refused_and_the_picker_stays() {
    for (text, result, reason) in [
        ("/usr", judged("/usr", Verdict::Outside), "not allowed"),
        ("/", Err(Refusal::Root), Refusal::Root.text()),
        ("/nope", Err(Refusal::NotFound), "folder not found"),
    ] {
        let mut a = app();
        broad(&mut a);
        super::chat_history::type_text(&mut a, text);
        a.update(enter());
        answer_pick(&mut a, result);
        let picker = chat(&a, id2()).folder_picker().expect("picker stays");
        assert_eq!(picker.error(), Some(reason), "{text}");
        assert_eq!(picker.field().text(), text);
        assert_eq!(chat(&a, id2()).origin_cwd(), Some(Path::new(LAUNCH)));
    }
}

#[test]
fn closing_the_pane_clears_what_waited_for_the_folder() {
    let mut a = app();
    open(&mut a);
    assert!(!a.opening.is_empty());
    press(&mut a, "wq");
    a.update(key('y'));
    assert!(a.opening.is_empty());
    // The late answer finds nobody.
    let effects = answer(&mut a, Purpose::Open, judged(LAUNCH, Verdict::Concrete));
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn a_reopened_picker_never_takes_the_answers_of_the_closed_one() {
    let mut a = app();
    broad(&mut a);
    let first = picker_id(&a);
    a.update(key_code(KeyCode::Esc));
    a.open_folder_picker(id2());
    let second = picker_id(&a);
    assert_ne!(first, second);
    a.update(AppEvent::FoldersProbed {
        pane: id2(),
        id: first,
        busy: false,
        found: vec![(Source::PaneRoot, judged("/w/old", Verdict::Concrete))],
    });
    assert!(chat(&a, id2()).folder_picker().unwrap().gathering());
}

#[test]
fn enter_spam_probes_once_until_the_answer() {
    let mut a = app();
    broad(&mut a);
    with_rows(&mut a);
    let first = a.update(enter());
    assert_eq!(probes(&first).len(), 1);
    super::let_go();
    let again = a.update(enter());
    assert!(probes(&again).is_empty(), "{again:?}");
}

// ---- review fixes ------------------------------------------------------------

fn prefix_key(a: &mut App, c: char) -> Vec<Effect> {
    a.update(ctrl_space());
    a.update(key(c))
}

fn editors_open(a: &App) -> bool {
    let chat = chat(a, id2());
    chat.tag_editor().is_some() || chat.note_editor().is_some() || chat.summary_draft().is_some()
}

#[test]
fn the_memory_and_tag_keys_wait_while_the_picker_is_open() {
    for c in ['n', 'o', 's', 'T'] {
        let mut a = app();
        a.history = true;
        broad(&mut a);
        let effects = prefix_key(&mut a, c);
        assert!(effects.is_empty(), "{c}: {effects:?}");
        assert!(!editors_open(&a), "{c}");
        assert_eq!(a.notice(), Some("Choose the chat folder first"), "{c}");
    }
}

#[test]
fn run_in_pane_and_the_approval_switch_wait_while_the_folder_is_asked() {
    for c in ['e', 'a'] {
        let mut a = app();
        broad(&mut a);
        let mode = chat(&a, id2()).approval_mode();
        let effects = prefix_key(&mut a, c);
        assert!(effects.is_empty(), "{c}: {effects:?}");
        assert_eq!(a.notice(), Some("Choose the chat folder first"), "{c}");
        assert_eq!(chat(&a, id2()).approval_mode(), mode, "{c}");
        assert!(chat(&a, id2()).approvals().pending().is_none(), "{c}");
        // The same while the open probe is still pending.
        let mut a = app();
        open(&mut a);
        prefix_key(&mut a, c);
        assert_eq!(a.notice(), Some("Choose the chat folder first"), "{c}");
    }
}

#[test]
fn the_same_keys_wait_while_the_open_probe_is_pending() {
    for c in ['n', 'o', 's', 'T'] {
        let mut a = app();
        a.history = true;
        open(&mut a);
        prefix_key(&mut a, c);
        assert!(!editors_open(&a), "{c}");
        assert_eq!(a.notice(), Some("Choose the chat folder first"), "{c}");
    }
}

#[test]
fn a_busy_chat_keeps_the_picker_open_with_the_reason() {
    let mut a = app();
    broad(&mut a);
    with_rows(&mut a);
    a.update(enter());
    a.waiting_sends.push((id2(), "hi".into()));
    let effects = answer_pick(&mut a, judged("/w/a", Verdict::Concrete));
    let picker = chat(&a, id2()).folder_picker().expect("the pick is kept");
    assert_eq!(picker.error(), Some("a message is waiting to be sent"));
    assert_eq!(chat(&a, id2()).origin_cwd(), Some(Path::new(LAUNCH)));
    assert_eq!((prepares(&effects), lists(&effects)), (0, 0), "{effects:?}");
    // The steps still wait: dropping the picker runs them.
    let effects = a.update(key_code(KeyCode::Esc));
    assert_eq!(prepares(&effects), 1, "{effects:?}");
}

#[test]
fn a_pane_opened_from_a_chat_in_home_asks_before_anything_starts() {
    let mut a = super::run_pane::one_shell_and_a_chat("hello");
    a.apply_folder(id2(), "/h".into());
    let opened = open(&mut a);
    let third = PaneId::for_test(3);
    assert_eq!(probes(&opened), vec![(PathBuf::from("/h"), Purpose::Open)]);
    let effects = a.update(AppEvent::FolderProbed {
        pane: third,
        purpose: Purpose::Open,
        result: judged("/h", Verdict::Home),
    });
    assert!(chat(&a, third).folder_picker().is_some());
    assert_eq!(prepares(&effects), 0, "{effects:?}");
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Memory(_))),
        "{effects:?}"
    );
}

#[test]
fn a_paste_waits_while_the_open_probe_is_pending() {
    let mut a = app();
    open(&mut a);
    a.update(AppEvent::Paste("text".into()));
    assert_eq!(chat(&a, id2()).composer().text(), "");
}

#[test]
fn settling_only_answers_the_probe_of_the_pane_that_opened() {
    let mut a = app();
    // A picker that is open already is not closed by an unrelated update.
    broad(&mut a);
    a.update_settled(key('x'));
    assert!(picker_open(&a));
}
