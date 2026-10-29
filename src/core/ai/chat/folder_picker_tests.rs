use super::folder::{Probed, Refusal, Verdict};
use super::folder_picker::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn found(source: Source, path: &str, verdict: Verdict) -> (Source, Result<Probed, Refusal>) {
    (
        source,
        Ok(Probed {
            path: PathBuf::from(path),
            verdict,
        }),
    )
}

fn concrete(source: Source, path: &str) -> (Source, Result<Probed, Refusal>) {
    found(source, path, Verdict::Concrete)
}

fn paths(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|r| r.path.to_str().unwrap()).collect()
}

#[test]
fn rows_follow_the_order_of_their_sources() {
    let rows = merge(vec![
        concrete(Source::LastShell, "/h/shell"),
        concrete(Source::Recent, "/h/recent"),
        concrete(Source::PaneRoot, "/h/pane"),
        concrete(Source::LastUsed, "/h/last"),
    ]);
    assert_eq!(
        paths(&rows),
        ["/h/last", "/h/pane", "/h/recent", "/h/shell"]
    );
}

#[test]
fn the_same_real_path_is_listed_once_under_its_first_source() {
    let rows = merge(vec![
        concrete(Source::Recent, "/h/a"),
        concrete(Source::PaneRoot, "/h/a"),
        concrete(Source::Recent, "/h/b"),
    ]);
    assert_eq!(paths(&rows), ["/h/a", "/h/b"]);
    assert_eq!(rows[0].source, Source::PaneRoot);
}

#[test]
fn only_concrete_folders_are_rows() {
    let rows = merge(vec![
        found(Source::LastUsed, "/h", Verdict::Home),
        found(Source::LastUsed, "/home", Verdict::AboveHome),
        found(Source::LastUsed, "/usr", Verdict::Outside),
        (Source::PaneRoot, Err(Refusal::Root)),
        (Source::PaneRoot, Err(Refusal::NotFound)),
        (Source::Recent, Err(Refusal::NotAllowed)),
        (Source::Recent, Err(Refusal::NotAFolder)),
        concrete(Source::LastShell, "/h/ok"),
    ]);
    assert_eq!(paths(&rows), ["/h/ok"]);
}

#[test]
fn at_most_twelve_rows() {
    let many = (0..20)
        .map(|n| concrete(Source::Recent, &format!("/h/{n:02}")))
        .collect();
    let rows = merge(many);
    assert_eq!(rows.len(), ROWS_MAX);
    assert_eq!(rows[0].path, PathBuf::from("/h/00"));
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key(p: &mut FolderPicker, code: KeyCode) -> PickerOutcome {
    p.on_key(&press(code), false)
}

fn type_in(p: &mut FolderPicker, text: &str) {
    for c in text.chars() {
        key(p, KeyCode::Char(c));
    }
}

fn row(path: &str) -> Row {
    Row {
        path: PathBuf::from(path),
        source: Source::Recent,
    }
}

fn picker(rows: &[&str]) -> FolderPicker {
    let mut p = FolderPicker::new(PathBuf::from("/h"), Some(PathBuf::from("/h/base")));
    p.set_rows(rows.iter().map(|r| row(r)).collect());
    p
}

fn probed(path: &str, verdict: Verdict) -> Result<Probed, Refusal> {
    Ok(Probed {
        path: PathBuf::from(path),
        verdict,
    })
}

#[test]
fn rows_arrive_with_the_first_selected_and_up_down_move_within_them() {
    let mut p = FolderPicker::new(PathBuf::from("/h"), None);
    assert!(p.gathering());
    p.set_rows(vec![row("/h/a"), row("/h/b"), row("/h/c")]);
    assert!(!p.gathering());
    assert_eq!(p.selected(), 0);
    key(&mut p, KeyCode::Up);
    assert_eq!(p.selected(), 0);
    key(&mut p, KeyCode::Down);
    key(&mut p, KeyCode::Down);
    key(&mut p, KeyCode::Down);
    assert_eq!(p.selected(), 2);
    key(&mut p, KeyCode::Up);
    assert_eq!(p.selected(), 1);
}

#[test]
fn the_field_edits_and_ctrl_u_clears_it() {
    let mut p = picker(&[]);
    type_in(&mut p, "~/ab");
    key(&mut p, KeyCode::Backspace);
    assert_eq!(p.field().text(), "~/a");
    p.on_key(
        &KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        false,
    );
    assert_eq!(p.field().text(), "");
}

#[test]
fn a_paste_goes_to_the_field_as_one_line() {
    let mut p = picker(&["/h/a"]);
    p.paste("/h/x\ny");
    assert_eq!(p.field().text(), "/h/x y");
}

#[test]
fn enter_takes_the_field_over_the_selected_row() {
    let mut p = picker(&["/h/a", "/h/b"]);
    key(&mut p, KeyCode::Down);
    type_in(&mut p, "~/typed");
    assert_eq!(
        key(&mut p, KeyCode::Enter),
        PickerOutcome::Probe(PathBuf::from("/h/typed"))
    );
}

#[test]
fn enter_with_an_empty_field_takes_the_selected_row() {
    let mut p = picker(&["/h/a", "/h/b"]);
    key(&mut p, KeyCode::Down);
    assert_eq!(
        key(&mut p, KeyCode::Enter),
        PickerOutcome::Probe(PathBuf::from("/h/b"))
    );
}

#[test]
fn a_relative_field_reads_against_the_base() {
    let mut p = picker(&[]);
    type_in(&mut p, "../x");
    assert_eq!(
        key(&mut p, KeyCode::Enter),
        PickerOutcome::Probe(PathBuf::from("/h/x"))
    );
}

#[test]
fn enter_with_an_empty_field_and_no_rows_does_nothing() {
    let mut p = picker(&[]);
    assert_eq!(key(&mut p, KeyCode::Enter), PickerOutcome::Handled);
    assert_eq!(p.error(), None);
}

#[test]
fn a_held_enter_never_decides() {
    let mut p = picker(&["/h/a"]);
    assert_eq!(
        p.on_key(&press(KeyCode::Enter), true),
        PickerOutcome::Handled
    );
}

#[test]
fn esc_cancels() {
    let mut p = picker(&["/h/a"]);
    assert_eq!(key(&mut p, KeyCode::Esc), PickerOutcome::Cancel);
}

#[test]
fn a_refused_path_keeps_the_text_and_says_why() {
    let mut p = picker(&["/h/a"]);
    type_in(&mut p, "/etc");
    key(&mut p, KeyCode::Enter);
    assert_eq!(p.on_probed(probed("/etc", Verdict::Outside)), None);
    assert_eq!(p.field().text(), "/etc");
    assert_eq!(p.error(), Some("not allowed"));
    // The next edit clears the reason and the picker decides again.
    key(&mut p, KeyCode::Backspace);
    assert_eq!(p.error(), None);
    assert!(matches!(
        key(&mut p, KeyCode::Enter),
        PickerOutcome::Probe(_)
    ));
    assert_eq!(p.on_probed(Err(Refusal::NotFound)), None);
    assert_eq!(p.error(), Some("folder not found"));
}

#[test]
fn an_accepted_folder_comes_back_and_a_late_answer_is_ignored() {
    let mut p = picker(&["/h/a"]);
    key(&mut p, KeyCode::Enter);
    assert_eq!(
        p.on_probed(probed("/h/a", Verdict::Concrete)),
        Some(PathBuf::from("/h/a"))
    );
    // Nothing is being probed any more.
    assert_eq!(p.on_probed(probed("/h/a", Verdict::Concrete)), None);
}

#[test]
fn while_probing_only_esc_acts() {
    let mut p = picker(&["/h/a", "/h/b"]);
    key(&mut p, KeyCode::Enter);
    key(&mut p, KeyCode::Down);
    key(&mut p, KeyCode::Char('x'));
    assert_eq!((p.selected(), p.field().text()), (0, String::new()));
    assert_eq!(key(&mut p, KeyCode::Esc), PickerOutcome::Cancel);
}

#[test]
fn tab_asks_for_the_directories_of_the_typed_folder() {
    let mut p = picker(&[]);
    type_in(&mut p, "~/pro");
    assert_eq!(
        key(&mut p, KeyCode::Tab),
        PickerOutcome::Complete {
            text: "~/pro".into(),
            dir: PathBuf::from("/h"),
            prefix: "pro".into()
        }
    );
}

#[test]
fn a_completion_extends_the_field_to_the_common_prefix() {
    let mut p = picker(&[]);
    type_in(&mut p, "~/pro");
    assert!(p.on_completed("~/pro", &["project-a".into(), "project-b".into()]));
    assert_eq!(p.field().text(), "~/project-");
    assert!(p.on_completed("~/project-", &["project-a".into()]));
    assert_eq!(p.field().text(), "~/project-a/");
}

#[test]
fn a_stale_completion_is_dropped() {
    let mut p = picker(&[]);
    type_in(&mut p, "~/pro");
    key(&mut p, KeyCode::Tab);
    type_in(&mut p, "x");
    assert!(!p.on_completed("~/pro", &["project".into()]));
    assert_eq!(p.field().text(), "~/prox");
}
