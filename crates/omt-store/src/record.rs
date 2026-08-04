//! The append-only record frame.
//!
//! A process that is killed mid-write leaves a partial record at the end of the
//! file. That is not corruption — it is the expected shape of a crash, and it
//! has to be distinguishable from corruption *in the middle*, because the two
//! want opposite responses: truncate the tail and carry on, versus stop and
//! quarantine before anything overwrites evidence.
//!
//! The frame is what makes them distinguishable:
//!
//! ```text
//! | magic(4) | len(u32) | crc32(u32) | payload(len) |
//! ```

use std::io::{self, Read, Write};

/// Marks the start of a record.
///
/// Present so a scan can resynchronize and so a file of some other kind is
/// rejected immediately rather than read as a length of four billion.
pub const MAGIC: [u8; 4] = *b"OMTR";

/// The fixed part of a record.
pub const HEADER_LEN: usize = 12;

/// The largest single record accepted.
///
/// An unbounded length prefix is a denial of service one allocation deep, and
/// a torn header can produce an arbitrary number.
pub const MAX_RECORD: usize = 16 << 20;

/// What reading a record produced.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    /// A complete, verified record.
    Record(Vec<u8>),
    /// The file ended cleanly on a record boundary.
    Eof,
    /// The file ended part-way through a record.
    ///
    /// The expected shape of a crash. Everything before this point is good.
    TornTail {
        /// How many bytes were left dangling.
        bytes: usize,
    },
    /// A record whose checksum does not match its contents.
    ///
    /// Genuine corruption, and deliberately *not* the same as a torn tail: a
    /// complete record whose bytes changed underneath us means something is
    /// wrong that truncation would hide.
    Corrupt {
        /// What the record claimed.
        expected_crc: u32,
        /// What its bytes actually hash to.
        actual_crc: u32,
    },
}

/// Write one record.
///
/// # Errors
/// Fails if the payload exceeds [`MAX_RECORD`] or the underlying write does.
pub fn write_record<W: Write>(w: &mut W, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_RECORD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("record of {} bytes exceeds the limit", payload.len()),
        ));
    }
    let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    w.write_all(&MAGIC)?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&crc32(payload).to_be_bytes())?;
    w.write_all(payload)?;
    Ok(())
}

/// Read one record.
///
/// # Errors
/// Fails only on an I/O error. A short or damaged record is an outcome, not an
/// error, because both are things a reader has to handle rather than propagate.
pub fn read_record<R: Read>(r: &mut R) -> io::Result<ReadOutcome> {
    let mut header = [0u8; HEADER_LEN];
    match read_exact_or_short(r, &mut header)? {
        0 => return Ok(ReadOutcome::Eof),
        n if n < HEADER_LEN => return Ok(ReadOutcome::TornTail { bytes: n }),
        _ => {}
    }

    if header[..4] != MAGIC {
        // Not a record at all. Reporting this as corruption rather than
        // guessing is what stops a file of another kind being read as a
        // gigantic length.
        return Ok(ReadOutcome::Corrupt {
            expected_crc: 0,
            actual_crc: 0,
        });
    }

    let len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let expected_crc = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);

    if len > MAX_RECORD {
        return Ok(ReadOutcome::Corrupt {
            expected_crc,
            actual_crc: 0,
        });
    }

    let mut payload = vec![0u8; len];
    let got = read_exact_or_short(r, &mut payload)?;
    if got < len {
        return Ok(ReadOutcome::TornTail {
            bytes: HEADER_LEN + got,
        });
    }

    let actual_crc = crc32(&payload);
    if actual_crc == expected_crc {
        Ok(ReadOutcome::Record(payload))
    } else {
        Ok(ReadOutcome::Corrupt {
            expected_crc,
            actual_crc,
        })
    }
}

fn read_exact_or_short<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// CRC-32 (IEEE), written out rather than pulled in.
///
/// Twenty lines against a dependency in a crate that persists the user's data;
/// the table is generated once on first use.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, entry) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *entry = c;
        }
        t
    });

    let mut crc = 0xffff_ffffu32;
    for b in data {
        crc = table[((crc ^ u32::from(*b)) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn encoded(payloads: &[&[u8]]) -> Vec<u8> {
        let mut buf = Vec::new();
        for p in payloads {
            write_record(&mut buf, p).expect("write");
        }
        buf
    }

    #[test]
    fn a_record_round_trips() {
        let buf = encoded(&[b"hello"]);
        let mut r = buf.as_slice();
        assert_eq!(
            read_record(&mut r).expect("read"),
            ReadOutcome::Record(b"hello".to_vec())
        );
        assert_eq!(read_record(&mut r).expect("read"), ReadOutcome::Eof);
    }

    #[test]
    fn records_read_back_in_order() {
        let buf = encoded(&[b"one", b"two", b"three"]);
        let mut r = buf.as_slice();
        for expected in [b"one".as_slice(), b"two", b"three"] {
            assert_eq!(
                read_record(&mut r).expect("read"),
                ReadOutcome::Record(expected.to_vec())
            );
        }
    }

    #[test]
    fn an_empty_file_is_eof_not_an_error() {
        let mut r: &[u8] = &[];
        assert_eq!(read_record(&mut r).expect("read"), ReadOutcome::Eof);
    }

    #[test]
    fn an_empty_payload_is_a_record() {
        // A record that carries nothing is still a record; treating it as EOF
        // would silently end a replay early.
        let buf = encoded(&[b""]);
        let mut r = buf.as_slice();
        assert_eq!(
            read_record(&mut r).expect("read"),
            ReadOutcome::Record(Vec::new())
        );
    }

    #[test]
    fn a_crash_mid_payload_is_a_torn_tail() {
        // The expected shape of a crash, and everything before it is good.
        let mut buf = encoded(&[b"one", b"a longer second record"]);
        buf.truncate(buf.len() - 5);
        let mut r = buf.as_slice();
        assert_eq!(
            read_record(&mut r).expect("read"),
            ReadOutcome::Record(b"one".to_vec()),
            "the complete record before it survives"
        );
        assert!(
            matches!(
                read_record(&mut r).expect("read"),
                ReadOutcome::TornTail { .. }
            ),
            "and the partial one is recognised as such"
        );
    }

    #[test]
    fn a_crash_mid_header_is_also_a_torn_tail() {
        // Killed between the magic and the length. Reading the truncated
        // header as a length would produce an enormous allocation.
        let mut buf = encoded(&[b"one", b"two"]);
        buf.truncate(buf.len() - 3 - HEADER_LEN + 4);
        let mut r = buf.as_slice();
        read_record(&mut r).expect("first");
        assert!(matches!(
            read_record(&mut r).expect("read"),
            ReadOutcome::TornTail { .. }
        ));
    }

    #[test]
    fn a_flipped_bit_is_corruption_not_a_torn_tail() {
        // The distinction that matters: truncating here would hide a real
        // problem, and the two cases want opposite responses.
        let mut buf = encoded(&[b"important data"]);
        let last = buf.len() - 1;
        buf[last] ^= 0b0000_0001;
        let mut r = buf.as_slice();
        let outcome = read_record(&mut r).expect("read");
        assert!(
            matches!(outcome, ReadOutcome::Corrupt { .. }),
            "{outcome:?}"
        );
    }

    #[test]
    fn corruption_reports_both_checksums() {
        // So a repair path can say what it found rather than only that
        // something was wrong.
        let mut buf = encoded(&[b"data"]);
        let last = buf.len() - 1;
        buf[last] ^= 0xff;
        let mut r = buf.as_slice();
        let ReadOutcome::Corrupt {
            expected_crc,
            actual_crc,
        } = read_record(&mut r).expect("read")
        else {
            panic!("expected corruption");
        };
        assert_ne!(expected_crc, actual_crc);
    }

    #[test]
    fn a_file_of_another_kind_is_rejected_immediately() {
        // Without the magic, arbitrary bytes read as a length of four billion.
        let mut r: &[u8] = b"this is not a record file at all, it is just text";
        assert!(matches!(
            read_record(&mut r).expect("read"),
            ReadOutcome::Corrupt { .. }
        ));
    }

    #[test]
    fn an_absurd_length_is_refused_rather_than_allocated() {
        // A torn header can produce any number, and honouring it is a denial
        // of service one allocation deep.
        let mut buf = MAGIC.to_vec();
        buf.extend_from_slice(&u32::MAX.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());
        let mut r = buf.as_slice();
        assert!(matches!(
            read_record(&mut r).expect("read"),
            ReadOutcome::Corrupt { .. }
        ));
    }

    #[test]
    fn an_oversized_payload_is_refused_at_write_time() {
        let mut sink = Vec::new();
        let huge = vec![0u8; MAX_RECORD + 1];
        assert!(write_record(&mut sink, &huge).is_err());
        assert!(sink.is_empty(), "and nothing was written");
    }

    #[test]
    fn the_checksum_matches_a_known_value() {
        // Pinned so a rewrite of the table cannot silently change what every
        // stored file hashes to.
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn a_record_survives_arbitrary_binary_payloads() {
        // Including bytes that look like the magic, which a naive resync would
        // trip over.
        let mut payload = MAGIC.to_vec();
        payload.extend_from_slice(&[0, 255, 0, 255]);
        let buf = encoded(&[&payload]);
        let mut r = buf.as_slice();
        assert_eq!(
            read_record(&mut r).expect("read"),
            ReadOutcome::Record(payload)
        );
    }
}
