//! portable-pty adapter.
//!
//! The reader and the writer each run on a detached `std::thread` (a
//! `spawn_blocking` task would hang the runtime's shutdown). The reader hands
//! events to a `PtySink`; the writer is fed by a `std::sync::mpsc` channel so a
//! stalled child can never block the caller.

use std::{
    io::{self, Read, Write},
    sync::mpsc::{self, Sender},
    thread,
};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::{PtyEvent, PtyHandle, PtySink, SpawnSpec};
use crate::core::pane::PaneSize;

const READ_CHUNK: usize = 8 * 1024;

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
    child: Box<dyn Child + Send + Sync>,
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
        let _ = self.child.kill();
        // Reap it so no zombie is left behind.
        let _ = self.child.wait();
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
pub fn spawn_portable(spec: &SpawnSpec, mut sink: PtySink) -> io::Result<Box<dyn PtyHandle>> {
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

    thread::spawn(move || {
        if pump(&mut reader, &mut sink) {
            sink(PtyEvent::Exited);
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
    Ok(Box::new(PortableHandle {
        master: pair.master,
        child,
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
