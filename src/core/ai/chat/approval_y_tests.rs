//! `may_run_on_y`: the one place that decides whether `y` runs a model's command.

use super::BlockWhy;
use super::approval::*;
use crate::core::ai::orchestra::WorkerKey;

fn run_ask() -> ApprovalAsk {
    ApprovalAsk::Run {
        command: "cargo build".into(),
    }
}

#[test]
fn the_default_policy_lets_y_run_a_chat_command() {
    assert_eq!(may_run_on_y(&run_ask(), None, RunPolicy::default()), Ok(()));
}

#[test]
fn a_blocked_conversation_gives_typed_only_with_the_block_reason() {
    for why in [BlockWhy::Marker, BlockWhy::NewerRows] {
        assert_eq!(
            may_run_on_y(&run_ask(), Some(why), RunPolicy::default()),
            Err(why.reason())
        );
    }
}

#[test]
fn a_worker_ask_never_runs_on_y() {
    let ask = ApprovalAsk::WorkerRead {
        path: "a".into(),
        label: "w".into(),
        worker: WorkerKey::for_test(1, 1),
        reach: Reach::Other,
        cloud: true,
    };
    assert_eq!(
        may_run_on_y(&ask, None, RunPolicy::default()),
        Err(WORKER_TYPED)
    );
}

#[test]
fn a_policy_that_forbids_runs_gives_typed_only_with_its_reason() {
    let policy = RunPolicy { y_runs: false };
    assert_eq!(
        may_run_on_y(&run_ask(), None, policy),
        Err("commands are not run in this chat (policy)")
    );
    assert_eq!(POLICY_TYPED, "commands are not run in this chat (policy)");
}

#[test]
fn the_block_reason_comes_before_the_policy_reason() {
    let policy = RunPolicy { y_runs: false };
    assert_eq!(
        may_run_on_y(&run_ask(), Some(BlockWhy::Marker), policy),
        Err(BlockWhy::Marker.reason())
    );
}

#[test]
fn the_key_hints_read_as_the_spec_words() {
    assert_eq!(RUN_KEYS, "y run it · n cancel · p next pane");
    assert_eq!(TYPE_KEYS, "y type it (no Enter) · n cancel · p next pane");
}
