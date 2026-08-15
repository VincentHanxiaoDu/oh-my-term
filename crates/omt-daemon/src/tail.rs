//! The most recent output of a session, so a subscriber can be behind of it.
//!
//! A session's bytes go into an emulator and are gone. That is right for the
//! emulator — it keeps the *result* — and leaves nothing for a client that
//! blinked. This is the small buffer that covers the blink.
//!
//! It is deliberately small. It exists so a client that was briefly slow can
//! catch up, not so a client that left can replay its afternoon; a client that
//! has fallen past the end is **seeded** with the screen as it now is, which is
//! both correct and cheaper than any amount of history. Replaying raw bytes
//! from an arbitrary point is not an option at any size: they are commands, and
//! half a command is not half an effect.

/// How much output is kept for catching up.
///
/// A screen of dense output is a few tens of kilobytes; this is enough for a
/// client to miss several and still be sent bytes rather than a fresh screen,
/// and small enough that a hundred idle sessions cost less than a megabyte
/// between them.
pub const TAIL_BYTES: usize = 256 * 1024;

/// Recent output, with the position of every byte in it.
///
/// Positions are absolute and count from the first byte the session ever
/// produced, so they survive the buffer discarding its front — which is the
/// only reason a client can tell "you are behind" from "you are ahead".
#[derive(Debug)]
pub struct Tail {
    bytes: std::collections::VecDeque<u8>,
    /// How many bytes have ever been written.
    written: u64,
    cap: usize,
}

impl Default for Tail {
    fn default() -> Self {
        Self::new(TAIL_BYTES)
    }
}

impl Tail {
    /// A buffer holding at most `cap` bytes.
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self {
            bytes: std::collections::VecDeque::new(),
            written: 0,
            cap: cap.max(1),
        }
    }

    /// Record output.
    pub fn push(&mut self, data: &[u8]) {
        self.written += data.len() as u64;
        // A single write larger than the whole buffer keeps only its end:
        // anything else would leave the buffer holding the *start* of a burst,
        // which is the half nobody wants.
        let keep = data.len().min(self.cap);
        self.bytes.extend(&data[data.len() - keep..]);
        while self.bytes.len() > self.cap {
            self.bytes.pop_front();
        }
    }

    /// The position after the last byte written.
    #[must_use]
    pub const fn position(&self) -> u64 {
        self.written
    }

    /// The oldest position still held.
    #[must_use]
    pub fn oldest(&self) -> u64 {
        self.written - self.bytes.len() as u64
    }

    /// What a client at `from` has not seen.
    ///
    /// `None` means it cannot be answered from here and the client must be
    /// seeded: either it is further back than what is held, or — the case that
    /// matters more — it is *ahead*, which happens when a session was replaced
    /// and the client is holding a position from the one before.
    #[must_use]
    pub fn since(&self, from: u64) -> Option<Vec<u8>> {
        if from > self.written || from < self.oldest() {
            return None;
        }
        let skip = (from - self.oldest()) as usize;
        Some(self.bytes.iter().skip(skip).copied().collect())
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn a_client_that_blinked_gets_what_it_missed() {
        let mut tail = Tail::new(64);
        tail.push(b"hello ");
        let mark = tail.position();
        tail.push(b"world");
        assert_eq!(tail.since(mark).as_deref(), Some(b"world".as_slice()));
    }

    #[test]
    fn a_client_that_is_up_to_date_gets_nothing_rather_than_a_repeat() {
        let mut tail = Tail::new(64);
        tail.push(b"done");
        assert_eq!(tail.since(tail.position()).as_deref(), Some(b"".as_slice()));
    }

    #[test]
    fn a_client_that_fell_past_the_end_is_told_it_cannot_be_answered() {
        // Not "here is what is left". The bytes are commands; starting a
        // client in the middle of one is how a retry becomes a disaster.
        let mut tail = Tail::new(8);
        tail.push(b"12345678");
        let old = tail.position();
        tail.push(b"abcdefgh");
        assert_eq!(tail.since(old - 8), None);
    }

    #[test]
    fn a_position_from_a_session_that_is_gone_is_refused() {
        // A client holding a position from a session that was replaced would
        // otherwise be handed the *new* session's output as if it were the
        // continuation of the old one's.
        let mut tail = Tail::new(64);
        tail.push(b"fresh");
        assert_eq!(tail.since(9_999), None);
    }

    #[test]
    fn the_buffer_never_grows_past_its_cap() {
        let mut tail = Tail::new(16);
        for _ in 0..100 {
            tail.push(b"0123456789");
        }
        assert!(tail.bytes.len() <= 16);
        assert_eq!(tail.position(), 1000);
    }

    #[test]
    fn one_write_larger_than_the_buffer_keeps_its_end() {
        // The end is what is closest to what the screen now shows. Keeping the
        // start would leave a client to catch up through the beginning of a
        // burst it can no longer use.
        let mut tail = Tail::new(4);
        tail.push(b"abcdefgh");
        assert_eq!(
            tail.since(tail.position() - 4).as_deref(),
            Some(b"efgh".as_slice())
        );
    }
}
