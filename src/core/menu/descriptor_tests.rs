//! Tests of the draft row descriptors (`ItemKind::Draft`): one declaration
//! per row, round trips, unique write ranks and the optional fields.

use super::*;

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

fn flag(choice: &AiChoice) -> &Choice<AiConfig> {
    let DraftValue::Flag(flag) = &choice.value;
    flag
}

#[test]
fn the_migrated_rows_are_drafts_in_their_tab_order() {
    let labels: Vec<_> = drafts().iter().map(|(item, _)| item.label).collect();
    assert_eq!(
        labels,
        [
            "Sound",
            "Tools",
            "Search",
            "Write",
            "Exec",
            "Git",
            "Git write",
            "Context"
        ]
    );
}

#[test]
fn every_descriptor_reads_back_what_it_wrote() {
    for (item, choice) in drafts() {
        let flag = flag(choice);
        assert!(!flag.values.is_empty(), "{}", item.label);
        for i in 0..flag.values.len() {
            let mut draft = AiConfig::default();
            (flag.set)(&mut draft, i);
            assert_eq!((flag.get)(&draft), i, "{}", item.label);
        }
    }
}

#[test]
fn every_flag_is_off_then_on_with_a_key_path() {
    for (item, choice) in drafts() {
        let flag = flag(choice);
        assert_eq!(flag.values, ["off", "on"], "{}", item.label);
        assert!(
            !flag.table.is_empty() && !flag.key.is_empty(),
            "{}",
            item.label
        );
    }
}

#[test]
fn the_key_paths_are_the_ones_the_config_has_always_used() {
    let paths: Vec<_> = drafts()
        .iter()
        .map(|(_, c)| (flag(c).table, flag(c).key))
        .collect();
    assert_eq!(
        paths,
        [
            ("ai", "notify_sound"),
            ("ai.tools", "enabled"),
            ("ai.tools", "search"),
            ("ai.tools", "write"),
            ("ai.tools", "exec"),
            ("ai.tools", "git"),
            ("ai.tools", "git_write"),
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
    for (item, choice) in drafts() {
        let risky = matches!(item.label, "Write" | "Exec" | "Git write");
        assert_eq!(choice.caution, risky, "{}", item.label);
        assert_eq!(
            choice.badge == Some(Badge::NeedsSandbox),
            item.label == "Exec",
            "{}",
            item.label
        );
        assert_eq!(choice.test, item.label == "Sound", "{}", item.label);
    }
}

#[test]
fn a_draft_row_needs_the_ai_section() {
    for (item, _) in drafts() {
        assert!(item.kind.needs_ai(), "{}", item.label);
    }
    assert!(ItemKind::Assign(None).needs_ai());
    assert!(ItemKind::HfAccount.needs_ai());
    assert!(!ItemKind::Feedback.needs_ai());
}

#[test]
fn each_family_row_uses_the_key_of_its_toggle_in_row_order() {
    let families: Vec<_> = drafts()
        .into_iter()
        .filter(|(_, c)| flag(c).table == TOOLS_TABLE && flag(c).key != "enabled")
        .collect();
    assert_eq!(families.len(), ToolToggle::ALL.len());
    for ((item, choice), toggle) in families.iter().zip(ToolToggle::ALL) {
        assert_eq!(flag(choice).key, toggle.key(), "{}", item.label);
    }
}

#[test]
fn flipping_each_family_writes_its_own_key_and_nothing_else() {
    for toggle in ToolToggle::ALL {
        let mut m = MenuState::open(&Config::default()).with_ai(AiConfig::default(), None);
        let (item, _) = drafts()
            .into_iter()
            .find(|(_, c)| flag(c).key == toggle.key())
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
/// them: profile, memory, notify, approval, orchestrator, idle.
const LITERAL_RANKS: [u8; 6] = [10, 30, 40, 70, 80, 90];

#[test]
fn draft_ranks_never_collide_with_the_literal_ranks() {
    let mut all: Vec<u8> = drafts().iter().map(|(_, c)| c.rank).collect();
    all.extend(LITERAL_RANKS);
    let count = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), count, "{all:?}");
}
