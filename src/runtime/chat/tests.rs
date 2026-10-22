//! Chat task tests. Fakes without IO run on the paused tokio clock; the tests
//! that need a real loopback socket or a real process use the real clock and
//! bound every wait.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    process::Command,
    sync::Notify,
    time::timeout,
};

use super::*;
use crate::{
    core::ai::chat::{
        ANSWER_CAP, ChatState, Follow, MsgStatus, REASONING_CAP,
        engine::{Availability, ChatError, Finish, Tools},
    },
    runtime::proc::GroupKill,
};

fn pane(n: u32) -> PaneId {
    PaneId::for_test(n)
}

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

fn failed(text: &str) -> StreamEvent {
    StreamEvent::Error(ChatError::Failed(text.into()))
}

pub(super) fn done() -> StreamEvent {
    StreamEvent::Done(Finish::Stop)
}

pub(super) fn request(system: &str) -> ChatRequest {
    ChatRequest {
        families: crate::core::ai::tools::registry::FamilySet::LEGACY,
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
        system: system.into(),
        memory: None,
        tools: Tools::Off,
        messages: Vec::new(),
    }
}

/// The text of the deltas in `events`, joined.
fn text_of(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Delta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

// The pipe -------------------------------------------------------------------

fn thinking(text: &str) -> StreamEvent {
    StreamEvent::ThinkingDelta(text.into())
}

/// The reasoning text in `events`, joined.
fn reasoning_of(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ThinkingDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn thinking_deltas_pass_as_thinking_deltas_and_are_sanitized() {
    let mut pipe = Pipe::new();
    let mut out = Vec::new();
    for piece in ["a\u{1b}[31mb", "c\u{07}d", "e\u{1b}]0;t\u{07}f"] {
        out.extend(pipe.event(thinking(piece)));
    }
    assert_eq!(reasoning_of(&out), "abcdef");
    assert!(
        out.iter()
            .all(|e| matches!(e, StreamEvent::ThinkingDelta(_))),
        "reasoning never turns into answer text: {out:?}"
    );
    assert!(text_of(&out).is_empty());
}

#[test]
fn a_sequence_split_across_thinking_deltas_is_removed() {
    let mut pipe = Pipe::new();
    let mut out = pipe.event(thinking("x\u{1b}[3"));
    out.extend(pipe.event(thinking("1my")));
    assert_eq!(reasoning_of(&out), "xy");
}

#[test]
fn an_unterminated_sequence_in_reasoning_never_alters_the_answer() {
    let mut pipe = Pipe::new();
    let mut out = pipe.event(thinking("ab\u{1b}]0;never ended"));
    out.extend(pipe.event(delta("the answer")));
    out.extend(pipe.event(done()));
    assert_eq!(reasoning_of(&out), "ab");
    assert_eq!(text_of(&out), "the answer");
    assert_eq!(out.last(), Some(&done()));
}

#[test]
fn an_unterminated_sequence_in_the_answer_never_alters_the_reasoning() {
    let mut pipe = Pipe::new();
    let mut out = pipe.event(delta("ok\u{1b}]0;never ended"));
    out.extend(pipe.event(thinking("still visible")));
    // The reasoning came after the answer began: it is dropped, not mangled.
    assert_eq!(text_of(&out), "ok");
    assert_eq!(reasoning_of(&out), "");
    let mut fresh = Pipe::new();
    let mut out = fresh.event(thinking("first"));
    out.extend(fresh.event(delta("ok\u{1b}]0;x")));
    out.extend(fresh.event(thinking("never")));
    assert_eq!(reasoning_of(&out), "first");
}

#[test]
fn reasoning_after_the_first_answer_text_is_dropped_without_an_error() {
    let mut pipe = Pipe::new();
    let mut out = pipe.event(thinking("first"));
    out.extend(pipe.event(delta("ok")));
    // Late reasoning, even interleaved between answer pieces, goes nowhere.
    assert_eq!(pipe.event(thinking("late")), Vec::new());
    out.extend(pipe.event(delta("!")));
    assert_eq!(pipe.event(thinking("later")), Vec::new());
    out.extend(pipe.event(done()));
    assert_eq!(reasoning_of(&out), "first");
    assert_eq!(text_of(&out), "ok!");
    assert_eq!(out.last(), Some(&done()));
    assert!(!out.iter().any(|e| matches!(e, StreamEvent::Error(_))));
    assert!(pipe.is_over());
}

#[test]
fn reasoning_stops_at_256_kib_without_an_error_and_the_answer_is_intact() {
    let mut pipe = Pipe::new();
    let chunk = "r".repeat(4096);
    let mut out = Vec::new();
    for _ in 0..(REASONING_CAP / 4096 + 8) {
        out.extend(pipe.event(thinking(&chunk)));
    }
    out.extend(pipe.event(delta("Hi")));
    out.extend(pipe.event(done()));
    assert_eq!(reasoning_of(&out).len(), REASONING_CAP);
    assert!(
        !out.iter().any(|e| matches!(e, StreamEvent::Error(_))),
        "no error for the dropped reasoning"
    );
    assert_eq!(text_of(&out), "Hi");
    assert_eq!(out.last(), Some(&done()));
}

#[test]
fn the_reasoning_cap_cuts_on_a_character_boundary() {
    let mut pipe = Pipe::new();
    let mut out = pipe.event(thinking(&"a".repeat(REASONING_CAP - 1)));
    out.extend(pipe.event(thinking("日本")));
    assert_eq!(reasoning_of(&out).len(), REASONING_CAP - 1);
}

#[test]
fn reasoning_that_sanitizes_to_nothing_sends_nothing() {
    let mut pipe = Pipe::new();
    assert_eq!(pipe.event(thinking("\u{1b}[31m")), Vec::new());
    assert_eq!(pipe.event(thinking("")), Vec::new());
}

#[test]
fn nothing_passes_after_the_final_event_not_even_reasoning() {
    let mut pipe = Pipe::new();
    pipe.event(done());
    assert_eq!(pipe.event(thinking("late")), Vec::new());
}

#[test]
fn plain_deltas_and_the_final_event_pass_in_order() {
    let mut pipe = Pipe::new();
    let mut out = Vec::new();
    out.extend(pipe.event(delta("Hel")));
    out.extend(pipe.event(delta("lo")));
    out.extend(pipe.event(done()));
    assert_eq!(out, vec![delta("Hel"), delta("lo"), done()]);
}

#[test]
fn nothing_passes_after_the_final_event() {
    let mut pipe = Pipe::new();
    assert_eq!(pipe.event(done()), vec![done()]);
    assert_eq!(pipe.event(delta("late")), Vec::new());
    assert_eq!(pipe.event(failed("late")), Vec::new());
    assert_eq!(pipe.end_without_final(), Vec::new());
}

// S6: sequences split across deltas must not get through.
#[test]
fn escape_sequences_split_across_deltas_never_get_through() {
    let pieces = [
        "a\u{1b}[3",
        "1mb\u{1b}",
        "]0;ti",
        "tle\u{7}c",
        "\u{9b}31m",
        "d\u{202e}e\u{2066}f",
        "\u{1b}P1;2|dcs",
        "\u{1b}",
        "\\g",
    ];
    let mut pipe = Pipe::new();
    let mut out = Vec::new();
    for piece in pieces {
        out.extend(pipe.event(delta(piece)));
    }
    out.extend(pipe.event(done()));
    // A lone C1 control is dropped like any control; its tail is plain text.
    let text = text_of(&out);
    assert_eq!(text, "abc31mdefg");
    assert!(!text.chars().any(|c| c.is_control()), "{text:?}");
    assert_eq!(out.last(), Some(&done()));
}

#[test]
fn bytes_split_inside_a_character_are_joined_and_a_cut_tail_is_replaced() {
    let mut pipe = Pipe::new();
    let bytes = "aé€".as_bytes();
    let mut out = Vec::new();
    out.extend(pipe.bytes(&bytes[..2]));
    out.extend(pipe.bytes(&bytes[2..4]));
    out.extend(pipe.bytes(&bytes[4..5]));
    assert_eq!(text_of(&out), "aé");
    let tail = pipe.event(done());
    assert_eq!(tail, vec![delta("\u{fffd}"), done()]);
}

// S5: every failure text is one clean short line.
#[test]
fn a_failure_text_is_cleaned_flattened_trimmed_and_capped() {
    let mut pipe = Pipe::new();
    let raw = format!(
        "  boom\u{1b}[31m red\n\tsecond\r\nline {}  ",
        "x".repeat(400)
    );
    let out = pipe.event(failed(&raw));
    let [StreamEvent::Error(ChatError::Failed(text))] = out.as_slice() else {
        panic!("expected one failure, got {out:?}");
    };
    assert!(text.starts_with("boom red second line xxx"), "{text:?}");
    assert_eq!(text.chars().count(), 200);
    assert!(!text.contains(['\n', '\t', '\r', '\u{1b}']));
    assert_eq!(text.trim(), text);
}

#[test]
fn a_failure_with_no_text_left_says_so() {
    let mut pipe = Pipe::new();
    assert_eq!(
        pipe.event(failed("\u{1b}[0m \n")),
        vec![failed(EMPTY_ERROR)]
    );
}

#[test]
fn other_errors_pass_unchanged() {
    for error in [ChatError::Interrupted, ChatError::Truncated] {
        let mut pipe = Pipe::new();
        assert_eq!(
            pipe.event(StreamEvent::Error(error.clone())),
            vec![StreamEvent::Error(error)]
        );
    }
}

#[test]
fn slow_passes_and_a_missing_final_event_becomes_interrupted() {
    let mut pipe = Pipe::new();
    assert_eq!(pipe.event(StreamEvent::Slow), vec![StreamEvent::Slow]);
    assert_eq!(
        pipe.end_without_final(),
        vec![StreamEvent::Error(ChatError::Interrupted)]
    );
}

// Answer cap: the runtime stops at 1 MiB and says so once.
#[test]
fn the_answer_stops_at_the_cap_with_one_truncated_error() {
    let mut pipe = Pipe::new();
    let block = "a".repeat(64 * 1024);
    let mut out = Vec::new();
    for _ in 0..(ANSWER_CAP / block.len()) {
        out.extend(pipe.event(delta(&block)));
    }
    assert_eq!(text_of(&out).len(), ANSWER_CAP);
    assert!(
        !out.iter().any(|e| matches!(e, StreamEvent::Error(_))),
        "exactly the cap is still fine"
    );
    let over = pipe.event(delta("bc"));
    assert_eq!(over, vec![StreamEvent::Error(ChatError::Truncated)]);
    assert_eq!(pipe.event(delta("more")), Vec::new());
    assert_eq!(pipe.event(done()), Vec::new());
}

#[test]
fn the_cap_cuts_on_a_character_boundary() {
    let mut pipe = Pipe::new();
    let out = pipe.event(delta(&"a".repeat(ANSWER_CAP - 1)));
    assert_eq!(text_of(&out).len(), ANSWER_CAP - 1);
    // One byte of room, a two-byte character: nothing of it fits.
    assert_eq!(
        pipe.event(delta("é")),
        vec![StreamEvent::Error(ChatError::Truncated)]
    );

    let mut pipe = Pipe::new();
    pipe.event(delta(&"a".repeat(ANSWER_CAP - 3)));
    assert_eq!(
        pipe.event(delta("bcdef")),
        vec![delta("bcd"), StreamEvent::Error(ChatError::Truncated)]
    );
}

#[test]
fn exactly_the_cap_then_done_is_a_complete_answer() {
    let mut pipe = Pipe::new();
    pipe.event(delta(&"a".repeat(ANSWER_CAP)));
    assert_eq!(pipe.event(done()), vec![done()]);
}

// The task -------------------------------------------------------------------

/// A runner that sends `events` in order and ends.
pub(super) fn scripted(events: Vec<StreamEvent>) -> Runner {
    Arc::new(move |_request, tx| {
        let events = events.clone();
        Box::pin(async move {
            for event in events {
                if tx.send(event).await.is_err() {
                    return;
                }
            }
        })
    })
}

fn chats(runner: Runner) -> (Chats, mpsc::Receiver<Tagged>) {
    let (tx, rx) = mpsc::channel(APP_CHANNEL);
    (Chats::new(runner, tx), rx)
}

async fn next(rx: &mut mpsc::Receiver<Tagged>) -> Tagged {
    timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an event within 5 s")
        .expect("the channel is open")
}

/// Events up to and including the final one of the turn.
async fn turn_events(rx: &mut mpsc::Receiver<Tagged>) -> Vec<Tagged> {
    let mut events = Vec::new();
    loop {
        let event = next(rx).await;
        let last = matches!(event.2, StreamEvent::Done(_) | StreamEvent::Error(_));
        events.push(event);
        if last {
            return events;
        }
    }
}

#[tokio::test(start_paused = true)]
async fn events_arrive_tagged_and_in_order() {
    let (mut chats, mut rx) = chats(scripted(vec![delta("a"), delta("b"), done()]));
    chats.send(pane(1), 7, request(""));
    assert_eq!(
        turn_events(&mut rx).await,
        vec![
            (pane(1), 7, delta("a")),
            (pane(1), 7, delta("b")),
            (pane(1), 7, done()),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn two_chats_stream_independently() {
    let gate = Arc::new(Notify::new());
    let held = gate.clone();
    let runner: Runner = Arc::new(move |request, tx| {
        let gate = held.clone();
        Box::pin(async move {
            if request.system == "wait" {
                gate.notified().await;
            }
            let _ = tx.send(delta(&request.system)).await;
            let _ = tx.send(done()).await;
        })
    });
    let (mut chats, mut rx) = chats(runner);
    chats.send(pane(1), 1, request("wait"));
    chats.send(pane(2), 1, request("quick"));
    assert_eq!(
        turn_events(&mut rx).await,
        vec![(pane(2), 1, delta("quick")), (pane(2), 1, done())],
        "the second chat finishes while the first still waits"
    );
    gate.notify_one();
    assert_eq!(
        turn_events(&mut rx).await,
        vec![(pane(1), 1, delta("wait")), (pane(1), 1, done())]
    );
}

#[tokio::test(start_paused = true)]
async fn cancel_stops_the_events_and_drops_the_adapter() {
    let dropped = Arc::new(AtomicBool::new(false));
    struct Flag(Arc<AtomicBool>);
    impl Drop for Flag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let flag = dropped.clone();
    let runner: Runner = Arc::new(move |_request, tx| {
        let flag = Flag(flag.clone());
        Box::pin(async move {
            let _flag = flag;
            loop {
                if tx.send(delta("x")).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
    });
    let (mut chats, mut rx) = chats(runner);
    chats.send(pane(1), 1, request(""));
    assert_eq!(next(&mut rx).await, (pane(1), 1, delta("x")));
    chats.cancel(pane(1));
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(rx.try_recv().is_err(), "no event after a cancel");
    assert!(dropped.load(Ordering::SeqCst), "the adapter was dropped");
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_pane_without_a_turn_does_nothing() {
    let (mut chats, mut rx) = chats(scripted(vec![done()]));
    chats.cancel(pane(9));
    chats.send(pane(1), 1, request(""));
    chats.cancel(pane(9));
    assert_eq!(turn_events(&mut rx).await, vec![(pane(1), 1, done())]);
}

#[tokio::test(start_paused = true)]
async fn an_adapter_that_ends_without_a_final_event_interrupts_the_turn() {
    let (mut chats, mut rx) = chats(scripted(vec![delta("par")]));
    chats.send(pane(1), 1, request(""));
    assert_eq!(
        turn_events(&mut rx).await,
        vec![
            (pane(1), 1, delta("par")),
            (pane(1), 1, StreamEvent::Error(ChatError::Interrupted)),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn an_adapter_that_panics_interrupts_the_turn() {
    let runner: Runner = Arc::new(|_request, tx| {
        Box::pin(async move {
            let _ = tx.send(delta("par")).await;
            panic!("adapter bug");
        })
    });
    let (mut chats, mut rx) = chats(runner);
    chats.send(pane(1), 1, request(""));
    assert_eq!(
        turn_events(&mut rx).await.last(),
        Some(&(pane(1), 1, StreamEvent::Error(ChatError::Interrupted)))
    );
}

#[tokio::test(start_paused = true)]
async fn a_runner_that_panics_before_returning_its_future_interrupts_the_turn() {
    let runner: Runner = Arc::new(|_request, _tx| panic!("runner bug before the future"));
    let (mut chats, mut rx) = chats(runner);
    chats.send(pane(1), 1, request(""));
    assert_eq!(
        turn_events(&mut rx).await,
        vec![(pane(1), 1, StreamEvent::Error(ChatError::Interrupted))]
    );
}

/// Records `in_adapter_poll()` when dropped, even while a panic unwinds.
struct SeesFlagOnDrop(Arc<AtomicBool>);

impl Drop for SeesFlagOnDrop {
    fn drop(&mut self) {
        self.0.store(in_adapter_poll(), Ordering::SeqCst);
    }
}

#[tokio::test]
async fn the_adapter_flag_is_set_only_during_its_polls() {
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    assert!(!in_adapter_poll());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let mut inside = pin!(scoped(async move {
        log.lock().unwrap().push(in_adapter_poll());
        tokio::task::yield_now().await;
        log.lock().unwrap().push(in_adapter_poll());
    }));
    let mut cx = Context::from_waker(Waker::noop());

    // First poll: the future runs, then returns Pending at the yield.
    assert!(inside.as_mut().poll(&mut cx).is_pending());
    assert!(!in_adapter_poll(), "off between polls");
    // Second poll: it finishes.
    assert_eq!(inside.as_mut().poll(&mut cx), Poll::Ready(()));
    assert!(!in_adapter_poll(), "cleared once the poll returns");
    assert_eq!(
        *seen.lock().unwrap(),
        vec![true, true],
        "set inside every poll"
    );
}

#[tokio::test]
async fn the_adapter_flag_is_set_while_a_panic_unwinds_and_cleared_after() {
    let at_unwind = Arc::new(AtomicBool::new(false));
    let seen = at_unwind.clone();
    let result = AssertUnwindSafe(scoped(async move {
        let _guard = SeesFlagOnDrop(seen);
        panic!("adapter bug");
    }))
    .catch_unwind()
    .await;
    assert!(result.is_err());
    assert!(
        at_unwind.load(Ordering::SeqCst),
        "the panic hook would still see the flag"
    );
    assert!(!in_adapter_poll(), "a panic must not leave the flag set");
}

#[tokio::test]
async fn nested_adapter_polls_keep_the_flag_until_the_outermost_ends() {
    let outer = scoped(async {
        scoped(async {}).await;
        in_adapter_poll()
    });
    assert!(outer.await);
    assert!(!in_adapter_poll());
}

#[tokio::test(start_paused = true)]
async fn a_lingering_sender_does_not_keep_a_finished_adapter_turn_open() {
    let lingering = Arc::new(Mutex::new(None));
    let slot = lingering.clone();
    let runner: Runner = Arc::new(move |_request, tx| {
        let slot = slot.clone();
        Box::pin(async move {
            let _ = tx.send(delta("one")).await;
            let _ = tx.send(delta("two")).await;
            // A detached task keeps a sender alive, and never sends again.
            let held = tx.clone();
            *slot.lock().unwrap() = Some(tokio::spawn(async move {
                let _held = held;
                std::future::pending::<()>().await;
            }));
        })
    });
    let (mut chats, mut rx) = chats(runner);
    chats.send(pane(1), 1, request(""));
    // Events sent before the adapter finished still arrive, in order.
    assert_eq!(
        turn_events(&mut rx).await,
        vec![
            (pane(1), 1, delta("one")),
            (pane(1), 1, delta("two")),
            (pane(1), 1, StreamEvent::Error(ChatError::Interrupted)),
        ]
    );
    if let Some(task) = lingering.lock().unwrap().take() {
        task.abort();
    }
}

#[tokio::test(start_paused = true)]
async fn a_failure_reaches_the_app_as_one_clean_line() {
    let (mut chats, mut rx) = chats(scripted(vec![failed("bad\u{1b}[31m\nnews")]));
    chats.send(pane(1), 1, request(""));
    assert_eq!(
        turn_events(&mut rx).await,
        vec![(pane(1), 1, failed("bad news"))]
    );
}

#[tokio::test(start_paused = true)]
async fn the_adapter_gets_a_channel_with_the_documented_capacity() {
    let seen = Arc::new(Mutex::new(None));
    let sink = seen.clone();
    let runner: Runner = Arc::new(move |_request, tx| {
        *sink.lock().unwrap() = Some(tx.capacity());
        Box::pin(async move {
            let _ = tx.send(done()).await;
        })
    });
    let (mut chats, mut rx) = chats(runner);
    chats.send(pane(1), 1, request(""));
    turn_events(&mut rx).await;
    assert_eq!(*seen.lock().unwrap(), Some(TURN_CHANNEL));
    // Nothing is queued before the first byte, so the one best-effort `Slow`
    // always finds room.
    const { assert!(TURN_CHANNEL > 1) };
}

// End to end with the pure state: nothing unsafe or oversized reaches it.
fn ready() -> Availability {
    Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    }
}

#[tokio::test(start_paused = true)]
async fn escapes_split_across_deltas_never_reach_the_chat_state() {
    let hostile = [
        "he\u{1b}[3",
        "1mllo \u{1b}]52;c;",
        "SGVsbG8=\u{7}wor",
        "ld\u{9b}2J\u{9d}0;t\u{9c}!",
        " \u{202e}rtl\u{2069}\u{200b}\u{feff}",
        "\u{1b}",
        "[2Jdone\u{0}",
    ];
    let mut events: Vec<StreamEvent> = hostile.iter().map(|t| delta(t)).collect();
    events.push(done());
    let (mut chats, mut rx) = chats(scripted(events));
    let mut state = ChatState::new(None);
    let crate::core::ai::chat::SendOutcome::Sent { turn, request } = state.send("hi", &ready(), "")
    else {
        panic!("send refused");
    };
    chats.send(pane(1), turn, request);
    for (_, id, event) in turn_events(&mut rx).await {
        assert_eq!(state.apply(id, event), Follow::Nothing);
    }
    let answer = state.messages().last().unwrap();
    assert_eq!(answer.text, "hello world2J0;t! rtldone");
    assert!(
        !answer.text.chars().any(|c| c.is_control()
            || matches!(
                c,
                '\u{202e}' | '\u{2066}' | '\u{2069}' | '\u{200b}' | '\u{feff}'
            )),
        "{:?}",
        answer.text
    );
    assert_eq!(answer.status, MsgStatus::Complete);
}

#[tokio::test(start_paused = true)]
async fn an_endless_answer_is_cut_at_1_mib_without_a_second_report() {
    let sent = Arc::new(Mutex::new(0usize));
    let counter = sent.clone();
    let runner: Runner = Arc::new(move |_request, tx| {
        let counter = counter.clone();
        Box::pin(async move {
            let block = "a".repeat(64 * 1024);
            loop {
                if tx.send(delta(&block)).await.is_err() {
                    return;
                }
                *counter.lock().unwrap() += 1;
            }
        })
    });
    let (mut chats, mut rx) = chats(runner);
    let mut state = ChatState::new(None);
    let crate::core::ai::chat::SendOutcome::Sent { turn, request } = state.send("hi", &ready(), "")
    else {
        panic!("send refused");
    };
    chats.send(pane(1), turn, request);
    let events = turn_events(&mut rx).await;
    for (_, id, event) in events {
        assert_eq!(
            state.apply(id, event),
            Follow::Nothing,
            "the state never has to cancel the engine: the runtime stopped first"
        );
    }
    let answer = state.messages().last().unwrap();
    assert_eq!(
        (answer.text.len(), answer.status),
        (ANSWER_CAP, MsgStatus::Truncated)
    );
    // The turn ended and the endless adapter stopped with it.
    let before = *sent.lock().unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(*sent.lock().unwrap(), before);
    assert!(rx.try_recv().is_err());
}

// Real sockets (real clock) --------------------------------------------------

/// A one-shot loopback server: reads the request, writes `reply`, then keeps
/// the connection until the client goes away (`linger`) or closes it.
struct Server {
    port: u16,
    request_read: Arc<Notify>,
    closed: Arc<Notify>,
}

async fn serve(reply: &'static [u8], linger: bool) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = Server {
        port: listener.local_addr().unwrap().port(),
        request_read: Arc::default(),
        closed: Arc::default(),
    };
    let (request_read, closed) = (server.request_read.clone(), server.closed.clone());
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let mut raw = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = stream.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            raw.extend_from_slice(&buf[..n]);
            if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        request_read.notify_one();
        let _ = stream.write_all(reply).await;
        if !linger {
            let _ = stream.shutdown().await;
        }
        while matches!(stream.read(&mut buf).await, Ok(n) if n > 0) {}
        closed.notify_one();
    });
    server
}

fn http_runner(wire: Wire, port: u16, path: &'static str) -> Runner {
    Arc::new(move |_request, tx| Box::pin(http_turn(wire, port, path, b"{}".to_vec(), tx)))
}

const OK_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";

#[tokio::test]
async fn each_turn_gets_a_decoder_that_has_seen_nothing() {
    // Turn 1 ends in the middle of a line; its leftover must not reach turn 2.
    let first = serve(
        b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"message\":{\"content\":\"par",
        false,
    )
    .await;
    let second = serve(
        b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"message\":{\"content\":\"hi\"},\"done\":false}\n{\"message\":{\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\"}\n",
        false,
    )
    .await;
    let (mut tx_chats, mut rx) = chats(http_runner(Wire::Ndjson, first.port, "/api/chat"));
    tx_chats.send(pane(1), 1, request(""));
    let one = turn_events(&mut rx).await;
    assert_eq!(
        one.last(),
        Some(&(pane(1), 1, failed("malformed stream data")))
    );
    tx_chats.runner = http_runner(Wire::Ndjson, second.port, "/api/chat");
    tx_chats.send(pane(1), 2, request(""));
    assert_eq!(
        turn_events(&mut rx).await,
        vec![(pane(1), 2, delta("hi")), (pane(1), 2, done())]
    );
}

#[tokio::test]
async fn the_sse_wire_decodes_through_the_same_task() {
    let server = serve(
        b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"yo\"}}]}\n\ndata: [DONE]\n\n",
        false,
    )
    .await;
    let (mut chats, mut rx) = chats(http_runner(Wire::Sse, server.port, "/v1/chat/completions"));
    chats.send(pane(1), 1, request(""));
    assert_eq!(
        turn_events(&mut rx).await,
        vec![(pane(1), 1, delta("yo")), (pane(1), 1, done())]
    );
}

#[tokio::test]
async fn cancel_closes_the_socket() {
    let server = serve(OK_HEAD, true).await;
    let (mut chats, _rx) = chats(http_runner(Wire::Ndjson, server.port, "/api/chat"));
    chats.send(pane(1), 1, request(""));
    timeout(Duration::from_secs(5), server.request_read.notified())
        .await
        .expect("the server read the request");
    let started = Instant::now();
    chats.cancel(pane(1));
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "cancel returns at once"
    );
    timeout(Duration::from_secs(10), server.closed.notified())
        .await
        .expect("the server saw the connection close");
}

#[tokio::test]
async fn shutdown_closes_every_socket() {
    let a = serve(OK_HEAD, true).await;
    let b = serve(OK_HEAD, true).await;
    let ports = [a.port, b.port];
    let runner: Runner = Arc::new(move |request, tx| {
        let port = ports[usize::from(request.system == "b")];
        Box::pin(http_turn(
            Wire::Ndjson,
            port,
            "/api/chat",
            b"{}".to_vec(),
            tx,
        ))
    });
    let (mut chats, _rx) = chats(runner);
    chats.send(pane(1), 1, request("a"));
    chats.send(pane(2), 1, request("b"));
    for server in [&a, &b] {
        timeout(Duration::from_secs(5), server.request_read.notified())
            .await
            .expect("the server read the request");
    }
    chats.shutdown(Duration::from_secs(3)).await;
    for server in [&a, &b] {
        timeout(Duration::from_secs(1), server.closed.notified())
            .await
            .expect("the connection is closed");
    }
}

// Real processes (real clock) ------------------------------------------------

/// A runner whose engine is a process group that ignores SIGTERM and SIGHUP:
/// a shell and a `sleep` grandchild. It reports `(leader, grandchild)` pids.
fn process_runner(pids: Arc<Mutex<Vec<(i32, i32)>>>) -> Runner {
    Arc::new(move |_request, tx| {
        let pids = pids.clone();
        Box::pin(async move {
            let mut child = Command::new("sh")
                .args(["-c", "trap '' TERM HUP; sleep 300 & echo $!; wait"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .process_group(0)
                .spawn()
                .expect("sh starts");
            let leader = child.id().expect("a pid") as libc::pid_t;
            // Declared after `child`, so it drops first.
            let mut group = GroupKill::new(leader).expect("a fresh group is a valid target");
            let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
            let grandchild: i32 = lines
                .next_line()
                .await
                .unwrap()
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            pids.lock().unwrap().push((leader, grandchild));
            let _ = tx.send(delta("started")).await;
            // Reap and disarm in one step: the safe pattern real adapters copy.
            let _ = group.reap(&mut child).await;
        })
    })
}

/// No such process, or only a zombie nobody has collected yet.
fn is_gone(pid: i32, allow_zombie: bool) -> bool {
    match crate::runtime::proc::process_state(pid) {
        None => true,
        Some(state) => allow_zombie && state == 'Z',
    }
}

/// How long a group may take to die once it was signalled. Polling returns as
/// soon as it is gone, so a generous bound costs nothing on a pass and keeps a
/// loaded CI runner from failing the tests.
const GONE: Duration = Duration::from_secs(20);

/// Waits up to `bound` for the leader to be reaped and the grandchild to die.
async fn wait_gone(pids: &[(i32, i32)], bound: Duration) -> bool {
    let deadline = Instant::now() + bound;
    loop {
        // The leader is our child: gone means reaped. The grandchild was
        // re-parented, and a container's init may not collect it quickly.
        if pids
            .iter()
            .all(|&(leader, grand)| is_gone(leader, false) && is_gone(grand, true))
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn started(rx: &mut mpsc::Receiver<Tagged>, n: usize) {
    for _ in 0..n {
        let event = next(rx).await;
        assert_eq!(event.2, delta("started"));
    }
}

#[tokio::test]
async fn cancel_kills_and_reaps_a_group_that_ignores_sigterm() {
    let pids = Arc::new(Mutex::new(Vec::new()));
    let (mut chats, mut rx) = chats(process_runner(pids.clone()));
    chats.send(pane(1), 1, request(""));
    started(&mut rx, 1).await;
    let pids = pids.lock().unwrap().clone();
    assert!(!is_gone(pids[0].0, false), "the leader runs");
    assert!(!is_gone(pids[0].1, true), "the grandchild runs");
    let at = Instant::now();
    chats.cancel(pane(1));
    // `cancel` only signals: it must not wait for the group to die.
    assert!(
        at.elapsed() < Duration::from_millis(500),
        "cancel returns at once"
    );
    assert!(
        wait_gone(&pids, GONE).await,
        "the group is dead and reaped: {pids:?}"
    );
}

#[tokio::test]
async fn shutdown_kills_every_engine_process_group() {
    let pids = Arc::new(Mutex::new(Vec::new()));
    let (mut chats, mut rx) = chats(process_runner(pids.clone()));
    chats.send(pane(1), 1, request(""));
    chats.send(pane(2), 1, request(""));
    started(&mut rx, 2).await;
    let pids = pids.lock().unwrap().clone();
    assert_eq!(pids.len(), 2);
    chats.shutdown(Duration::from_secs(3)).await;
    assert!(wait_gone(&pids, GONE).await, "{pids:?}");
}

#[test]
fn a_notice_is_sanitized_flattened_and_dropped_after_the_end() {
    let mut pipe = Pipe::new();
    assert_eq!(
        pipe.event(StreamEvent::Notice("a\n\u{1b}[31mb\tc".into())),
        vec![StreamEvent::Notice("a b c".into())]
    );
    pipe.event(StreamEvent::Done(Finish::Stop));
    assert_eq!(pipe.event(StreamEvent::Notice("late".into())), vec![]);
}

fn tool_call(id: u64, summary: &str) -> StreamEvent {
    StreamEvent::ToolCall {
        id: crate::core::ai::chat::engine::CallId(id),
        name: "find_dir".into(),
        summary: summary.into(),
        approval: None,
    }
}

fn tool_result(id: u64, summary: &str) -> StreamEvent {
    StreamEvent::ToolResult {
        id: crate::core::ai::chat::engine::CallId(id),
        status: crate::core::ai::chat::engine::ToolStatus::Ok,
        summary: summary.into(),
    }
}

#[test]
fn tool_summaries_are_flattened_sanitized_and_capped_on_the_way_to_the_app() {
    use crate::core::ai::tools::summary::SUMMARY_CAP;
    let mut pipe = Pipe::new();
    let out = pipe.event(tool_call(1, "a\u{1b}[31m red\nline\u{7}"));
    assert_eq!(out, [tool_call(1, "a red line")]);
    let long = "x".repeat(SUMMARY_CAP * 3);
    let StreamEvent::ToolResult { summary, .. } = &pipe.event(tool_result(1, &long))[0] else {
        panic!("not a result");
    };
    assert_eq!(summary.chars().count(), SUMMARY_CAP);
}

#[test]
fn tool_events_keep_their_place_between_deltas_and_stop_after_the_final_event() {
    let mut pipe = Pipe::new();
    let mut out = pipe.event(tool_call(1, "q"));
    out.extend(pipe.event(tool_result(1, "ok")));
    out.extend(pipe.event(delta("text")));
    out.extend(pipe.event(done()));
    assert!(matches!(out[0], StreamEvent::ToolCall { .. }));
    assert!(matches!(out[1], StreamEvent::ToolResult { .. }));
    assert_eq!(text_of(&out), "text");
    assert_eq!(out.last(), Some(&done()));
    // A late result, or a late call, is dropped here.
    assert!(pipe.event(tool_result(2, "late")).is_empty());
    assert!(pipe.event(tool_call(3, "late")).is_empty());
}

#[test]
fn answer_text_that_imitates_a_tool_row_stays_a_delta() {
    let mut pipe = Pipe::new();
    let out = pipe.bytes("» find_dir x\n  ok: 1 result".as_bytes());
    assert!(
        out.iter().all(|e| matches!(e, StreamEvent::Delta(_))),
        "{out:?}"
    );
}
