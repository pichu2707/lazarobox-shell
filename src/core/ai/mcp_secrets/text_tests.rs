//! `SecretText` never formats its value (mcp-connect, Secret fields: Debug
//! and panics never format it).

use super::{SecretText, text::wipe};

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
fn wipe_overwrites_every_byte_and_keeps_the_length() {
    let mut bytes = b"tok-7f3a".to_vec();
    wipe(&mut bytes);
    assert_eq!(bytes, [0; 8]);
}

#[test]
fn it_edits_like_a_line() {
    let mut s = secret("ab");
    s.push('c');
    s.pop();
    assert_eq!((s.expose(), s.len(), s.is_empty()), ("ab", 2, false));
}
