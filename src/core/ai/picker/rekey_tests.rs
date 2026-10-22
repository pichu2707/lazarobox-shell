use super::machine_tests::{at_models, code_with, det, engine_key, state};
use super::*;

fn picker_on(draft: &AiConfig, d: &Detection, key: PickKey) -> Picker {
    let mut p = Picker::open(Target::Task(Task::Code), draft, Some(d));
    p.cursor = p
        .rows(draft, Some(d))
        .iter()
        .position(|r| r.key == key)
        .unwrap();
    p
}

#[test]
fn key_is_the_identity_of_the_cursor_row() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let p = picker_on(&draft, &d, engine_key("claude"));
    assert_eq!(p.key(&draft, Some(&d)), Some(engine_key("claude")));
}

#[test]
fn the_cursor_follows_its_row_to_a_new_position() {
    let d1 = det(&[]);
    let draft = AiConfig::default();
    let mut p = picker_on(&draft, &d1, engine_key("claude"));
    let before = p.key(&draft, Some(&d1));
    assert_eq!(p.cursor(), 2);
    let d2 = Detection {
        engines: vec![
            state("ollama", Status::NotRunning, &[]),
            state("claude", Status::Available, &[]),
        ],
    };
    p.reanchor(before, &draft, Some(&d2));
    assert_eq!(p.cursor(), 1);
}

#[test]
fn a_row_that_became_disabled_keeps_the_cursor() {
    let d1 = det(&["a:1b", "b:1b"]);
    let draft = code_with("ollama", "b:1b");
    let mut p = at_models(&draft, &d1, "ollama");
    assert_eq!(p.cursor(), 1);
    let before = p.key(&draft, Some(&d1));
    let d2 = det(&["a:1b"]);
    p.reanchor(before, &draft, Some(&d2));
    let rows = p.rows(&draft, Some(&d2));
    assert_eq!(p.cursor(), 1);
    assert!(!rows[1].enabled);
    assert_eq!(rows[1].key, PickKey::Model("b:1b".to_owned()));
}

#[test]
fn a_removed_row_clamps_the_cursor() {
    let d1 = det(&[]);
    let draft = AiConfig::default();
    let mut p = picker_on(&draft, &d1, engine_key("claude"));
    let before = p.key(&draft, Some(&d1));
    let d2 = Detection { engines: vec![] };
    p.reanchor(before, &draft, Some(&d2));
    assert_eq!(p.rows(&draft, Some(&d2)).len(), 1);
    assert_eq!(p.cursor(), 0);
}

#[test]
fn the_window_is_refitted_after_the_list_shrinks() {
    let names = [
        "a:1b", "b:1b", "c:1b", "d:1b", "e:1b", "f:1b", "g:1b", "h:1b",
    ];
    let d1 = det(&names);
    let draft = AiConfig::default();
    let mut p = at_models(&draft, &d1, "ollama");
    for _ in 0..17 {
        p.step(true, &draft, Some(&d1));
    }
    assert_eq!((p.cursor(), p.offset()), (17, 8));
    let before = p.key(&draft, Some(&d1));
    let d2 = det(&[]);
    p.reanchor(before, &draft, Some(&d2));
    assert_eq!(p.rows(&draft, Some(&d2)).len(), 10);
    assert_eq!((p.cursor(), p.offset()), (9, 0));
}

#[test]
fn reanchor_without_a_previous_key_only_clamps() {
    let d = det(&[]);
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    p.cursor = 40;
    p.reanchor(None, &draft, Some(&d));
    // none, ollama, claude, then lmstudio (installed, not running).
    assert_eq!(p.cursor(), 3);
}
