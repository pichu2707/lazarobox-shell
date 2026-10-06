//! In-memory `PtyHandle` for tests: records what the app asked for.

use std::{
    io,
    sync::{Arc, Mutex},
};

use super::PtyHandle;
use crate::core::pane::PaneSize;

/// What a `FakePty` has been asked to do.
#[derive(Default, Debug)]
pub struct FakeLog {
    pub writes: Vec<Vec<u8>>,
    pub resizes: Vec<PaneSize>,
    pub kills: usize,
}

/// Fake child. Clone the `log` before boxing it to inspect it afterwards.
#[derive(Default)]
pub struct FakePty {
    pub log: Arc<Mutex<FakeLog>>,
}

impl PtyHandle for FakePty {
    fn write(&mut self, bytes: Vec<u8>) {
        self.log.lock().unwrap().writes.push(bytes);
    }

    fn resize(&mut self, size: PaneSize) -> io::Result<()> {
        self.log.lock().unwrap().resizes.push(size);
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        None
    }

    fn kill(&mut self) {
        self.log.lock().unwrap().kills += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_writes_in_order() {
        let mut pty = FakePty::default();
        let log = pty.log.clone();
        pty.write(b"a".to_vec());
        pty.write(b"b".to_vec());
        assert_eq!(
            log.lock().unwrap().writes,
            vec![b"a".to_vec(), b"b".to_vec()]
        );
    }

    #[test]
    fn records_resizes_and_kills() {
        let mut pty = FakePty::default();
        let size = PaneSize { rows: 5, cols: 9 };
        pty.resize(size).unwrap();
        pty.kill();
        let log = pty.log.lock().unwrap();
        assert_eq!(log.resizes, vec![size]);
        assert_eq!(log.kills, 1);
    }
}
