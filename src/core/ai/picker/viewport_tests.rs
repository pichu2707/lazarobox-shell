use super::machine_tests::{at_models, code_with, det};
use super::*;

const NAMES: [&str; 12] = [
    "a:1b", "b:1b", "c:1b", "d:1b", "e:1b", "f:1b", "g:1b", "h:1b", "i:1b", "j:1b", "k:1b", "l:1b",
];

/// Models level of ollama with 8 detected + 10 curated = 18 rows.
fn long(draft: &AiConfig, d: &Detection) -> Picker {
    let p = at_models(draft, d, "ollama");
    assert_eq!(p.rows(draft, Some(d)).len(), 18);
    p
}

fn go(p: &mut Picker, n: usize, forward: bool, draft: &AiConfig, d: &Detection) {
    for _ in 0..n {
        p.step(forward, draft, Some(d));
    }
}

#[test]
fn the_window_stays_put_while_the_cursor_is_inside_it() {
    let d = det(&NAMES[..8]);
    let draft = AiConfig::default();
    let mut p = long(&draft, &d);
    go(&mut p, 9, true, &draft, &d);
    assert_eq!((p.cursor(), p.offset()), (9, 0));
}

#[test]
fn the_cursor_below_the_window_becomes_its_last_row() {
    let d = det(&NAMES[..8]);
    let draft = AiConfig::default();
    let mut p = long(&draft, &d);
    go(&mut p, 10, true, &draft, &d);
    assert_eq!((p.cursor(), p.offset()), (10, 1));
    go(&mut p, 7, true, &draft, &d);
    assert_eq!((p.cursor(), p.offset()), (17, 8));
}

#[test]
fn the_cursor_above_the_window_becomes_its_first_row() {
    let d = det(&NAMES[..8]);
    let draft = AiConfig::default();
    let mut p = long(&draft, &d);
    go(&mut p, 17, true, &draft, &d);
    go(&mut p, 9, false, &draft, &d);
    assert_eq!((p.cursor(), p.offset()), (8, 8));
    go(&mut p, 1, false, &draft, &d);
    assert_eq!((p.cursor(), p.offset()), (7, 7));
}

#[test]
fn wrapping_moves_the_window_with_the_cursor() {
    let d = det(&NAMES[..8]);
    let draft = AiConfig::default();
    let mut p = long(&draft, &d);
    go(&mut p, 1, false, &draft, &d);
    assert_eq!((p.cursor(), p.offset()), (17, 8));
    go(&mut p, 1, true, &draft, &d);
    assert_eq!((p.cursor(), p.offset()), (0, 0));
}

#[test]
fn opening_a_deep_current_model_scrolls_to_it() {
    let d = det(&NAMES);
    let draft = code_with("ollama", "l:1b");
    let p = at_models(&draft, &d, "ollama");
    assert_eq!((p.cursor(), p.offset()), (11, 2));
}

#[test]
fn visible_is_the_window_clipped_to_the_list() {
    let d = det(&NAMES[..8]);
    let draft = AiConfig::default();
    let mut p = long(&draft, &d);
    assert_eq!(p.visible(18), 0..10);
    go(&mut p, 17, true, &draft, &d);
    assert_eq!(p.visible(18), 8..18);
    assert_eq!(p.visible(4), 0..4);
}

#[test]
fn the_counter_shows_only_when_the_list_is_longer_than_the_view() {
    let d = det(&NAMES[..8]);
    let draft = AiConfig::default();
    let mut p = long(&draft, &d);
    assert_eq!(p.counter(18), Some("1/18".to_owned()));
    go(&mut p, 17, true, &draft, &d);
    assert_eq!(p.counter(18), Some("18/18".to_owned()));
    assert_eq!(p.counter(VIEW), None);
    assert_eq!(p.counter(3), None);
}
