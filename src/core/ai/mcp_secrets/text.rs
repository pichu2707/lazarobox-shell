//! A value typed into a secret field (D23). It has no `Display` and no
//! `Serialize`, its `Debug` prints `<redacted>`, and its bytes are overwritten
//! when it drops (best effort: a moved copy or the allocator may keep some; no
//! `zeroize` crate is added).

/// The longest value a field takes: reserved up front, so a push never moves
/// the buffer and leaves a copy of the text behind.
pub const CAPACITY: usize = 4096;

#[derive(PartialEq, Eq)]
pub struct SecretText(String);

impl Default for SecretText {
    fn default() -> Self {
        Self(String::with_capacity(CAPACITY))
    }
}

impl Clone for SecretText {
    fn clone(&self) -> Self {
        let mut copy = Self::default();
        copy.push_str(&self.0);
        copy
    }
}

impl std::fmt::Debug for SecretText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        wipe_capacity(&mut self.0);
    }
}

impl SecretText {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.0.capacity()
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

/// Zeroes the whole buffer, the bytes past the length included (what `pop`
/// removed), and leaves an empty string.
pub(super) fn wipe_capacity(text: &mut String) {
    // SAFETY: bytes are written only; the length is set to 0 first, so no
    // invalid UTF-8 is ever readable through the string.
    unsafe {
        let bytes = text.as_mut_vec();
        let cap = bytes.capacity();
        bytes.set_len(0);
        let ptr = bytes.as_mut_ptr();
        for i in 0..cap {
            std::ptr::write_volatile(ptr.add(i), 0);
        }
    }
}
