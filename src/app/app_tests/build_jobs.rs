//! `[build] jobs` reaches every new pane shell as `CARGO_BUILD_JOBS` and
//! `CMAKE_BUILD_PARALLEL_LEVEL`; a menu change applies to the panes opened
//! after it, and Esc puts the old count back.

use super::*;
use crate::core::build_jobs::Machine;

const GIB: u64 = 1024 * 1024 * 1024;

/// 16 cores and 7.5 GiB: `auto` gives 3.
fn wsl() -> Machine {
    Machine::new(16, Some(7 * GIB + GIB / 2))
}

fn jobs_env(spec: &SpawnSpec) -> Vec<(&str, &str)> {
    spec.env
        .iter()
        .filter(|(key, _)| crate::core::build_jobs::ENV_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

fn both(n: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![("CARGO_BUILD_JOBS", n), ("CMAKE_BUILD_PARALLEL_LEVEL", n)]
}

/// The spec of the shell a new tab starts.
fn new_tab_spec(a: &mut App) -> SpawnSpec {
    let effects = press(a, "tn");
    let specs: Vec<_> = effects
        .into_iter()
        .filter_map(|e| match e {
            Effect::SpawnPane(_, spec) => Some(spec),
            _ => None,
        })
        .collect();
    assert_eq!(specs.len(), 1, "one new shell");
    specs.into_iter().next().unwrap()
}

/// Opens the menu on the `Build jobs` row and presses `k`.
fn change_jobs(a: &mut App, k: char) {
    open_menu(a);
    a.menu.as_mut().unwrap().goto("Build jobs");
    a.update(key(k));
}

#[test]
fn a_shell_gets_the_auto_count_of_the_machine() {
    let a = launched(80, 25).with_build_machine(wsl());
    assert_eq!(jobs_env(&a.initial_spawn().1), both("3"));
}

#[test]
fn a_fixed_count_from_the_config_wins() {
    let config = Config {
        build_jobs: crate::core::build_jobs::BuildJobs::Auto.step(wsl(), false),
        ..Config::default()
    };
    let a = launched(80, 25)
        .with_build_machine(wsl())
        .with_config(&config);
    assert_eq!(jobs_env(&a.initial_spawn().1), both("16"));
}

#[test]
fn without_a_measured_machine_no_job_variable_is_set() {
    let a = launched(80, 25);
    assert_eq!(jobs_env(&a.initial_spawn().1), vec![]);
}

#[test]
fn a_saved_menu_change_reaches_the_next_new_shell() {
    let mut a = launched(80, 25).with_build_machine(wsl());
    change_jobs(&mut a, 'h');
    let effects = a.update(key('s'));
    assert!(
        effects.iter().any(|e| matches!(e, Effect::SaveConfig(edits)
            if edits.iter().any(|e| (e.table, e.key, e.value.as_ref()) == ("build", "jobs", "16")))),
        "{effects:?}"
    );
    a.update(AppEvent::ConfigSaved);
    assert!(a.menu().is_none());
    assert_eq!(jobs_env(&new_tab_spec(&mut a)), both("16"));
}

#[test]
fn esc_puts_the_old_count_back_for_new_shells() {
    let mut a = launched(80, 25).with_build_machine(wsl());
    change_jobs(&mut a, 'h');
    a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    )));
    assert!(a.menu().is_none());
    assert_eq!(jobs_env(&new_tab_spec(&mut a)), both("3"));
}
