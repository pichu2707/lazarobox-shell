//! The install and login of a connector run in a new shell tab, typed as
//! visible text and sent with Enter once the shell speaks (mcp-connect,
//! "Install and login run in a visible pane"; the `y` prompt is the menu's,
//! S4), and the probe's replies reach the app.

use super::*;
use crate::core::ai::mcp_catalog::{
    checks::{Probed, Verdict},
    entry,
};

const LINE: &str = "gcloud auth application-default login --client-id-file=~/x.json";

fn spawned(effects: &[Effect]) -> Vec<(PaneId, SpawnSpec)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SpawnPane(id, spec) => Some((*id, spec.clone())),
            _ => None,
        })
        .collect()
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

fn output(id: PaneId) -> AppEvent {
    AppEvent::Pty(id, PtyEvent::Output(b"$ ".to_vec()))
}

/// An app with the menu open and the connector line asked for.
fn asked() -> (App, PaneId, Vec<Effect>) {
    let mut a =
        launched(80, 25).with_env(Some("/bin/zsh"), Some("/home/ana".into()), LAUNCH.into());
    a.update(ctrl_space());
    a.update(key('m'));
    assert!(a.menu().is_some());
    let effects = a.on_menu_command(MenuCommand::Connector(LINE.to_owned()));
    let (id, _) = spawned(&effects).remove(0);
    (a, id, effects)
}

#[test]
fn one_new_shell_pane_is_created_and_nothing_is_typed_yet() {
    let (a, _, effects) = asked();
    assert_eq!(spawned(&effects).len(), 1, "{effects:?}");
    assert_eq!(writes(&effects), []);
    assert!(a.menu().is_none(), "the menu closes");
}

#[test]
fn the_pane_runs_a_shell_in_home_and_lazarobox_never_sets_browser() {
    let (_, _, effects) = asked();
    let (_, spec) = spawned(&effects).remove(0);
    assert_eq!(spec.cwd, PathBuf::from("/home/ana"));
    assert_eq!(
        spec.program,
        SpawnSpec::new(Some("/bin/zsh"), "/".into(), spec.size).program
    );
    let touched = |name: &str| {
        spec.env.iter().any(|(k, _)| k == name) || spec.env_os.iter().any(|(k, _)| k == name)
    };
    assert!(!touched("BROWSER"), "{spec:?}");
}

#[test]
fn the_line_and_enter_are_typed_once_on_the_first_output() {
    let (mut a, id, _) = asked();
    let typed = a.update(output(id));
    assert_eq!(writes(&typed), [(id, format!("{LINE}\r").into_bytes())]);
    assert_eq!(writes(&a.update(output(id))), [], "only once");
}

#[test]
fn with_no_output_the_line_is_typed_after_two_seconds() {
    let (mut a, id, _) = asked();
    let base = NOW_MS.with(|n| n.get());
    assert!(a.probe_waiting(), "the runtime ticks while a line waits");
    assert_eq!(writes(&at(&mut a, base + 1_000, AppEvent::ProbeTick)), []);
    let typed = at(&mut a, base + 3_000, AppEvent::ProbeTick);
    assert_eq!(writes(&typed), [(id, format!("{LINE}\r").into_bytes())]);
    assert!(!a.probe_waiting());
    assert_eq!(writes(&a.update(output(id))), [], "only once");
}

#[test]
fn a_pane_closed_before_it_spoke_types_nothing() {
    let (mut a, id, _) = asked();
    a.update(AppEvent::Pty(id, PtyEvent::Exited));
    let base = NOW_MS.with(|n| n.get());
    assert_eq!(writes(&at(&mut a, base + 5_000, AppEvent::ProbeTick)), []);
    assert!(!a.probe_waiting());
}

#[test]
fn no_pane_and_no_write_exist_without_the_command() {
    let mut a = launched(80, 25);
    let panes = a.panes.len();
    let effects = a.update(output(PaneId::FIRST));
    assert_eq!((spawned(&effects).len(), writes(&effects)), (0, vec![]));
    assert_eq!(a.panes.len(), panes);
}

#[test]
fn a_line_with_a_control_or_format_character_is_refused_with_a_notice() {
    for line in ["ls\nrm x", "ls\u{1b}[2J", "ls \u{202e}xr mr"] {
        let mut a = launched(80, 25);
        let panes = a.panes.len();
        let effects = a.on_menu_command(MenuCommand::Connector(line.to_owned()));
        assert_eq!(effects, [], "{line:?}");
        assert_eq!(a.panes.len(), panes);
        assert!(a.notice.is_some());
    }
}

#[test]
fn every_catalog_link_goes_to_the_existing_opener() {
    let mut a = launched(80, 25);
    let links = entry("ga4").unwrap().links;
    assert!(!links.is_empty());
    for link in links {
        let effects = a.on_menu_command(MenuCommand::Open((*link).to_owned()));
        assert_eq!(effects, [Effect::OpenUrl((*link).to_owned())]);
    }
}

#[test]
fn a_probe_command_becomes_an_effect_and_esc_a_cancel() {
    let mut a = launched(80, 25);
    let server = crate::core::ai::config::McpServer {
        name: "clarity".to_owned(),
        command: "clarity-mcp-server".to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled: true,
    };
    let effects = a.on_menu_command(MenuCommand::Probe(Box::new(server.clone())));
    assert_eq!(effects, [Effect::McpProbe(1, Box::new(server))]);
    assert_eq!(
        a.on_menu_command(MenuCommand::CancelProbe),
        [Effect::CancelMcpProbe]
    );
}

#[test]
fn a_probe_reply_is_kept_for_the_session_and_redraws() {
    let mut a = launched(80, 25);
    ask_probe(&mut a);
    a.take_dirty();
    let report = report(Verdict::Started);
    assert_eq!(a.update(AppEvent::McpProbe(1, report.clone())), []);
    assert_eq!(a.mcp_probe("clarity"), Some(&report));
    assert_eq!(a.mcp_probe("ga4"), None);
    assert!(a.take_dirty());
}

fn report(verdict: Verdict) -> Probed {
    Probed {
        server: "clarity".to_owned(),
        verdict,
        lines: vec![format!("{verdict:?}")],
        problems: Vec::new(),
    }
}

fn ask_probe(a: &mut App) -> Vec<Effect> {
    let server = crate::core::ai::config::McpServer {
        name: "clarity".to_owned(),
        command: "clarity-mcp-server".to_owned(),
        args: Vec::new(),
        env: Vec::new(),
        project: None,
        enabled: true,
    };
    a.on_menu_command(MenuCommand::Probe(Box::new(server)))
}

#[test]
fn a_late_report_of_a_superseded_probe_does_not_overwrite_the_newer_one() {
    let mut a = launched(80, 25);
    ask_probe(&mut a);
    let second = ask_probe(&mut a);
    assert!(matches!(second[0], Effect::McpProbe(2, _)), "{second:?}");
    a.update(AppEvent::McpProbe(2, report(Verdict::Started)));
    a.update(AppEvent::McpProbe(1, report(Verdict::Stopped)));
    assert_eq!(a.mcp_probe("clarity"), Some(&report(Verdict::Started)));
}

#[test]
fn a_report_nobody_asked_for_is_dropped() {
    let mut a = launched(80, 25);
    a.update(AppEvent::McpProbe(7, report(Verdict::Started)));
    assert_eq!(a.mcp_probe("clarity"), None);
}
