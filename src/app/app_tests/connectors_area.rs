//! The Connectors area through the app (mcp-connect, Entry list, Entry
//! screen, Install and login, Probe from the area, and the config re-read of
//! D20): the commands it emits, the replies it gets, and that nothing runs
//! without the user's `y`.

use super::*;
use crate::core::ai::mcp_secrets::Secrets;
use crate::core::{
    ai::{
        mcp_catalog::{
            checks::{Probed, Verdict},
            entry,
            link::Step,
            presence::Presence,
        },
        mcp_secrets::Loaded,
    },
    config::Config as FileConfig,
    menu::Footer,
};

const CLARITY: &str = "[[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"x\"\n";

fn rows(a: &App) -> Vec<String> {
    a.menu()
        .unwrap()
        .rows(&Config::default())
        .into_iter()
        .map(|row| match row {
            Row::Title(t) | Row::Text(t) => t.into_owned(),
            Row::Item { label, value, .. } => format!("{label}: {}", value.unwrap_or_default()),
            other => format!("{other:?}"),
        })
        .collect()
}

/// One key after the hold window of the last, so the same key twice is two
/// presses and not a held key.
fn tap(a: &mut App, event: AppEvent) -> Vec<Effect> {
    let_go();
    a.update(event)
}

fn spawned(effects: &[Effect]) -> Option<PaneId> {
    effects.iter().find_map(|e| match e {
        Effect::SpawnPane(id, _) => Some(*id),
        _ => None,
    })
}

fn writes(effects: &[Effect]) -> Vec<(PaneId, Vec<u8>)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::WritePty(id, bytes) => Some((*id, bytes.clone())),
            _ => None,
        })
        .collect()
}

/// The area open over an app that loaded `CLARITY` at startup.
fn in_area() -> App {
    let ai = FileConfig::parse(CLARITY).unwrap().ai;
    let mut a = launched(80, 25).with_ai(ai);
    tap(&mut a, ctrl_space());
    tap(&mut a, key('m'));
    tap(&mut a, key('4'));
    let effects = tap(&mut a, code(KeyCode::Enter));
    assert_eq!(effects, [Effect::LoadPresence], "opening reads the files");
    a
}

/// The same, on the Clarity entry screen (below the Connect Google row).
fn on_entry() -> App {
    let mut a = in_area();
    tap(&mut a, key('j'));
    tap(&mut a, code(KeyCode::Enter));
    a
}

fn failed() -> Probed {
    Probed {
        server: "clarity".to_owned(),
        verdict: Verdict::Failed,
        lines: vec!["mcp: clarity unavailable: boom".to_owned()],
        problems: Vec::new(),
    }
}

#[test]
fn the_load_presence_command_becomes_its_effect() {
    let mut a = launched(80, 25);
    assert_eq!(
        a.on_menu_command(MenuCommand::LoadPresence),
        [Effect::LoadPresence]
    );
}

#[test]
fn the_menu_opens_with_the_startup_servers() {
    assert_eq!(rows(&in_area())[2], "Clarity: declared");
}

#[test]
fn a_pasted_entry_asks_for_a_restart_on_the_list_and_the_entry() {
    let mut a = in_area();
    let none = Presence::read(Ok(String::new()), &Loaded::default());
    tap(&mut a, AppEvent::McpPresence(none));
    assert_eq!(rows(&a)[2], "Clarity: not declared");
    assert!(rows(&a).contains(&"restart lazarobox to use it in chats".to_owned()));
}

#[test]
fn a_presence_reply_after_the_menu_closed_changes_nothing() {
    let mut a = launched(80, 25);
    let none = Presence::read(Ok(String::new()), &Loaded::default());
    assert_eq!(tap(&mut a, AppEvent::McpPresence(none)), []);
}

#[test]
fn the_install_runs_only_after_y_and_with_the_shown_line() {
    let mut a = on_entry();
    let before = a.panes.len();
    let asked = tap(&mut a, key('i'));
    assert_eq!((spawned(&asked), writes(&asked)), (None, vec![]));
    let declined = tap(&mut a, key('n'));
    assert_eq!((spawned(&declined), writes(&declined)), (None, vec![]));
    assert_eq!(a.panes.len(), before);
    tap(&mut a, key('i'));
    let run = tap(&mut a, key('y'));
    let id = spawned(&run).expect("one new pane");
    let typed = tap(&mut a, AppEvent::Pty(id, PtyEvent::Output(b"$ ".to_vec())));
    let line = entry("clarity").unwrap().install;
    assert_eq!(writes(&typed), [(id, format!("{line}\r").into_bytes())]);
}

#[test]
fn the_probe_starts_only_after_y() {
    let mut a = on_entry();
    assert_eq!(tap(&mut a, key('p')), []);
    assert_eq!(tap(&mut a, key('n')), []);
    tap(&mut a, key('p'));
    let started = tap(&mut a, key('y'));
    assert!(matches!(started.as_slice(), [Effect::McpProbe(_, s)] if s.name == "clarity"));
}

#[test]
fn a_probe_reply_updates_the_area_even_a_failed_one() {
    let mut a = on_entry();
    tap(&mut a, key('p'));
    let started = tap(&mut a, key('y'));
    let Some(Effect::McpProbe(id, _)) = started.first() else {
        panic!("probe asked");
    };
    assert!(matches!(
        a.menu().unwrap().footer(),
        Footer::Hint(h) if h.starts_with("probing clarity")
    ));
    tap(&mut a, AppEvent::McpProbe(*id, failed()));
    assert!(matches!(
        a.menu().unwrap().footer(),
        Footer::Hint(h) if !h.starts_with("probing")
    ));
    assert!(rows(&a).iter().any(|l| l.contains("unavailable: boom")));
}

#[test]
fn esc_while_probing_cancels_the_probe() {
    let mut a = on_entry();
    tap(&mut a, key('p'));
    tap(&mut a, key('y'));
    assert_eq!(tap(&mut a, esc()), [Effect::CancelMcpProbe]);
}

// Spec (The area writes no configuration): no key of the area saves anything
// or types anything on its own.
#[test]
fn visiting_the_area_saves_and_types_nothing() {
    let mut a = in_area();
    let mut effects = Vec::new();
    for c in "jjkk".chars() {
        effects.extend(tap(&mut a, key(c)));
    }
    effects.extend(tap(&mut a, code(KeyCode::Enter)));
    for c in "jkctz12".chars() {
        effects.extend(tap(&mut a, key(c)));
    }
    effects.extend(tap(&mut a, esc()));
    effects.extend(tap(&mut a, esc()));
    assert!(
        !effects.iter().any(|e| matches!(
            e,
            Effect::SaveConfig(_) | Effect::WritePty(..) | Effect::McpProbe(..)
        )),
        "{effects:?}"
    );
    assert_eq!(spawned(&effects), None);
}

// A resize while the menu is open re-wraps the area at once, not at the next key.
#[test]
fn a_resize_rewraps_an_open_prompt_without_a_key() {
    let mut a = on_entry();
    tap(&mut a, key('i'));
    assert!(rows(&a).iter().any(|l| l.contains("npm i -g")));
    tap(&mut a, AppEvent::Resize { cols: 20, rows: 25 });
    assert_eq!(rows(&a), ["widen the terminal to read the command"]);
    tap(&mut a, AppEvent::Resize { cols: 80, rows: 25 });
    assert!(rows(&a).iter().any(|l| l.contains("npm i -g")));
}

// ---- S6a2: the secret field through the app ---------------------------------

const MATCHING: &str = "[[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"clarity-mcp-server\"\n";

fn secret(text: &str) -> crate::core::ai::mcp_secrets::SecretText {
    let mut s = crate::core::ai::mcp_secrets::SecretText::new();
    s.push_str(text);
    s
}

fn footer(a: &App) -> String {
    match a.menu().unwrap().footer() {
        Footer::Hint(h) => h.into_owned(),
        Footer::Error(e) => e.to_owned(),
    }
}

/// The Clarity field open over a chat that declared the catalog command.
fn field_in_a_chat() -> App {
    let ai = FileConfig::parse(MATCHING).unwrap().ai;
    let mut a = super::run_pane::one_shell_and_a_chat("hello").with_ai(ai);
    tap(&mut a, ctrl_space());
    tap(&mut a, key('m'));
    tap(&mut a, key('4'));
    tap(&mut a, code(KeyCode::Enter));
    tap(&mut a, key('j'));
    tap(&mut a, code(KeyCode::Enter));
    tap(&mut a, key('f'));
    tap(&mut a, code(KeyCode::Enter));
    a
}

// Spec: Save a new value (the command becomes the effect, once).
#[test]
fn a_confirmed_field_becomes_one_save_effect_with_the_value() {
    let mut a = field_in_a_chat();
    for c in "tok".chars() {
        assert_eq!(tap(&mut a, key(c)), []);
    }
    assert_eq!(
        tap(&mut a, code(KeyCode::Enter)),
        [Effect::SaveSecret {
            server: "clarity",
            name: "CLARITY_API_TOKEN",
            value: secret("tok")
        }]
    );
}

// Spec: Paste goes to the field only.
#[test]
fn a_paste_goes_to_the_open_field_and_not_to_the_chat() {
    let mut a = field_in_a_chat();
    assert_eq!(tap(&mut a, AppEvent::Paste("sk-123".to_owned())), []);
    assert_eq!(super::run_pane::chat(&a, id2()).composer().text(), "");
    let rows = rows(&a);
    assert!(rows.iter().any(|r| r.trim() == "••••••••"), "{rows:?}");
    assert_eq!(
        tap(&mut a, code(KeyCode::Enter)),
        [Effect::SaveSecret {
            server: "clarity",
            name: "CLARITY_API_TOKEN",
            value: secret("sk-123")
        }]
    );
    assert_eq!(super::run_pane::chat(&a, id2()).composer().text(), "");
}

#[test]
fn the_reply_of_a_save_shows_its_notice_and_reads_the_files_again() {
    let mut a = field_in_a_chat();
    let said = "saved CLARITY_API_TOKEN".to_owned();
    assert_eq!(
        tap(&mut a, AppEvent::SecretSaved(Ok(said.clone()))),
        [Effect::LoadPresence]
    );
    assert_eq!(footer(&a), said);
    let refusal = "secrets.toml is a symlink; edit it by hand".to_owned();
    assert_eq!(tap(&mut a, AppEvent::SecretSaved(Err(refusal.clone()))), []);
    assert_eq!(footer(&a), refusal);
}

// A reply that arrives after the menu closed is still said, never dropped.
#[test]
fn a_reply_after_the_menu_closed_is_said_in_the_notice() {
    let mut a = launched(80, 25);
    assert_eq!(
        tap(&mut a, AppEvent::SecretSaved(Ok("saved X".to_owned()))),
        []
    );
    assert_eq!(a.notice.as_deref(), Some("saved X"));
    tap(
        &mut a,
        AppEvent::BackupDeleted(Err("secrets.toml.bak: there is no backup".to_owned())),
    );
    assert_eq!(
        a.notice.as_deref(),
        Some("secrets.toml.bak: there is no backup")
    );
}

// Spec: the backup action is a command, then an effect, only after `y`.
#[test]
fn deleting_the_backup_is_an_effect_only_after_y() {
    let mut a = on_entry();
    let present = Presence::read(Ok(CLARITY.to_owned()), &Loaded::default()).with_backup(true);
    tap(&mut a, AppEvent::McpPresence(present));
    tap(&mut a, key('f'));
    assert_eq!(tap(&mut a, key('d')), []);
    assert_eq!(tap(&mut a, key('y')), [Effect::DeleteBackup]);
    assert_eq!(
        tap(
            &mut a,
            AppEvent::BackupDeleted(Ok("deleted secrets.toml.bak".to_owned()))
        ),
        [Effect::LoadPresence]
    );
}

// Spec: The value appears nowhere else. Every surface the app has to show or
// format what happened, from the paste to the reply.
#[test]
fn a_saved_confidential_value_reaches_no_surface_of_the_app() {
    let value = "tok-7f3a9c1e2b4d6f80";
    let mut a = field_in_a_chat();
    let mut seen = vec![format!(
        "{:?}",
        tap(&mut a, AppEvent::Paste(value.to_owned()))
    )];
    seen.push(rows(&a).concat());
    let effects = tap(&mut a, code(KeyCode::Enter));
    seen.push(format!("{effects:?}"));
    let reply = AppEvent::SecretSaved(Ok("saved CLARITY_API_TOKEN".to_owned()));
    seen.push(format!("{reply:?} {:?}", tap(&mut a, reply.clone())));
    let loaded = format!("[clarity]\nCLARITY_API_TOKEN = \"{value}\"\n");
    let presence = Presence::read(Ok(MATCHING.to_owned()), &Secrets::parse(loaded.as_bytes()));
    tap(&mut a, AppEvent::McpPresence(presence));
    seen.extend([
        rows(&a).concat(),
        footer(&a),
        format!("{:?}", a.menu().unwrap()),
    ]);
    seen.push(super::run_pane::chat(&a, id2()).composer().text());
    seen.push(a.notice.clone().unwrap_or_default());
    let all = seen.join("\n");
    for at in 0..=value.len() - 4 {
        assert!(
            !all.contains(&value[at..at + 4]),
            "{} leaked",
            &value[at..at + 4]
        );
    }
}

// Spec (config-menu, Connectors row): a secret save is not a draft, and every
// key goes to the open field.
#[test]
fn a_secret_field_is_not_a_draft_and_takes_every_key() {
    let mut a = field_in_a_chat();
    for c in "s4q".chars() {
        assert_eq!(tap(&mut a, key(c)), []);
    }
    assert_eq!(
        rows(&a).len(),
        2,
        "the field stays one screen: {:?}",
        rows(&a)
    );
    tap(
        &mut a,
        AppEvent::SecretSaved(Ok("saved CLARITY_API_TOKEN".to_owned())),
    );
    assert!(a.menu().unwrap().edits(&Config::default()).is_empty());
    let effects = [esc(), esc(), esc(), esc()]
        .map(|e| tap(&mut a, e))
        .concat();
    assert!(
        !effects.iter().any(|e| matches!(
            e,
            Effect::SaveConfig(_) | Effect::SaveSecret { .. } | Effect::DeleteBackup
        )),
        "{effects:?}"
    );
}

// Spec: every other action of the area writes nothing; only a confirmed field
// or `y` on the backup prompt produces a write effect.
#[test]
fn nothing_but_a_confirmed_field_or_a_confirmed_backup_prompt_writes() {
    let mut a = on_entry();
    let mut effects = Vec::new();
    for e in [
        key('f'),
        code(KeyCode::Enter),
        key('t'),
        key('o'),
        key('k'),
        esc(),
        key('d'),
        key('n'),
        esc(),
    ] {
        effects.extend(tap(&mut a, e));
    }
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::SaveSecret { .. } | Effect::DeleteBackup)),
        "{effects:?}"
    );
}

// ---- S6a3: the client form through the app ----------------------------------

/// The `ga4` fields list over a chat, with `o` pressed.
fn client_form_in_a_chat() -> App {
    let mut a = super::run_pane::one_shell_and_a_chat("hello");
    for e in [
        ctrl_space(),
        key('m'),
        key('4'),
        code(KeyCode::Enter),
        key('j'),
        key('j'),
        code(KeyCode::Enter),
        key('f'),
        key('o'),
    ] {
        tap(&mut a, e);
    }
    a
}

#[test]
fn a_confirmed_client_form_becomes_one_save_client_effect_and_the_paste_stays_in_the_form() {
    let mut a = client_form_in_a_chat();
    let id = "123-abc.apps.googleusercontent.com";
    assert_eq!(tap(&mut a, AppEvent::Paste(id.to_owned())), []);
    assert_eq!(tap(&mut a, code(KeyCode::Enter)), []);
    assert_eq!(tap(&mut a, AppEvent::Paste("GOCSPX-abc".to_owned())), []);
    assert_eq!(super::run_pane::chat(&a, id2()).composer().text(), "");
    assert_eq!(
        tap(&mut a, code(KeyCode::Enter)),
        [Effect::SaveClient {
            id: id.to_owned(),
            secret: secret("GOCSPX-abc"),
            replace: false
        }]
    );
}

// ---- S6b1: Link with Google for gsc through the app --------------------------

const GSC_CONFIG: &str = "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"mcp-search-console\"\n\
    env = [\"GSC_OAUTH_CLIENT_SECRETS_FILE\", \"GSC_CONFIG_DIR\", \"GSC_CREDENTIALS_PATH\", \"GSC_SKIP_OAUTH\"]\n";
const GSC_OAUTH: &str = "[gsc]\nGSC_OAUTH_CLIENT_SECRETS_FILE = \"/h/.config/lazarobox/google-oauth-client.json\"\n\
    GSC_CONFIG_DIR = \"/h/.config/mcp-gsc\"\nGSC_SKIP_OAUTH = \"false\"\n";
const GSC_LINE: &str = "env -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u GSC_ALLOW_DESTRUCTIVE -u GSC_SKIP_OAUTH GSC_CONFIG_DIR='/h/.config/mcp-gsc' GSC_OAUTH_CLIENT_SECRETS_FILE='/h/.config/lazarobox/google-oauth-client.json' '/h/.local/share/uv/tools/mcp-search-console/bin/python' -I -c 'import os; os.umask(0o077); import gsc_server as g; g.get_gsc_service_oauth(); print(\"Search Console linked\")'";

fn gsc_plan() -> crate::core::ai::mcp_catalog::gsc::Plan {
    crate::core::ai::mcp_catalog::gsc::Plan {
        dir: "/h/.config/mcp-gsc".to_owned(),
        file: "/h/.config/lazarobox/google-oauth-client.json".to_owned(),
    }
}

/// The `gsc` entry screen, filled in the OAuth way, `g` not yet pressed.
fn on_gsc() -> App {
    let ai = FileConfig::parse(GSC_CONFIG).unwrap().ai;
    let mut a = launched(80, 40).with_ai(ai);
    for e in [ctrl_space(), key('m'), key('4'), code(KeyCode::Enter)] {
        tap(&mut a, e);
    }
    let loaded = Secrets::parse(GSC_OAUTH.as_bytes());
    tap(
        &mut a,
        AppEvent::McpPresence(
            Presence::read(Ok(GSC_CONFIG.to_owned()), &loaded).with_home(Some("/h".to_owned())),
        ),
    );
    for e in [key('j'), key('j'), key('j'), code(KeyCode::Enter)] {
        tap(&mut a, e);
    }
    a
}

// Spec mcp-connect, gsc link types the line; Nothing without y.
#[test]
fn g_asks_for_the_checks_and_only_y_after_the_answer_types_the_line_in_one_new_pane() {
    let mut a = on_gsc();
    let before = a.panes.len();
    let asked = tap(&mut a, key('g'));
    assert_eq!(asked, [Effect::LinkGsc(gsc_plan())]);
    assert_eq!(tap(&mut a, AppEvent::GscLine(Ok(GSC_LINE.to_owned()))), []);
    let declined = tap(&mut a, key('n'));
    assert_eq!((spawned(&declined), writes(&declined)), (None, vec![]));
    assert_eq!(a.panes.len(), before);
    tap(&mut a, key('g'));
    tap(&mut a, AppEvent::GscLine(Ok(GSC_LINE.to_owned())));
    let run = tap(&mut a, key('y'));
    let id = spawned(&run).expect("one new pane");
    let typed = tap(&mut a, AppEvent::Pty(id, PtyEvent::Output(b"$ ".to_vec())));
    assert_eq!(writes(&typed), [(id, format!("{GSC_LINE}\r").into_bytes())]);
    // No MCP server was started by the menu.
    assert!(!run.iter().any(|e| matches!(e, Effect::McpProbe(..))));
}

#[test]
fn a_refused_check_is_a_footer_notice_and_types_nothing() {
    let mut a = on_gsc();
    tap(&mut a, key('g'));
    let why = "uv not found: install uv first";
    assert_eq!(tap(&mut a, AppEvent::GscLine(Err(why.to_owned()))), []);
    assert!(matches!(a.menu().unwrap().footer(), Footer::Hint(h) if h == why));
}

#[test]
fn a_check_that_answers_after_the_menu_closed_says_why_and_prompts_nothing() {
    let mut a = on_gsc();
    tap(&mut a, key('g'));
    tap(&mut a, esc());
    tap(&mut a, esc());
    tap(&mut a, esc());
    assert!(a.menu().is_none() || !rows(&a).concat().contains("link gsc"));
    let why = "uv failed";
    assert_eq!(tap(&mut a, AppEvent::GscLine(Err(why.to_owned()))), []);
    let effects = tap(&mut a, AppEvent::GscLine(Ok(GSC_LINE.to_owned())));
    assert_eq!((spawned(&effects), writes(&effects)), (None, vec![]));
}

// ---- S6b2: Link with Google for google through the app -----------------------

const GOOGLE_CONFIG: &str = "[[ai.mcp.servers]]\nname = \"google\"\ncommand = \"workspace-mcp\"\n\
    env = [\"GOOGLE_OAUTH_CLIENT_ID\", \"GOOGLE_OAUTH_CLIENT_SECRET\", \"USER_GOOGLE_EMAIL\", \"WORKSPACE_MCP_DISABLE_LOCAL_FILES\"]\n";
const GOOGLE_VALUES: &str = "[google]\nGOOGLE_OAUTH_CLIENT_ID = \"the-id.apps.googleusercontent.com\"\n\
    GOOGLE_OAUTH_CLIENT_SECRET = \"the-secret\"\nUSER_GOOGLE_EMAIL = \"me@x.org\"\n";
const GOOGLE_URL: &str = "https://accounts.google.com/o/oauth2/auth?client_id=the-id.apps.googleusercontent.com&redirect_uri=http%3A%2F%2Flocalhost%3A8000%2F";

/// The `google` entry screen, filled, `g` not yet pressed.
fn on_google(opener: bool) -> App {
    let ai = FileConfig::parse(GOOGLE_CONFIG).unwrap().ai;
    let mut a = launched(80, 40).with_ai(ai);
    a.opener = opener;
    for e in [ctrl_space(), key('m'), key('4'), code(KeyCode::Enter)] {
        tap(&mut a, e);
    }
    let loaded = Secrets::parse(GOOGLE_VALUES.as_bytes());
    tap(
        &mut a,
        AppEvent::McpPresence(
            Presence::read(Ok(GOOGLE_CONFIG.to_owned()), &loaded).with_home(Some("/h".to_owned())),
        ),
    );
    for e in [key('j'), key('j'), key('j'), key('j'), code(KeyCode::Enter)] {
        tap(&mut a, e);
    }
    a
}

fn hint_of(a: &App) -> String {
    match a.menu().unwrap().footer() {
        Footer::Hint(h) => h.into_owned(),
        other => format!("{other:?}"),
    }
}

/// `g` then `y`: the session was asked for.
fn linking(opener: bool) -> App {
    let mut a = on_google(opener);
    tap(&mut a, key('g'));
    let asked = tap(&mut a, key('y'));
    assert!(
        matches!(asked.as_slice(), [Effect::LinkGoogle { .. }]),
        "{asked:?}"
    );
    a
}

// Spec mcp-connect, Nothing without y; the session starts only after `y`.
#[test]
fn g_asks_and_only_y_becomes_one_link_effect_and_no_pane() {
    let mut a = on_google(false);
    let before = a.panes.len();
    assert_eq!(tap(&mut a, key('g')), []);
    let declined = tap(&mut a, key('n'));
    assert_eq!((spawned(&declined), writes(&declined)), (None, vec![]));
    tap(&mut a, key('g'));
    let run = tap(&mut a, key('y'));
    let [Effect::LinkGoogle { server, ask }] = run.as_slice() else {
        panic!("one link effect: {run:?}");
    };
    assert_eq!(server.name, "google");
    assert_eq!(ask.email, "me@x.org");
    assert_eq!(a.panes.len(), before);
}

// Spec: google link succeeds. The link is opened through the opener.
#[test]
fn the_link_is_opened_once_and_shown_while_waiting() {
    let mut a = linking(true);
    let step = Step::Waiting(GOOGLE_URL.to_owned());
    let effects = tap(&mut a, AppEvent::McpLink(step));
    assert_eq!(effects, [Effect::OpenUrl(GOOGLE_URL.to_owned())]);
    assert!(rows(&a).concat().replace(' ', "").contains(GOOGLE_URL));
    // Esc cancels the session; the runtime stops it.
    assert_eq!(tap(&mut a, esc()), [Effect::CancelMcpProbe]);
}

#[test]
fn without_an_opener_the_link_is_copied() {
    let mut a = linking(false);
    let effects = tap(
        &mut a,
        AppEvent::McpLink(Step::Waiting(GOOGLE_URL.to_owned())),
    );
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::OpenUrl(_))),
        "{effects:?}"
    );
    assert!(!effects.is_empty(), "the link is copied instead");
}

#[test]
fn success_reads_the_files_again_and_a_failure_says_why() {
    let mut a = linking(false);
    let ended = Step::Ended(Ok("linked google".to_owned()));
    assert_eq!(
        tap(&mut a, AppEvent::McpLink(ended)),
        [Effect::LoadPresence]
    );
    assert_eq!(hint_of(&a), "linked google");
    let mut a = linking(false);
    let why = "link failed: ports 8000-8004 are in use";
    let ended = Step::Ended(Err(why.to_owned()));
    assert_eq!(tap(&mut a, AppEvent::McpLink(ended)), []);
    assert_eq!(hint_of(&a), why);
}

// With the menu closed a step is not lost: the end is a notice and the
// session, which nobody sees, is stopped.
#[test]
fn a_step_after_the_menu_closed_is_said_or_stopped() {
    let mut a = linking(false);
    a.close_menu(Outcome::Reverted);
    let waiting = tap(
        &mut a,
        AppEvent::McpLink(Step::Waiting(GOOGLE_URL.to_owned())),
    );
    assert_eq!(waiting, [Effect::CancelMcpProbe]);
    let why = "link cancelled";
    assert_eq!(
        tap(&mut a, AppEvent::McpLink(Step::Ended(Err(why.to_owned())))),
        []
    );
    assert_eq!(a.notice.as_deref(), Some(why));
}

// S6b2 review: a session this menu did not start (the menu was reopened)
// shows nothing, so it is stopped and its end is said as a notice.
#[test]
fn a_step_for_a_menu_that_did_not_start_the_session_stops_it_and_is_said() {
    let mut a = on_google(false);
    let waiting = tap(
        &mut a,
        AppEvent::McpLink(Step::Waiting(GOOGLE_URL.to_owned())),
    );
    assert_eq!(waiting, [Effect::CancelMcpProbe]);
    let why = "link cancelled";
    assert_eq!(
        tap(&mut a, AppEvent::McpLink(Step::Ended(Err(why.to_owned())))),
        []
    );
    assert_eq!(a.notice.as_deref(), Some(why));
}

// ---- Connect Google (openspec change `google-connect`, S5) ----------------------

const CLIENT_ID: &str = "123-abc.apps.googleusercontent.com";

/// The Connect Google screen open, the client read.
fn on_connect_google() -> App {
    let mut a = in_area();
    assert_eq!(tap(&mut a, code(KeyCode::Enter)), [], "the first row");
    let client = AppEvent::GoogleClient(Ok(Some(CLIENT_ID.to_owned())));
    assert_eq!(tap(&mut a, client), []);
    a
}

/// The runtime's plan for Search Console alone.
fn gsc_preview() -> crate::core::ai::mcp_catalog::connect::Preview {
    use crate::core::{ai::mcp_catalog::snippet, config_append::Connect};
    crate::core::ai::mcp_catalog::connect::Preview {
        write: Connect {
            set: vec![("gsc", "GSC_SKIP_OAUTH", secret("false"))],
            snippets: snippet(entry("gsc").unwrap()),
            names: vec!["gsc"],
            expected: None,
        },
        kept: Vec::new(),
        installs: vec![entry("gsc").unwrap().install],
        declared: Vec::new(),
        blocked: None,
    }
}

// Spec S5.6-S5.7: `n` asks for the plan, `y` on it writes, each one effect.
#[test]
fn connect_google_plans_on_n_and_writes_only_on_y_after_the_plan() {
    use crate::core::{ai::mcp_catalog::connect::Ask, config_append::Connected};
    let mut a = on_connect_google();
    assert_eq!(tap(&mut a, key('y')), [], "no plan yet");
    assert_eq!(tap(&mut a, key('w')), [], "Workspace unticked");
    assert_eq!(
        tap(&mut a, key('n')),
        [Effect::PlanGoogle(Ask {
            chosen: vec!["gsc"],
            email: String::new()
        })]
    );
    let plan = AppEvent::GooglePlan(Ok(Box::new(gsc_preview())));
    assert_eq!(tap(&mut a, plan), []);
    assert_eq!(rows(&a)[0], "Connect Google · plan");
    for c in ['n', 'x', 'i', 'g'] {
        assert_eq!(tap(&mut a, key(c)), [], "{c}");
    }
    assert_eq!(
        tap(&mut a, key('y')),
        [Effect::ConnectGoogle(Box::new(gsc_preview().write))]
    );
    assert_eq!(tap(&mut a, key('y')), [], "once");
    let done = Connected::Done("connect: added gsc to config.toml".to_owned());
    assert_eq!(
        tap(&mut a, AppEvent::GoogleConnected(done)),
        [Effect::LoadPresence]
    );
    assert_eq!(footer(&a), "connect: added gsc to config.toml");
}

#[test]
fn a_refused_plan_and_a_refused_write_are_footer_notices() {
    use crate::core::config_append::Connected;
    let mut a = on_connect_google();
    tap(&mut a, key('w'));
    tap(&mut a, key('n'));
    let why = "config.toml is a symlink; edit it by hand".to_owned();
    assert_eq!(tap(&mut a, AppEvent::GooglePlan(Err(why.clone()))), []);
    assert_eq!(footer(&a), why);
    tap(&mut a, key('n'));
    tap(&mut a, AppEvent::GooglePlan(Ok(Box::new(gsc_preview()))));
    tap(&mut a, key('y'));
    let refused =
        Connected::Refused("connect: nothing was written: x; paste the entries by hand".to_owned());
    assert_eq!(tap(&mut a, AppEvent::GoogleConnected(refused.clone())), []);
    assert_eq!(footer(&a), refused.notice());
}

// The answers after the menu closed: the write is still said (as before
// the screen existed), a plan or a client read is dropped.
#[test]
fn connect_google_answers_after_the_menu_closed() {
    use crate::core::config_append::Connected;
    let mut a = launched(80, 25);
    let done = Connected::ValuesOnly("connect: saved 3 values in secrets.toml, but x".to_owned());
    assert_eq!(tap(&mut a, AppEvent::GoogleConnected(done)), []);
    assert_eq!(
        a.notice.as_deref(),
        Some("connect: saved 3 values in secrets.toml, but x")
    );
    a.notice = None;
    let plan = AppEvent::GooglePlan(Ok(Box::new(gsc_preview())));
    assert_eq!(tap(&mut a, plan), []);
    assert_eq!(tap(&mut a, AppEvent::GoogleClient(Ok(None))), []);
    assert_eq!(a.notice, None);
}

// Spec S5.8: `i` types both install lines in one new tab, after `y`.
#[test]
fn connect_google_installs_both_servers_in_one_tab_after_y() {
    use crate::core::ai::mcp_catalog::snippet;
    let mut a = on_connect_google();
    let config = format!(
        "{}\n{}",
        snippet(entry("gsc").unwrap()),
        snippet(entry("google").unwrap())
    );
    let presence = Presence::read(Ok(config), &Loaded::default());
    tap(&mut a, AppEvent::McpPresence(presence));
    let asked = tap(&mut a, key('i'));
    assert_eq!((spawned(&asked), writes(&asked)), (None, vec![]));
    let run = tap(&mut a, key('y'));
    let id = spawned(&run).expect("one new pane");
    let typed = tap(&mut a, AppEvent::Pty(id, PtyEvent::Output(b"$ ".to_vec())));
    let line = format!(
        "{} && {}",
        entry("gsc").unwrap().install,
        entry("google").unwrap().install
    );
    assert_eq!(writes(&typed), [(id, format!("{line}\r").into_bytes())]);
}
