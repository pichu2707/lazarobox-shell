//! Spec (issue-draft, chat-pane): the y/n of a drafted issue and the form that
//! opens at the end of the turn with what the user accepted.

use super::chat_live::{ctrl, plain};
use super::run_pane::chat;
use super::tool_approval::{announce, live, pending, read, result};
use super::*;
use crate::core::ai::{
    chat::{
        approval::{ANOTHER_PENDING, ApprovalAsk, ISSUE_DRAFT_KEYS},
        engine::{CallId, Decision, Finish, StreamEvent, ToolStatus},
    },
    config::{ApprovalMode, ToolToggle},
    tools::{
        issue_draft::IssueDraft,
        registry::{Family, FamilySet},
    },
};
use crate::core::menu::feedback::Kind;

fn draft() -> ApprovalAsk {
    ApprovalAsk::IssueDraft(IssueDraft {
        kind: Kind::Idea,
        title: "See the shell over ssh".into(),
        description: "Why.\nWhat.".into(),
    })
}

fn decision(turn: u64, call: u64, decision: Decision) -> Effect {
    Effect::ToolDecision {
        pane: id2(),
        worker: None,
        turn,
        call: CallId(call),
        decision,
    }
}

fn done(a: &mut App, turn: u64) -> Vec<Effect> {
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)))
}

/// The form's kind, title and description, while it is open.
fn form(a: &App) -> Option<(Kind, String, String)> {
    a.menu()?
        .feedback_form()
        .map(|f| (f.kind(), f.title(), f.description()))
}

fn opened() -> Option<(Kind, String, String)> {
    Some((
        Kind::Idea,
        "See the shell over ssh".to_owned(),
        "Why.\nWhat.".to_owned(),
    ))
}

fn lines(a: &App) -> Vec<String> {
    super::chat_history::systems(chat(a, id2()))
}

/// The `y` then the tool's result, and a turn that goes on streaming after.
fn accepted() -> (App, u64) {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, draft());
    assert_eq!(a.update(key('y')), vec![decision(turn, 1, Decision::Sent)]);
    result(&mut a, turn, 1, ToolStatus::Ok);
    (a, turn)
}

// D-permission: every mode prompts, the prompt names the kind and the title,
// and only an unmodified lowercase `y` accepts.
#[test]
fn every_mode_prompts_and_only_a_lowercase_y_accepts() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = live();
        super::tool_run::chat_mut(&mut a).set_approval_mode(mode);
        assert_eq!(announce(&mut a, turn, 1, draft()), Vec::new(), "{mode:?}");
        assert_eq!(pending(&a).map(|(ask, _)| ask), Some(draft()), "{mode:?}");
        let prompt = a.approval_prompt(id2()).unwrap();
        assert_eq!(prompt.target, "draft issue: Idea or goal");
        assert_eq!(prompt.command, "See the shell over ssh");
        assert_eq!(prompt.keys, ISSUE_DRAFT_KEYS);
        for other in [key('Y'), plain(KeyCode::Enter), key(' '), key('a')] {
            assert_eq!(a.update(other), vec![], "{mode:?}");
            assert!(pending(&a).is_some(), "{mode:?}");
        }
        assert_eq!(a.update(ctrl('y')), vec![], "{mode:?}: only unmodified");
        assert!(pending(&a).is_some(), "{mode:?}");
        assert_eq!(
            a.update(key('y')),
            vec![decision(turn, 1, Decision::Sent)],
            "{mode:?}"
        );
        assert_eq!(pending(&a), None, "{mode:?}");
    }
}

// D2, D3: `y` answers at once, and the form is not opened until the turn ends.
#[test]
fn y_answers_at_once_and_the_form_waits_for_the_end_of_the_turn() {
    let (mut a, turn) = accepted();
    assert!(a.menu().is_none(), "still streaming");
    assert_eq!(a.input(), InputMode::Terminal);
    done(&mut a, turn);
    assert_eq!(a.input(), InputMode::Menu);
    assert_eq!(form(&a), opened());
    a.update(plain(KeyCode::Esc));
    assert!(a.menu().is_some_and(|m| !m.feedback_open()));
    let menu = a.menu().unwrap();
    assert_eq!(menu.selected_label(a.config()), Some("Feedback"));
}

// Nothing leaves before the Ctrl+S of the form: opening the form is not a send.
#[test]
fn the_form_opening_emits_no_send() {
    let (mut a, turn) = accepted();
    let effects = done(&mut a, turn);
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::SendIssue(_))),
        "{effects:?}"
    );
}

// A failed turn opens the form too: the user accepted before it failed.
#[test]
fn a_failed_turn_still_opens_the_accepted_draft() {
    let (mut a, turn) = accepted();
    a.update(AppEvent::Chat(
        id2(),
        turn,
        StreamEvent::Error(crate::core::ai::chat::engine::ChatError::Failed(
            "boom".into(),
        )),
    ));
    assert_eq!(form(&a), opened());
}

// A cancel ends the turn: the accepted draft still opens, not at a later turn.
#[test]
fn a_cancelled_turn_opens_the_accepted_draft_at_once() {
    let (mut a, _turn) = accepted();
    a.update(ctrl('c'));
    assert_eq!(form(&a), opened());
}

// `n` and Esc end as denied, and no draft opens at the turn's end.
#[test]
fn a_decline_opens_nothing_and_says_so() {
    for decline in [key('n'), plain(KeyCode::Esc)] {
        let (mut a, turn) = live();
        announce(&mut a, turn, 1, draft());
        assert_eq!(
            a.update(decline),
            vec![decision(turn, 1, Decision::Denied("the user declined"))]
        );
        assert_eq!(pending(&a), None);
        assert!(
            lines(&a).iter().any(|l| l.contains("no draft issue")),
            "{:?}",
            lines(&a)
        );
        result(&mut a, turn, 1, ToolStatus::Denied);
        done(&mut a, turn);
        assert!(a.menu().is_none());
    }
}

// Ctrl+C at the prompt: denied, the stream is cancelled, and no form opens.
#[test]
fn a_cancel_at_the_prompt_is_denied_and_opens_nothing() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, draft());
    let effects = a.update(ctrl('c'));
    assert_eq!(
        effects,
        vec![
            decision(turn, 1, Decision::Denied("cancelled")),
            Effect::ChatCancel(id2())
        ]
    );
    assert!(a.menu().is_none());
}

// A timeout ends the call without a key: the prompt goes, the notice says no
// draft opened, and the end of the turn opens nothing.
#[test]
fn a_timed_out_prompt_goes_away_and_opens_nothing() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, draft());
    result(&mut a, turn, 1, ToolStatus::Denied);
    assert_eq!(pending(&a), None);
    assert!(lines(&a).iter().any(|l| l.contains("no draft issue")));
    done(&mut a, turn);
    assert!(a.menu().is_none());
}

// The turn ends with the prompt still up: the runtime denies it, and the chat
// says that no draft opened.
#[test]
fn a_turn_that_ends_with_the_prompt_up_opens_nothing_and_says_so() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, draft());
    done(&mut a, turn);
    assert_eq!(pending(&a), None);
    assert!(a.menu().is_none());
    assert!(lines(&a).iter().any(|l| l.contains("no draft issue")));
}

// A closed chat: the pane is gone, so nothing can answer and nothing opens.
#[test]
fn a_closed_chat_opens_nothing() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, draft());
    press(&mut a, "wq");
    a.update(key('y'));
    assert!(a.pane(id2()).is_none());
    done(&mut a, turn);
    assert!(a.menu().is_none());
}

// One pending approval: a draft that arrives while another waits is denied at
// once, and the prompt that waits is left alone.
#[test]
fn a_draft_while_another_approval_waits_is_denied_at_once() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, read("/home/u/x"));
    assert_eq!(
        announce(&mut a, turn, 2, draft()),
        vec![decision(turn, 2, Decision::Denied(ANOTHER_PENDING))]
    );
    assert!(matches!(
        pending(&a).map(|(ask, _)| ask),
        Some(ApprovalAsk::Read { .. })
    ));
}

// A form the user has open is never overwritten: the call is denied, no
// prompt opens, and the form stays as it was.
#[test]
fn a_call_while_the_form_is_open_is_denied_and_the_form_is_kept() {
    let (mut a, turn) = live();
    a.input = InputMode::Terminal;
    let _ = a.open_menu();
    a.menu.as_mut().unwrap().open_feedback("My own text\nmore");
    assert_eq!(
        announce(&mut a, turn, 1, draft()),
        vec![decision(
            turn,
            1,
            Decision::Denied("a feedback form is already open")
        )]
    );
    assert_eq!(pending(&a), None);
    let form = a.menu().unwrap().feedback_form().unwrap();
    assert_eq!(form.title(), "My own text");
}

// A form opened by hand between the `y` and the end of the turn stays: the
// draft does not replace it and the user is told.
#[test]
fn a_form_opened_after_y_is_not_overwritten_at_the_end_of_the_turn() {
    let (mut a, turn) = accepted();
    let _ = a.open_menu();
    a.menu.as_mut().unwrap().open_feedback("Mine\nbody");
    done(&mut a, turn);
    assert_eq!(a.menu().unwrap().feedback_form().unwrap().title(), "Mine");
    assert!(
        lines(&a).iter().any(|l| l.contains("not opened")),
        "{:?}",
        lines(&a)
    );
}

// An open menu without the form keeps the user's edits: the draft opens in it.
#[test]
fn an_open_menu_without_the_form_gets_the_draft_in_place() {
    let (mut a, turn) = accepted();
    let _ = a.open_menu();
    done(&mut a, turn);
    assert_eq!(form(&a), opened());
    assert_eq!(a.input(), InputMode::Menu);
}

// The user is doing something else (prefix mode): the draft does not steal it.
#[test]
fn the_form_does_not_take_over_another_input_mode() {
    let (mut a, turn) = accepted();
    a.update(ctrl_space());
    assert_eq!(a.input(), InputMode::Prefix);
    done(&mut a, turn);
    assert!(a.menu().is_none());
    assert_eq!(a.input(), InputMode::Prefix);
    assert!(
        lines(&a).iter().any(|l| l.contains("not opened")),
        "{:?}",
        lines(&a)
    );
}

// The host can still deny after the app's `y` (a parallel decline of the
// turn): what it denied does not open.
#[test]
fn a_draft_the_host_denied_after_y_does_not_open() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, draft());
    a.update(key('y'));
    result(&mut a, turn, 1, ToolStatus::Denied);
    done(&mut a, turn);
    assert!(a.menu().is_none());
}

// A second accepted draft in a turn replaces the first: one form, the last `y`.
#[test]
fn the_last_accepted_draft_of_the_turn_wins() {
    let (mut a, turn) = accepted();
    let second = ApprovalAsk::IssueDraft(IssueDraft {
        kind: Kind::Bug,
        title: "A bug".into(),
        description: "It breaks".into(),
    });
    announce(&mut a, turn, 2, second);
    a.update(key('y'));
    result(&mut a, turn, 2, ToolStatus::Ok);
    done(&mut a, turn);
    assert_eq!(
        form(&a),
        Some((Kind::Bug, "A bug".to_owned(), "It breaks".to_owned()))
    );
}

// With the toggle or the master switch off, the family is not offered.
#[test]
fn the_family_follows_its_toggle_and_the_master_switch() {
    let (mut a, _turn) = live();
    a.ai.set_tools_enabled(true);
    assert!(a.ai.families().contains(Family::IssueDraft), "default on");
    assert!(a.offered_families(id2()).contains(Family::IssueDraft));
    a.ai.set_toggle(ToolToggle::IssueDraft, false);
    assert!(!a.offered_families(id2()).contains(Family::IssueDraft));
    a.ai.set_toggle(ToolToggle::IssueDraft, true);
    a.ai.set_tools_enabled(false);
    assert_eq!(a.offered_families(id2()), FamilySet::LEGACY);
}

// No approval slot is taken by the pending draft: a later approval of the
// same turn still reaches the user, which is why the form waits (D3).
#[test]
fn an_accepted_draft_leaves_room_for_a_later_approval_of_the_turn() {
    let (mut a, turn) = accepted();
    announce(&mut a, turn, 2, read("/home/u/y"));
    assert!(pending(&a).is_some());
    assert!(a.menu().is_none());
}

// A second parallel call is refused; the `y` on the first still opens its form.
#[test]
fn a_y_on_the_first_of_two_parallel_drafts_opens_its_form() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, draft());
    assert_eq!(
        announce(&mut a, turn, 2, draft()),
        vec![decision(turn, 2, Decision::Denied(ANOTHER_PENDING))]
    );
    result(&mut a, turn, 2, ToolStatus::Denied);
    a.update(key('y'));
    result(&mut a, turn, 1, ToolStatus::Ok);
    done(&mut a, turn);
    assert_eq!(form(&a), opened());
}

// A `y` that did not end `ok` is never silent: the chat says no draft opened.
#[test]
fn a_y_the_host_did_not_honour_leaves_a_line() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, draft());
    a.update(key('y'));
    result(&mut a, turn, 1, ToolStatus::Denied);
    assert!(
        lines(&a).iter().any(|l| l.contains("no draft issue")),
        "{:?}",
        lines(&a)
    );
}

// The Memory warning is up when the turn ends: moving the selection under it
// would lose its acceptance, so the form stays shut and the chat says so.
#[test]
fn the_form_does_not_open_under_the_memory_warning() {
    let (mut a, turn) = accepted();
    let _ = a.open_menu();
    a.menu.as_mut().unwrap().goto("Memory");
    a.update(key('l'));
    assert!(a.menu().unwrap().in_warning());
    done(&mut a, turn);
    let menu = a.menu().unwrap();
    assert!(menu.in_warning() && !menu.feedback_open());
    assert!(
        lines(&a)
            .iter()
            .any(|l| l.contains("not opened") && l.contains("See the shell over ssh")),
        "{:?}",
        lines(&a)
    );
    a.update(key('y'));
    let menu = a.menu().unwrap();
    assert!(!menu.in_warning());
    assert!(menu.ai_draft().memory.enabled, "the acceptance is kept");
}

// -- PR 2: the menu views a draft never opens over --------------------------

/// The menu's rows and footer, to show a view was left as it was.
fn view(a: &App) -> String {
    let menu = a.menu().unwrap();
    format!("{:?} {:?}", menu.rows(&a.config), menu.footer())
}

/// An accepted draft, then `open` puts a view of the menu up before the turn
/// ends: the form stays shut, the view is untouched and the chat names the
/// draft's title.
fn kept_under(open: impl FnOnce(&mut App)) {
    let (mut a, turn) = accepted();
    let _ = a.open_menu();
    open(&mut a);
    let before = view(&a);
    done(&mut a, turn);
    assert!(!a.menu().unwrap().feedback_open(), "{before}");
    assert_eq!(view(&a), before);
    assert_eq!(a.input(), InputMode::Menu);
    assert!(
        lines(&a)
            .iter()
            .any(|l| l.contains("not opened") && l.contains("See the shell over ssh")),
        "{:?}",
        lines(&a)
    );
}

#[test]
fn the_form_does_not_open_over_the_engine_picker() {
    kept_under(|a| {
        goto_row(a, "chat");
        a.update(plain(KeyCode::Enter));
        assert!(a.menu().unwrap().in_sub_level(), "the picker is open");
    });
}

#[test]
fn the_form_does_not_open_over_the_model_delete_confirmation() {
    use super::model_delete::{confirming, machine, open_picker};
    kept_under(|a| {
        a.engines = Some(machine());
        a.menu = Some(MenuState::open(&a.config).with_ai(a.ai.clone(), a.engines.clone()));
        a.input = InputMode::Menu;
        open_picker(a, "qwen3:4b");
        confirming(a);
        assert!(view(a).contains("y delete"), "{}", view(a));
    });
}

#[test]
fn the_form_does_not_open_over_the_token_field() {
    kept_under(|a| {
        goto_row(a, "Hugging Face");
        a.update(plain(KeyCode::Enter));
        assert!(a.menu().unwrap().in_sub_level(), "the token field is open");
    });
}

#[test]
fn the_form_does_not_open_over_the_disconnect_prompt() {
    use crate::core::ai::hf::{AccountReply, TokenFacts};
    kept_under(|a| {
        a.hf_tokens = || TokenFacts {
            env: false,
            saved: true,
        };
        // The menu reads the token facts when it opens.
        a.menu = None;
        a.input = InputMode::Terminal;
        let _ = a.open_menu();
        goto_row(a, "Hugging Face");
        a.update(plain(KeyCode::Enter));
        a.update(AppEvent::HfAccount(AccountReply::Checked(Ok("ana".into()))));
        a.update(plain(KeyCode::Enter));
        assert!(!a.menu().unwrap().in_sub_level(), "a footer, not a view");
    });
}

#[test]
fn the_form_does_not_open_over_the_usage_view() {
    kept_under(|a| {
        goto_row(a, "Usage (alpha)");
        a.update(plain(KeyCode::Enter));
        assert!(a.menu().unwrap().in_sub_level(), "the usage view is open");
    });
}

// -- PR 2 (D6): the `/feedback` hint where the draft tool is not offered -----

const HINT: &str = crate::app::tool::FEEDBACK_HINT;

/// A turn sent after `setup`, once the first turn of the chat has ended. The
/// engine is verified for tools, so only `setup` takes the draft tool away.
fn sent_after(setup: impl FnOnce(&mut App)) -> (App, u64) {
    let (mut a, turn) = live();
    done(&mut a, turn);
    a.tool_engines = &["ollama"];
    setup(&mut a);
    a.update(key('q'));
    let turn = super::run_pane::enter_turn(&mut a);
    (a, turn)
}

fn toggle_off(a: &mut App) {
    a.ai.set_toggle(ToolToggle::IssueDraft, false);
}

/// The three ways a turn is sent without the draft tool.
const WITHOUT_DRAFT: [fn(&mut App); 3] = [
    toggle_off,
    |a| a.ai.set_tools_enabled(false),
    |a| a.tool_engines = &[],
];

fn fail(a: &mut App, turn: u64) {
    a.update(AppEvent::Chat(
        id2(),
        turn,
        StreamEvent::Error(crate::core::ai::chat::engine::ChatError::Failed(
            "boom".into(),
        )),
    ));
}

fn hints(a: &App) -> usize {
    lines(a).iter().filter(|l| *l == HINT).count()
}

#[test]
fn a_failed_turn_without_the_draft_tool_suggests_feedback() {
    for setup in WITHOUT_DRAFT {
        let (mut a, turn) = sent_after(setup);
        assert_eq!(hints(&a), 0);
        fail(&mut a, turn);
        assert_eq!(hints(&a), 1, "{:?}", lines(&a));
    }
}

#[test]
fn a_tool_error_without_the_draft_tool_suggests_feedback() {
    let (mut a, turn) = sent_after(toggle_off);
    result(&mut a, turn, 1, ToolStatus::Error);
    assert_eq!(hints(&a), 1, "{:?}", lines(&a));
}

// A refusal is the user's or the app's choice, not a failure; a clean turn
// says nothing.
#[test]
fn a_denied_tool_or_a_clean_turn_suggests_nothing() {
    let (mut a, turn) = sent_after(toggle_off);
    result(&mut a, turn, 1, ToolStatus::Denied);
    result(&mut a, turn, 2, ToolStatus::Ok);
    done(&mut a, turn);
    assert_eq!(hints(&a), 0, "{:?}", lines(&a));
    // A cancel is the user's choice too.
    let (mut a, _turn) = sent_after(toggle_off);
    a.update(ctrl('c'));
    assert_eq!(hints(&a), 0, "{:?}", lines(&a));
}

// With the draft tool offered the model offers the draft itself: no hint.
#[test]
fn with_the_draft_tool_offered_there_is_no_hint() {
    let (mut a, turn) = sent_after(|_| {});
    assert!(a.offered_families(id2()).contains(Family::IssueDraft));
    result(&mut a, turn, 1, ToolStatus::Error);
    fail(&mut a, turn);
    assert_eq!(hints(&a), 0, "{:?}", lines(&a));
}

// Once per turn, however many failures it has; the next turn may say it again.
#[test]
fn the_hint_comes_at_most_once_per_turn() {
    let (mut a, turn) = sent_after(toggle_off);
    result(&mut a, turn, 1, ToolStatus::Error);
    result(&mut a, turn, 2, ToolStatus::Error);
    fail(&mut a, turn);
    assert_eq!(hints(&a), 1, "{:?}", lines(&a));
    a.update(key('q'));
    let next = super::run_pane::enter_turn(&mut a);
    fail(&mut a, next);
    assert_eq!(hints(&a), 2, "{:?}", lines(&a));
}
