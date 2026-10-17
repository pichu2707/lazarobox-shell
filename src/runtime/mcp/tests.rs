use super::*;

#[test]
fn a_token_is_128_bits_of_lowercase_hex_and_never_repeats() {
    let a = Token::generate().unwrap();
    let b = Token::generate().unwrap();
    assert_eq!(a.reveal().len(), 32);
    assert!(
        a.reveal()
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_ne!(a.reveal(), b.reveal());
}

#[test]
fn a_token_matches_only_itself() {
    let t = Token::generate().unwrap();
    assert!(t.matches(t.reveal()));
    assert!(!t.matches(""));
    assert!(!t.matches(&t.reveal()[..31]));
    assert!(!t.matches(&format!("{}0", t.reveal())));
    let mut flipped = t.reveal().to_owned();
    flipped.replace_range(31.., if flipped.ends_with('0') { "1" } else { "0" });
    assert!(!t.matches(&flipped));
}

#[test]
fn a_token_does_not_print() {
    let t = Token::generate().unwrap();
    assert!(!format!("{t:?}").contains(t.reveal()));
}

#[test]
fn candidates_are_the_runtime_dir_then_the_data_run_dir() {
    let got = candidate_dirs(Some(OsStr::new("/run/user/1")), Some(Path::new("/d/lb")));
    assert_eq!(
        got,
        [
            PathBuf::from("/run/user/1/lazarobox"),
            PathBuf::from("/d/lb/run")
        ]
    );
}

#[test]
fn a_relative_or_missing_runtime_dir_falls_back_to_data() {
    let data = Some(Path::new("/d/lb"));
    let run = [PathBuf::from("/d/lb/run")];
    assert_eq!(candidate_dirs(Some(OsStr::new("rel")), data), run);
    assert_eq!(candidate_dirs(None, data), run);
    assert!(candidate_dirs(None, None).is_empty());
}
