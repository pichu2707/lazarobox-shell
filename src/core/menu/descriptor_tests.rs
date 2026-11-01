//! Tests of the draft row descriptors (`ItemKind::Draft`): one declaration
//! per row, round trips, unique write ranks and the optional fields.

use super::*;
use crate::core::ai::config::{ApprovalMode, NotifyMode, Profile};

fn drafts() -> Vec<(&'static Item, &'static AiChoice)> {
    SECTIONS
        .iter()
        .chain([&MODELS_SECTION, &TOOLS_SECTION, &CHAT_SECTION])
        .flat_map(|section| section.items.iter())
        .filter_map(|item| match &item.kind {
            ItemKind::Draft(choice) => Some((item, choice)),
            _ => None,
        })
        .collect()
}

/// The cycling list of a flag or list row.
fn choice(choice: &AiChoice) -> Option<&Choice<AiConfig>> {
    match &choice.value {
        DraftValue::Flag(c) | DraftValue::List(c) => Some(c),
        DraftValue::Minutes { .. } => None,
    }
}

fn row(label: &str) -> &'static AiChoice {
    drafts()
        .into_iter()
        .find(|(item, _)| item.label == label)
        .map(|(_, c)| c)
        .unwrap_or_else(|| panic!("no draft row {label}"))
}

#[test]
fn the_migrated_rows_are_drafts_in_tab_then_row_order() {
    let labels: Vec<_> = drafts().iter().map(|(item, _)| item.label).collect();
    assert_eq!(
        labels,
        [
            "Notify",
            "Sound",
            "llama-server idle",
            "Tools",
            "Search",
            "Write",
            "Exec",
            "Git",
            "Git write",
            "Approval",
            "Profile",
            "Context"
        ]
    );
}

#[test]
fn every_cycling_descriptor_reads_back_what_it_wrote() {
    for (item, c) in drafts() {
        let Some(list) = choice(c) else { continue };
        assert!(!list.values.is_empty(), "{}", item.label);
        for i in 0..list.values.len() {
            let mut draft = AiConfig::default();
            (list.set)(&mut draft, i);
            assert_eq!((list.get)(&draft), i, "{}", item.label);
        }
    }
}

#[test]
fn a_flag_is_off_then_on_and_every_row_has_a_key_path() {
    for (item, c) in drafts() {
        assert!(
            !c.table().is_empty() && !c.key().is_empty(),
            "{}",
            item.label
        );
        if let DraftValue::Flag(flag) = &c.value {
            assert_eq!(flag.values, ["off", "on"], "{}", item.label);
        }
    }
}

#[test]
fn the_list_values_are_the_ids_of_their_enums() {
    let ids = |values: &[&str], ids: Vec<&str>| assert_eq!(values, ids.as_slice());
    let values = |label| choice(row(label)).unwrap().values;
    ids(
        values("Profile"),
        Profile::ALL.iter().map(|p| p.id()).collect(),
    );
    ids(
        values("Notify"),
        NotifyMode::ALL.iter().map(|m| m.id()).collect(),
    );
    ids(
        values("Approval"),
        ApprovalMode::ALL.iter().map(|m| m.id()).collect(),
    );
}

#[test]
fn the_key_paths_are_the_ones_the_config_has_always_used() {
    let paths: Vec<_> = drafts().iter().map(|(_, c)| (c.table(), c.key())).collect();
    assert_eq!(
        paths,
        [
            ("ai", "notify"),
            ("ai", "notify_sound"),
            ("ai.servers.llamacpp", "idle_minutes"),
            ("ai.tools", "enabled"),
            ("ai.tools", "search"),
            ("ai.tools", "write"),
            ("ai.tools", "exec"),
            ("ai.tools", "git"),
            ("ai.tools", "git_write"),
            ("ai.tools", "approval"),
            ("ai", "profile"),
            ("ai", "context"),
        ]
    );
    let mut unique = paths.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), paths.len(), "no duplicate key path");
}

#[test]
fn the_ranks_are_unique() {
    let mut ranks: Vec<u8> = drafts().iter().map(|(_, c)| c.rank).collect();
    ranks.sort_unstable();
    let count = ranks.len();
    ranks.dedup();
    assert_eq!(ranks.len(), count);
}

#[test]
fn the_optional_fields_sit_on_the_rows_that_had_them() {
    for (item, c) in drafts() {
        let risky = matches!(item.label, "Write" | "Exec" | "Git write");
        assert_eq!(c.caution, risky, "{}", item.label);
        assert_eq!(
            c.badge == Some(Badge::NeedsSandbox),
            item.label == "Exec",
            "{}",
            item.label
        );
        assert_eq!(
            c.test,
            matches!(item.label, "Notify" | "Sound"),
            "{}",
            item.label
        );
    }
}

#[test]
fn every_ai_only_kind_needs_the_ai_section() {
    for (item, _) in drafts() {
        assert!(item.kind.needs_ai(), "{}", item.label);
    }
    for kind in [
        &ItemKind::Assign(None),
        &ItemKind::HfAccount,
        &ItemKind::Orchestrator,
    ] {
        assert!(kind.needs_ai());
    }
    assert!(!ItemKind::Feedback.needs_ai());
    assert!(!ItemKind::Updates.needs_ai());
    let memory = CHAT_SECTION.items.iter().find(|i| i.label == "Memory");
    assert!(memory.is_some_and(|i| i.kind.needs_ai()));
}

// ---- list and minutes rows --------------------------------------------------

fn shown(label: &str, ai: &AiConfig) -> String {
    row(label).value.shown(ai).into_owned()
}

#[test]
fn a_list_row_cycles_both_ways_and_wraps() {
    for (label, first, second, last) in [
        ("Profile", "analyst", "programmer", "programmer"),
        ("Notify", "auto", "desktop", "off"),
        ("Approval", "ask", "auto", "run"),
    ] {
        let mut ai = AiConfig::default();
        let list = choice(row(label)).unwrap();
        (list.set)(&mut ai, 0);
        assert_eq!(shown(label, &ai), first);
        row(label).value.step(&mut ai, true);
        assert_eq!(shown(label, &ai), second, "{label}");
        (list.set)(&mut ai, 0);
        row(label).value.step(&mut ai, false);
        assert_eq!(shown(label, &ai), last, "{label} wraps back");
    }
}

#[test]
fn the_idle_row_steps_from_an_off_list_value_both_ways() {
    let mut ai = AiConfig::default();
    ai.set_idle_minutes(45);
    assert_eq!(shown("llama-server idle", &ai), "45 min");
    let idle = &row("llama-server idle").value;
    idle.step(&mut ai, false);
    assert_eq!(shown("llama-server idle", &ai), "30 min");
    ai.set_idle_minutes(45);
    idle.step(&mut ai, true);
    assert_eq!(shown("llama-server idle", &ai), "never");
    idle.step(&mut ai, true);
    assert_eq!(shown("llama-server idle", &ai), "5 min");
}

#[test]
fn the_idle_row_writes_the_integer() {
    let mut was = AiConfig::default();
    was.set_idle_minutes(45);
    let mut now = was.clone();
    row("llama-server idle").value.step(&mut now, true);
    assert_eq!(
        row("llama-server idle")
            .value
            .written(&now, &was)
            .as_deref(),
        Some("0")
    );
    assert_eq!(row("llama-server idle").value.written(&was, &was), None);
}

#[test]
fn each_family_row_uses_the_key_of_its_toggle_in_row_order() {
    let families: Vec<_> = drafts()
        .into_iter()
        .filter(|(_, c)| c.table() == TOOLS_TABLE && !matches!(c.key(), "enabled" | "approval"))
        .collect();
    assert_eq!(families.len(), ToolToggle::ALL.len());
    for ((item, choice), toggle) in families.iter().zip(ToolToggle::ALL) {
        assert_eq!(choice.key(), toggle.key(), "{}", item.label);
    }
}

#[test]
fn flipping_each_family_writes_its_own_key_and_nothing_else() {
    for toggle in ToolToggle::ALL {
        let mut m = MenuState::open(&Config::default()).with_ai(AiConfig::default(), None);
        let (item, _) = drafts()
            .into_iter()
            .find(|(_, c)| c.key() == toggle.key())
            .expect("a row per toggle");
        m.goto(item.label);
        m.on_key(
            &KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
            false,
            &mut Config::default(),
        );
        let now = !AiConfig::default().toggle(toggle);
        assert_eq!(
            m.edits(&Config::default()),
            vec![ConfigEdit::toggle(toggle, now)],
            "{}",
            item.label
        );
    }
}

/// The ranks of the rows that are not descriptors yet, as `edits()` writes
/// them: memory and the orchestrator gate.
const LITERAL_RANKS: [u8; 2] = [MEMORY_RANK, ORCHESTRATOR_RANK];

#[test]
fn draft_ranks_never_collide_with_the_literal_ranks() {
    let mut all: Vec<u8> = drafts().iter().map(|(_, c)| c.rank).collect();
    all.extend(LITERAL_RANKS);
    let count = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), count, "{all:?}");
}

#[test]
fn a_list_row_changed_and_changed_back_writes_nothing() {
    for label in ["Notify", "Profile", "Approval"] {
        let mut m = MenuState::open(&Config::default()).with_ai(AiConfig::default(), None);
        let press = |m: &mut MenuState, c: char| {
            m.on_key(
                &KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                false,
                &mut Config::default(),
            );
        };
        m.goto(label);
        press(&mut m, 'l');
        assert!(!m.edits(&Config::default()).is_empty(), "{label} changed");
        press(&mut m, 'h');
        assert_eq!(m.edits(&Config::default()), vec![], "{label} back");
    }
}

#[test]
fn the_idle_row_stepped_away_and_back_from_an_on_list_value_writes_nothing() {
    let mut m = MenuState::open(&Config::default()).with_ai(AiConfig::default(), None);
    m.goto("llama-server idle");
    for c in ['l', 'h'] {
        m.on_key(
            &KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            false,
            &mut Config::default(),
        );
    }
    assert_eq!(m.edits(&Config::default()), vec![]);
}

#[test]
fn the_sandbox_badge_follows_the_flag_value_not_its_text() {
    let mut ai = AiConfig::default();
    ai.set_toggle(ToolToggle::Exec, true);
    let m = MenuState::open(&Config::default())
        .with_ai(ai, None)
        .with_sandbox(false);
    let shown = m.all_rows(&Config::default());
    let value = |label: &str| {
        shown.iter().find_map(|r| match r {
            Row::Item {
                label: l, value, ..
            } if *l == label => value.clone(),
            _ => None,
        })
    };
    assert_eq!(value("Exec").as_deref(), Some("on · needs Linux sandbox"));
    assert_eq!(value("Git").as_deref(), Some("off"));
}
