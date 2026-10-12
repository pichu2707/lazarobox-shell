//! The pure side of the engram memory: what a note is called, what is
//! redacted before it leaves the machine, how the context block is built and
//! how a `/health` reply is classified. No IO lives here.

pub mod context;
pub mod health;
pub mod kind;
pub mod port;
pub mod redact;
pub mod reply;
pub mod route;
pub mod slug;

/// Helpers for tests that read the recorded engram responses.
#[cfg(test)]
pub(crate) mod testing {
    /// Splits a recorded raw HTTP response into its status and body.
    pub fn split(raw: &[u8]) -> (u16, &[u8]) {
        let end = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("head terminator");
        let head = std::str::from_utf8(&raw[..end]).expect("utf8 head");
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .expect("status code");
        (status, &raw[end + 4..])
    }
}
