//! Tests of the `Hugging Face` row of the Models tab (config-menu, Hugging Face
//! account row), at the `MenuState` level.

use super::account::{
    HINT_CONNECT, HINT_TOKEN, HINT_TOKEN_COPY, TOKEN_HINT, TOKEN_TITLE, TOKEN_URL, TOKEN_URL_HINT,
};
use super::ai_tests::{machine, press, value_of};
use super::footer_tests::hint;
use super::*;
use crate::core::ai::hf::{REJECTED, Secret};

fn open_with(facts: TokenFacts, name: Option<&str>) -> MenuState {
    MenuState::open(&Config::default())
        .with_ai(AiConfig::default(), Some(machine()))
        .with_hf(facts, name.map(str::to_owned))
}

fn on_row(facts: TokenFacts, name: Option<&str>) -> MenuState {
    let mut m = open_with(facts, name);
    m.goto("Hugging Face");
    m
}

pub(super) const SAVED: TokenFacts = TokenFacts {
    env: false,
    saved: true,
};

fn send(menu: &mut MenuState, code: KeyCode) -> MenuCommand {
    menu.on_key(
        &KeyEvent::new(code, KeyModifiers::NONE),
        false,
        &mut Config::default(),
    )
}

fn row_value(menu: &MenuState) -> String {
    value_of(menu, "Hugging Face")
}

/// The menu with the token field open and `hf_abc` pasted.
fn pasted() -> MenuState {
    let mut m = on_row(TokenFacts::default(), None);
    assert_eq!(send(&mut m, KeyCode::Enter), MenuCommand::None);
    m.on_paste("hf_abc\nignored");
    m
}

// Spec (config-menu): Account row. The row is the last of the Models tab,
// after the task rows, and reads `not connected` with no token.
#[test]
fn the_account_row_is_last_and_reads_not_connected_without_a_token() {
    let items = MODELS_SECTION.items;
    let last = items.last().unwrap();
    assert_eq!(last.label, "Hugging Face");
    assert!(matches!(last.kind, ItemKind::HfAccount));
    assert_eq!(items[items.len() - 2].label, "orchestrate");
    let m = open_with(TokenFacts::default(), None);
    assert_eq!(row_value(&m), "not connected");
    let m = on_row(TokenFacts::default(), None);
    assert_eq!(hint(&m), HINT_CONNECT);
}

#[test]
fn the_row_value_follows_the_tokens_and_the_known_account() {
    assert_eq!(row_value(&open_with(SAVED, Some("ana"))), "ana");
    assert_eq!(row_value(&open_with(SAVED, None)), "token saved");
    // A name from earlier in the session whose token is gone is not shown.
    assert_eq!(
        row_value(&open_with(TokenFacts::default(), Some("ana"))),
        "not connected"
    );
    let env = TokenFacts {
        env: true,
        saved: true,
    };
    assert_eq!(
        row_value(&open_with(env, Some("ana"))),
        "token set (HF_TOKEN)"
    );
    // A name from the Hub is drawn like the other Hub labels.
    assert_eq!(row_value(&open_with(SAVED, Some("a\u{1b}b"))), "a?b");
}

// Spec (config-menu): The field hides the token.
#[test]
fn enter_opens_a_field_that_draws_one_dot_per_character() {
    let m = pasted();
    assert!(m.in_sub_level(), "the tab keys are inert in the field");
    let rows = m.rows(&Config::default());
    assert_eq!(
        rows,
        [
            Row::Title(Cow::Borrowed(TOKEN_TITLE)),
            Row::Field("••••••".to_owned()),
            Row::Text(Cow::Borrowed(TOKEN_HINT)),
            Row::Text(Cow::Borrowed(TOKEN_URL_HINT)),
            Row::Text(Cow::Borrowed(TOKEN_URL)),
        ]
    );
    assert_eq!(TOKEN_HINT, "paste a Hugging Face token");
    assert_eq!(TOKEN_URL_HINT, "create a read token at:");
    assert_eq!(
        TOKEN_URL,
        "https://huggingface.co/settings/tokens/new?tokenType=read"
    );
    assert_eq!(hint(&m), HINT_TOKEN_COPY, "no opener in this menu");
    assert!(!format!("{m:?}").contains("hf_abc"), "{m:?}");
}

// Spec (config-menu; open-links S1, S3): Ctrl+O in the field opens the link
// to create a token, and the field stays open for the paste; with no opener
// on the host it copies the link, and the footer says which.
#[test]
fn ctrl_o_opens_the_token_link_or_copies_it_without_an_opener() {
    let ctrl_o = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
    for (opener, command, footer) in [
        (true, MenuCommand::Open(TOKEN_URL.to_owned()), HINT_TOKEN),
        (
            false,
            MenuCommand::Copy(TOKEN_URL.to_owned()),
            HINT_TOKEN_COPY,
        ),
    ] {
        let mut m = on_row(TokenFacts::default(), None).with_opener(opener);
        send(&mut m, KeyCode::Enter);
        assert_eq!(hint(&m), footer);
        assert_eq!(m.on_key(&ctrl_o, false, &mut Config::default()), command);
        assert!(
            m.rows(&Config::default())
                .contains(&Row::Field(String::new()))
        );
        assert_eq!(
            m.on_key(&ctrl_o, true, &mut Config::default()),
            MenuCommand::None
        );
    }
    assert_eq!(
        HINT_TOKEN,
        "Enter connect · Ctrl+O open link · Ctrl+U clear · Esc back"
    );
    assert_eq!(
        HINT_TOKEN_COPY,
        "Enter connect · Ctrl+O copy link · Ctrl+U clear · Esc back"
    );
}

// A terminal that sends ^H for Backspace delivers Ctrl+H.
#[test]
fn ctrl_h_deletes_like_backspace_in_the_token_field() {
    let mut m = on_row(TokenFacts::default(), None);
    send(&mut m, KeyCode::Enter);
    press(&mut m, 'h');
    press(&mut m, 'f');
    m.on_key(
        &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
        false,
        &mut Config::default(),
    );
    assert!(
        m.rows(&Config::default())
            .contains(&Row::Field("•".to_owned()))
    );
}

#[test]
fn the_field_takes_typing_backspace_and_ctrl_u() {
    let mut m = on_row(TokenFacts::default(), None);
    send(&mut m, KeyCode::Enter);
    for c in ['h', 'f', 'j', 's'] {
        assert_eq!(press(&mut m, c), MenuCommand::None, "typed, not a shortcut");
    }
    send(&mut m, KeyCode::Backspace);
    assert!(
        m.rows(&Config::default())
            .contains(&Row::Field("•••".to_owned()))
    );
    m.on_key(
        &KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        false,
        &mut Config::default(),
    );
    assert!(
        m.rows(&Config::default())
            .contains(&Row::Field(String::new()))
    );
    // Empty text checks nothing.
    assert_eq!(send(&mut m, KeyCode::Enter), MenuCommand::None);
    // Esc goes back to the row, not out of the menu.
    assert_eq!(send(&mut m, KeyCode::Esc), MenuCommand::None);
    assert!(!m.in_sub_level());
    assert_eq!(row_value(&m), "not connected");
}

// Spec (model-download): Connect, the menu side.
#[test]
fn enter_in_the_field_checks_the_token_and_an_accepted_one_shows_the_account() {
    let mut m = pasted();
    assert_eq!(
        send(&mut m, KeyCode::Enter),
        MenuCommand::HfAccount(AccountAsk::Connect(Secret::new("hf_abc".to_owned())))
    );
    assert_eq!(hint(&m), "asking Hugging Face…");
    // Keys other than Esc wait for the reply.
    press(&mut m, 'x');
    assert_eq!(send(&mut m, KeyCode::Enter), MenuCommand::None);
    assert!(
        m.rows(&Config::default())
            .contains(&Row::Field("••••••".to_owned()))
    );
    m.account_reply(AccountReply::Connected(Ok("ana".to_owned())));
    assert!(!m.in_sub_level(), "the field closed");
    assert_eq!(row_value(&m), "ana");
    assert!(!format!("{m:?}").contains("hf_abc"), "the token is dropped");
    assert!(m.edits(&Config::default()).is_empty(), "no config edit");
}

// Spec (model-download): Rejected token, the menu side.
#[test]
fn a_rejected_token_reads_as_the_footer_error_and_is_dropped() {
    let mut m = pasted();
    send(&mut m, KeyCode::Enter);
    m.account_reply(AccountReply::Connected(Err(REJECTED.to_owned())));
    assert_eq!(
        m.footer(),
        Footer::Error("Hugging Face did not accept the token")
    );
    assert!(
        m.rows(&Config::default())
            .contains(&Row::Field(String::new()))
    );
    assert!(!format!("{m:?}").contains("hf_abc"));
    send(&mut m, KeyCode::Esc);
    assert_eq!(row_value(&m), "not connected");
}

#[test]
fn a_reply_after_esc_updates_the_account_but_shows_no_error() {
    let mut m = pasted();
    send(&mut m, KeyCode::Enter);
    // Esc during the check stops it, so the token is not saved.
    assert_eq!(
        send(&mut m, KeyCode::Esc),
        MenuCommand::HfAccount(AccountAsk::CancelConnect)
    );
    assert!(!m.in_sub_level());
    m.account_reply(AccountReply::Connected(Err(REJECTED.to_owned())));
    assert_eq!(hint(&m), HINT_CONNECT);
    m.account_reply(AccountReply::Connected(Ok("ana".to_owned())));
    assert_eq!(row_value(&m), "ana", "the file was saved all the same");
}

// Spec (model-download): Disconnect, the menu side.
#[test]
fn a_known_account_offers_to_disconnect_and_only_y_does() {
    let mut m = on_row(SAVED, Some("ana"));
    assert_eq!(send(&mut m, KeyCode::Enter), MenuCommand::None);
    assert_eq!(hint(&m), "disconnect ana? (y/n)");
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_ne!(hint(&m), "disconnect ana? (y/n)");
    assert_eq!(row_value(&m), "ana", "n keeps the token");
    send(&mut m, KeyCode::Enter);
    assert_eq!(
        press(&mut m, 'y'),
        MenuCommand::HfAccount(AccountAsk::Disconnect)
    );
    assert_eq!(hint(&m), "asking Hugging Face…");
    m.account_reply(AccountReply::Disconnected(Ok(())));
    assert_eq!(row_value(&m), "not connected");
    assert!(m.edits(&Config::default()).is_empty(), "no config edit");
}

// A saved token can be disconnected without a check that works (the Hub
// is offline, or the token no longer works): `d` asks first.
#[test]
fn d_disconnects_a_saved_token_whose_account_is_unknown() {
    let mut m = on_row(SAVED, None);
    assert_eq!(
        hint(&m),
        "Enter check · d disconnect · j/k move · s save · Esc cancel"
    );
    assert_eq!(press(&mut m, 'd'), MenuCommand::None);
    assert_eq!(hint(&m), "disconnect the saved token? (y/n)");
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_eq!(row_value(&m), "token saved", "n keeps the token");
    press(&mut m, 'd');
    assert_eq!(
        press(&mut m, 'y'),
        MenuCommand::HfAccount(AccountAsk::Disconnect)
    );
    m.account_reply(AccountReply::Disconnected(Ok(())));
    assert_eq!(row_value(&m), "not connected");
    // With no saved token, `d` does nothing.
    assert_eq!(press(&mut m, 'd'), MenuCommand::None);
    assert_eq!(hint(&m), HINT_CONNECT);
    // A known account: `d` asks as Enter does.
    let mut known = on_row(SAVED, Some("ana"));
    press(&mut known, 'd');
    assert_eq!(hint(&known), "disconnect ana? (y/n)");
}

// Spec (config-menu): the account of a saved token comes from a whoami
// check made when the row is opened.
#[test]
fn enter_on_a_saved_token_checks_whose_it_is() {
    let mut m = on_row(SAVED, None);
    assert_eq!(
        send(&mut m, KeyCode::Enter),
        MenuCommand::HfAccount(AccountAsk::Check)
    );
    assert_eq!(hint(&m), "asking Hugging Face…");
    // A second Enter while it runs asks nothing.
    assert_eq!(send(&mut m, KeyCode::Enter), MenuCommand::None);
    m.account_reply(AccountReply::Checked(Ok("ana".to_owned())));
    assert_eq!(row_value(&m), "ana");
}

#[test]
fn a_saved_token_the_hub_refuses_opens_the_field_for_another() {
    let mut m = on_row(SAVED, None);
    send(&mut m, KeyCode::Enter);
    m.account_reply(AccountReply::Checked(Err(REJECTED.to_owned())));
    assert_eq!(m.footer(), Footer::Error(REJECTED));
    assert!(
        m.rows(&Config::default())
            .contains(&Row::Field(String::new()))
    );
}

// ---- A refused saved token that arrives while another view is open ----------

/// The row asked to check the saved token and the user moved on to another view
/// before the Hub answered.
fn checking_then(open_view: impl FnOnce(&mut MenuState)) -> MenuState {
    let mut m = on_row(SAVED, None);
    assert_eq!(
        send(&mut m, KeyCode::Enter),
        MenuCommand::HfAccount(AccountAsk::Check)
    );
    open_view(&mut m);
    assert!(m.in_sub_level());
    m.account_reply(AccountReply::Checked(Err(REJECTED.to_owned())));
    m
}

#[test]
fn a_refused_token_waits_for_the_picker_to_close_and_says_so() {
    let mut m = checking_then(|m| {
        m.goto("code");
        send(m, KeyCode::Enter);
    });
    assert_eq!(m.sub_view(), Some(SubView::Picker), "the picker stays");
    assert!(m.account.field().is_none(), "the field did not open");
    assert_eq!(
        m.footer(),
        Footer::Error("Hugging Face did not accept the token")
    );
    while m.sub_view() == Some(SubView::Picker) {
        send(&mut m, KeyCode::Esc);
    }
    assert_eq!(m.sub_view(), Some(SubView::AccountField), "now it opens");
    assert_eq!(
        m.footer(),
        Footer::Hint("Hugging Face token refused".into())
    );
    assert!(
        m.rows(&Config::default())
            .contains(&Row::Field(String::new()))
    );
}

#[test]
fn a_refused_token_never_takes_the_paste_from_the_feedback_form() {
    let mut m = checking_then(|m| {
        m.goto("Feedback");
        send(m, KeyCode::Enter);
    });
    assert_eq!(m.sub_view(), Some(SubView::Feedback));
    send(&mut m, KeyCode::Tab);
    m.on_paste("zzz-note");
    assert!(format!("{:?}", m.rows(&Config::default())).contains("zzz-note"));
    assert!(m.account.field().is_none());
    send(&mut m, KeyCode::Esc);
    assert_eq!(m.sub_view(), Some(SubView::AccountField));
}

#[test]
fn a_refused_token_with_nothing_open_opens_the_field_at_once() {
    let mut m = on_row(SAVED, None);
    send(&mut m, KeyCode::Enter);
    m.account_reply(AccountReply::Checked(Err(REJECTED.to_owned())));
    assert_eq!(m.sub_view(), Some(SubView::AccountField));
}
