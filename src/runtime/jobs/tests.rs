use std::{
    io::{BufRead, BufReader},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::Duration,
};

use super::*;
use crate::runtime::proc::start_time;

/// A shell leading its own session, like a pane's, with job control on so
/// each background job gets its own group; it prints `ready` once they run.
/// bash, as dash turns job control off without a terminal.
fn pane(body: &str) -> (Child, Session) {
    let mut command = Command::new("bash");
    command
        .args(["-c", &format!("set -m\n{body}\necho ready\nwait")])
        .stdout(Stdio::piped());
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let sid = child.id() as libc::pid_t;
    let session = Session {
        sid,
        start: start_time(sid).unwrap(),
    };
    (child, session)
}

/// Kills the whole session the test started.
fn end(mut child: Child, jobs: &[Job]) {
    for job in jobs {
        // SAFETY: killpg only sends a signal to a group the test started.
        unsafe { libc::killpg(job.pgid, libc::SIGKILL) };
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn commands(jobs: &[Job]) -> Vec<&str> {
    jobs.iter().map(|job| job.command.as_str()).collect()
}

// Spec S7: the jobs of a pane are named, its shell is not.
#[test]
fn the_background_jobs_of_a_pane_are_named_and_its_shell_is_not() {
    let (child, session) = pane("sleep 31 & sleep 32 &");
    let jobs = running(&[session]);
    assert_eq!(commands(&jobs), ["sleep 31", "sleep 32"]);
    assert!(jobs.iter().all(|job| job.pgid != session.sid));
    end(child, &jobs);
}

#[test]
fn a_job_outlives_its_shell_and_is_still_named() {
    let (mut child, session) = pane("sleep 33 &");
    let jobs = running(&[session]);
    // The shell exits (like `exit` in the last pane); its job stays.
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(commands(&running(&[session])), ["sleep 33"]);
    end(child, &jobs);
}

#[test]
fn a_session_whose_leader_is_another_process_is_not_ours() {
    let (child, session) = pane("sleep 34 &");
    let jobs = running(&[session]);
    assert_eq!(jobs.len(), 1);
    let reused = Session {
        start: session.start + 1,
        ..session
    };
    assert!(running(&[reused]).is_empty());
    end(child, &jobs);
}

#[test]
fn no_session_means_nothing_runs() {
    assert!(running(&[]).is_empty());
}

fn job(pgid: libc::pid_t, command: &str) -> Job {
    Job {
        pgid,
        command: command.to_owned(),
    }
}

// Spec S7: after a quit without the prompt the list is written out.
#[test]
fn the_report_names_each_job_with_its_group() {
    assert_eq!(
        report(&[job(41, "sleep 77"), job(52, "npm run dev")]).as_deref(),
        Some(
            "lazarobox-shell: still running from your panes:\n  \
             sleep 77 (process group 41)\n  npm run dev (process group 52)\n"
        )
    );
    assert_eq!(report(&[]), None);
}

#[test]
fn what_is_left_running_is_read_again_after_the_hangup_settles() {
    let (child, session) = pane("sleep 35 & sleep 0.2 &");
    let jobs = running(&[session]);
    assert_eq!(commands(&jobs), ["sleep 35", "sleep 0.2"]);
    assert_eq!(
        commands(&left_running(&[session], Duration::from_millis(600))),
        ["sleep 35"]
    );
    end(child, &jobs);
}
