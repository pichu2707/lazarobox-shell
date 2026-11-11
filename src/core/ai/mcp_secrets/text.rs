//! A value typed into a secret field (D23). It has no `Display` and no
//! `Serialize`, its `Debug` prints `<redacted>`, and its bytes are overwritten
//! when it drops (best effort: a moved copy or the allocator may keep some; no
//! `zeroize` crate is added).

#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretText(String);

impl std::fmt::Debug for SecretText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        // SAFETY: zero bytes are valid UTF-8, and the string is dropped next.
        wipe(unsafe { self.0.as_bytes_mut() });
    }
}

impl SecretText {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn push(&mut self, c: char) {
        self.0.push(c);
    }

    pub fn push_str(&mut self, text: &str) {
        self.0.push_str(text);
    }

    pub fn pop(&mut self) {
        self.0.pop();
    }

    /// The text, for the one place that sends it (the save) and for the field
    /// of a value that is not confidential.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Volatile writes, so the optimizer does not drop them as dead stores.
pub(super) fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        // SAFETY: `byte` is a valid, aligned, exclusive reference.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
}
