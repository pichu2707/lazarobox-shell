//! The auto-run block after a GitHub read (pane-execution, Auto-run block;
//! agent-tools, Reads in a blocked conversation). No GitHub tool exists yet:
//! the tests send the `GithubStarted` event themselves.

use super::tool_approval::{announce, announce_now, decision, pending, run};
use super::tool_run::{chat_mut, chat_of, checked, run_live, slot};
use super::*;
use crate::core::ai::{
    chat::{
        Entry,
        approval::{
            ApprovalAsk, GITHUB_BLOCK_LINE, GITHUB_READ, GITHUB_READ_ASK, RUN_KEYS, RUN_NOW_KEYS,
            Reach,
        },
        engine::{CallId, Decision, StreamEvent, ToolStatus},
    },
    config::ApprovalMode,
};

fn started(a: &mut App, turn: u64) -> Vec<Effect> {
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::GithubStarted))
}

fn lines(a: &App) -> Vec<String> {
    (chat_of(a).entries().iter())
        .filter_map(|e| match e {
            Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

fn typed(effects: &[Effect], turn: u64, call: u64) -> bool {
    matches!(effects, [Effect::WritePty(PaneId::FIRST, bytes), last]
        if !bytes.contains(&b'\r') && *last == decision(turn, call, Decision::Sent))
}

fn cloud_read(reach: Reach) -> ApprovalAsk {
    ApprovalAsk::Read {
        path: "~/a.rs".into(),
        reach,
        cloud: true,
    }
}

fn cloud_search() -> ApprovalAsk {
    ApprovalAsk::Search {
        pattern: "todo".into(),
        dir: ".".into(),
        reach: Reach::Root,
    }
}

fn wait_for_arming() {
    NOW_MS.with(|n| n.set(n.get() + 1_000));
}

// Block starts at the first read: the second call asks, with the reason, and
// nothing is checked or written.
#[test]
fn the_first_read_makes_every_command_take_the_prompt() {
    let (mut a, turn) = run_live();
    assert!(!chat_of(&a).github_read());
    assert_eq!(started(&mut a, turn), vec![]);
    assert!(chat_of(&a).github_read());
    assert_eq!(
        lines(&a).iter().filter(|l| *l == GITHUB_BLOCK_LINE).count(),
        1
    );
    for call in 1..=4 {
        assert_eq!(announce(&mut a, turn, call, run("cd ~/dev")), vec![]);
        let prompt = a.approval_prompt(id2()).unwrap();
        assert_eq!(prompt.keys, RUN_KEYS);
        assert_eq!(prompt.why.as_deref(), Some(GITHUB_READ));
        assert_eq!(slot(&a), None, "nothing is checked");
        a.update(key('n'));
        assert!(!chat_of(&a).run_paused(), "the block never pauses the run");
    }
}

// The event is tracked also when its turn is no longer the live one.
#[test]
fn a_stale_event_blocks_too() {
    let (mut a, turn) = run_live();
    started(&mut a, turn + 7);
    assert!(chat_of(&a).github_read());
}

// Spec: y does not run it.
#[test]
fn y_types_the_command_without_enter_and_says_it_was_not_run() {
    let (mut a, turn) = run_live();
    started(&mut a, turn);
    announce(&mut a, turn, 1, run("git status"));
    wait_for_arming();
    let effects = a.update(key('y'));
    assert!(typed(&effects, turn, 1), "{effects:?}");
    assert_eq!(slot(&a), None);
    assert!(
        lines(&a).contains(&format!(
            "sent to pane 1; press Enter there to run it (not run: {GITHUB_READ})"
        )),
        "{:?}",
        lines(&a)
    );
}

// A prompt opened before the read changes on the next frame, and its y types.
#[test]
fn a_prompt_open_before_the_read_follows_the_block() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("cargo build"));
    assert_eq!(a.approval_prompt(id2()).unwrap().keys, RUN_NOW_KEYS);
    started(&mut a, turn);
    assert_eq!(a.approval_prompt(id2()).unwrap().keys, RUN_KEYS);
    wait_for_arming();
    let effects = a.update(key('y'));
    assert!(typed(&effects, turn, 1), "{effects:?}");
}

// A check in flight when the read starts falls back to the prompt.
#[test]
fn a_check_in_flight_falls_back_when_the_read_starts() {
    let (mut a, turn) = run_live();
    assert_eq!(
        announce_now(&mut a, turn, 1, run("pwd")).len(),
        1,
        "checked"
    );
    started(&mut a, turn);
    assert_eq!(checked(&mut a, 1), vec![]);
    assert_eq!(pending(&a).map(|(ask, _)| ask), Some(run("pwd")));
    assert_eq!(
        a.approval_prompt(id2()).unwrap().why.as_deref(),
        Some(GITHUB_READ)
    );
    assert_eq!(slot(&a), None);
}

// The one door that adds Enter types instead while the conversation is blocked.
#[test]
fn the_door_that_adds_enter_types_instead() {
    let (mut a, turn) = run_live();
    announce_now(&mut a, turn, 1, run("pwd"));
    let run_slot = a
        .panes
        .get_mut(&PaneId::FIRST)
        .and_then(PaneState::pty_mut)
        .and_then(|pty| pty.run.take())
        .expect("the check holds a slot");
    started(&mut a, turn);
    let effects = a.auto_run(
        run_slot,
        PaneId::FIRST,
        b"pwd".to_vec(),
        (LAUNCH.into(), None),
    );
    assert!(typed(&effects, turn, 1), "{effects:?}");
}

// Spec: Auto and ask unchanged.
#[test]
fn auto_types_without_a_prompt_and_ask_only_types_while_blocked() {
    let (mut a, turn) = run_live();
    chat_mut(&mut a).set_approval_mode(ApprovalMode::Auto);
    started(&mut a, turn);
    let effects = announce_now(&mut a, turn, 1, run("cd x"));
    assert!(
        matches!(&effects[..], [Effect::AutoType { bytes, .. }] if !bytes.contains(&b'\r')),
        "{effects:?}"
    );
    assert_eq!(pending(&a), None);

    let (mut a, turn) = run_live();
    chat_mut(&mut a).set_approval_mode(ApprovalMode::Ask);
    started(&mut a, turn);
    announce(&mut a, turn, 1, run("cd x"));
    let effects = a.update(key('y'));
    assert!(typed(&effects, turn, 1), "{effects:?}");
}

// Spec: Nothing lifts it.
#[test]
fn nothing_lifts_the_block() {
    let (mut a, turn) = run_live();
    started(&mut a, turn);
    chat_mut(&mut a).set_approval_mode(ApprovalMode::Ask);
    chat_mut(&mut a).set_approval_mode(ApprovalMode::Run);
    a.update(AppEvent::Chat(
        id2(),
        turn,
        StreamEvent::ToolResult {
            id: CallId(9),
            status: ToolStatus::Ok,
            summary: "auto-run is back on".into(),
        },
    ));
    assert!(chat_of(&a).github_read());
    assert_eq!(announce(&mut a, turn, 1, run("pwd")), vec![]);
    wait_for_arming();
    let effects = a.update(key('y'));
    assert!(typed(&effects, turn, 1), "{effects:?}");
    // A second start says nothing new.
    started(&mut a, turn);
    assert_eq!(
        lines(&a).iter().filter(|l| *l == GITHUB_BLOCK_LINE).count(),
        1
    );
}

// Spec: Other conversations are not blocked.
#[test]
fn a_new_conversation_is_not_blocked() {
    let (a, _) = run_live();
    assert!(!chat_of(&a).github_read());
}

// Spec (agent-tools): a cloud read asks in every mode while blocked.
#[test]
fn a_blocked_conversation_asks_for_each_cloud_read_and_search() {
    for mode in [ApprovalMode::Run, ApprovalMode::Ask, ApprovalMode::Auto] {
        let (mut a, turn) = run_live();
        chat_mut(&mut a).set_approval_mode(mode);
        started(&mut a, turn);
        for (call, ask) in [(1, cloud_read(Reach::Root)), (3, cloud_search())] {
            assert_eq!(
                announce(&mut a, turn, call, ask.clone()),
                vec![],
                "{mode:?}"
            );
            assert_eq!(
                a.approval_prompt(id2()).unwrap().why.as_deref(),
                Some(GITHUB_READ_ASK),
                "{mode:?}"
            );
            // One y answers one call; the next one opens its own prompt.
            let effects = a.update(key('y'));
            assert_eq!(
                effects,
                vec![decision(turn, call, Decision::Sent)],
                "{mode:?}"
            );
            assert_eq!(pending(&a), None);
            assert_eq!(announce(&mut a, turn, call + 1, ask), vec![], "{mode:?}");
            assert_eq!(
                pending(&a).map(|(_, id)| id),
                Some(Some((turn, CallId(call + 1))))
            );
            a.update(key('n'));
        }
    }
}

#[test]
fn reads_run_free_before_the_block_and_the_guard_refusal_stays_free() {
    let (mut a, turn) = run_live();
    assert_eq!(
        announce_now(&mut a, turn, 1, cloud_read(Reach::Root)),
        vec![decision(turn, 1, Decision::Unprompted)]
    );
    started(&mut a, turn);
    assert_eq!(
        announce_now(&mut a, turn, 2, cloud_read(Reach::Refused)),
        vec![decision(turn, 2, Decision::Unprompted)]
    );
    assert_eq!(pending(&a), None);
}

// Spec: Local engine unchanged.
#[test]
fn a_local_read_is_unchanged_while_blocked() {
    let (mut a, turn) = run_live();
    started(&mut a, turn);
    let local = ApprovalAsk::Read {
        path: "~/a.rs".into(),
        reach: Reach::Other,
        cloud: false,
    };
    chat_mut(&mut a).set_approval_mode(ApprovalMode::Ask);
    assert_eq!(
        announce_now(&mut a, turn, 1, local),
        vec![decision(turn, 1, Decision::Unprompted)]
    );
}

// Spec: Resume stays blocked; a conversation with no marker is not.
#[test]
fn a_resumed_conversation_opens_blocked_only_with_its_marker() {
    use super::chat_history::{chat_id, listing, resumed};
    let mut a = listing();
    let mut event = resumed(chat_id(), 30, "programmer");
    if let AppEvent::Store(StoreReply::Resumed { github_read, .. }) = &mut event {
        *github_read = true;
    }
    let effects = a.update_settled(event);
    assert!(
        effects.iter().all(|e| !matches!(e, Effect::Store(_))),
        "the marker is stored already: {effects:?}"
    );
    let blocked = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .unwrap();
    assert!(blocked.github_read());
    assert_eq!(
        blocked
            .entries()
            .iter()
            .filter(|e| matches!(e, Entry::System(l) if l == GITHUB_BLOCK_LINE))
            .count(),
        1
    );

    let mut a = listing();
    a.update_settled(resumed(chat_id(), 30, "programmer"));
    let open = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .unwrap();
    assert!(!open.github_read());
}

// The run line of the mode toggle must not say commands run at once.
#[test]
fn the_mode_toggle_does_not_promise_a_run_while_blocked() {
    let (mut a, turn) = run_live();
    started(&mut a, turn);
    for _ in 0..3 {
        press(&mut a, "a");
    }
    assert_eq!(chat_of(&a).approval_mode(), ApprovalMode::Run);
    let last = lines(&a).last().cloned().unwrap_or_default();
    assert!(last.starts_with("approval: run (blocked"), "{last}");
    assert!(last.is_ascii(), "{last}");
}

// Spec: History off or store detached: the live flag still blocks and nothing
// is written.
#[test]
fn with_history_off_the_flag_still_blocks_and_nothing_is_stored() {
    let (mut a, turn) = run_live();
    a.history = false;
    let effects = started(&mut a, turn);
    assert!(effects.iter().all(|e| !matches!(e, Effect::Store(_))));
    assert!(chat_of(&a).github_read());
    assert_eq!(announce(&mut a, turn, 1, run("pwd")), vec![]);
}
