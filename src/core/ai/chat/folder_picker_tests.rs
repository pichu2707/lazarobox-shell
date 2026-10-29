use super::folder::{Probed, Refusal, Verdict};
use super::folder_picker::*;
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
