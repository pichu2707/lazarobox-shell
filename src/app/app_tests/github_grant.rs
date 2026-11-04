//! The chat grant of the GitHub tools (github-tools, The chat grant): the
//! families a turn offers and the budget it reserves come from one function,
//! and the grant ends exactly where the spec says.

use std::path::PathBuf;

use super::chat_live::{ollama, plain};
use super::run_pane::chat;
use super::*;
use crate::core::ai::{
    chat::{
        adapters::tools_reserve_tokens,
        engine::{ChatRequest, Finish, StreamEvent},
    },
    config::{Assignment, ToolToggle},
    tools::registry::Family,
};

pub(super) fn tools_app() -> App {
    let mut a = launched(80, 25)
        .with_ai(ollama())
        .with_tool_engines(&["ollama"]);
    press(&mut a, "c");
    super::run_pane::answer(&mut a, id2(), "hi");
    a
}

pub(super) fn grant(a: &mut App, id: PaneId) {
    if let Some(chat) = a.panes.get_mut(&id).and_then(PaneState::chat_mut) {
        chat.set_github_granted(true);
    }
}

pub(super) fn granted(a: &App, id: PaneId) -> bool {
    chat(a, id).github_granted()
}

/// Sends one message and returns the turn and its request.
fn send(a: &mut App) -> (u64, ChatRequest) {
    a.update(key('q'));
    let effects = a.update(plain(KeyCode::Enter));
    let [Effect::ChatSend { turn, request, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    (*turn, request.clone())
}

fn finish(a: &mut App, turn: u64) {
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
}

fn lines(a: &App, id: PaneId) -> Vec<String> {
    (chat(a, id).entries().iter())
        .filter_map(|e| match e {
            crate::core::ai::chat::Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

const FOLDER_LINE: &str = "GitHub tools are off because the folder changed";
const TOOLS_LINE: &str = "GitHub tools are off because tools were turned off";
const TOGGLE_LINE: &str = "GitHub tools are off because the GitHub toggle was turned off";

fn count(a: &App, id: PaneId, line: &str) -> usize {
    lines(a, id).iter().filter(|l| *l == line).count()
}

// Spec (Availability, Master switch wins): a grant adds the family only when
// tools are on; the saved toggles stay in the set.
#[test]
fn a_grant_adds_the_github_family_only_with_tools_on() {
    let mut a = tools_app();
    assert_eq!(a.offered_families(id2()), a.ai.families());
    assert!(!a.offered_families(id2()).contains(Family::Github));
    grant(&mut a, id2());
    let saved = a.ai.families();
    assert_eq!(a.offered_families(id2()), saved.with(Family::Github));
    // The other families of the saved toggles are kept (triangulation).
    a.ai.set_toggle(ToolToggle::Search, true);
    let both = a.offered_families(id2());
    assert!(both.contains(Family::Github) && both.contains(Family::Search));
    // The master switch beats the grant.
    a.ai.set_tools_enabled(false);
    assert_eq!(a.offered_families(id2()), a.ai.families());
    assert!(!a.offered_families(id2()).contains(Family::Github));
    // A pane that is gone offers the saved set.
    assert_eq!(a.offered_families(PaneId::FIRST), a.ai.families());
}

// Spec (Grant counts in the budget): the tool list and the context budget read
// the same function, so they cannot drift.
#[test]
fn the_offered_tools_and_the_budget_cannot_drift() {
    let mut reserves = Vec::new();
    for granted in [false, true] {
        let mut a = tools_app();
        if granted {
            grant(&mut a, id2());
        }
        let (_, request) = send(&mut a);
        assert_eq!(request.families, a.offered_families(id2()), "{granted}");
        assert_eq!(
            request.families.contains(Family::Github),
            granted,
            "{granted}"
        );
        assert_eq!(
            chat(&a, id2()).tools_reserve(),
            tools_reserve_tokens(request.families),
            "{granted}"
        );
        reserves.push(chat(&a, id2()).tools_reserve());
    }
    assert!(
        reserves[1] > reserves[0],
        "the three schemas weigh: {reserves:?}"
    );
}

// Spec (Grant takes effect next message, Grant does not touch the toggle).
#[test]
fn a_grant_waits_for_the_next_message_and_leaves_the_toggle_alone() {
    let mut a = tools_app();
    let (turn, running) = send(&mut a);
    grant(&mut a, id2());
    assert!(!running.families.contains(Family::Github));
    finish(&mut a, turn);
    let (_, next) = send(&mut a);
    assert!(next.families.contains(Family::Github));
    assert!(!a.ai.toggle(ToolToggle::Github), "the saved toggle is off");
    // A second chat has no tool, and a new chat does not inherit it.
    press(&mut a, "c");
    let other = a.focused();
    assert_ne!(other, id2());
    assert!(!granted(&a, other));
    assert!(!a.offered_families(other).contains(Family::Github));
}

// Spec: folder change and folder restore end the grant, with a visible line.
#[test]
fn a_folder_change_or_restore_ends_the_grant_and_says_so() {
    for restore in [false, true] {
        let mut a = tools_app();
        grant(&mut a, id2());
        let new = PathBuf::from("/tmp");
        if restore {
            a.restore_folder(id2(), new);
        } else {
            a.apply_folder(id2(), new);
        }
        assert!(!granted(&a, id2()), "restore {restore}");
        assert_eq!(
            lines(&a, id2())
                .iter()
                .filter(|l| *l == FOLDER_LINE)
                .count(),
            1,
            "restore {restore}"
        );
        assert!(!a.offered_families(id2()).contains(Family::Github));
    }
    // Without a grant there is nothing to announce.
    let mut a = tools_app();
    a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert!(!lines(&a, id2()).iter().any(|l| l == FOLDER_LINE));
}

// Spec: tools off then on ends it.
#[test]
fn turning_the_tools_off_and_on_ends_the_grant() {
    let mut a = tools_app();
    grant(&mut a, id2());
    a.ai.set_tools_enabled(false);
    a.sync_tools();
    a.ai.set_tools_enabled(true);
    a.sync_tools();
    assert!(!granted(&a, id2()));
    assert!(!a.offered_families(id2()).contains(Family::Github));
}

// Spec: a grant that ends without a user action says so, in each chat that
// held one and in no other.
#[test]
fn tools_off_says_so_in_each_chat_that_held_a_grant() {
    let mut a = tools_app();
    grant(&mut a, id2());
    press(&mut a, "c");
    let bare = a.focused();
    assert_ne!(bare, id2());
    a.ai.set_tools_enabled(false);
    a.sync_tools();
    a.sync_tools();
    assert_eq!(count(&a, id2(), TOOLS_LINE), 1, "{:?}", lines(&a, id2()));
    assert_eq!(count(&a, bare, TOOLS_LINE), 0, "{:?}", lines(&a, bare));
}

pub(super) fn save(a: &mut App, draft: AiConfig) {
    a.menu = Some(MenuState::open(&a.config).with_ai(draft, a.engines.clone()));
    a.input = InputMode::Menu;
    a.update(AppEvent::ConfigSaved);
}

// Spec: the saved toggle going from on to off ends every chat's grant; a save
// that leaves it as it was does not.
#[test]
fn a_save_turning_the_toggle_off_ends_every_grant() {
    let mut a = tools_app();
    grant(&mut a, id2());
    press(&mut a, "c");
    let second = a.focused();
    grant(&mut a, second);
    let mut draft = a.ai.clone();
    draft.set_toggle(ToolToggle::Search, true);
    save(&mut a, draft);
    assert!(
        granted(&a, id2()) && granted(&a, second),
        "off to off keeps"
    );
    let mut draft = a.ai.clone();
    draft.set_toggle(ToolToggle::Github, true);
    save(&mut a, draft);
    assert!(granted(&a, id2()) && granted(&a, second), "off to on keeps");
    let mut draft = a.ai.clone();
    draft.set_toggle(ToolToggle::Github, false);
    save(&mut a, draft);
    assert!(!granted(&a, id2()) && !granted(&a, second));
    // Each chat that held one is told, once.
    for id in [id2(), second] {
        assert_eq!(count(&a, id, TOGGLE_LINE), 1, "{:?}", lines(&a, id));
    }
}

// Spec: a change of engine keeps it.
#[test]
fn a_change_of_engine_keeps_the_grant() {
    let mut a = tools_app();
    grant(&mut a, id2());
    a.update(AppEvent::EnginesDetected(super::chat_live::detection(&[
        "llama3:8b",
        "qwen:7b",
    ])));
    a.ai.set(
        Task::Chat,
        Assignment {
            engine: Some("ollama".into()),
            model: Some("qwen:7b".into()),
        },
    );
    assert!(granted(&a, id2()));
    let (_, request) = send(&mut a);
    assert!(request.families.contains(Family::Github));
}

// Spec (The chat grant, Grant ends with the pane): the grant lives in the
// pane's state, so closing the pane drops it and a new pane does not inherit
// it, even one opened right after in the same tab.
#[test]
fn the_grant_ends_when_the_pane_ends() {
    let mut a = tools_app();
    grant(&mut a, id2());
    assert!(a.offered_families(id2()).contains(Family::Github));
    press(&mut a, "wq");
    a.update(key('y'));
    assert!(!a.panes.contains_key(&id2()), "the pane is closed");
    press(&mut a, "c");
    let fresh = a.focused();
    assert!(a.panes.contains_key(&fresh));
    assert!(!granted(&a, fresh));
    assert!(!a.offered_families(fresh).contains(Family::Github));
}

// Spec (Resume has no grant): resuming a conversation into a pane that holds a
// grant ends it, and the next turn offers the saved set.
#[test]
fn a_resume_into_a_pane_with_a_grant_leaves_no_grant() {
    use super::chat_history::{chat_id, listing, resumed};
    let mut a = listing().with_tool_engines(&["ollama"]);
    grant(&mut a, chat_id());
    assert!(granted(&a, chat_id()));
    a.update_settled(resumed(chat_id(), 30, "programmer"));
    assert!(!granted(&a, chat_id()));
    assert_eq!(a.offered_families(chat_id()), a.ai.families());
}
