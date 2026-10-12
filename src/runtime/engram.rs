//! The engram memory in the runtime: a worker task between the app and the
//! local engram server. The app never waits on it.

pub mod worker;

pub use worker::{Loopback, MemoryLink, Transport, spawn};
