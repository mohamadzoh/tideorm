//! A broken rule's message, built in a `const`, where no `String` exists.

/// Text of at most [`Message::CAPACITY`] bytes; longer text is cut at a
/// character boundary. The derive's `const` items panic with it, which
/// `cargo check` reports as an error.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct Message {
    bytes: [u8; Message::CAPACITY],
    len: usize,
}

impl Message {
    const CAPACITY: usize = 1024;

    pub(crate) const fn new() -> Self {
        Self {
            bytes: [0; Self::CAPACITY],
            len: 0,
        }
    }

    pub(crate) const fn push(&mut self, text: &str) -> &mut Self {
        let text = text.as_bytes();
        let mut end = text.len();
        if end > Self::CAPACITY - self.len {
            end = Self::CAPACITY - self.len;
            // Back off to the first byte of the character the cut falls in.
            while end > 0 && text[end] & 0b1100_0000 == 0b1000_0000 {
                end -= 1;
            }
        }
        let mut at = 0;
        while at < end {
            self.bytes[self.len] = text[at];
            self.len += 1;
            at += 1;
        }
        self
    }

    pub(crate) const fn number(&mut self, mut number: usize) -> &mut Self {
        let mut digits = [0u8; 20];
        let mut count = 0;
        loop {
            digits[digits.len() - 1 - count] = b'0' + (number % 10) as u8;
            count += 1;
            number /= 10;
            if number == 0 {
                break;
            }
        }
        let (_, digits) = digits.split_at(digits.len() - count);
        match core::str::from_utf8(digits) {
            Ok(digits) => self.push(digits),
            Err(_) => self,
        }
    }

    /// End with the rule's name, as every broken rule's message does.
    pub(crate) const fn rule(&mut self, rule: &str) -> &mut Self {
        self.push(" (").push(rule).push(" in tideorm.toml)")
    }

    /// The message's text.
    pub const fn as_str(&self) -> &str {
        match core::str::from_utf8(self.bytes.split_at(self.len).0) {
            Ok(text) => text,
            Err(_) => "",
        }
    }
}
