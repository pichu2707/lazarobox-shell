//! Tests of the `llama-server idle` row of the AI tab (spec S8 of
//! managed-llama-server-followups): the idle stop of the managed server.

use super::ai_tests::{machine, press, selected_label};
use super::*;
use crate::core::config_edit::apply_edit;

const ROW: &str = "llama-server idle";

fn on_idle(minutes: Option<u32>) -> MenuState {
    let mut ai = AiConfig::default();
    if let Some(minutes) = minutes {
        ai.set_idle_minutes(minutes);
    }
    let mut m = MenuState::open(&Config::default()).with_ai(ai, Some(machine()));
    m.goto(ROW);
    m
}

fn value(menu: &MenuState) -> String {
    menu.all_rows(&Config::default())
        .into_iter()
        .find_map(|row| match row {
            Row::Item {
                label,
                value: Some(value),
                ..
            } if label == ROW => Some(value.into_owned()),
            _ => None,
        })
        .expect("the row has a value")
}

fn idle_edit(value: &str) -> ConfigEdit {
    ConfigEdit {
        table: "ai.servers.llamacpp",
        key: "idle_minutes",
        value: value.to_owned().into(),
    }
}

// Spec S8: the row sits in Settings after Sound (the other chat settings),
// so the popup keeps its height, and shows 15 min by default.
#[test]
fn the_row_follows_sound_and_shows_the_default() {
    let labels: Vec<&str> = SECTIONS[0].items.iter().map(|i| i.label).collect();
    let at = labels
        .iter()
        .position(|l| *l == ROW)
        .expect("the row exists");
    assert_eq!((labels[at - 1], labels[at + 1]), ("Sound", "Updates"));
    let m = on_idle(None);
    assert_eq!(selected_label(&m), ROW);
    assert_eq!(value(&m), "15 min");
}

// Spec S8: "quizás alguien necesite 30 u otra persona 5 minutos o quien no
// quiere que se deconecte".
#[test]
fn l_and_h_step_through_5_15_30_and_never() {
    let mut m = on_idle(None);
    let mut seen = Vec::new();
    for _ in 0..4 {
        press(&mut m, 'l');
        seen.push(value(&m));
    }
    assert_eq!(seen, ["30 min", "never", "5 min", "15 min"]);
    press(&mut m, 'h');
    assert_eq!(value(&m), "5 min");
}

#[test]
fn a_value_of_the_config_is_shown_and_steps_on() {
    let mut m = on_idle(Some(45));
    assert_eq!(value(&m), "45 min");
    press(&mut m, 'l');
    assert_eq!(value(&m), "never");
    let mut m = on_idle(Some(0));
    assert_eq!(value(&m), "never");
    press(&mut m, 'l');
    assert_eq!(value(&m), "5 min");
}

#[test]
fn a_change_saves_only_its_key_as_a_number() {
    let mut m = on_idle(None);
    assert_eq!(m.edits(&Config::default()), vec![]);
    press(&mut m, 'l');
    let edits = m.edits(&Config::default());
    assert_eq!(edits, vec![idle_edit("30")]);
    let before = "[ai.servers.llamacpp]\nport = 39080 # mine\n";
    let text = apply_edit(Some(before), &edits).unwrap();
    assert!(text.contains("idle_minutes = 30\n"), "{text}");
    assert!(text.contains("port = 39080 # mine"), "{text}");
    let table: toml::Table = text.parse().unwrap();
    let mut problems = Vec::new();
    let ai = crate::core::ai::config::read_with_port_env(&table, &mut problems, None);
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(ai.managed_options().idle_minutes, 30);
    // Back to where it was: nothing to save.
    for _ in 0..3 {
        press(&mut m, 'l');
    }
    assert_eq!(value(&m), "15 min");
    assert_eq!(m.edits(&Config::default()), vec![]);
}
