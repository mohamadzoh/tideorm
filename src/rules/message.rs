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
            end = char_start(text, Self::CAPACITY - self.len);
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

    /// End with the rule's name, as every broken rule's message does, cutting
    /// the text before it when both would not fit.
    pub(crate) const fn rule(&mut self, rule: &str) -> &mut Self {
        const BEFORE: &str = " (";
        const AFTER: &str = " in tideorm.toml)";
        let room = Self::CAPACITY.saturating_sub(BEFORE.len() + rule.len() + AFTER.len());
        if self.len > room {
            self.len = char_start(&self.bytes, room);
        }
        self.push(BEFORE).push(rule).push(AFTER)
    }

    /// The message's text.
    pub const fn as_str(&self) -> &str {
        match core::str::from_utf8(self.bytes.split_at(self.len).0) {
            Ok(text) => text,
            Err(_) => "",
        }
    }
}

/// `at`, backed off to the first byte of the character it falls in.
const fn char_start(bytes: &[u8], mut at: usize) -> usize {
    while at > 0 && bytes[at] & 0b1100_0000 == 0b1000_0000 {
        at -= 1;
    }
    at
}
