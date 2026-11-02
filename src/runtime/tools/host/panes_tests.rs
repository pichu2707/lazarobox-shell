//! `list_panes` through the host: free on every engine, wrapped as data, and
//! offered only with the pane family.

use std::path::PathBuf;

use super::{
    tests::{Scripted, events, fixture, scope},
    *,
};
use crate::core::ai::tools::{panes::PaneRow, registry::Family};

fn row(number: u32, focused: bool, argv: Option<&[&str]>) -> PaneRow {
    PaneRow {
        number,
        focused,
        cwd: Some(PathBuf::from("/tmp")),
        argv: argv.map(|words| words.iter().map(|w| (*w).to_owned()).collect()),
    }
}

// The roster reaches the model on a local and a cloud engine without an ask.
#[tokio::test]
async fn list_panes_runs_free_and_wraps_the_rows_as_data() {
    let (_dir, home, host) = fixture();
    host.roster().set(vec![
        row(1, true, Some(&["zsh"])),
        row(2, false, Some(&["nvim", "prueba.rs"])),
    ]);
    for locality in [Locality::Local, Locality::Cloud] {
        let (mut turn, mut rx) = scope(locality, &home);
        let asks = Scripted::new(Decision::Denied("no"));
        turn.approver = asks.clone();
        let out = host.dispatch(turn).call(Call::ListPanes).await;
        assert!(!out.is_error, "{out:?}");
        assert!(out.text.contains("<untrusted-tool-output"), "{out:?}");
        assert!(
            out.text.contains("pane 1 (focused) cwd: /tmp program: zsh"),
            "{out:?}"
        );
        assert!(out.text.contains("program: nvim prueba.rs"), "{out:?}");
        assert!(asks.asked.lock().unwrap().is_empty(), "{locality:?}");
        // The row shows what was sent.
        let shown = events(&mut rx);
        assert!(
            shown.iter().any(|e| matches!(e,
                StreamEvent::ToolResult { summary, .. } if summary.contains("nvim prueba.rs"))),
            "{shown:?}"
        );
    }
}

// A pane whose process could not be read says so; the tool does not fail.
#[tokio::test]
async fn an_unreadable_pane_shows_program_unknown_and_the_call_succeeds() {
    let (_dir, home, host) = fixture();
    host.roster().set(vec![row(3, true, None)]);
    let (turn, _rx) = scope(Locality::Local, &home);
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    assert!(
        !out.is_error && out.text.contains("program: unknown"),
        "{out:?}"
    );
}

// Paths under the host's HOME are shown from `~`.
#[tokio::test]
async fn paths_under_home_are_shortened() {
    let (_dir, home, host) = fixture();
    let file = format!("{}/p/a.rs", home.display());
    host.roster()
        .set(vec![row(1, true, Some(&["nvim", &file]))]);
    let (turn, _rx) = scope(Locality::Local, &home);
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    assert!(out.text.contains("nvim ~/p/a.rs"), "{out:?}");
    assert!(!out.text.contains(&home.display().to_string()), "{out:?}");
}

// Without the pane family the call is an unknown tool, as for any family.
#[tokio::test]
async fn list_panes_is_refused_without_the_pane_family() {
    let (_dir, home, host) = fixture();
    host.roster().set(vec![row(1, true, Some(&["zsh"]))]);
    let (mut turn, _rx) = scope(Locality::Local, &home);
    turn.families = FamilySet::EMPTY.with(Family::Read);
    let out = host.dispatch(turn).call(Call::ListPanes).await;
    assert!(out.is_error && !out.text.contains("zsh"), "{out:?}");
}
