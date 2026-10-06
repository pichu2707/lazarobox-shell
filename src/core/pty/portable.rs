//! portable-pty adapter.
//!
//! The reader, the writer and a waiter each run on a detached `std::thread` (a
//! `spawn_blocking` task would hang the runtime's shutdown). The reader hands
//! events to a `PtySink`; the writer is fed by a `std::sync::mpsc` channel so a
//! stalled child can never block the caller.
//!
//! The waiter owns the `Child` and blocks on `wait()`, so `Exited` is reported
//! when the shell dies even if a background job still holds the PTY open and
//! the reader never sees EOF. `Exited` is sent at most once, by whichever of
//! the reader (EOF) or the waiter gets there first.

use std::{
    io::{self, Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread,
    time::Duration,
};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::{PtyEvent, PtyHandle, PtySink, SpawnSpec};
use crate::core::pane::PaneSize;

const READ_CHUNK: usize = 8 * 1024;
/// How long the waiter lets the reader drain the last output after the child
/// died, before reporting `Exited` anyway.
const DRAIN_GRACE: Duration = Duration::from_millis(100);

/// The sink shared by the reader and the waiter, plus the once-only guard.
struct Events {
    sink: Mutex<PtySink>,
    exited: AtomicBool,
}

impl Events {
    fn send(&self, event: PtyEvent) -> bool {
        match self.sink.lock() {
            Ok(mut sink) => sink(event),
            Err(_) => false,
        }
    }

    /// Emits `Exited` unless it was already emitted.
    fn exited(&self) {
        if !self.exited.swap(true, Ordering::SeqCst) {
            self.send(PtyEvent::Exited);
        }
    }
}

fn pty_size(size: PaneSize) -> PtySize {
    PtySize {
        rows: size.rows.max(1),
        cols: size.cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

struct PortableHandle {
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// Signalled by the waiter right after `wait()` returns, i.e. once the
    /// child has been reaped. Deliberately not a join handle: the waiter may
    /// still be blocked delivering `Exited` to a sink nobody drains anymore.
    reaped: Receiver<()>,
    writer: Sender<Vec<u8>>,
    pid: Option<u32>,
    killed: bool,
}

impl PtyHandle for PortableHandle {
    fn write(&mut self, bytes: Vec<u8>) {
        // The writer thread only ends once the child is gone; nothing to do then.
        let _ = self.writer.send(bytes);
    }

    fn resize(&mut self, size: PaneSize) -> io::Result<()> {
        self.master.resize(pty_size(size)).map_err(io::Error::other)
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn kill(&mut self) {
        if self.killed {
            return;
        }
        self.killed = true;
        // Jobs the shell left behind can outlive it, so the group is always
        // signalled. A reaped child's pid, though, may be reused: only the
        // child itself is signalled while it still runs.
        let running = matches!(self.reaped.try_recv(), Err(TryRecvError::Empty));
        self.kill_process_group();
        if running {
            let _ = self.killer.kill();
            // The waiter reaps the child, so no zombie is left behind.
            let _ = self.reaped.recv();
        }
    }
}

impl PortableHandle {
    /// The child is a session leader, so its pid is also its process group:
    /// background jobs started from the shell die with it instead of
    /// outliving the app.
    fn kill_process_group(&self) {
        if let Some(pid) = self.pid.and_then(|pid| i32::try_from(pid).ok()) {
            // SAFETY: killpg only sends a signal; it touches no memory.
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
        }
    }
}

impl Drop for PortableHandle {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Forwards everything `reader` produces to `sink` as `Output`. Returns `true`
/// when the stream ended (EOF or a real error) and `false` when the sink asked
/// to stop. A read interrupted by a signal is retried, not treated as the end.
fn pump<R: Read + ?Sized>(reader: &mut R, sink: &mut PtySink) -> bool {
    let mut buf = [0u8; READ_CHUNK];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return true,
            Ok(n) => {
                if !sink(PtyEvent::Output(buf[..n].to_vec())) {
                    return false;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return true,
        }
    }
}

/// Starts the child described by `spec` on a new PTY. `sink` receives its
/// output and, once, `Exited` when the PTY closes.
pub fn spawn_portable(spec: &SpawnSpec, sink: PtySink) -> io::Result<Box<dyn PtyHandle>> {
    let pair = native_pty_system()
        .openpty(pty_size(spec.size))
        .map_err(io::Error::other)?;

    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.cwd(&spec.cwd);
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    let child = pair.slave.spawn_command(cmd).map_err(io::Error::other)?;
    // Keep no slave end open here, or the reader would never see EOF.
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
    let mut writer = pair.master.take_writer().map_err(io::Error::other)?;

    let events = Arc::new(Events {
        sink: Mutex::new(sink),
        exited: AtomicBool::new(false),
    });

    // Dropped when the reader is done, which tells the waiter it may report.
    let (eof_tx, eof_rx) = mpsc::channel::<()>();
    let reader_events = Arc::clone(&events);
    thread::spawn(move || {
        let _eof = eof_tx;
        let events = Arc::clone(&reader_events);
        let mut sink: PtySink = Box::new(move |event| events.send(event));
        if pump(&mut reader, &mut sink) {
            reader_events.exited();
        }
    });

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        while let Ok(bytes) = rx.recv() {
            if writer
                .write_all(&bytes)
                .and_then(|()| writer.flush())
                .is_err()
            {
                break;
            }
        }
    });

    let pid = child.process_id();
    let killer = child.clone_killer();
    let (reaped_tx, reaped) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
        let _ = reaped_tx.send(());
        // Let the reader flush what the child wrote before it died.
        let _ = eof_rx.recv_timeout(DRAIN_GRACE);
        events.exited();
    });

    Ok(Box::new(PortableHandle {
        master: pair.master,
        killer,
        reaped,
        writer: tx,
        pid,
        killed: false,
    }))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{
        path::Path,
        sync::mpsc::{self, Receiver},
        time::{Duration, Instant},
    };

    use super::*;
    use crate::core::{
        pane::PaneSize,
        pty::{PtyEvent, SpawnSpec},
    };

    const TIMEOUT: Duration = Duration::from_secs(5);

    fn spawn(program: &str, size: PaneSize) -> (Box<dyn PtyHandle>, Receiver<PtyEvent>) {
        let (tx, rx) = mpsc::channel();
        let spec = SpawnSpec::new(Some(program), "/".into(), size);
        let sink: PtySink = Box::new(move |event| tx.send(event).is_ok());
        (spawn_portable(&spec, sink).expect("spawn"), rx)
    }

    /// Collects output until `needle` shows up or the timeout expires.
    fn wait_for(rx: &Receiver<PtyEvent>, needle: &str) -> String {
        let deadline = Instant::now() + TIMEOUT;
        let mut seen = String::new();
        while Instant::now() < deadline && !seen.contains(needle) {
            if let Ok(PtyEvent::Output(bytes)) = rx.recv_timeout(Duration::from_millis(100)) {
                seen.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        seen
    }

    fn size(rows: u16, cols: u16) -> PaneSize {
        PaneSize { rows, cols }
    }

    // Spec: echo round trip.
    #[test]
    fn echo_round_trip() {
        let (mut pty, rx) = spawn("/bin/cat", size(24, 80));
        pty.write(b"hello\n".to_vec());
        assert!(wait_for(&rx, "hello").contains("hello"));
    }

    // Spec: resize propagates to the child.
    #[test]
    fn stty_size_reflects_a_resize() {
        let (mut pty, rx) = spawn("/bin/sh", size(24, 80));
        pty.resize(size(30, 100)).unwrap();
        pty.write(b"stty size\n".to_vec());
        assert!(wait_for(&rx, "30 100").contains("30 100"));
    }

    // Spec: shell exits -> Exited event.
    #[test]
    fn exit_is_reported_as_exited() {
        let (mut pty, rx) = spawn("/bin/sh", size(24, 80));
        pty.write(b"exit\n".to_vec());
        let deadline = Instant::now() + TIMEOUT;
        let exited = loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(PtyEvent::Exited) => break true,
                Ok(_) => {}
                Err(_) if Instant::now() > deadline => break false,
                Err(_) => {}
            }
        };
        assert!(exited, "no Exited event");
    }

    /// Counts `Exited` events seen until `window` passes.
    fn count_exited(rx: &Receiver<PtyEvent>, window: Duration) -> usize {
        let deadline = Instant::now() + window;
        let mut exited = 0;
        while Instant::now() < deadline {
            if let Ok(PtyEvent::Exited) = rx.recv_timeout(Duration::from_millis(50)) {
                exited += 1;
            }
        }
        exited
    }

    // Spec: child exits -> app quits, even when a background job still holds
    // the PTY open (so the reader never sees EOF).
    #[test]
    fn exit_is_reported_while_a_background_job_holds_the_pty() {
        let (mut pty, rx) = spawn("/bin/sh", size(24, 80));
        pty.write(b"sleep 20 &\nexit\n".to_vec());
        assert_eq!(count_exited(&rx, Duration::from_secs(3)), 1);
    }

    #[test]
    fn exited_is_emitted_exactly_once() {
        let (mut pty, rx) = spawn("/bin/sh", size(24, 80));
        pty.write(b"exit\n".to_vec());
        assert_eq!(count_exited(&rx, Duration::from_secs(1)), 1);
    }

    fn is_alive(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|stat| {
                !stat
                    .rsplit(')')
                    .next()
                    .unwrap_or("")
                    .trim_start()
                    .starts_with('Z')
            })
            .unwrap_or(false)
    }

    // Spec: quit leaves no orphan, background jobs included.
    #[test]
    fn drop_kills_background_jobs() {
        let (mut pty, rx) = spawn("/bin/sh", size(24, 80));
        // Job control off: the job shares the shell's process group.
        pty.write(b"set +m; sleep 20 & echo job=$!\n".to_vec());
        // The tty echoes the command line too, so look for `job=` + digits.
        let deadline = Instant::now() + TIMEOUT;
        let mut seen = String::new();
        let job = loop {
            seen.push_str(&wait_for(&rx, "\n"));
            let parsed = seen
                .split("job=")
                .filter_map(|rest| {
                    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                    digits.parse::<u32>().ok()
                })
                .next();
            match parsed {
                Some(job) => break job,
                None if Instant::now() > deadline => panic!("no job pid in {seen:?}"),
                None => {}
            }
        };
        assert!(is_alive(job));
        drop(pty);
        let deadline = Instant::now() + TIMEOUT;
        while is_alive(job) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!is_alive(job), "background job survived the drop");
    }

    // Spec: quit leaves no orphan.
    #[test]
    fn drop_leaves_no_orphan() {
        let (pty, _rx) = spawn("/bin/cat", size(24, 80));
        let pid = pty.pid().expect("pid");
        assert!(Path::new(&format!("/proc/{pid}")).exists());
        drop(pty);
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }
}

#[cfg(test)]
mod pump_tests {
    use std::{cell::Cell, sync::mpsc};

    use super::*;

    /// Yields `Interrupted` once, then `data`, then EOF.
    struct Flaky {
        interrupted: Cell<bool>,
        data: Option<Vec<u8>>,
    }

    impl Read for Flaky {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted.replace(true) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            match self.data.take() {
                Some(data) => {
                    buf[..data.len()].copy_from_slice(&data);
                    Ok(data.len())
                }
                None => Ok(0),
            }
        }
    }

    // EINTR must not be mistaken for the end of the stream.
    #[test]
    fn interrupted_reads_are_retried() {
        let (tx, rx) = mpsc::channel();
        let mut sink: PtySink = Box::new(move |event| tx.send(event).is_ok());
        let mut reader = Flaky {
            interrupted: Cell::new(false),
            data: Some(b"hi".to_vec()),
        };
        assert!(pump(&mut reader, &mut sink));
        assert_eq!(rx.try_recv().unwrap(), PtyEvent::Output(b"hi".to_vec()));
    }

    #[test]
    fn a_real_read_error_ends_the_stream() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::Other.into())
            }
        }
        let mut sink: PtySink = Box::new(|_| true);
        assert!(pump(&mut Broken, &mut sink));
    }

    #[test]
    fn a_closed_sink_stops_the_pump() {
        let mut sink: PtySink = Box::new(|_| false);
        let mut reader = Flaky {
            interrupted: Cell::new(true),
            data: Some(b"x".to_vec()),
        };
        assert!(!pump(&mut reader, &mut sink));
    }
}
