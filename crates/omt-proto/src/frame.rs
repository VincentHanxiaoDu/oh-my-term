//! Framing, and the binary terminal frame header.

use serde::{Deserialize, Serialize};

/// The largest control frame accepted.
///
/// Bounded because an unbounded length prefix is a denial of service one
/// allocation deep.
pub const MAX_TEXT_FRAME: usize = 1 << 20; // 1 MiB

/// A frame's kind tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[repr(u8)]
pub enum FrameKind {
    /// A JSON control message.
    Text = 0,
    /// Terminal bytes, or a blob chunk.
    Binary = 1,
}

impl FrameKind {
    /// Parse a tag byte.
    #[must_use]
    pub const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Text),
            1 => Some(Self::Binary),
            _ => None,
        }
    }
}

/// The header on every binary frame.
///
/// Twenty-four bytes, laid out so both sequence fields are `u64`:
///
/// ```text
/// | ver(1) | kind(1) | stream(u16) | reserved(u32) | seq_or_off(u64) | ack(u64) |
/// ```
///
/// `seq_or_off` is `u64` to match `Seq` everywhere else. A `u32` wraps, and a
/// client resuming after a wrap rejoins at a point that looks valid and is not.
///
/// `ack` carries the highest client input position the instance has consumed.
/// It has two independent jobs, and both are load-bearing: it is what lets a
/// client render predicted local echo honestly, and it is the *only* safe way
/// to resume a raw byte stream — which must never be replayed, since re-sending
/// the tail of a shell command is how a retry becomes a disaster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryHeader {
    /// Protocol version of this frame.
    pub ver: u8,
    /// What the payload is.
    pub kind: FrameKind,
    /// Which multiplexed stream.
    pub stream: u16,
    /// The sequence position, or a byte offset for a blob chunk.
    pub seq_or_off: u64,
    /// Highest client input position consumed.
    pub ack: u64,
}

impl BinaryHeader {
    /// The header's size on the wire.
    pub const SIZE: usize = 24;

    /// The current frame version.
    pub const VERSION: u8 = 1;

    /// Write the header.
    #[must_use]
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[0] = self.ver;
        out[1] = self.kind as u8;
        out[2..4].copy_from_slice(&self.stream.to_be_bytes());
        // bytes 4..8 reserved, left zero — they exist so both u64s land on an
        // 8-byte boundary, which keeps decoding a pair of aligned reads.
        out[8..16].copy_from_slice(&self.seq_or_off.to_be_bytes());
        out[16..24].copy_from_slice(&self.ack.to_be_bytes());
        out
    }

    /// Read a header.
    ///
    /// # Errors
    /// Fails on a short buffer, an unknown version or an unknown kind tag.
    /// Refusing an unknown version rather than guessing is what stops a
    /// future frame being misread as a current one.
    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        if bytes.len() < Self::SIZE {
            return Err(FrameError::Short {
                need: Self::SIZE,
                got: bytes.len(),
            });
        }
        let ver = bytes[0];
        if ver != Self::VERSION {
            return Err(FrameError::UnknownVersion { ver });
        }
        let kind =
            FrameKind::from_tag(bytes[1]).ok_or(FrameError::UnknownKind { tag: bytes[1] })?;
        let mut stream = [0u8; 2];
        stream.copy_from_slice(&bytes[2..4]);
        let mut seq = [0u8; 8];
        seq.copy_from_slice(&bytes[8..16]);
        let mut ack = [0u8; 8];
        ack.copy_from_slice(&bytes[16..24]);
        Ok(Self {
            ver,
            kind,
            stream: u16::from_be_bytes(stream),
            seq_or_off: u64::from_be_bytes(seq),
            ack: u64::from_be_bytes(ack),
        })
    }
}

/// What can go wrong reading a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// Not enough bytes for a header.
    #[error("frame header needs {need} bytes, got {got}")]
    Short {
        /// How many are needed.
        need: usize,
        /// How many arrived.
        got: usize,
    },
    /// A version this build does not speak.
    #[error("frame version {ver} is not understood; refusing rather than misreading it")]
    UnknownVersion {
        /// The version seen.
        ver: u8,
    },
    /// A kind tag outside the closed set.
    #[error("frame kind tag {tag} is not in the closed set")]
    UnknownKind {
        /// The tag seen.
        tag: u8,
    },
    /// A control frame larger than the bound.
    #[error("frame length {len} exceeds the {max}-byte limit")]
    TooLarge {
        /// The claimed length.
        len: usize,
        /// The limit.
        max: usize,
    },
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn header() -> BinaryHeader {
        BinaryHeader {
            ver: BinaryHeader::VERSION,
            kind: FrameKind::Binary,
            stream: 7,
            seq_or_off: 0x0123_4567_89ab_cdef,
            ack: 42,
        }
    }

    #[test]
    fn the_header_is_twenty_four_bytes() {
        assert_eq!(header().encode().len(), 24);
        assert_eq!(BinaryHeader::SIZE, 24);
    }

    #[test]
    fn the_byte_layout_is_fixed() {
        // Asserting the layout, not just the round trip: a field that moved
        // would still round-trip within one build while breaking every other.
        let bytes = header().encode();
        assert_eq!(bytes[0], 1, "version");
        assert_eq!(bytes[1], 1, "kind = binary");
        assert_eq!(&bytes[2..4], &7u16.to_be_bytes(), "stream");
        assert_eq!(&bytes[4..8], &[0, 0, 0, 0], "reserved");
        assert_eq!(
            &bytes[8..16],
            &0x0123_4567_89ab_cdefu64.to_be_bytes(),
            "seq"
        );
        assert_eq!(&bytes[16..24], &42u64.to_be_bytes(), "ack");
    }

    #[test]
    fn headers_round_trip() {
        let h = header();
        assert_eq!(BinaryHeader::decode(&h.encode()).expect("decode"), h);
    }

    #[test]
    fn a_sequence_beyond_u32_survives() {
        // The whole reason the field is u64: a busy session passes 4.3 billion,
        // and a wrap makes a resume rejoin at a plausible wrong point.
        let h = BinaryHeader {
            seq_or_off: u64::from(u32::MAX) + 1_000,
            ..header()
        };
        let back = BinaryHeader::decode(&h.encode()).expect("decode");
        assert_eq!(back.seq_or_off, u64::from(u32::MAX) + 1_000);
    }

    #[test]
    fn the_maximum_sequence_survives() {
        let h = BinaryHeader {
            seq_or_off: u64::MAX,
            ack: u64::MAX,
            ..header()
        };
        let back = BinaryHeader::decode(&h.encode()).expect("decode");
        assert_eq!(back.seq_or_off, u64::MAX);
        assert_eq!(back.ack, u64::MAX);
    }

    #[test]
    fn a_short_buffer_is_refused() {
        let err = BinaryHeader::decode(&[0u8; 10]).expect_err("must refuse");
        assert!(matches!(err, FrameError::Short { .. }));
    }

    #[test]
    fn an_unknown_version_is_refused_not_guessed() {
        let mut bytes = header().encode();
        bytes[0] = 99;
        let err = BinaryHeader::decode(&bytes).expect_err("must refuse");
        assert_eq!(err, FrameError::UnknownVersion { ver: 99 });
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        let mut bytes = header().encode();
        bytes[1] = 200;
        let err = BinaryHeader::decode(&bytes).expect_err("must refuse");
        assert_eq!(err, FrameError::UnknownKind { tag: 200 });
    }

    #[test]
    fn decoding_ignores_trailing_payload() {
        let mut buf = header().encode().to_vec();
        buf.extend_from_slice(b"payload bytes");
        assert_eq!(BinaryHeader::decode(&buf).expect("decode"), header());
    }
}
