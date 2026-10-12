//! The CLI jail, as far as the chat core needs to know it: whether this
//! system can confine a CLI. The spec, the probe and the helper land in C13a.

/// The OS mechanism that confines a CLI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Landlock,
}

/// Whether this system can jail a CLI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JailSupport {
    Available { backend: Backend, abi: u32 },
    Unavailable(&'static str),
}

impl JailSupport {
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available { .. })
    }
}
