//! PTY port: what the app layer needs to know about the child process.

/// Something the child process did, as reported by the PTY adapter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PtyEvent {
    Output(Vec<u8>),
    Exited,
}
