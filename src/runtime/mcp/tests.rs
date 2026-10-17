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

/// Records `in_dispatch()` when dropped, even while a panic unwinds.
struct SeesFlagOnDrop(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Drop for SeesFlagOnDrop {
    fn drop(&mut self) {
        self.0
            .store(in_dispatch(), std::sync::atomic::Ordering::SeqCst);
    }
}

struct Probe(std::sync::Mutex<Vec<bool>>);

impl Dispatch for Probe {
    fn call(&self, _: Call) -> BoxFuture<'static, Outcome> {
        self.0.lock().unwrap().push(in_dispatch());
        Box::pin(async {
            Outcome {
                text: String::new(),
                is_error: false,
            }
        })
    }
}

#[tokio::test]
async fn the_dispatch_flag_is_set_in_the_sync_call_and_in_every_poll_only() {
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    assert!(!in_dispatch());
    let probe = Probe(Default::default());
    let call = Call::FindDir { query: "x".into() };
    let mut fut = pin!(guarded_call(&probe, call));
    assert_eq!(*probe.0.lock().unwrap(), [true], "set in the sync part");
    assert!(!in_dispatch(), "and cleared after it");
    let mut cx = Context::from_waker(Waker::noop());
    assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(_)));
    assert!(!in_dispatch(), "cleared once the poll returns");
    let seen = scoped(async { in_dispatch() }).await;
    assert!(seen);
}

#[tokio::test]
async fn the_dispatch_flag_is_set_while_a_panic_unwinds_and_cleared_after() {
    use futures::FutureExt;
    use std::{
        panic::AssertUnwindSafe,
        sync::{Arc, atomic::AtomicBool, atomic::Ordering},
    };
    let at_unwind = Arc::new(AtomicBool::new(false));
    let seen = at_unwind.clone();
    let result = AssertUnwindSafe(scoped(async move {
        let _guard = SeesFlagOnDrop(seen);
        panic!("dispatch bug");
    }))
    .catch_unwind()
    .await;
    assert!(result.is_err());
    assert!(at_unwind.load(Ordering::SeqCst));
    assert!(!in_dispatch(), "a panic must not leave the flag set");
}

#[tokio::test]
async fn nested_scopes_keep_the_flag_until_the_outermost_ends() {
    assert!(
        scoped(async {
            scoped(async {}).await;
            in_dispatch()
        })
        .await
    );
    assert!(!in_dispatch());
}
