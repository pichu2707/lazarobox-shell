//! Connect Google (openspec change `google-connect`, S5) at the `MenuState`
//! level: the row, the screen, the plan, the `y`, and the next steps.

use super::ai_tests::press;
use super::connectors_tests::{area, declared, presence, texts, whole};
use super::footer_tests::hint;
use super::picker_tests::{key, send};
use super::*;
use crate::core::{
    ai::{
        config::McpServer,
        mcp_catalog::{
            connect::{Ask, CHECKLIST, Preview},
            entry, gsc, snippet,
        },
        mcp_secrets::SecretText,
    },
    config_append::{Connect, Connected},
};

const ID: &str = "123-abc.apps.googleusercontent.com";
const SECRET: &str = "GOCSPX-abc";

/// The catalog entries as Connect Google appends them.
fn both_declared() -> String {
    format!(
        "{}\n{}",
        snippet(entry("gsc").unwrap()),
        snippet(entry("google").unwrap())
    )
}

/// The values Connect Google writes for both.
const WRITTEN: &str = "[gsc]\nGSC_OAUTH_CLIENT_SECRETS_FILE = \"/h/.config/lazarobox/google-oauth-client.json\"\n\
    GSC_CONFIG_DIR = \"/h/.config/mcp-gsc\"\nGSC_SKIP_OAUTH = \"false\"\n\
    [google]\nGOOGLE_OAUTH_CLIENT_ID = \"123-abc.apps.googleusercontent.com\"\n\
    GOOGLE_OAUTH_CLIENT_SECRET = \"GOCSPX-abc\"\nUSER_GOOGLE_EMAIL = \"ana@example.com\"\n\
    WORKSPACE_MCP_DISABLE_LOCAL_FILES = \"true\"\n";

/// The Connect Google screen, `servers` loaded at startup, the client read.
fn connect(servers: Vec<McpServer>) -> MenuState {
    let mut m = area(servers);
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    m.google_client(Ok(Some(ID.to_owned())));
    m
}

/// The same with this disk copy.
fn connect_with(config: &str, secrets: &str, gsc_linked: bool, google_linked: bool) -> MenuState {
    let mut m = connect(Vec::new());
    m.presence_reply(
        presence(config, secrets)
            .with_home(Some("/h".to_owned()))
            .with_token(move |_| gsc_linked)
            .with_credentials(move |_| google_linked),
    );
    m
}

fn secret(text: &str) -> SecretText {
    let mut value = SecretText::new();
    value.push_str(text);
    value
}

/// What the runtime answers for both services, nothing declared.
fn preview(blocked: Option<&str>) -> Preview {
    let set = [
        (
            "gsc",
            "GSC_OAUTH_CLIENT_SECRETS_FILE",
            "/h/.config/lazarobox/google-oauth-client.json",
        ),
        ("gsc", "GSC_CONFIG_DIR", "/h/.config/mcp-gsc"),
        ("gsc", "GSC_SKIP_OAUTH", "false"),
        ("google", "GOOGLE_OAUTH_CLIENT_ID", ID),
        ("google", "GOOGLE_OAUTH_CLIENT_SECRET", SECRET),
        ("google", "WORKSPACE_MCP_DISABLE_LOCAL_FILES", "true"),
    ];
    Preview {
        write: Connect {
            set: set.iter().map(|(t, n, v)| (*t, *n, secret(v))).collect(),
            snippets: both_declared(),
            names: vec!["gsc", "google"],
            expected: Some([7; 32]),
        },
        kept: vec![("google", "USER_GOOGLE_EMAIL")],
        installs: vec![
            "uv tool install mcp-search-console==0.4.2",
            "uv tool install workspace-mcp==2.4.1",
        ],
        declared: Vec::new(),
        blocked: blocked.map(str::to_owned),
    }
}

/// The plan shown: `e`-mail typed, `n`, the runtime's answer.
fn planned(blocked: Option<&str>) -> MenuState {
    let mut m = connect(Vec::new());
    type_email(&mut m, "ana@example.com");
    assert!(matches!(press(&mut m, 'n'), MenuCommand::PlanGoogle(_)));
    m.google_plan(Ok(Box::new(preview(blocked))));
    m
}

fn type_email(m: &mut MenuState, email: &str) {
    assert_eq!(press(m, 'e'), MenuCommand::None);
    for c in email.chars() {
        press(m, c);
    }
    assert_eq!(send(m, key(KeyCode::Enter)), MenuCommand::None);
}

/// Every row of the screen from its top, whatever the scroll.
fn all(m: &mut MenuState) -> Vec<String> {
    for _ in 0..60 {
        press(m, 'k');
    }
    whole(m)
}

/// The same as one text without spaces: a wrapped line reads whole.
fn flat(m: &mut MenuState) -> String {
    all(m).concat().replace(' ', "")
}

fn has(m: &mut MenuState, line: &str) -> bool {
    flat(m).contains(&line.replace(' ', ""))
}

fn selected(m: &MenuState) -> Option<&'static str> {
    m.rows(&Config::default())
        .into_iter()
        .find_map(|row| match row {
            Row::Item {
                label,
                selected: true,
                ..
            } => Some(label),
            _ => None,
        })
}

// ---- the row -----------------------------------------------------------------

// Spec S5.1: a Connect Google row at the top of the list; Enter opens it.
#[test]
fn the_list_starts_with_the_connect_google_row_and_enter_opens_its_screen() {
    let mut m = area(Vec::new());
    assert_eq!(
        texts(&m)[0],
        "Connect Google: Search Console + Gmail, Docs, Sheets"
    );
    assert_eq!(selected(&m), Some("Connect Google"));
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    assert!(
        texts(&m)[0].starts_with("Connect Google ·"),
        "{:?}",
        texts(&m)
    );
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(
        selected(&m),
        Some("Connect Google"),
        "Esc goes back to its row"
    );
    press(&mut m, 'k');
    assert_eq!(
        selected(&m),
        Some("Apps Script"),
        "k wraps to the last entry"
    );
    press(&mut m, 'j');
    press(&mut m, 'j');
    assert_eq!(selected(&m), Some("Clarity"));
}

#[test]
fn the_row_says_when_both_services_are_declared() {
    let m = area(vec![declared("gsc", true), declared("google", false)]);
    assert_eq!(texts(&m)[0], "Connect Google: both declared");
}

// ---- the screen ----------------------------------------------------------------

// Spec S5.2-S5.5: checklist, services, client and e-mail.
#[test]
fn the_screen_shows_the_services_the_client_the_e_mail_and_the_checklist() {
    let mut m = connect(Vec::new());
    let all = all(&mut m);
    for line in [
        "  [x] Search Console",
        "  [x] Google Workspace: Gmail read + drafts, Docs, Sheets",
        "client: 123-abc.apps.googleusercontent.com",
        "e-mail: not set — press e",
        "  [1] Create a Google Cloud project",
        "  [9] The step-by-step guide (in Spanish)",
    ] {
        assert!(all.contains(&line.to_owned()), "{line}\n{all:#?}");
    }
    assert!(has(&mut m, "Error 403: access_denied"));
    assert!(has(&mut m, "so tokens do not expire after 7 days"));
    assert!(m.rows(&Config::default()).len() <= VIEW_BODY);
}

#[test]
fn the_client_line_follows_the_runtime_read() {
    let mut m = connect(Vec::new());
    m.google_client(Ok(None));
    assert!(all(&mut m).contains(&"client: missing — press o".to_owned()));
    let mut m = connect(Vec::new());
    m.google_client(Err("google-oauth-client.json: mode must be 0600".to_owned()));
    let all = all(&mut m).join("\n");
    assert!(all.contains("client: google-oauth-client.json: mode must be 0600"));
}

// Spec S5.2: the links open with 1-9, or are copied without a browser.
#[test]
fn the_digits_open_the_checklist_links_or_copy_them() {
    let mut m = connect(Vec::new()).with_opener(true);
    for (n, (_, link)) in CHECKLIST.iter().enumerate() {
        let digit = char::from(b'1' + n as u8);
        assert_eq!(press(&mut m, digit), MenuCommand::Open((*link).to_owned()));
    }
    let mut m = connect(Vec::new());
    assert_eq!(
        press(&mut m, '1'),
        MenuCommand::Copy(CHECKLIST[0].1.to_owned())
    );
}

// Spec S5.3: two checkboxes, both on; a declared one cannot be ticked.
#[test]
fn s_and_w_tick_the_services_and_a_declared_one_stays_alone() {
    let mut m = connect(Vec::new());
    press(&mut m, 's');
    assert!(texts(&m).contains(&"  [ ] Search Console".to_owned()));
    press(&mut m, 'w');
    assert!(
        texts(&m).contains(&"  [ ] Google Workspace: Gmail read + drafts, Docs, Sheets".to_owned())
    );
    assert_eq!(press(&mut m, 'n'), MenuCommand::None, "nothing chosen");
    assert_eq!(
        m.notice.as_deref(),
        Some("choose Search Console, Google Workspace or both")
    );
    press(&mut m, 's');
    assert!(texts(&m).contains(&"  [x] Search Console".to_owned()));

    let mut m = connect(vec![declared("gsc", true)]);
    assert!(texts(&m).contains(&"  [-] Search Console: already declared".to_owned()));
    press(&mut m, 's');
    assert!(texts(&m).contains(&"  [-] Search Console: already declared".to_owned()));
    assert!(m.notice.as_deref().unwrap().contains("already declared"));
}

// Spec S5.4: `o` opens the existing client form from this screen.
#[test]
fn o_opens_the_client_form_and_its_enter_saves_the_client() {
    let mut m = connect(Vec::new());
    assert_eq!(press(&mut m, 'o'), MenuCommand::None);
    assert!(hint(&m).contains("client id"), "{}", hint(&m));
    m.on_paste(ID);
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    m.on_paste(SECRET);
    let rows = texts(&m).concat();
    assert!(!rows.contains(SECRET), "{rows}");
    let command = send(&mut m, key(KeyCode::Enter));
    let MenuCommand::SaveClient {
        id,
        secret,
        replace,
    } = command
    else {
        panic!("{command:?}");
    };
    assert_eq!((id.as_str(), secret.expose(), replace), (ID, SECRET, false));
    assert!(
        texts(&m)[0].starts_with("Connect Google ·"),
        "back on the screen"
    );
}

#[test]
fn esc_closes_the_client_form_and_stays_on_the_screen() {
    let mut m = connect(Vec::new());
    press(&mut m, 'o');
    press(&mut m, 'x');
    send(&mut m, key(KeyCode::Esc));
    assert!(texts(&m)[0].starts_with("Connect Google ·"));
    assert!(hint(&m).contains("n plan"), "{}", hint(&m));
}

// Spec S5.5: the e-mail is one text field.
#[test]
fn e_edits_the_e_mail_enter_keeps_it_and_esc_cancels() {
    let mut m = connect(Vec::new());
    type_email(&mut m, "ana@example.com");
    assert!(all(&mut m).contains(&"e-mail: ana@example.com (e changes it)".to_owned()));
    press(&mut m, 'e');
    assert!(hint(&m).contains("Google e-mail"), "{}", hint(&m));
    for c in "zz".chars() {
        press(&mut m, c);
    }
    send(&mut m, key(KeyCode::Esc));
    assert!(all(&mut m).contains(&"e-mail: ana@example.com (e changes it)".to_owned()));
}

#[test]
fn the_e_mail_field_takes_a_paste_and_repeated_keys_and_refuses_a_bad_address() {
    let mut m = connect(Vec::new());
    press(&mut m, 'e');
    m.on_paste("an");
    m.on_key(&key(KeyCode::Char('n')), true, &mut Config::default());
    assert!(
        texts(&m).iter().any(|row| row.ends_with("ann")),
        "{:?}",
        texts(&m)
    );
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("type your Google e-mail address (name@domain, no spaces)")
    );
    assert!(hint(&m).contains("Google e-mail"), "the field stays open");
}

#[test]
fn an_e_mail_already_in_secrets_toml_is_kept_and_not_asked() {
    let mut m = connect_with(
        "",
        "[google]\nUSER_GOOGLE_EMAIL = \"me@x.org\"\n",
        false,
        false,
    );
    assert!(all(&mut m).contains(&"e-mail: kept in secrets.toml (me@x.org)".to_owned()));
    assert_eq!(
        press(&mut m, 'n'),
        MenuCommand::PlanGoogle(Ask {
            chosen: vec!["gsc", "google"],
            email: String::new(),
        })
    );
}

// ---- the plan --------------------------------------------------------------------

// Spec S5.6: a key builds the plan from fresh disk state (the runtime).
#[test]
fn n_or_enter_asks_the_runtime_for_the_plan_of_the_ticked_services() {
    let mut m = connect(Vec::new());
    type_email(&mut m, "ana@example.com");
    let ask = Ask {
        chosen: vec!["gsc", "google"],
        email: "ana@example.com".to_owned(),
    };
    assert_eq!(press(&mut m, 'n'), MenuCommand::PlanGoogle(ask.clone()));
    assert_eq!(hint(&m), "reading the files…");
    send(&mut m, key(KeyCode::Esc));
    let mut m = connect(Vec::new());
    type_email(&mut m, "ana@example.com");
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::PlanGoogle(ask)
    );
    let mut m = connect(vec![declared("google", true)]);
    assert_eq!(
        press(&mut m, 'n'),
        MenuCommand::PlanGoogle(Ask {
            chosen: vec!["gsc"],
            email: String::new()
        }),
        "a declared service is not asked for, nor its e-mail"
    );
}

#[test]
fn n_without_a_client_or_an_e_mail_says_what_is_missing_and_asks_nothing() {
    let mut m = connect(Vec::new());
    m.google_client(Ok(None));
    type_email(&mut m, "ana@example.com");
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("no client yet: press o to save your client id and secret")
    );
    let mut m = connect(Vec::new());
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("type your Google e-mail first (e)")
    );
    let mut m = connect(vec![declared("gsc", true), declared("google", true)]);
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("both services are declared: g links them")
    );
}

#[test]
fn a_refused_plan_is_the_footer_notice_and_the_screen_stays() {
    let mut m = connect(Vec::new());
    type_email(&mut m, "ana@example.com");
    press(&mut m, 'n');
    m.google_plan(Err("config.toml is a symlink; edit it by hand".to_owned()));
    assert_eq!(
        m.notice.as_deref(),
        Some("config.toml is a symlink; edit it by hand")
    );
    assert!(matches!(press(&mut m, 'n'), MenuCommand::PlanGoogle(_)));
}

// Spec S5.6: the plan names entries, values (secret masked), kept values,
// installs, and the config.toml line.
#[test]
fn the_plan_lists_what_will_be_written_and_never_a_secret() {
    let mut m = planned(None);
    assert_eq!(texts(&m)[0], "Connect Google · plan");
    let all = all(&mut m);
    for line in [
        "Append to config.toml",
        "  gsc (Search Console)",
        "  google (Google Workspace)",
        "  [gsc] GSC_CONFIG_DIR = /h/.config/mcp-gsc",
        "  [google] GOOGLE_OAUTH_CLIENT_SECRET = ••••••••",
        "  [google] USER_GOOGLE_EMAIL",
        "  uv tool install mcp-search-console==0.4.2",
    ] {
        assert!(all.contains(&line.to_owned()), "{line}\n{all:#?}");
    }
    assert!(has(
        &mut m,
        "config.toml gets these entries at its end; the old file is kept as config.toml.bak"
    ));
    assert!(!flat(&mut m).contains(SECRET));
    assert!(!format!("{m:?}").contains(SECRET));
    assert_eq!(
        hint(&m),
        "j/k scroll · y write · c copy the entries · Esc back"
    );
}

// Spec S5.7: `y` sends the write of the plan shown.
#[test]
fn y_sends_the_write_of_the_plan_and_only_once() {
    let mut m = planned(None);
    let command = press(&mut m, 'y');
    assert_eq!(
        command,
        MenuCommand::ConnectGoogle(Box::new(preview(None).write))
    );
    assert_eq!(hint(&m), "writing…");
    assert_eq!(press(&mut m, 'y'), MenuCommand::None, "no second write");
}

#[test]
fn y_before_a_plan_writes_nothing() {
    let mut m = connect(Vec::new());
    assert_eq!(press(&mut m, 'y'), MenuCommand::None);
    assert_eq!(m.notice.as_deref(), Some("n shows the plan first"));
}

#[test]
fn a_blocked_plan_says_why_and_y_saves_the_values_only() {
    let why = "config.toml: the entries would not parse after it (an inline ai.mcp.servers list?)";
    let mut m = planned(Some(why));
    assert_eq!(m.notice.as_deref(), Some(why));
    assert!(has(&mut m, "y saves the values only"));
    assert!(matches!(press(&mut m, 'y'), MenuCommand::ConnectGoogle(_)));
}

#[test]
fn esc_drops_the_plan_and_a_late_answer_shows_nothing() {
    let mut m = planned(None);
    send(&mut m, key(KeyCode::Esc));
    assert!(texts(&m)[0].starts_with("Connect Google ·"));
    assert_ne!(texts(&m)[0], "Connect Google · plan");
    assert_eq!(press(&mut m, 'y'), MenuCommand::None);
    let mut m = connect(Vec::new());
    type_email(&mut m, "ana@example.com");
    press(&mut m, 'n');
    send(&mut m, key(KeyCode::Esc));
    m.google_plan(Ok(Box::new(preview(None))));
    assert_ne!(
        texts(&m)[0],
        "Connect Google · plan",
        "a late answer is dropped"
    );
}

// Spec S5.7: the paste fallback.
#[test]
fn c_copies_the_entries_of_the_ticked_services_that_are_not_declared() {
    let mut m = connect(Vec::new());
    assert_eq!(press(&mut m, 'c'), MenuCommand::Copy(both_declared()));
    let mut m = connect(vec![declared("google", true)]);
    assert_eq!(
        press(&mut m, 'c'),
        MenuCommand::Copy(snippet(entry("gsc").unwrap()))
    );
    let mut m = planned(None);
    assert_eq!(press(&mut m, 'c'), MenuCommand::Copy(both_declared()));
}

// Spec S5.7: the answer of the write.
#[test]
fn done_reads_the_files_again_and_a_refusal_offers_the_paste() {
    let mut m = planned(None);
    press(&mut m, 'y');
    let done = Connected::Done(
        "connect: saved 6 values in secrets.toml and added gsc, google to config.toml".to_owned(),
    );
    assert_eq!(m.google_connected(done.clone()), MenuCommand::LoadPresence);
    assert_eq!(m.notice.as_deref(), Some(done.notice()));
    assert!(all(&mut m).contains(&format!("  {}", connectors::RESTART)));

    for answer in [
        Connected::ValuesOnly(
            "connect: saved 6 values, but gsc, google were not added: x; paste the entries by hand"
                .to_owned(),
        ),
        Connected::Refused("connect: nothing was written: x; paste the entries by hand".to_owned()),
    ] {
        let mut m = planned(None);
        press(&mut m, 'y');
        let reload = !matches!(answer, Connected::Refused(_));
        let command = m.google_connected(answer.clone());
        assert_eq!(command == MenuCommand::LoadPresence, reload, "{answer:?}");
        assert_eq!(m.notice.as_deref(), Some(answer.notice()));
        assert!(has(
            &mut m,
            "paste them at the end of config.toml (c copies them)"
        ));
        assert_eq!(press(&mut m, 'c'), MenuCommand::Copy(both_declared()));
    }
}

// ---- next steps ---------------------------------------------------------------------

// Spec S5.8: `i` runs the install lines in one new tab, after its prompt.
#[test]
fn i_asks_to_run_both_install_lines_in_one_tab_and_y_runs_them() {
    let mut m = connect_with(&both_declared(), WRITTEN, false, false);
    let line = "uv tool install mcp-search-console==0.4.2 && uv tool install workspace-mcp==2.4.1";
    assert!(has(&mut m, &format!("i install: {line}")));
    assert_eq!(press(&mut m, 'i'), MenuCommand::None);
    let shown = texts(&m).concat();
    assert!(shown.contains("install gsc, google? (y/n)"), "{shown}");
    assert!(shown.replace(' ', "").contains(&line.replace(' ', "")));
    assert_eq!(press(&mut m, 'y'), MenuCommand::Connector(line.to_owned()));
}

#[test]
fn i_with_nothing_declared_installs_nothing() {
    let mut m = connect(Vec::new());
    assert_eq!(press(&mut m, 'i'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("nothing to install yet: n plans the entries first")
    );
}

// Spec S5.8: `g` links the next unlinked service, Search Console first.
#[test]
fn g_links_search_console_first_then_workspace() {
    let mut m = connect_with(&both_declared(), WRITTEN, false, false);
    assert!(has(
        &mut m,
        "g link: Search Console not linked · Google Workspace not linked"
    ));
    let plan = gsc::Plan {
        dir: "/h/.config/mcp-gsc".to_owned(),
        file: "/h/.config/lazarobox/google-oauth-client.json".to_owned(),
    };
    assert_eq!(press(&mut m, 'g'), MenuCommand::LinkGsc(plan));
    assert_eq!(m.link_reply(Ok("the line".to_owned())), MenuCommand::None);
    assert!(
        texts(&m)[0].starts_with("link gsc"),
        "the prompt on this screen"
    );
    assert_eq!(
        press(&mut m, 'y'),
        MenuCommand::Connector("the line".to_owned())
    );

    let mut m = connect_with(&both_declared(), WRITTEN, true, false);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert!(
        texts(&m)
            .concat()
            .contains("ask Google to link ana@example.com"),
        "{:?}",
        texts(&m)
    );
    assert!(matches!(press(&mut m, 'y'), MenuCommand::LinkGoogle { .. }));
}

#[test]
fn g_with_nothing_to_link_says_why() {
    let mut m = connect(Vec::new());
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("nothing to link yet: n plans the entries first")
    );
    let mut m = connect_with(&both_declared(), WRITTEN, true, true);
    assert_eq!(press(&mut m, 'g'), MenuCommand::None);
    assert_eq!(
        m.notice.as_deref(),
        Some("every declared service is linked; restart lazarobox to use it in chats")
    );
}

// ---- Esc and the menu's life -----------------------------------------------------------

// Spec S5.9: Esc goes back level by level.
#[test]
fn esc_closes_the_field_then_the_plan_then_the_screen() {
    let mut m = planned(None);
    send(&mut m, key(KeyCode::Esc));
    assert!(hint(&m).contains("n plan"));
    press(&mut m, 'e');
    send(&mut m, key(KeyCode::Esc));
    assert!(hint(&m).contains("n plan"));
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(selected(&m), Some("Connect Google"));
    send(&mut m, key(KeyCode::Esc));
    assert!(!m.in_sub_level());
}

// Spec S5.9: a fresh menu recovers the screen; a write that answers later
// still reads the files again.
#[test]
fn an_answer_after_leaving_the_screen_is_still_said() {
    let mut m = planned(None);
    press(&mut m, 'y');
    send(&mut m, key(KeyCode::Esc));
    let done = Connected::Done("connect: added gsc, google to config.toml".to_owned());
    assert_eq!(m.google_connected(done), MenuCommand::LoadPresence);
    assert_eq!(
        m.notice.as_deref(),
        Some("connect: added gsc, google to config.toml")
    );
    let mut fresh = connect(Vec::new());
    assert_eq!(
        press(&mut fresh, 'y'),
        MenuCommand::None,
        "no write is pending"
    );
}

#[test]
fn the_screen_scrolls_and_never_grows_past_the_body() {
    let mut m = connect(Vec::new());
    let first = texts(&m);
    for _ in 0..60 {
        press(&mut m, 'j');
        assert!(m.rows(&Config::default()).len() <= VIEW_BODY);
    }
    assert_ne!(texts(&m), first);
    assert_eq!(texts(&m)[0], first[0], "the title stays");
    for _ in 0..60 {
        press(&mut m, 'k');
    }
    assert_eq!(texts(&m), first);
}

#[test]
fn the_hint_names_the_screen_keys() {
    let m = connect(Vec::new());
    assert_eq!(
        hint(&m),
        "j/k scroll · s/w tick · e e-mail · o client · n plan · Esc"
    );
}
