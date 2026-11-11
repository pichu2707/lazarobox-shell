//! `SecretText` never formats its value (mcp-connect, Secret fields: Debug
//! and panics never format it).

use super::{
    SecretText,
    text::{CAPACITY, wipe_capacity},
};

fn secret(text: &str) -> SecretText {
    let mut s = SecretText::new();
    s.push_str(text);
    s
}

#[test]
fn debug_prints_redacted_and_never_the_value() {
    let s = secret("tok-7f3a");
    assert_eq!(format!("{s:?}"), "<redacted>");
    assert_eq!(format!("{:#?}", Some(&s)), "Some(\n    <redacted>,\n)");
}

#[test]
fn a_panic_on_a_result_that_carries_it_does_not_format_it() {
    let boom = std::panic::catch_unwind(|| {
        let r: Result<(), SecretText> = std::hint::black_box(Err(secret("tok-7f3a")));
        r.expect("save failed");
    })
    .unwrap_err();
    let said = boom
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| boom.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap();
    assert!(said.contains("<redacted>"), "{said}");
    assert!(!said.contains("tok-7f3a"), "{said}");
}

#[test]
fn it_edits_like_a_line() {
    let mut s = secret("ab");
    s.push('c');
    s.pop();
    assert_eq!((s.expose(), s.len(), s.is_empty()), ("ab", 2, false));
}

#[test]
fn the_capacity_is_reserved_up_front_and_never_regrows_up_to_the_bound() {
    let mut s = SecretText::new();
    let at = s.capacity();
    assert!(at >= CAPACITY);
    for _ in 0..CAPACITY / 8 {
        s.push_str("12345678");
    }
    assert_eq!((s.len(), s.capacity()), (CAPACITY, at));
    assert!(secret("x").clone().capacity() >= CAPACITY);
}

#[test]
fn wiping_reaches_bytes_that_pop_left_beyond_the_length() {
    let mut s = String::with_capacity(16);
    s.push_str("tok-7f3a");
    while s.pop().is_some() {}
    wipe_capacity(&mut s);
    // SAFETY: the 8 bytes were written above and are inside the capacity.
    let left = unsafe { std::slice::from_raw_parts(s.as_ptr(), 8) };
    assert_eq!(left, [0; 8]);
}
