//! The GitHub star prompt (goal #744): one muted line on the composer hint of
//! an idle, ready chat; `g` stars with `gh` or opens the page, any other key
//! dismisses it and still reaches the chat. Shown at most once a run.

use super::chat_live::{chat_id, code, composer, plain, ready_app, repeat};
use super::tool_approval::{announce, live, run};
use super::*;
use crate::app::star::{ASK_GH, ASK_PAGE};
use crate::core::ai::chat::Entry;
use crate::core::star::{Due, REPO_URL, StarReply};

fn asked(gh: bool) -> App {
    let mut a = ready_app();
    a.update(AppEvent::StarChecked(Due::Ask { gh }));
    a
}

fn shown(a: &App) -> Option<String> {
    a.chat_guidance(chat_id())
}

fn lines(a: &App) -> Vec<String> {
    let chat = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .expect("a chat");
    (chat.entries().iter())
        .filter_map(|e| match e {
            Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

fn star_effects(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| {
            matches!(
                e,
                Effect::StarRepo | Effect::StarMark { .. } | Effect::OpenUrl(_)
            )
        })
        .collect()
}

// S2: nothing shows until the start-up check says to ask.
#[test]
fn nothing_shows_before_the_check_asks() {
    let mut a = ready_app();
    let before = shown(&a);
    a.update(AppEvent::StarChecked(Due::No));
    assert_eq!(shown(&a), before);
    assert_ne!(shown(&a).as_deref(), Some(ASK_GH));
}

// S2/S3: an idle, ready chat with an empty composer shows the prompt muted;
// the text says what `g` does with and without `gh`.
#[test]
fn an_idle_empty_chat_shows_the_prompt_as_a_hint() {
    let a = asked(true);
    assert_eq!(shown(&a).as_deref(), Some(ASK_GH));
    assert!(a.guidance_is_hint(chat_id()));
    assert_eq!(shown(&asked(false)).as_deref(), Some(ASK_PAGE));
}

// S3/S5: `g` stars with the user's `gh`, and the key goes no further.
#[test]
fn g_stars_with_gh_and_is_not_typed() {
    let mut a = asked(true);
    let effects = a.update(key('g'));
    assert_eq!(star_effects(&effects), vec![&Effect::StarRepo]);
    assert_eq!(composer(&a), "");
    assert_ne!(shown(&a).as_deref(), Some(ASK_GH));
}

// S3: GitHub took the star: a thank-you line in the chat, nothing opened.
#[test]
fn a_star_taken_says_thank_you() {
    let mut a = asked(true).with_opener(true);
    a.update(key('g'));
    let effects = a.update(AppEvent::Starred(StarReply::Starred));
    assert!(star_effects(&effects).is_empty());
    assert_eq!(
        lines(&a).last().map(String::as_str),
        Some("Starred pichu2707/lazarobox-shell on GitHub. Thank you!")
    );
}

// S3: a failed star opens the repository page instead, and says why.
#[test]
fn a_failed_star_opens_the_page() {
    let mut a = asked(true).with_opener(true);
    a.update(key('g'));
    let effects = a.update(AppEvent::Starred(StarReply::Failed("HTTP 403".into())));
    assert_eq!(
        star_effects(&effects),
        vec![
            &Effect::StarMark { dismissed: false },
            &Effect::OpenUrl(REPO_URL.to_owned())
        ]
    );
    let last = lines(&a).pop().unwrap_or_default();
    assert!(
        last.contains("HTTP 403") && last.contains(REPO_URL),
        "{last}"
    );
}

// S3: without `gh`, `g` opens the page and marks the prompt answered; with
// no opener it names the page instead.
#[test]
fn g_without_gh_opens_the_page_or_names_it() {
    let mut a = asked(false).with_opener(true);
    let effects = a.update(key('g'));
    assert_eq!(
        star_effects(&effects),
        vec![
            &Effect::StarMark { dismissed: false },
            &Effect::OpenUrl(REPO_URL.to_owned())
        ]
    );
    let mut a = asked(false);
    let effects = a.update(key('g'));
    assert_eq!(
        star_effects(&effects),
        vec![&Effect::StarMark { dismissed: false }]
    );
    assert!(lines(&a).last().is_some_and(|l| l.contains(REPO_URL)));
}

// S4: any other key dismisses the prompt, is still typed, and the prompt does
// not come back this run once the composer is empty again.
#[test]
fn another_key_dismisses_and_still_reaches_the_composer() {
    let mut a = asked(true);
    let effects = a.update(key('h'));
    assert_eq!(
        star_effects(&effects),
        vec![&Effect::StarMark { dismissed: true }]
    );
    assert_eq!(composer(&a), "h");
    a.update(plain(KeyCode::Backspace));
    assert_eq!(composer(&a), "");
    assert_ne!(shown(&a).as_deref(), Some(ASK_GH));
}

// S4: a shifted `G` is not `g`: it dismisses and is typed.
#[test]
fn a_shifted_g_dismisses() {
    let mut a = asked(true);
    let effects = a.update(code(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(
        star_effects(&effects),
        vec![&Effect::StarMark { dismissed: true }]
    );
    assert_eq!(composer(&a), "G");
}

// A held key never answers the prompt.
#[test]
fn a_held_g_does_not_star() {
    let mut a = asked(true);
    let effects = a.update(repeat(KeyCode::Char('g'), KeyModifiers::NONE));
    assert!(!effects.contains(&Effect::StarRepo));
    assert!(!effects.contains(&Effect::StarMark { dismissed: true }));
}

// S2: text in the composer hides the prompt; emptying it shows it again
// (nothing was answered while it was hidden).
#[test]
fn a_composer_with_text_hides_the_prompt() {
    let mut a = ready_app();
    a.update(key('h'));
    a.update(AppEvent::StarChecked(Due::Ask { gh: true }));
    assert_ne!(shown(&a).as_deref(), Some(ASK_GH));
    let effects = a.update(plain(KeyCode::Backspace));
    assert!(star_effects(&effects).is_empty());
    assert_eq!(shown(&a).as_deref(), Some(ASK_GH));
}

// S2: never during a turn, and a key during it does not dismiss.
#[test]
fn a_turn_in_flight_hides_the_prompt() {
    let (mut a, _turn) = live();
    a.update(AppEvent::StarChecked(Due::Ask { gh: true }));
    assert_ne!(shown(&a).as_deref(), Some(ASK_GH));
    let effects = a.update(key('g'));
    assert!(star_effects(&effects).is_empty());
}

// S2: an approval waiting in another pane hides the prompt in an idle chat.
#[test]
fn an_approval_in_any_pane_hides_the_prompt() {
    let (mut a, turn) = live();
    press(&mut a, "c");
    let other = a.focused();
    assert_ne!(other, id2());
    a.update(AppEvent::PromptReady(
        other,
        crate::core::ai::chat::prompts::Resolved {
            text: "system".to_owned(),
            notices: Vec::new(),
        },
    ));
    a.update(AppEvent::StarChecked(Due::Ask { gh: true }));
    assert_eq!(a.chat_guidance(other).as_deref(), Some(ASK_GH));
    announce(&mut a, turn, 1, run("ls"));
    assert!(a.approval_pending(id2()));
    assert_ne!(a.chat_guidance(other).as_deref(), Some(ASK_GH));
}
