//! Tests of the `Build jobs` row at the `MenuState` level.

use std::num::NonZeroU16;

use super::ai_tests::{machine, menu, selected_label};
use super::*;
use crate::core::build_jobs::{BuildJobs, Machine};

const GIB: u64 = 1024 * 1024 * 1024;

/// 16 cores and 7.5 GiB: `auto` gives 3.
fn wsl() -> Machine {
    Machine::new(16, Some(7 * GIB + GIB / 2))
}

fn fixed(n: u16) -> BuildJobs {
    BuildJobs::Fixed(NonZeroU16::new(n).unwrap())
}

fn config(jobs: BuildJobs) -> Config {
    Config {
        build_jobs: jobs,
        ..Config::default()
    }
}

fn jobs_edit(value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table: "build",
        key: "jobs",
        value: value.into(),
    }
}

fn key(menu: &mut MenuState, key: char, live: &mut Config) -> MenuCommand {
    let event = KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE);
    menu.on_key(&event, false, live)
}

fn jobs_row(menu: &MenuState, live: &Config) -> Row {
    menu.rows(live)
        .into_iter()
        .find(|row| {
            matches!(
                row,
                Row::Item {
                    label: "Build jobs",
                    ..
                }
            )
        })
        .expect("the Build jobs row")
}

#[test]
fn the_row_comes_after_mouse_without_the_ai_section() {
    let mut m = MenuState::open(&Config::default());
    let mut live = Config::default();
    for expected in ["Tab bar position", "Mouse", "Build jobs", "Updates"] {
        key(&mut m, 'j', &mut live);
        assert_eq!(selected_label(&m), expected);
    }
}

#[test]
fn the_row_shows_auto_with_the_count_it_gives_here() {
    let m = MenuState::open(&Config::default()).with_build_machine(wsl());
    assert_eq!(
        jobs_row(&m, &Config::default()),
        Row::Item {
            label: "Build jobs",
            value: Some("auto (3)".into()),
            selected: false,
            enabled: true,
        }
    );
    let live = config(fixed(16));
    let Row::Item { value, .. } = jobs_row(&m, &live) else {
        unreachable!()
    };
    assert_eq!(value.as_deref(), Some("16"));
}

#[test]
fn right_counts_up_from_one_and_left_from_auto_is_the_most_jobs() {
    let mut m = menu(None, None, Some(machine())).with_build_machine(wsl());
    m.goto("Build jobs");
    let mut live = Config::default();
    assert_eq!(key(&mut m, 'l', &mut live), MenuCommand::Changed);
    assert_eq!(live.build_jobs, fixed(1));
    key(&mut m, 'l', &mut live);
    assert_eq!(live.build_jobs, fixed(2));
    key(&mut m, 'h', &mut live);
    key(&mut m, 'h', &mut live);
    assert_eq!(live.build_jobs, BuildJobs::Auto);
    assert_eq!(key(&mut m, 'h', &mut live), MenuCommand::Changed);
    assert_eq!(
        live.build_jobs,
        fixed(16),
        "left from auto is all the cores"
    );
    key(&mut m, 'l', &mut live);
    assert_eq!(live.build_jobs, BuildJobs::Auto, "past the cores is auto");
    assert_eq!(selected_label(&m), "Build jobs");
    assert_eq!(live.mouse, Config::default().mouse, "no other key moved");
}

#[test]
fn edits_write_the_count_or_auto_only_when_it_changed() {
    let m = MenuState::open(&Config::default());
    assert_eq!(m.edits(&Config::default()), vec![]);
    assert_eq!(m.edits(&config(fixed(6))), vec![jobs_edit("6")]);
    let m = MenuState::open(&config(fixed(6)));
    assert_eq!(m.edits(&config(fixed(6))), vec![]);
    assert_eq!(m.edits(&config(BuildJobs::Auto)), vec![jobs_edit("auto")]);
}

#[test]
fn the_build_jobs_edit_saves_with_the_other_settings() {
    let m = MenuState::open(&Config::default());
    let mut live = config(fixed(4));
    live.mouse = false;
    let keys: Vec<_> = m.edits(&live).iter().map(|e| (e.table, e.key)).collect();
    assert_eq!(keys, [("", "mouse"), ("build", "jobs")]);
}
