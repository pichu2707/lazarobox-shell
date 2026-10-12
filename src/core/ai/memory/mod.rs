//! The pure side of the engram memory: what a note is called, what is
//! redacted before it leaves the machine, how the context block is built and
//! how a `/health` reply is classified. No IO lives here.

pub mod kind;
pub mod redact;
pub mod slug;
