use std::{
    io::{BufRead, BufReader, Cursor},
    os::unix::process::{CommandExt, ExitStatusExt},
    process::{Child, Command, Stdio},
};

use super::*;
use crate::runtime::proc::process_state;

/// `sh -c body` as the leader of its own process group.
fn group(body: &str) -> Child {
    Command::new("sh")
        .args(["-c", body])
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap()
}

fn pgid(child: &Child) -> libc::pid_t {
    child.id() as libc::pid_t
}

/// The `+` line lazarobox sends for `child`.
fn listed(child: &Child) -> String {
    let pid = pgid(child);
    format!("+ {pid} {}\n", start_time(pid).unwrap())
}

/// The signal that ended `child`, reaping it.
fn ended_by(mut child: Child) -> Option<i32> {
    child.wait().unwrap().signal()
}

/// Running, or killed and reaped by the caller: the test cleans up after
/// a group the watcher had to leave alone.
fn still_running(child: &mut Child) -> bool {
    let running = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let _ = child.wait();
    running
}

/// Gone or a zombie waiting to be reaped: either way no longer running.
fn dead(pid: libc::pid_t) -> bool {
    matches!(process_state(pid), None | Some('Z'))
}

const SHORT: Duration = Duration::from_millis(300);

// Spec S5: what lazarobox started is stopped once it ends.
#[test]
fn a_listed_group_and_its_children_are_stopped_when_the_list_ends() {
    let mut child = group("sleep 30 & echo $!; wait");
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let grandchild: libc::pid_t = line.trim().parse().unwrap();
    watch(Cursor::new(listed(&child)), SHORT);
    assert_eq!(ended_by(child), Some(libc::SIGHUP), "asked politely first");
    let deadline = Instant::now() + Duration::from_secs(2);
    while !dead(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(dead(grandchild), "the group's child is left running");
}

#[test]
fn a_group_that_ignores_hup_and_term_is_killed_after_the_grace() {
    let mut child = group("trap '' HUP TERM; echo ready; exec sleep 30");
    // Only once the trap is set: a signal before it would end the shell.
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let started = Instant::now();
    watch(Cursor::new(listed(&child)), SHORT);
    assert!(started.elapsed() >= SHORT, "SIGKILL came before the grace");
    assert_eq!(ended_by(child), Some(libc::SIGKILL));
}

#[test]
fn a_group_lazarobox_reaped_is_left_alone() {
    let mut child = group("exec sleep 30");
    let input = format!("{}- {}\n", listed(&child), pgid(&child));
    watch(Cursor::new(input), SHORT);
    assert!(still_running(&mut child));
}

#[test]
fn a_leader_that_is_not_the_listed_process_is_left_alone() {
    let mut child = group("exec sleep 30");
    let pid = pgid(&child);
    // Another start time: the pid was reused by an unrelated process.
    let input = format!("+ {pid} {}\n", start_time(pid).unwrap() + 1);
    watch(Cursor::new(input), SHORT);
    assert!(still_running(&mut child));
}

#[test]
fn an_empty_list_ends_at_once() {
    let started = Instant::now();
    watch(Cursor::new(""), GRACE);
    assert!(started.elapsed() < Duration::from_millis(100));
}

#[test]
fn only_well_formed_lines_change_the_list() {
    let mut groups = BTreeMap::new();
    for line in [
        "+ 12", "+ x 3", "+ 12 y", "+ 12 3 4", "-", "- x", "garbage", "",
    ] {
        apply(&mut groups, line);
    }
    assert!(groups.is_empty());
    apply(&mut groups, "+ 12 3");
    apply(&mut groups, "+ 13 4");
    apply(&mut groups, "- 12");
    assert_eq!(groups, BTreeMap::from([(13, 4)]));
}

#[test]
fn groups_0_1_and_the_watcher_s_own_are_never_targets() {
    let groups = BTreeMap::from([(0, 1), (1, 1), (50, 1), (60, 1), (70, 1), (80, 1)]);
    // 60 is ours; 70 now has another leader; 80 has no leader left.
    let started = |pid| match pid {
        70 => Some(2),
        80 => None,
        _ => Some(1),
    };
    assert_eq!(targets(&groups, 60, started), vec![(50, 1), (80, 1)]);
}

/// Waits for the line `sh` prints once its traps are set.
fn ready(child: &mut Child) {
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
}

fn listed_with(child: &Child) -> Listed {
    let listed = Listed::default();
    listed.add(pgid(child), start_time(pgid(child)).unwrap());
    listed
}

// Spec S6: quitting asks first, so a program can end on its own.
#[test]
fn quitting_asks_a_listed_group_to_end_before_any_kill() {
    let mut child = group("trap 'exit 7' HUP TERM; echo ready; sleep 30 & wait");
    ready(&mut child);
    let started = Instant::now();
    listed_with(&child).stop(Duration::from_secs(2));
    // Its leader is our child, a zombie until we reap it: that is an end.
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "it waited the grace"
    );
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(7), "it was killed: {status:?}");
}

#[test]
fn quitting_kills_a_group_that_ignores_the_polite_signals_after_the_grace() {
    let mut child = group("trap '' HUP TERM; echo ready; exec sleep 30");
    ready(&mut child);
    let started = Instant::now();
    listed_with(&child).stop(SHORT);
    assert!(started.elapsed() >= SHORT, "SIGKILL came before the grace");
    assert_eq!(ended_by(child), Some(libc::SIGKILL));
}

#[test]
fn a_group_reaped_during_the_grace_gets_no_kill() {
    let mut child = group("trap '' HUP TERM; echo ready; exec sleep 30");
    ready(&mut child);
    let listed = std::sync::Arc::new(listed_with(&child));
    let pid = pgid(&child);
    let remover = {
        let listed = std::sync::Arc::clone(&listed);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            listed.remove(pid);
        })
    };
    listed.stop(SHORT);
    remover.join().unwrap();
    assert!(still_running(&mut child), "a group off the list was killed");
}

#[test]
fn quitting_with_nothing_listed_ends_at_once() {
    let started = Instant::now();
    Listed::default().stop(GRACE);
    assert!(started.elapsed() < Duration::from_millis(100));
}

// Spec S6 (verifier A1): once lazarobox is gone, what a leader left behind
// still gets the grace to end on its own.
#[test]
fn the_watcher_gives_the_rest_of_a_group_the_grace_after_its_leader_ends() {
    let lab = tempfile::tempdir().unwrap();
    let done = lab.path().join("done");
    let body = format!(
        "trap 'exit 0' HUP TERM
        (trap 'sleep 0.3; echo graceful > \"{}\"; exit 0' HUP TERM
         echo ready
         while :; do sleep 0.05; done) &
        wait",
        done.display()
    );
    let mut child = group(&body);
    ready(&mut child);
    watch(Cursor::new(listed(&child)), Duration::from_secs(1));
    let _ = child.wait();
    assert_eq!(
        std::fs::read_to_string(&done).ok().as_deref(),
        Some("graceful\n"),
        "the rest of the group was killed before its grace"
    );
}
