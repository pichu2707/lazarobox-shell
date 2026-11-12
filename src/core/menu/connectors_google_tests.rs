//! Link with Google for `google` (mcp-connect, Link with Google, S6b2) at the
//! `MenuState` level: the preconditions, the prompt, the `y`, the wait and
//! its end. The session itself is the runtime's (`google_link`).

use super::ai_tests::press;
use super::connectors_tests::{entry, presence, texts, whole};
use super::footer_tests::hint;
use super::picker_tests::{key, send};
use super::*;
use crate::core::ai::mcp_catalog::link::{Ask, Step};

const CONFIG: &str = "[[ai.mcp.servers]]\nname = \"google\"\ncommand = \"workspace-mcp\"\n\
    env = [\"GOOGLE_OAUTH_CLIENT_ID\", \"GOOGLE_OAUTH_CLIENT_SECRET\", \"USER_GOOGLE_EMAIL\", \"WORKSPACE_MCP_DISABLE_LOCAL_FILES\"]\n";
const VALUES: &str = "[google]\nGOOGLE_OAUTH_CLIENT_ID = \"the-id.apps.googleusercontent.com\"\n\
    GOOGLE_OAUTH_CLIENT_SECRET = \"the-secret\"\nUSER_GOOGLE_EMAIL = \"me@x.org\"\n";
const URL: &str = "https://accounts.google.com/o/oauth2/auth?client_id=the-id.apps.googleusercontent.com&redirect_uri=http%3A%2F%2Flocalhost%3A8000%2F";
const CREDENTIALS: &str = "/h/.google_workspace_mcp/credentials/me@x.org.json";

fn ask() -> Ask {
    Ask {
        client_id: "the-id.apps.googleusercontent.com".to_owned(),
        email: "me@x.org".to_owned(),
    }
}

/// The `google` entry screen with this disk copy.
fn google(config: &str, secrets: &str, linked: bool) -> MenuState {
    let mut m = entry(Vec::new(), 3);
    let stat = move |p: &std::path::Path| linked && p == std::path::Path::new(CREDENTIALS);
    m.presence_reply(
        presence(config, secrets)
            .with_home(Some("/h".to_owned()))
            .with_credentials(stat),
    );
    m
}

fn ready() -> MenuState {
    google(CONFIG, VALUES, false)
}

/// The prompt up.
fn prompted() -> MenuState {
    let mut m = ready();
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    m
}

/// The session started: `g`, then `y`.
fn started() -> (MenuState, MenuCommand) {
    let mut m = prompted();
    let command = press(&mut m, 'y');
    (m, command)
}

fn state_of(m: &MenuState) -> String {
    texts(m)
        .into_iter()
        .find(|t| t.starts_with("State:"))
        .unwrap()
}

// Spec (mcp-connect, Entry list): `not linked` until the credentials file
// exists, by `stat` alone.
#[test]
fn google_shows_linked_by_the_credentials_files_existence() {
    assert_eq!(
        state_of(&google(CONFIG, VALUES, false)),
        "State: declared · not linked"
    );
    assert_eq!(
        state_of(&google(CONFIG, VALUES, true)),
        "State: declared · linked"
    );
    // Without its values the state is `values missing`, as before.
    assert!(state_of(&google(CONFIG, "", true)).contains("values missing"));
}

// Spec: the prompt for `google`.
#[test]
fn g_asks_before_starting_anything_and_names_the_account() {
    let m = prompted();
    assert_eq!(
        texts(&m)[0],
        "start workspace-mcp and ask Google to link me@x.org? (y/n)"
    );
    assert!(hint(&m).contains("y yes"), "{}", hint(&m));
}

#[test]
fn y_starts_the_session_for_the_declared_server() {
    let (m, command) = started();
    let MenuCommand::LinkGoogle { server, ask: got } = command else {
        panic!("the session starts: {command:?}");
    };
    assert_eq!((server.name.as_str(), got), ("google", ask()));
    assert_eq!(hint(&m), "starting workspace-mcp… · Esc cancels");
}

// Spec: Nothing without y.
#[test]
fn n_esc_ctrl_y_or_any_other_key_starts_no_server() {
    for code in [
        KeyCode::Char('n'),
        KeyCode::Esc,
        KeyCode::Char('x'),
        KeyCode::Enter,
    ] {
        let mut m = prompted();
        assert_eq!(send(&mut m, key(code)), MenuCommand::None, "{code:?}");
        assert!(
            texts(&m)[0].starts_with("State:") || texts(&m)[0].contains("Google"),
            "{code:?}"
        );
        assert!(!hint(&m).contains("Esc cancels"), "{code:?}");
    }
    let mut m = prompted();
    let ctrl_y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(
        m.on_key(&ctrl_y, false, &mut Config::default()),
        MenuCommand::None
    );
}

// Spec: No client id yet.
#[test]
fn a_missing_value_names_it_and_points_at_f() {
    let no_id = VALUES.replace("GOOGLE_OAUTH_CLIENT_ID", "OTHER");
    let mut m = google(CONFIG, &no_id, false);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(hint(&m), "fill GOOGLE_OAUTH_CLIENT_ID first (f)");
    assert!(!texts(&m).concat().contains("link me"));
}

#[test]
fn an_undeclared_a_disabled_or_a_partial_entry_starts_nothing() {
    let mut m = google("", VALUES, false);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert!(hint(&m).starts_with("not declared"), "{}", hint(&m));
    let off = CONFIG.replace("command", "enabled = false\ncommand");
    let mut m = google(&off, VALUES, false);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(hint(&m), "disabled in config.toml");
    let partial = CONFIG.replace("\"USER_GOOGLE_EMAIL\", ", "");
    let mut m = google(&partial, VALUES, false);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert!(
        hint(&m).contains("does not pass USER_GOOGLE_EMAIL"),
        "{}",
        hint(&m)
    );
}

#[test]
fn the_entry_lists_the_link_action_and_its_key() {
    let mut m = ready();
    assert!(whole(&mut m).concat().contains("g link with Google"));
    let m = ready();
    assert!(hint(&m).contains("g link"), "{}", hint(&m));
    // Other entries keep their rows.
    let mut m = entry(Vec::new(), 4);
    assert!(!whole(&mut m).concat().contains("g link with Google"));
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(hint(&m), "no such key here");
}

// Spec: google link succeeds. The link is opened (or copied) and shown.
#[test]
fn the_link_is_opened_and_shown_while_waiting() {
    let (mut m, _) = started();
    let command = m.link_step(Step::Waiting(URL.to_owned()));
    assert_eq!(command, MenuCommand::Copy(URL.to_owned()));
    assert_eq!(hint(&m), "waiting for Google… · Esc cancels");
    let shown: String = texts(&m).concat().replace(' ', "");
    assert!(shown.contains(URL), "{shown}");
    // With an opener the browser gets it.
    let (m, _) = started();
    let mut m = m.with_opener(true);
    assert_eq!(
        m.link_step(Step::Waiting(URL.to_owned())),
        MenuCommand::Open(URL.to_owned())
    );
}

#[test]
fn success_says_linked_and_reads_the_files_again() {
    let (mut m, _) = started();
    m.link_step(Step::Waiting(URL.to_owned()));
    let command = m.link_step(Step::Ended(Ok("linked google".to_owned())));
    assert_eq!(command, MenuCommand::LoadPresence);
    assert_eq!(hint(&m), "linked google");
    assert!(!texts(&m).concat().contains("waiting"));
}

// Spec: the ending notices are shown as the session says them.
#[test]
fn a_failure_says_why_and_ends_the_wait() {
    for why in [
        "link failed: ports 8000-8004 are in use",
        "link cancelled",
        "link timed out after 5 min",
    ] {
        let (mut m, _) = started();
        let command = m.link_step(Step::Ended(Err(why.to_owned())));
        assert_eq!(command, MenuCommand::None);
        assert_eq!(hint(&m), why);
        // The keys work again.
        assert_eq!(press(&mut m, 'g'), MenuCommand::None);
        assert!(texts(&m)[0].contains("link me@x.org"), "{why}");
    }
}

// Spec: google link cancelled. Esc is the only way out of the wait.
#[test]
fn esc_cancels_the_session_and_other_keys_wait() {
    let (mut m, _) = started();
    m.link_step(Step::Waiting(URL.to_owned()));
    for c in ['j', 'g', 'p', 'i', 'f', 'q'] {
        assert_eq!(press(&mut m, c), MenuCommand::None, "{c}");
        assert_eq!(hint(&m), "waiting for Google… · Esc cancels", "{c}");
    }
    assert_eq!(send(&mut m, key(KeyCode::Esc)), MenuCommand::CancelProbe);
}

#[test]
fn a_probe_and_a_link_never_run_together() {
    let (mut m, _) = started();
    assert_eq!(press(&mut m, 'p'), MenuCommand::None);
    assert_eq!(hint(&m), "starting workspace-mcp… · Esc cancels");
    let mut m = ready();
    press(&mut m, 'p');
    assert!(matches!(press(&mut m, 'y'), MenuCommand::Probe(_)));
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(hint(&m), "a probe is already running");
}

#[test]
fn a_step_nobody_asked_for_changes_nothing() {
    let mut m = ready();
    assert_eq!(
        m.link_step(Step::Waiting(URL.to_owned())),
        MenuCommand::None
    );
    assert_eq!(
        m.link_step(Step::Ended(Ok("linked google".to_owned()))),
        MenuCommand::None
    );
    assert!(!hint(&m).contains("linked google"));
}

// Spec: Token files never opened: the state reads only the account's names.
#[test]
fn the_state_asks_only_whether_the_accounts_files_exist() {
    let seen = std::cell::RefCell::new(Vec::new());
    let stat = |p: &std::path::Path| {
        seen.borrow_mut().push(p.to_owned());
        false
    };
    let secrets = VALUES.replace("me@x.org", "me+a@x.org");
    presence(CONFIG, &secrets)
        .with_home(Some("/h".to_owned()))
        .with_credentials(stat);
    let base = std::path::Path::new("/h/.google_workspace_mcp/credentials");
    assert_eq!(
        *seen.borrow(),
        [base.join("me%2Ba@x.org.json"), base.join("me_a@x.org.json")]
    );
    // No account, no home or a relative home: nothing is looked at.
    seen.borrow_mut().clear();
    presence(CONFIG, "")
        .with_home(Some("/h".to_owned()))
        .with_credentials(stat);
    presence(CONFIG, VALUES).with_credentials(stat);
    presence(CONFIG, VALUES)
        .with_home(Some("h".to_owned()))
        .with_credentials(stat);
    assert!(seen.borrow().is_empty());
}
