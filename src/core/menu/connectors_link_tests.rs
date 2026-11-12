//! Link with Google for `gsc` (mcp-connect, Link with Google, S6b1) at the
//! `MenuState` level: the preconditions, the prompt and the `y`.

use super::ai_tests::press;
use super::connectors_tests::{entry, presence, texts, whole};
use super::footer_tests::hint;
use super::picker_tests::{key, send};
use super::*;
use crate::core::ai::mcp_catalog::gsc::Plan;

const CONFIG: &str = "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"mcp-search-console\"\n\
    env = [\"GSC_OAUTH_CLIENT_SECRETS_FILE\", \"GSC_CONFIG_DIR\", \"GSC_CREDENTIALS_PATH\", \"GSC_SKIP_OAUTH\"]\n";
const OAUTH: &str = "[gsc]\nGSC_OAUTH_CLIENT_SECRETS_FILE = \"/h/.config/lazarobox/google-oauth-client.json\"\n\
    GSC_CONFIG_DIR = \"/h/.config/mcp-gsc\"\nGSC_SKIP_OAUTH = \"false\"\n";
const LINE: &str = "env -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u GSC_ALLOW_DESTRUCTIVE -u GSC_SKIP_OAUTH GSC_CONFIG_DIR='/h/.config/mcp-gsc' GSC_OAUTH_CLIENT_SECRETS_FILE='/h/.config/lazarobox/google-oauth-client.json' '/h/.local/share/uv/tools/mcp-search-console/bin/python' -I -c 'import gsc_server as g; g.get_gsc_service_oauth(); print(\"Search Console linked\")'";

fn plan() -> Plan {
    Plan {
        dir: "/h/.config/mcp-gsc".to_owned(),
        file: "/h/.config/lazarobox/google-oauth-client.json".to_owned(),
    }
}

/// The `gsc` entry screen with this disk copy.
fn gsc(config: &str, secrets: &str) -> MenuState {
    let mut m = entry(Vec::new(), 2);
    m.presence_reply(presence(config, secrets));
    m
}

fn ready() -> MenuState {
    gsc(CONFIG, OAUTH)
}

/// The prompt up: `g`, then the runtime's good answer.
fn prompted() -> MenuState {
    let mut m = ready();
    assert_eq!(press(&mut m, 'g'), MenuCommand::LinkGsc(plan()));
    assert_eq!(m.link_reply(Ok(LINE.to_owned())), MenuCommand::None);
    m
}

// Spec: gsc link prompt shows the exact line.
#[test]
fn g_asks_the_runtime_to_check_and_the_prompt_shows_the_exact_line() {
    let m = prompted();
    let rows = texts(&m);
    assert!(rows[0].starts_with("link gsc"), "{rows:?}");
    let shown: String = rows.concat().replace(' ', "");
    assert!(shown.contains(&LINE.replace(' ', "")), "{shown}");
    assert!(hint(&m).contains("y yes"));
}

// Spec: gsc link types the line.
#[test]
fn y_types_that_line_in_a_tab_and_nothing_else_does() {
    let mut m = prompted();
    assert_eq!(press(&mut m, 'y'), MenuCommand::Connector(LINE.to_owned()));
    assert!(
        texts(&m)[0].starts_with("Search Console"),
        "no second prompt"
    );
}

// Spec: Nothing without y.
#[test]
fn n_esc_ctrl_y_or_any_other_key_types_nothing() {
    for code in [
        KeyCode::Char('n'),
        KeyCode::Esc,
        KeyCode::Char('x'),
        KeyCode::Enter,
    ] {
        let mut m = prompted();
        assert_eq!(send(&mut m, key(code)), MenuCommand::None, "{code:?}");
        assert!(texts(&m)[0].starts_with("Search Console"), "{code:?}");
    }
    let mut m = prompted();
    let ctrl_y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
    let cmd = m.on_key(&ctrl_y, false, &mut Config::default());
    assert_eq!(cmd, MenuCommand::None);
}

#[test]
fn a_prompt_too_narrow_to_show_the_line_whole_is_never_confirmed() {
    let mut m = prompted();
    m.set_term(20, 24);
    assert_eq!(press(&mut m, 'y'), MenuCommand::None);
    assert!(hint(&m).contains("widen the terminal"), "{}", hint(&m));
}

// Spec: gsc in the service-account way.
#[test]
fn the_service_account_way_offers_no_link_and_says_how_to_choose_the_oauth_way() {
    let mut m = gsc(CONFIG, OAUTH);
    press(&mut m, 'w');
    assert!(!whole(&mut m).concat().contains("g link with Google"));
    let mut m = gsc(CONFIG, OAUTH);
    press(&mut m, 'w');
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(hint(&m), "linking needs the OAuth way (w chooses it)");
}

#[test]
fn the_oauth_way_lists_the_link_action() {
    let mut m = ready();
    assert!(whole(&mut m).concat().contains("g link with Google"));
}

// Spec: gsc values the server would not get.
#[test]
fn a_declared_entry_that_does_not_pass_a_path_is_refused_with_the_name() {
    let s5 = "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"x\"\n\
              env = [\"GSC_CREDENTIALS_PATH\", \"GSC_SKIP_OAUTH\"]\n";
    let mut m = gsc(s5, OAUTH);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert!(hint(&m).contains("paste the current entry"), "{}", hint(&m));
    let no_dir = "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"x\"\n\
                  env = [\"GSC_OAUTH_CLIENT_SECRETS_FILE\", \"GSC_SKIP_OAUTH\"]\n";
    let mut m = gsc(no_dir, OAUTH);
    press(&mut m, 'g');
    assert_eq!(
        hint(&m),
        "the declared gsc entry does not pass GSC_CONFIG_DIR; paste the current entry"
    );
}

#[test]
fn unfilled_values_a_missing_entry_or_a_disabled_one_start_nothing() {
    let mut m = gsc(CONFIG, "");
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(hint(&m), "fill the OAuth values first (f)");
    let mut m = gsc("", OAUTH);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert!(hint(&m).starts_with("not declared"), "{}", hint(&m));
    let off = CONFIG.replace("command", "enabled = false\ncommand");
    let mut m = gsc(&off, OAUTH);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(hint(&m), "disabled in config.toml");
}

// Spec: gsc without uv or the pinned version, without the client file.
#[test]
fn a_refused_check_shows_its_notice_and_no_prompt() {
    let mut m = ready();
    press(&mut m, 'g');
    let why = "mcp-search-console 0.4.3; run: uv tool install --force mcp-search-console==0.4.2";
    assert_eq!(m.link_reply(Err(why.to_owned())), MenuCommand::None);
    assert_eq!(hint(&m), why);
    assert!(texts(&m)[0].starts_with("Search Console"));
}

#[test]
fn an_answer_after_the_area_was_left_prompts_nothing() {
    let mut m = ready();
    press(&mut m, 'g');
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(m.link_reply(Ok(LINE.to_owned())), MenuCommand::None);
    assert!(!texts(&m).concat().contains("link gsc"));
    // An answer nobody asked for is dropped too.
    let mut m = ready();
    assert_eq!(m.link_reply(Ok(LINE.to_owned())), MenuCommand::None);
    assert!(
        texts(&m)[0].starts_with("Search Console"),
        "{:?}",
        texts(&m)
    );
}

#[test]
fn g_on_another_entry_is_still_no_such_key() {
    let mut m = entry(Vec::new(), 1);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(hint(&m), "no such key here");
}

#[test]
fn a_clean_draft_does_not_name_unsaved_changes_in_the_prompt() {
    // The dirty case is the shared `run_rows` of install and login.
    let m = prompted();
    assert!(!texts(&m).concat().contains("Unsaved menu changes"));
}
