//! The lazarobox client route in the menu (openspec change
//! `gsc-lazarobox-client`): Connect Google's route state and `u`, the screen
//! on each route, the Link notices, and the `f` row of the gsc entry.

use super::ai_tests::press;
use super::connectors_tests::{area, entry, presence, texts, whole};
use super::picker_tests::{key, send};
use super::*;
use crate::core::ai::{
    mcp_catalog::{
        self,
        connect::{Ask, ClientRoute},
        gsc, snippet,
    },
    mcp_secrets::{CLIENT_LINE, SecretText},
};

const PIN: &str = "123456789012";
const ID: &str = "123-abc.apps.googleusercontent.com";
const OWN: &str = "/h/.config/lazarobox/google-oauth-client.json";
const CACHE: &str = "/h/.config/lazarobox/lazarobox-google-client.json";
const XDG_CACHE: &str = "/x/cfg/lazarobox/lazarobox-google-client.json";
const NOT_CONFIGURED: &str =
    "lazarobox client not configured in this build; this screen uses your own client";
const ALONE: &str = "the lazarobox client is only for Search Console alone; with Workspace ticked your own client is used";

/// The Connect Google screen with this pin, the own client read as `client`.
fn connect(pin: Option<&'static str>, client: Option<&str>) -> MenuState {
    let mut m = area(Vec::new());
    m.set_client_pin(pin);
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    m.google_client(Ok(client.map(str::to_owned)));
    m
}

/// The same with Workspace unticked: Search Console alone.
fn alone(pin: Option<&'static str>, client: Option<&str>) -> MenuState {
    let mut m = connect(pin, client);
    assert_eq!(press(&mut m, 'w'), MenuCommand::None);
    m
}

fn route_asked(m: &mut MenuState) -> ClientRoute {
    match press(m, 'n') {
        MenuCommand::PlanGoogle(Ask { route, .. }) => route,
        other => panic!("no plan asked: {other:?} {:?}", m.notice),
    }
}

fn all(m: &mut MenuState) -> String {
    for _ in 0..60 {
        press(m, 'k');
    }
    // A wrapped line reads whole.
    whole(m)
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn said(m: &MenuState) -> String {
    m.notice
        .clone()
        .or_else(|| texts(m).last().cloned())
        .unwrap_or_default()
}

// ---- 2.5: route state and `u` ----------------------------------------------------

// Spec (Client route): the default is the lazarobox client with a pin.
#[test]
fn the_default_route_is_lazarobox_with_a_pin_and_own_without_one() {
    // No own client needed on the lazarobox route.
    assert_eq!(
        route_asked(&mut alone(Some(PIN), None)),
        ClientRoute::Lazarobox
    );
    assert_eq!(route_asked(&mut alone(None, Some(ID))), ClientRoute::Own);
}

#[test]
fn u_toggles_the_route() {
    let mut m = alone(Some(PIN), Some(ID));
    assert_eq!(press(&mut m, 'u'), MenuCommand::None);
    assert_eq!(route_asked(&mut m), ClientRoute::Own);
    let mut m = alone(Some(PIN), Some(ID));
    press(&mut m, 'u');
    press(&mut m, 'u');
    assert_eq!(route_asked(&mut m), ClientRoute::Lazarobox);
}

// Spec (Pin not filled): shown disabled with the notice, never hidden.
#[test]
fn without_a_pin_the_route_is_disabled_with_a_notice_and_own_is_selected() {
    let mut m = alone(None, Some(ID));
    let screen = all(&mut m);
    assert!(screen.contains("route: my own client"), "{screen}");
    assert!(screen.contains(NOT_CONFIGURED), "{screen}");
    press(&mut m, 'u');
    assert_eq!(said(&m), NOT_CONFIGURED);
    assert_eq!(route_asked(&mut m), ClientRoute::Own);
}

// Owner decision 2026-11-12: Workspace ticked, both use the own client.
#[test]
fn with_workspace_ticked_u_has_no_effect_and_the_screen_says_why() {
    for pin in [Some(PIN), None] {
        let mut m = connect(pin, Some(ID));
        press(&mut m, 'u');
        assert!(said(&m).contains(ALONE), "{}", said(&m));
        assert_eq!(route_asked_or_email(&mut m), ClientRoute::Own);
    }
    let mut m = connect(Some(PIN), Some(ID));
    assert!(all(&mut m).contains(ALONE));
}

/// `n` with Workspace ticked wants the e-mail first: type it.
fn route_asked_or_email(m: &mut MenuState) -> ClientRoute {
    press(m, 'e');
    for c in "ana@example.com".chars() {
        press(m, c);
    }
    send(m, key(KeyCode::Enter));
    route_asked(m)
}

#[test]
fn the_choice_of_the_own_client_is_not_kept_after_leaving_the_screen() {
    // Leave the screen and come back: the default again.
    let mut m = alone(Some(PIN), Some(ID));
    press(&mut m, 'u');
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(send(&mut m, key(KeyCode::Enter)), MenuCommand::None);
    m.google_client(Ok(Some(ID.to_owned())));
    assert_eq!(route_asked(&mut m), ClientRoute::Lazarobox);
}

// ---- review: held values, `o` and a declared gsc ---------------------------------

fn kept(label: &str) -> String {
    format!("kept from secrets.toml: {label}; f on the entry changes it")
}

/// Connect Google with this value held for the gsc client file, not declared.
fn held_on_screen(path: &str, pin: Option<&'static str>) -> MenuState {
    let mut m = connect(pin, Some(ID));
    m.presence_reply(
        presence("", &held(path))
            .with_home(Some("/h".to_owned()))
            .with_client_cache(Some(CACHE.to_owned())),
    );
    m
}

// The held value is kept (#736), so the route and the screen say what it is.
#[test]
fn a_held_own_path_makes_the_route_own_and_says_so() {
    let mut m = held_on_screen(OWN, Some(PIN));
    press(&mut m, 'w');
    let screen = all(&mut m);
    assert!(screen.contains(&kept("your own client")), "{screen}");
    assert!(screen.contains("route: my own client"), "{screen}");
    assert!(!screen.contains("(u uses"), "{screen}");
    press(&mut m, 'u');
    assert!(said(&m).contains(&kept("your own client")), "{}", said(&m));
    assert_eq!(route_asked(&mut m), ClientRoute::Own);
}

#[test]
fn a_held_cache_path_makes_the_route_lazarobox_and_says_so() {
    let mut m = held_on_screen(CACHE, Some(PIN));
    press(&mut m, 'w');
    let screen = all(&mut m);
    assert!(screen.contains(&kept("lazarobox client")), "{screen}");
    assert!(screen.contains(CLIENT_LINE), "{screen}");
    // The user asked for their own client: the held value still wins.
    press(&mut m, 'o');
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(route_asked(&mut m), ClientRoute::Lazarobox);
    // With Workspace ticked too, the own client is used for both and the
    // screen still says what stays for Search Console.
    let mut m = held_on_screen(CACHE, Some(PIN));
    assert!(all(&mut m).contains(&kept("lazarobox client")));
}

#[test]
fn no_caveat_without_a_held_value() {
    let mut m = alone(Some(PIN), Some(ID));
    assert!(!all(&mut m).contains("kept from secrets.toml"));
}

// `o` selects the own route only once the form saves, not when it is cancelled.
#[test]
fn a_cancelled_o_form_keeps_the_lazarobox_route_and_a_saved_one_selects_own() {
    let mut m = alone(Some(PIN), None);
    press(&mut m, 'o');
    send(&mut m, key(KeyCode::Esc));
    assert_eq!(route_asked(&mut m), ClientRoute::Lazarobox);
    let mut m = alone(Some(PIN), None);
    press(&mut m, 'o');
    for c in ID.chars() {
        press(&mut m, c);
    }
    send(&mut m, key(KeyCode::Enter));
    for c in "GOCSPX-abc".chars() {
        press(&mut m, c);
    }
    assert!(matches!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::SaveClient { .. }
    ));
    // Only the accepted write selects it.
    assert_eq!(
        route_asked_after_save(&mut m, Err("not saved")),
        ClientRoute::Lazarobox
    );
    let mut m = alone(Some(PIN), None);
    press(&mut m, 'o');
    for c in ID.chars() {
        press(&mut m, c);
    }
    send(&mut m, key(KeyCode::Enter));
    for c in "GOCSPX-abc".chars() {
        press(&mut m, c);
    }
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(
        route_asked_after_save(&mut m, Ok("saved google-oauth-client.json")),
        ClientRoute::Own
    );
}

fn route_asked_after_save(m: &mut MenuState, reply: Result<&str, &str>) -> ClientRoute {
    m.secret_reply(reply.map(str::to_owned).map_err(str::to_owned));
    m.google_client(Ok(Some(ID.to_owned())));
    route_asked(m)
}

// Declining the replace prompt writes nothing and leaves the route alone.
#[test]
fn declining_the_replace_prompt_leaves_the_route_unchanged() {
    let mut m = alone(Some(PIN), Some(ID));
    m.presence_reply(presence("", "").with_client_file(true));
    press(&mut m, 'o');
    for c in ID.chars() {
        press(&mut m, c);
    }
    send(&mut m, key(KeyCode::Enter));
    for c in "GOCSPX-abc".chars() {
        press(&mut m, c);
    }
    assert_eq!(
        send(&mut m, key(KeyCode::Enter)),
        MenuCommand::None,
        "asks first"
    );
    assert_eq!(press(&mut m, 'n'), MenuCommand::None, "declined");
    assert_eq!(route_asked(&mut m), ClientRoute::Lazarobox);
}

// From the gsc entry the Esc step is false (f works there), so it is dropped.
#[test]
fn from_the_gsc_entry_the_notice_has_no_leading_esc() {
    let mut m = entry(Vec::new(), GSC);
    let secrets = format!(
        "[gsc]\n{NAME} = \"{CACHE}\"\nGSC_CONFIG_DIR = \"/h/.config/mcp-gsc\"\nGSC_SKIP_OAUTH = \"false\"\n"
    );
    let config = snippet(mcp_catalog::entry("gsc").unwrap());
    m.presence_reply(
        presence(&config, &secrets)
            .with_home(Some("/h".to_owned()))
            .with_token(|_| false),
    );
    assert!(matches!(press(&mut m, 'g'), MenuCommand::LinkGsc(_)));
    let notice = "own client: Esc, f on gsc \u{b7} lazarobox: refused (x)";
    m.link_reply(Err(notice.to_owned()), None);
    assert_eq!(
        m.notice.as_deref(),
        Some("own client: f \u{b7} lazarobox: refused (x)")
    );
}

// `u` with gsc declared only points at `f`, and Enter on the entry's client
// row is the real switch.
#[test]
fn with_gsc_declared_u_points_at_f_and_enter_on_the_entry_switches() {
    let mut m = linking();
    assert!(matches!(
        m.link_reply(Err("x".to_owned()), None),
        MenuCommand::None
    ));
    press(&mut m, 'u');
    assert_eq!(
        m.notice.as_deref(),
        Some("u picks the client of Search Console; f on its entry changes it")
    );
    let mut m = entry(Vec::new(), GSC);
    m.set_client_pin(Some(PIN));
    let config = snippet(mcp_catalog::entry("gsc").unwrap());
    m.presence_reply(
        presence(&config, &held(CACHE))
            .with_home(Some("/h".to_owned()))
            .with_client_cache(Some(CACHE.to_owned())),
    );
    assert_eq!(press(&mut m, 'f'), MenuCommand::None);
    assert!(
        texts(&m)
            .join(" ")
            .contains("Enter switches to your own client")
    );
    assert_eq!(enter(&mut m), MenuCommand::None, "asks first");
    assert_eq!(press(&mut m, 'y'), save(OWN));
}

// ---- 2.6: the screen on each route -----------------------------------------------

#[test]
fn the_lazarobox_route_draws_its_client_line_and_no_checklist_or_own_client() {
    let mut m = alone(Some(PIN), Some(ID));
    let screen = all(&mut m);
    assert!(screen.contains(CLIENT_LINE), "{screen}");
    assert!(!screen.contains("Google Cloud, once"), "{screen}");
    assert!(
        !screen.contains(ID) && !screen.contains("GOCSPX"),
        "{screen}"
    );
    assert!(!screen.contains("client: missing"), "{screen}");
}

#[test]
fn the_own_route_keeps_its_client_line_checklist_and_pre_check() {
    let mut m = alone(Some(PIN), Some(ID));
    press(&mut m, 'u');
    let screen = all(&mut m);
    assert!(screen.contains(&format!("client: {ID}")), "{screen}");
    assert!(screen.contains("Google Cloud, once"), "{screen}");
    assert!(!screen.contains(CLIENT_LINE));
    // The pre-check still fires on the own route and for Workspace.
    for mut m in [alone(None, None), connect(Some(PIN), None)] {
        assert_eq!(press(&mut m, 'n'), MenuCommand::None);
        assert_eq!(
            m.notice.as_deref(),
            Some("no client yet: press o to save your client id and secret")
        );
    }
}

// ---- 2.7: Link failure offers the own client, the fallback shows -----------------

/// Connect Google with `gsc` declared on the lazarobox client, `g` pressed:
/// the Link checks are running.
fn linking() -> MenuState {
    let mut m = connect(Some(PIN), None);
    let secrets = format!(
        "[gsc]\n{NAME} = \"{CACHE}\"\nGSC_CONFIG_DIR = \"/h/.config/mcp-gsc\"\nGSC_SKIP_OAUTH = \"false\"\n"
    );
    let config = snippet(mcp_catalog::entry("gsc").unwrap());
    m.presence_reply(
        presence(&config, &secrets)
            .with_home(Some("/h".to_owned()))
            .with_token(|_| false),
    );
    let plan = gsc::Plan {
        dir: "/h/.config/mcp-gsc".to_owned(),
        file: CACHE.to_owned(),
    };
    assert_eq!(press(&mut m, 'g'), MenuCommand::LinkGsc(plan));
    m
}

#[test]
fn a_link_failure_is_the_footer_notice_and_offers_the_own_client() {
    let mut m = linking();
    let why = "own client: Esc, f on gsc · lazarobox: not fetched (HTTP 404); link later";
    assert_eq!(m.link_reply(Err(why.to_owned()), None), MenuCommand::None);
    assert_eq!(m.notice.as_deref(), Some(why));
    assert!(why.contains("own client: Esc, f on gsc"));
}

#[test]
fn a_fallback_notice_shows_while_the_link_prompt_is_asked() {
    let mut m = linking();
    let fallback = "lazarobox client not refreshed (HTTP 500); linking with the saved copy";
    assert_eq!(
        m.link_reply(Ok("the line".to_owned()), Some(fallback.to_owned())),
        MenuCommand::None
    );
    assert_eq!(m.notice.as_deref(), Some(fallback));
}

// ---- 2.8: the `f` row of the gsc entry -------------------------------------------

const GSC: usize = 2;
const NAME: &str = "GSC_OAUTH_CLIENT_SECRETS_FILE";

fn held(path: &str) -> String {
    format!("[gsc]\n{NAME} = \"{path}\"\n")
}

/// The gsc fields list with this pin and cache path.
fn fields(pin: Option<&'static str>, cache: &str, secrets: &str) -> MenuState {
    let mut m = entry(Vec::new(), GSC);
    m.set_client_pin(pin);
    m.presence_reply(
        presence("", secrets)
            .with_home(Some("/h".to_owned()))
            .with_client_cache(Some(cache.to_owned()))
            .with_client_file(true),
    );
    assert_eq!(press(&mut m, 'f'), MenuCommand::None);
    m
}

fn save(path: &str) -> MenuCommand {
    let mut value = SecretText::new();
    value.push_str(path);
    MenuCommand::SaveSecret {
        server: "gsc",
        name: NAME,
        value,
    }
}

fn enter(m: &mut MenuState) -> MenuCommand {
    send(m, key(KeyCode::Enter))
}

// Spec (gsc entry `f` preset): the cache path by default, even with an own
// client file next to it, and under XDG_CONFIG_HOME.
#[test]
fn the_preset_writes_the_cache_path_with_a_pin_even_with_an_own_client() {
    for cache in [CACHE, XDG_CACHE] {
        let mut m = fields(Some(PIN), cache, "");
        let rows = texts(&m).join(" ");
        assert!(rows.contains("Enter writes the lazarobox client"), "{rows}");
        assert_eq!(enter(&mut m), save(cache));
    }
}

#[test]
fn without_a_pin_the_preset_offers_the_own_client_and_says_so() {
    let mut m = fields(None, CACHE, "");
    let rows = texts(&m).join(" ");
    assert!(
        rows.contains("lazarobox client not configured in this build"),
        "{rows}"
    );
    assert_eq!(enter(&mut m), save(OWN));
}

// Spec: Enter writes the other route's path, after the replace prompt.
#[test]
fn enter_switches_between_the_routes_after_the_replace_prompt() {
    let mut m = fields(Some(PIN), CACHE, &held(CACHE));
    let rows = texts(&m).join(" ");
    assert!(rows.contains("Enter switches to your own client"), "{rows}");
    assert_eq!(enter(&mut m), MenuCommand::None, "asks first");
    assert_eq!(press(&mut m, 'y'), save(OWN));

    let mut m = fields(Some(PIN), CACHE, &held(OWN));
    let rows = texts(&m).join(" ");
    assert!(rows.contains("Enter writes the lazarobox client"), "{rows}");
    assert_eq!(enter(&mut m), MenuCommand::None, "asks first");
    assert_eq!(press(&mut m, 'y'), save(CACHE));
}

// The `f` screen guides the steps the footer only points at.
#[test]
fn the_client_file_line_guides_the_switch_to_your_own_client() {
    let rows = |m: &MenuState| texts(m).iter().map(|r| r.chars().count()).max().unwrap();
    let mut m = fields(Some(PIN), CACHE, &held(CACHE));
    let screen = texts(&m).join(" ");
    assert!(
        screen.contains("Enter switches to your own client; then Esc, g links"),
        "{screen}"
    );
    assert!(rows(&m) <= 60, "{:?}", texts(&m));
    // With no own client file, `o` saves it first.
    let mut none = entry(Vec::new(), GSC);
    none.set_client_pin(Some(PIN));
    none.presence_reply(
        presence("", &held(CACHE))
            .with_home(Some("/h".to_owned()))
            .with_client_cache(Some(CACHE.to_owned())),
    );
    press(&mut none, 'f');
    let screen = texts(&none).join(" ");
    assert!(
        screen.contains(
            "o saves your own client first; Enter switches to your own client; then Esc, g links"
        ),
        "{screen}"
    );
    assert!(rows(&none) <= 60, "{:?}", texts(&none));
    let _ = &mut m;
}

#[test]
fn declining_the_prompt_keeps_the_held_value() {
    let mut m = fields(Some(PIN), CACHE, &held(CACHE));
    assert_eq!(enter(&mut m), MenuCommand::None);
    assert_eq!(press(&mut m, 'n'), MenuCommand::None);
    assert!(
        texts(&m)
            .join(" ")
            .contains("Enter switches to your own client")
    );
}

// The other rows keep their own preset (characterization).
#[test]
fn the_other_gsc_rows_and_entries_are_unchanged() {
    let mut m = fields(Some(PIN), CACHE, "");
    press(&mut m, 'j');
    let mut dir = SecretText::new();
    dir.push_str("/h/.config/mcp-gsc");
    assert_eq!(
        enter(&mut m),
        MenuCommand::SaveSecret {
            server: "gsc",
            name: "GSC_CONFIG_DIR",
            value: dir,
        }
    );
}
