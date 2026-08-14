//! A header that arrives in pieces still means one thing.
//!
//! A socket hands over arbitrary slices. Twenty-four bytes is small enough
//! that it *usually* arrives whole, which is exactly what makes this class of
//! bug survive: it works on a loopback socket and on a fast LAN, and it
//! appears on a phone on a train, once, in a way nobody can reproduce.
//!
//! So this is a property over every split point rather than an example at one.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use omt_proto::{BinaryHeader, FrameKind};

/// A deterministic pseudo-random source, so a failure reproduces from a seed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        self.0 >> 33
    }
}

fn kinds() -> Vec<FrameKind> {
    vec![FrameKind::Text, FrameKind::Binary]
}

#[test]
fn a_header_split_at_any_point_decodes_to_the_same_thing() {
    // The 24 bytes may arrive as 1+23, 23+1, or any of the twenty-three
    // splits between. Every one must reassemble into one header, and a decoder
    // handed a prefix must say "not yet" rather than guessing.
    for (i, kind) in kinds().into_iter().enumerate() {
        let header = BinaryHeader {
            ver: BinaryHeader::VERSION,
            kind,
            stream: (i as u16) * 7 + 1,
            seq_or_off: u64::MAX - i as u64,
            ack: i as u64 * 1_000_000,
        };
        let bytes = header.encode();

        for split in 0..bytes.len() {
            // A prefix is not a header. Decoding one would produce a frame
            // built from whatever the buffer happened to contain next.
            assert!(
                BinaryHeader::decode(&bytes[..split]).is_err(),
                "a {split}-byte prefix decoded as a whole header"
            );

            // Reassembled from the two halves, it is the original.
            let mut buffer = Vec::with_capacity(bytes.len());
            buffer.extend_from_slice(&bytes[..split]);
            buffer.extend_from_slice(&bytes[split..]);
            let decoded = BinaryHeader::decode(&buffer)
                .unwrap_or_else(|e| panic!("split at {split}: {e}"));
            assert_eq!(decoded, header, "split at {split}");
        }
    }
}

#[test]
fn a_header_with_bytes_after_it_reads_only_its_own() {
    // A stream does not stop at a frame boundary: the next frame's first bytes
    // are usually already in the buffer. A decoder that consumed them would
    // desynchronize the whole connection, one frame at a time.
    let header = BinaryHeader {
        ver: BinaryHeader::VERSION,
        kind: FrameKind::Binary,
        stream: 3,
        seq_or_off: 4096,
        ack: 12,
    };
    let mut buffer = header.encode().to_vec();
    buffer.extend_from_slice(b"the payload, and then the next frame's header");
    assert_eq!(
        BinaryHeader::decode(&buffer).expect("decode"),
        header,
        "the decoder read past its own header"
    );
}

#[test]
fn every_field_survives_its_full_range() {
    // The fields are the reason the header is 24 bytes rather than 16: `seq`
    // and `ack` are `u64` because a long-lived session really does exceed
    // 2^32 (D-2). A field silently truncated to `u32` would work for weeks and
    // then start resuming at the wrong place.
    let mut rng = Lcg(0x5eed);
    for _ in 0..500 {
        let header = BinaryHeader {
            ver: BinaryHeader::VERSION,
            kind: FrameKind::Binary,
            stream: rng.next() as u16,
            seq_or_off: rng.next().wrapping_mul(rng.next()),
            ack: rng.next().wrapping_mul(rng.next()),
        };
        assert_eq!(
            BinaryHeader::decode(&header.encode()).expect("decode"),
            header
        );
    }
}

#[test]
fn a_header_is_exactly_twenty_four_bytes() {
    // Stated in the protocol document and asserted here, because every client
    // that is not this code slices by that number.
    assert_eq!(BinaryHeader::SIZE, 24);
    assert_eq!(
        BinaryHeader {
            ver: 1,
            kind: FrameKind::Text,
            stream: 0,
            seq_or_off: 0,
            ack: 0,
        }
        .encode()
        .len(),
        24
    );
}

#[test]
fn a_frame_from_a_version_this_build_does_not_know_is_refused() {
    // Not decoded on the assumption the fields happen to line up. A version
    // byte exists so that a future layout is a refusal rather than a
    // misreading.
    let mut bytes = BinaryHeader {
        ver: BinaryHeader::VERSION,
        kind: FrameKind::Text,
        stream: 1,
        seq_or_off: 1,
        ack: 1,
    }
    .encode();
    bytes[0] = BinaryHeader::VERSION.wrapping_add(9);
    assert!(
        BinaryHeader::decode(&bytes).is_err(),
        "a frame from an unknown protocol version was decoded anyway"
    );
}
