//! The watcher as lazarobox starts it: `lazarobox-shell __watch`, in its own
//! session, stopping what was listed once its stdin ends.

use std::{
    io::Write,
    os::unix::process::{CommandExt, ExitStatusExt},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use lazarobox_shell::runtime::{proc::start_time, watch::SUBCOMMAND};

#[test]
fn the_watcher_stops_the_listed_group_once_lazarobox_closes_its_stdin() {
    let mut group = Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let pgid = group.id() as libc::pid_t;
    let mut watcher = Command::new(env!("CARGO_BIN_EXE_lazarobox-shell"))
        .arg(SUBCOMMAND)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let line = format!("+ {pgid} {}\n", start_time(pgid).unwrap());
    let mut stdin = watcher.stdin.take().unwrap();
    stdin.write_all(line.as_bytes()).unwrap();
    // What the kernel does when lazarobox ends, whatever the way.
    drop(stdin);
    let status = group.wait().unwrap();
    assert_eq!(status.signal(), Some(libc::SIGHUP));
    let deadline = Instant::now() + Duration::from_secs(5);
    while watcher.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the watcher did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(watcher.wait().unwrap().success());
}
