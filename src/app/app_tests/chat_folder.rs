//! The chat folder (chat-folder): when it may change, and what a change does.

use super::run_pane::one_shell_and_a_chat;
use super::tool_approval::{announce, run};
use super::tool_run::run_live;
use super::*;

#[test]
fn an_idle_chat_may_change_folder() {
    let a = one_shell_and_a_chat("hello");
    assert_eq!(a.folder_busy(id2()), None);
}

#[test]
fn a_command_running_for_the_chat_blocks_the_change() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("cd ~/dev"));
    assert_eq!(a.folder_busy(id2()), Some("a command is running"));
}

#[test]
fn a_chat_that_is_gone_is_not_busy() {
    let a = one_shell_and_a_chat("hello");
    assert_eq!(a.folder_busy(PaneId::for_test(99)), None);
}
