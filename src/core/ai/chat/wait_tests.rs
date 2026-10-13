//! No view waits for memory without a bound (engram-memory 10.9).

use std::time::{Duration, Instant};

use super::{memory::*, state_tests::chat, *};

fn t0() -> Instant {
    use std::sync::OnceLock;
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

fn after(ms: u64) -> Instant {
    t0() + Duration::from_millis(ms)
}

const NO_ANSWER: &str = "engram did not answer";

#[test]
fn the_bound_is_five_seconds() {
    assert_eq!(MEMORY_WAIT, Duration::from_secs(5));
}

#[test]
fn a_chat_that_waits_for_nothing_has_nothing_to_expire() {
    let mut state = chat();
    assert!(!state.memory_waiting());
    assert_eq!(state.expire_waits(after(60_000)), Expired::default());
}

#[test]
fn a_project_request_gives_up_exactly_at_five_seconds() {
    let mut state = chat();
    state.start_resolving_at(false, t0());
    assert!(state.memory_waiting());
    assert_eq!(state.expire_waits(after(4_999)), Expired::default());
    assert_eq!(
        state.readiness(),
        &Readiness::Resolving { want_note: false }
    );
    let expired = state.expire_waits(after(5_000));
    assert_eq!(expired.resolve, Some(false));
    assert_eq!(state.readiness(), &Readiness::Unavailable(NO_ANSWER.into()));
    assert!(!state.memory_waiting());
    assert_eq!(state.expire_waits(after(9_000)), Expired::default(), "once");
}

#[test]
fn a_save_key_waiting_for_the_project_is_reported_as_wanting_the_note() {
    let mut state = chat();
    state.start_resolving_at(false, t0());
    state.start_resolving_at(true, after(3_000));
    let expired = state.expire_waits(after(5_000));
    assert_eq!(expired.resolve, Some(true));
}

#[test]
fn asking_again_does_not_extend_the_wait() {
    let mut state = chat();
    state.start_resolving_at(false, t0());
    state.start_resolving_at(true, after(4_000));
    assert_eq!(state.expire_waits(after(5_000)).resolve, Some(true));
}

#[test]
fn a_context_request_gives_up_and_a_late_reply_is_ignored() {
    let mut state = chat();
    assert!(state.start_context_at(t0()));
    assert!(state.memory_waiting());
    assert!(!state.expire_waits(after(4_999)).context);
    assert!(state.expire_waits(after(5_000)).context);
    assert!(!state.memory_waiting());
    assert!(
        !state.finish_context(),
        "the late reply finds nobody waiting"
    );
}

#[test]
fn a_context_that_arrived_in_time_never_expires() {
    let mut state = chat();
    state.start_context_at(t0());
    assert!(state.finish_context());
    assert!(!state.expire_waits(after(60_000)).context);
}

#[test]
fn each_save_gives_up_on_its_own_clock() {
    let mut state = chat();
    let first = state.save_sent_at("p", t0());
    let second = state.save_sent_at("p", after(2_000));
    assert!(state.memory_waiting());
    let expired = state.expire_waits(after(5_000));
    assert_eq!(
        expired.saves.iter().map(|s| s.id).collect::<Vec<_>>(),
        [first]
    );
    assert_eq!(state.pending_save_ids(), [second]);
    let expired = state.expire_waits(after(7_000));
    assert_eq!(
        expired.saves.iter().map(|s| s.id).collect::<Vec<_>>(),
        [second]
    );
    assert!(!state.memory_waiting());
    assert!(
        state.save_answered(first).is_none(),
        "the late reply is unknown"
    );
}

#[test]
fn any_answer_ends_the_project_wait() {
    let answers: [fn(&mut ChatState); 3] = [
        |s| {
            s.project_resolved(Ok("p".into()));
        },
        |s| {
            s.memory_unavailable("engram is not running");
        },
        |s| s.set_unavailable("memory needs a git project"),
    ];
    for answer in answers {
        let mut state = chat();
        state.start_resolving_at(false, t0());
        assert!(state.memory_waiting());
        answer(&mut state);
        assert!(!state.memory_waiting());
        assert_eq!(state.expire_waits(after(60_000)), Expired::default());
    }
}
