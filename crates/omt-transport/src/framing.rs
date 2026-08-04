//! Length-prefixed framing over any byte stream.
//!
//! Four bytes of length, one byte of kind, then the payload. Deliberately
//! boring: the interesting failures in a transport are the ones where a
//! malformed length or a partial read is mistaken for valid data, so the
//! decoder's job is to refuse rather than to be clever.

use std::io::{self, Read, Write};

use omt_proto::{FrameKind, MAX_TEXT_FRAME};

/// A frame's fixed prefix: length, then kind.
pub const PREFIX_LEN: usize = 5;

/// Write one frame.
///
/// # Errors
/// The underlying write error, or [`FramingError::TooLarge`] when the payload
/// exceeds the bound — refused here rather than sent and refused by the peer,
/// so the failure names the sender.
pub fn write_frame(
    mut out: impl Write,
    kind: FrameKind,
    payload: &[u8],
) -> Result<(), FramingError> {
    if payload.len() > MAX_TEXT_FRAME {
        return Err(FramingError::TooLarge {
            len: payload.len(),
            max: MAX_TEXT_FRAME,
        });
    }
    let len = u32::try_from(payload.len()).map_err(|_| FramingError::TooLarge {
        len: payload.len(),
        max: MAX_TEXT_FRAME,
    })?;
    out.write_all(&len.to_be_bytes())?;
    out.write_all(&[kind as u8])?;
    out.write_all(payload)?;
    out.flush()?;
    Ok(())
}

/// Read one frame.
///
/// # Errors
/// [`FramingError::Eof`] on a clean close, [`FramingError::TooLarge`] on a
/// length that would allocate more than the bound — checked *before*
/// allocating, since an unbounded length prefix is a denial of service one
/// allocation deep — or the underlying read error.
pub fn read_frame(mut input: impl Read) -> Result<(FrameKind, Vec<u8>), FramingError> {
    let mut prefix = [0u8; PREFIX_LEN];
    match input.read_exact(&mut prefix) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Err(FramingError::Eof),
        Err(e) => return Err(FramingError::Io(e.to_string())),
    }

    let len = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
    if len > MAX_TEXT_FRAME {
        return Err(FramingError::TooLarge {
            len,
            max: MAX_TEXT_FRAME,
        });
    }
    let kind =
        FrameKind::from_tag(prefix[4]).ok_or(FramingError::UnknownKind { tag: prefix[4] })?;

    let mut payload = vec![0u8; len];
    input.read_exact(&mut payload).map_err(|e| match e.kind() {
        // A truncated payload is not a clean close: the peer said how many
        // bytes were coming and then sent fewer, which is corruption, not
        // a goodbye.
        io::ErrorKind::UnexpectedEof => FramingError::Truncated { expected: len },
        _ => FramingError::Io(e.to_string()),
    })?;
    Ok((kind, payload))
}

/// What can go wrong framing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FramingError {
    /// The peer closed cleanly between frames.
    #[error("the peer closed the connection")]
    Eof,
    /// The peer announced more bytes than it sent.
    #[error("frame truncated: {expected} bytes announced, fewer arrived")]
    Truncated {
        /// How many were promised.
        expected: usize,
    },
    /// A length beyond the bound.
    #[error("frame length {len} exceeds the {max}-byte limit")]
    TooLarge {
        /// The claimed length.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// A kind tag outside the closed set.
    #[error("frame kind tag {tag} is not in the closed set")]
    UnknownKind {
        /// The tag seen.
        tag: u8,
    },
    /// Something else went wrong on the underlying stream.
    #[error("transport I/O: {0}")]
    Io(String),
}

impl From<io::Error> for FramingError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::UnexpectedEof => Self::Eof,
            _ => Self::Io(e.to_string()),
        }
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
    fn a_frame_round_trips() {
        let mut buf = Vec::new();
        write_frame(&mut buf, FrameKind::Text, b"hello").expect("write");
        let (kind, payload) = read_frame(&buf[..]).expect("read");
        assert_eq!(kind, FrameKind::Text);
        assert_eq!(payload, b"hello");
    }

    #[test]
    fn frames_are_read_back_in_order() {
        let mut buf = Vec::new();
        for n in 0..5u8 {
            write_frame(&mut buf, FrameKind::Text, &[n]).expect("write");
        }
        let mut cursor = &buf[..];
        for n in 0..5u8 {
            let (_, payload) = read_frame(&mut cursor).expect("read");
            assert_eq!(payload, [n]);
        }
        assert_eq!(read_frame(&mut cursor), Err(FramingError::Eof));
    }

    #[test]
    fn an_empty_payload_is_a_valid_frame() {
        let mut buf = Vec::new();
        write_frame(&mut buf, FrameKind::Binary, b"").expect("write");
        let (kind, payload) = read_frame(&buf[..]).expect("read");
        assert_eq!(kind, FrameKind::Binary);
        assert!(payload.is_empty());
    }

    #[test]
    fn a_clean_close_is_distinguishable_from_corruption() {
        // These want different handling: one is a peer leaving, the other is a
        // peer lying about what it sent.
        assert_eq!(read_frame(&[][..]), Err(FramingError::Eof));

        let mut truncated = Vec::new();
        write_frame(&mut truncated, FrameKind::Text, b"hello").expect("write");
        truncated.truncate(PREFIX_LEN + 2);
        assert_eq!(
            read_frame(&truncated[..]),
            Err(FramingError::Truncated { expected: 5 })
        );
    }

    #[test]
    fn an_oversized_length_is_refused_before_allocating() {
        // An unbounded length prefix is a denial of service one allocation
        // deep, so the bound is checked before the Vec is made.
        let mut buf = (MAX_TEXT_FRAME as u32 + 1).to_be_bytes().to_vec();
        buf.push(FrameKind::Text as u8);
        let err = read_frame(&buf[..]).expect_err("must refuse");
        assert!(matches!(err, FramingError::TooLarge { .. }));
    }

    #[test]
    fn an_oversized_payload_is_refused_by_the_sender() {
        // Named on the sending side, so the failure points at whoever produced
        // it rather than at the peer that received it.
        let big = vec![0u8; MAX_TEXT_FRAME + 1];
        let err = write_frame(Vec::new(), FrameKind::Text, &big).expect_err("must refuse");
        assert!(matches!(err, FramingError::TooLarge { .. }));
    }

    #[test]
    fn an_unknown_kind_tag_is_refused() {
        let mut buf = 1u32.to_be_bytes().to_vec();
        buf.push(200);
        buf.push(b'x');
        assert_eq!(
            read_frame(&buf[..]),
            Err(FramingError::UnknownKind { tag: 200 })
        );
    }

    #[test]
    fn a_frame_split_across_reads_is_reassembled() {
        // The property that matters over a real socket, where a write is not a
        // read: the decoder must not treat a partial arrival as a short frame.
        struct Dribble<'a>(&'a [u8]);
        impl Read for Dribble<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if self.0.is_empty() {
                    return Ok(0);
                }
                let n = out.len().min(1); // one byte at a time
                out[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }

        let mut buf = Vec::new();
        write_frame(&mut buf, FrameKind::Text, b"reassembled").expect("write");
        let (kind, payload) = read_frame(Dribble(&buf)).expect("read");
        assert_eq!(kind, FrameKind::Text);
        assert_eq!(payload, b"reassembled");
    }

    #[test]
    fn the_maximum_sized_frame_is_accepted() {
        // Boundary: the limit itself is allowed, only beyond it is refused.
        let payload = vec![7u8; MAX_TEXT_FRAME];
        let mut buf = Vec::new();
        write_frame(&mut buf, FrameKind::Binary, &payload).expect("write");
        let (_, back) = read_frame(&buf[..]).expect("read");
        assert_eq!(back.len(), MAX_TEXT_FRAME);
    }
}
