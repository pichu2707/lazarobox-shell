//! The first-run prefix probe through `App::update` (terminal-agnostic S2b).

use super::*;
use crate::core::probe::{CHECK_NOTICE, OFFER_NOTICE, Outcome as Probed};

const KITTY: &str = "kitty";

fn probing(stored: Option<Probed>, rerun: bool) -> App {
    app().with_probe(KITTY.into(), Ok(stored), rerun)
}

fn save(outcome: Probed) -> Effect {
    Effect::SaveProbe {
        key: KITTY.into(),
        outcome,
    }
}

fn no_save(effects: &[Effect]) -> bool {
    !effects
        .iter()
        .any(|e| matches!(e, Effect::SaveProbe { .. }))
}

#[test]
fn a_first_run_says_it_is_checking_and_waits() {
    let a = probing(None, false);
    assert_eq!(a.notice(), Some(CHECK_NOTICE));
    assert!(a.probe_waiting());
}

// Spec: "Ctrl+Space arrives".
#[test]
fn ctrl_space_passes_and_enters_prefix_with_no_offer() {
    let mut a = probing(None, false);
    assert_eq!(a.update(ctrl_space()), vec![save(Probed::Pass)]);
    assert_eq!(a.input, InputMode::Prefix);
    assert_eq!(a.notice(), None);
    assert!(!a.probe_waiting());
}

// Spec: "Probe never traps the user", Ctrl+] records `alias`, no offer (D8).
#[test]
fn the_alias_is_recorded_enters_prefix_and_offers_nothing() {
    for alias in alias_keys() {
        let mut a = probing(None, false);
        assert_eq!(a.update(alias), vec![save(Probed::Alias)]);
        assert_eq!(a.input, InputMode::Prefix);
        assert_eq!(a.notice(), None);
        assert!(!a.probe_waiting());
    }
}

// Spec: "Another key arrives": the key still reaches the pane.
#[test]
fn a_space_or_a_control_key_is_missed_offers_the_alias_and_still_types() {
    let mut a = probing(None, false);
    assert_eq!(
        a.update(key(' ')),
        vec![
            save(Probed::Missed),
            Effect::WritePty(PaneId::FIRST, b" ".to_vec())
        ]
    );
    assert_eq!(a.notice(), Some(OFFER_NOTICE));
    assert!(!a.probe_waiting());
    let mut b = probing(None, false);
    assert_eq!(b.update(esc()).first(), Some(&save(Probed::Missed)));
    assert_eq!(b.notice(), Some(OFFER_NOTICE));
}

// Spec: "Nothing arrives": nothing stored, no offer, probe again next start.
#[test]
fn ten_quiet_seconds_end_the_probe_with_nothing_stored_and_no_offer() {
    let mut a = probing(None, false);
    // The first tick opens the window.
    assert_eq!(at(&mut a, 0, AppEvent::ProbeTick), vec![]);
    assert_eq!(at(&mut a, 9_999, AppEvent::ProbeTick), vec![]);
    assert!(a.probe_waiting());
    assert_eq!(a.notice(), Some(CHECK_NOTICE));
    assert_eq!(at(&mut a, 10_000, AppEvent::ProbeTick), vec![]);
    assert!(!a.probe_waiting());
    assert_eq!(a.notice(), None);
    // The probe is over: a later key stores nothing.
    assert!(no_save(&a.update(esc())));
}

// Only the check text goes away; a notice joined to it stays.
#[test]
fn the_end_of_a_rerun_keeps_the_notice_it_joined() {
    let mut a = app().with_notice(Some("config: bad".into())).with_probe(
        KITTY.into(),
        Ok(Some(Probed::Pass)),
        true,
    );
    at(&mut a, 0, AppEvent::ProbeTick);
    at(&mut a, 10_000, AppEvent::ProbeTick);
    assert_eq!(a.notice(), Some("config: bad"));
}

// User decision: typing does not end the probe, and the check stays in view.
#[test]
fn typing_keeps_the_probe_waiting_and_the_key_goes_to_the_pane() {
    let mut a = probing(None, false);
    assert_eq!(
        a.update(key('x')),
        vec![Effect::WritePty(PaneId::FIRST, b"x".to_vec())]
    );
    assert!(a.probe_waiting());
    assert_eq!(a.notice(), Some(CHECK_NOTICE));
    // A later deciding key still counts.
    assert_eq!(a.update(ctrl_space()), vec![save(Probed::Pass)]);
}

// The probe window never opens before the loop runs.
#[test]
fn the_window_does_not_start_before_the_first_tick() {
    let mut a = probing(None, false);
    assert_eq!(at(&mut a, 60_000, AppEvent::ProbeTick), vec![]);
    assert!(a.probe_waiting());
    assert_eq!(at(&mut a, 69_999, AppEvent::ProbeTick), vec![]);
    assert!(a.probe_waiting());
}

/// Every place a key can land, built the way a user gets there.
type Build = fn() -> App;

fn modes() -> Vec<(&'static str, Build)> {
    vec![
        ("Terminal", || app()),
        ("Terminal", || {
            let mut a = app();
            a.open_chat(None);
            a
        }),
        ("Prefix", in_prefix),
        ("Group", || in_group('w')),
        ("Copy", in_copy),
        ("Resize", resizing),
        ("Menu", || {
            let mut a = app().with_tool_engines(&[]);
            press(&mut a, "m");
            a
        }),
        ("Confirm", || {
            let mut a = app();
            press(&mut a, "q");
            a
        }),
    ]
}

// The probe only watches: in every mode the key does what it always did,
// plus the one SaveProbe.
#[test]
fn the_probe_never_swallows_a_key_in_any_mode() {
    let [bracket, five] = alias_keys();
    let keys = [
        (esc(), Probed::Missed),
        (key(' '), Probed::Missed),
        (ctrl_space(), Probed::Pass),
        (bracket, Probed::Alias),
        (five, Probed::Alias),
    ];
    for (name, build) in modes() {
        for (event, outcome) in &keys {
            let mut control = build();
            assert!(
                format!("{:?}", control.input).starts_with(name),
                "{name}: {:?}",
                control.input
            );
            let normal = control.update(event.clone());
            let mut watched = build().with_probe(KITTY.into(), Ok(None), true);
            assert!(watched.probe_waiting(), "{name}");
            let mut expected = vec![save(*outcome)];
            expected.extend(normal);
            assert_eq!(watched.update(event.clone()), expected, "{name} {event:?}");
            assert_eq!(watched.input, control.input, "{name} {event:?}");
        }
    }
}

#[test]
fn a_release_does_not_end_the_probe() {
    let mut a = probing(None, false);
    let release = KeyEvent::new_with_kind(
        KeyCode::Char(' '),
        KeyModifiers::CONTROL,
        KeyEventKind::Release,
    );
    assert_eq!(a.update(AppEvent::Key(release)), vec![]);
    assert!(a.probe_waiting());
}

// Spec: "Probe does not repeat" and "Alias always works".
#[test]
fn a_stored_result_means_no_probe_and_the_alias_still_works() {
    for stored in [Probed::Pass, Probed::Alias, Probed::Missed] {
        let mut a = probing(Some(stored), false);
        assert_eq!(a.notice(), None);
        assert!(!a.probe_waiting());
        assert_eq!(
            at(&mut a, 60_000, AppEvent::ProbeTick),
            vec![],
            "{stored:?}"
        );
        let [bracket, _] = alias_keys();
        assert!(no_save(&a.update(bracket)));
        assert_eq!(a.input, InputMode::Prefix);
        let mut b = probing(Some(stored), false);
        assert_eq!(
            b.update(key('x')),
            vec![Effect::WritePty(PaneId::FIRST, b"x".to_vec())]
        );
    }
}

// D13: a startup notice wins, the probe waits for the next start.
#[test]
fn a_startup_notice_defers_the_probe_and_stores_nothing() {
    let mut a =
        app()
            .with_notice(Some("config: bad".into()))
            .with_probe(KITTY.into(), Ok(None), false);
    assert_eq!(a.notice(), Some("config: bad"));
    assert!(!a.probe_waiting());
    assert_eq!(at(&mut a, 30_000, AppEvent::ProbeTick), vec![]);
    assert!(no_save(&a.update(ctrl_space())));
}

// Spec: "Rerun on request".
#[test]
fn a_rerun_says_so_and_probes_over_a_stored_result() {
    let mut a = probing(Some(Probed::Pass), true);
    assert_eq!(a.notice(), Some(CHECK_NOTICE));
    assert!(a.probe_waiting());
    assert_eq!(a.update(esc()).first(), Some(&save(Probed::Missed)));
}

#[test]
fn a_rerun_keeps_the_startup_notice_in_view() {
    let a = app().with_notice(Some("config: bad".into())).with_probe(
        KITTY.into(),
        Ok(Some(Probed::Pass)),
        true,
    );
    assert_eq!(
        a.notice(),
        Some("config: bad; Checking the prefix key: press Ctrl+Space")
    );
    assert!(a.probe_waiting());
}

// D12: a stored miss heals to a pass when Ctrl+Space arrives later.
#[test]
fn a_stored_miss_heals_once_and_a_stored_pass_never_saves() {
    let mut a = probing(Some(Probed::Missed), false);
    assert_eq!(a.update(ctrl_space()), vec![save(Probed::Pass)]);
    a.update(esc());
    assert!(no_save(&a.update(ctrl_space())));
    let mut b = probing(Some(Probed::Pass), false);
    assert!(no_save(&b.update(ctrl_space())));
}

// Spec: "Storage unavailable".
#[test]
fn a_store_that_cannot_be_read_shows_a_notice_and_runs_no_probe() {
    let a = app().with_probe(
        KITTY.into(),
        Err("/s/terminals.json: cannot be read".into()),
        false,
    );
    assert_eq!(
        a.notice(),
        Some(
            "Could not read the prefix check result: /s/terminals.json: cannot be read; delete the file to check again"
        )
    );
    assert!(!a.probe_waiting());
    let b = app().with_notice(Some("config: bad".into())).with_probe(
        KITTY.into(),
        Err("x".into()),
        true,
    );
    assert_eq!(
        b.notice(),
        Some(
            "config: bad; Could not read the prefix check result: x; delete the file to check again"
        )
    );
}

#[test]
fn a_store_that_cannot_be_written_shows_a_notice() {
    let mut a = probing(None, false);
    a.update(ctrl_space());
    a.update(AppEvent::ProbeSaveFailed(
        "/s/terminals.json: cannot be written: denied".into(),
    ));
    assert_eq!(
        a.notice(),
        Some(
            "Could not keep the prefix check result: /s/terminals.json: cannot be written: denied"
        )
    );
    assert!(a.dirty);
    // A notice already showing is kept, not dropped.
    let mut b =
        app()
            .with_notice(Some("config: bad".into()))
            .with_probe(KITTY.into(), Ok(None), false);
    b.update(AppEvent::ProbeSaveFailed("x".into()));
    assert_eq!(
        b.notice(),
        Some("config: bad; Could not keep the prefix check result: x")
    );
}
