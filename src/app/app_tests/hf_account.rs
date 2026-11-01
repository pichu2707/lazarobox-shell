//! The `Hugging Face` row of the Models tab (config-menu, Hugging Face account
//! row; model-download, Connect a Hugging Face account): the requests go out
//! as effects, the answers come back as events, and the account is known for
//! the rest of the session.

use super::*;
use crate::core::ai::hf::{AccountAsk, AccountReply, REJECTED, Secret, TokenFacts};
use crate::core::menu::Footer;

fn enter() -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

fn no_tokens() -> TokenFacts {
    TokenFacts::default()
}

fn saved() -> TokenFacts {
    TokenFacts {
        env: false,
        saved: true,
    }
}

/// The menu opened the way a user does, on the `Hugging Face` row.
fn on_row(facts: fn() -> TokenFacts) -> App {
    let mut a = app().with_hf_tokens(facts);
    a.update(ctrl_space());
    a.update(key('m'));
    assert!(a.menu().is_some());
    goto_row(&mut a, "Hugging Face");
    a
}

fn row_value(a: &App) -> String {
    a.menu()
        .unwrap()
        .rows(&a.config)
        .into_iter()
        .find_map(|row| match row {
            Row::Item {
                label: "Hugging Face",
                value,
                ..
            } => value.map(|v| v.into_owned()),
            _ => None,
        })
        .expect("the Hugging Face row")
}

// Spec: Connect (the app side): the pasted token leaves once, as the
// effect, and the accepted account is shown.
#[test]
fn a_pasted_token_is_checked_and_the_account_shown() {
    let mut a = on_row(no_tokens);
    assert_eq!(row_value(&a), "not connected");
    assert_eq!(a.update(enter()), vec![]);
    assert_eq!(a.update(AppEvent::Paste("hf_abc".into())), vec![]);
    assert_eq!(
        a.update(enter()),
        vec![Effect::HfAccount(AccountAsk::Connect(Secret::new(
            "hf_abc".into()
        )))]
    );
    let effects = a.update(AppEvent::HfAccount(AccountReply::Connected(Ok(
        "ana".into()
    ))));
    assert_eq!(effects, vec![]);
    assert_eq!(row_value(&a), "ana");
    assert!(!format!("{:?}", a.menu()).contains("hf_abc"));
    // Known for the session: a menu opened again (the file now saved)
    // shows it.
    a.update(esc());
    assert!(a.menu().is_none());
    a.hf_tokens = saved;
    a.update(ctrl_space());
    a.update(key('m'));
    goto_row(&mut a, "Hugging Face");
    assert_eq!(row_value(&a), "ana");
}

// Spec: Rejected token (the app side).
#[test]
fn a_rejected_token_reads_as_the_footer_error() {
    let mut a = on_row(no_tokens);
    a.update(enter());
    a.update(AppEvent::Paste("hf_bad".into()));
    a.update(enter());
    a.update(AppEvent::HfAccount(AccountReply::Connected(Err(
        REJECTED.into()
    ))));
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Error("Hugging Face did not accept the token")
    );
}

// Spec: Disconnect (the app side): `y` asks the runtime to delete the
// file, `n` asks nothing.
#[test]
fn disconnect_asks_first() {
    let mut a = on_row(saved);
    assert_eq!(
        a.update(enter()),
        vec![Effect::HfAccount(AccountAsk::Check)]
    );
    a.update(AppEvent::HfAccount(AccountReply::Checked(Ok("ana".into()))));
    assert_eq!(row_value(&a), "ana");
    assert_eq!(a.update(enter()), vec![]);
    assert_eq!(a.update(key('n')), vec![]);
    assert_eq!(row_value(&a), "ana");
    a.update(enter());
    assert_eq!(
        a.update(key('y')),
        vec![Effect::HfAccount(AccountAsk::Disconnect)]
    );
    a.update(AppEvent::HfAccount(AccountReply::Disconnected(Ok(()))));
    assert_eq!(row_value(&a), "not connected");
    assert_eq!(a.hf_account, None, "forgotten for the session");
}

#[test]
fn hf_token_in_the_environment_says_so() {
    let a = on_row(|| TokenFacts {
        env: true,
        saved: false,
    });
    assert_eq!(row_value(&a), "token set (HF_TOKEN)");
}

#[test]
fn a_reply_with_the_menu_closed_still_keeps_the_account() {
    let mut a = app().with_hf_tokens(saved);
    assert_eq!(
        a.update(AppEvent::HfAccount(AccountReply::Checked(Ok("ana".into())))),
        vec![]
    );
    assert_eq!(a.hf_account.as_deref(), Some("ana"));
}

// Spec (open-links S1, S3): Ctrl+O in the token field opens the link to
// create a token through the host's opener and says so; with no opener the
// link is copied instead.
#[test]
fn ctrl_o_in_the_token_field_opens_the_link_in_the_browser() {
    let ctrl_o = AppEvent::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    let url = "https://huggingface.co/settings/tokens/new?tokenType=read";

    let mut a = app().with_opener(true).with_hf_tokens(no_tokens);
    a.update(ctrl_space());
    a.update(key('m'));
    goto_row(&mut a, "Hugging Face");
    a.update(enter());
    assert_eq!(
        a.update(ctrl_o.clone()),
        vec![Effect::OpenUrl(url.to_owned())]
    );
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Hint("Opened in your browser".into())
    );

    let mut a = on_row(no_tokens);
    a.update(enter());
    let effects = a.update(ctrl_o);
    assert!(
        matches!(effects.as_slice(), [Effect::SetClipboard(_)]),
        "{effects:?}"
    );
}

// The footer says the link is being copied, then keeps the browser context
// in the tool's result.
#[test]
fn a_failed_open_with_a_tool_keeps_the_browser_context_in_the_result() {
    let url = "https://huggingface.co/settings/tokens/new?tokenType=read";
    let mut a = app()
        .with_opener(true)
        .with_hf_tokens(no_tokens)
        .with_clipboard(Some(crate::core::clipboard::Tool::WlCopy), false);
    a.update(ctrl_space());
    a.update(key('m'));
    goto_row(&mut a, "Hugging Face");
    a.update(enter());
    let effects = a.update(AppEvent::OpenFailed(url.to_owned()));
    assert_eq!(effects.len(), 2, "{effects:?}");
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Hint("The browser did not open: copying link".into())
    );
    a.update(AppEvent::Clipboard {
        seq: 1,
        result: Ok(()),
    });
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Hint("The browser did not open: Copied (wl-copy)".into())
    );
}

// Review finding (opener failure): an opener that fails is not reported as
// opened: the link is copied, the footer says why, and the next Ctrl+O
// copies at once.
#[test]
fn a_failed_open_copies_the_link_and_stops_opening() {
    let ctrl_o = AppEvent::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    let url = "https://huggingface.co/settings/tokens/new?tokenType=read";
    let mut a = app().with_opener(true).with_hf_tokens(no_tokens);
    a.update(ctrl_space());
    a.update(key('m'));
    goto_row(&mut a, "Hugging Face");
    a.update(enter());
    assert_eq!(
        a.update(ctrl_o.clone()),
        vec![Effect::OpenUrl(url.to_owned())]
    );
    let effects = a.update(AppEvent::OpenFailed(url.to_owned()));
    assert!(
        matches!(effects.as_slice(), [Effect::SetClipboard(_)]),
        "{effects:?}"
    );
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Hint("The browser did not open: link copied".into())
    );
    let_go();
    let effects = a.update(ctrl_o);
    assert!(
        matches!(effects.as_slice(), [Effect::SetClipboard(_)]),
        "{effects:?}"
    );
}
