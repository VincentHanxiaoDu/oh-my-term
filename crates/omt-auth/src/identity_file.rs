//! The file that survives losing every device.
//!
//! `docs/architecture/23-identity-and-devices.md` §4.
//!
//! Two jobs in one artefact, deliberately: **backup** — the thing that outlives
//! every device somebody owns — and **sneakernet sync** — the thing that
//! bootstraps a device when home is unreachable and there is no network path to
//! it. Two formats would drift, and the day they drifted would be the day
//! somebody needed the one that had stopped working.
//!
//! The header is authenticated but never encrypted, and that is the decision
//! worth stating. Somebody holding an unlabelled encrypted blob has no way to
//! know which identity it is for, which passphrase to try, or whether it is
//! even an omt file — and the moment they need it is the moment they have lost
//! everything else. Encrypting the label protects nothing an attacker with the
//! file does not already have, and costs the owner the only clue they have.
//!
//! What is *in* the header is chosen on the same reasoning: enough to identify
//! and unlock, never anything that is a secret on its own.

use serde::{Deserialize, Serialize};

/// The magic bytes at the front.
///
/// Versioned in the magic rather than only in the header, so a file from a
/// future version fails as "not this format" before anything tries to parse a
/// header it does not understand.
pub const MAGIC: &[u8] = b"OMTID1\n";

/// The part anybody holding the file can read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    /// Format version.
    pub v: u32,
    /// Which identity this is, so somebody with three backups knows which.
    pub identity: String,
    /// The display name, so they know it without a lookup they cannot do.
    pub label: String,
    /// The key-derivation parameters.
    ///
    /// In the clear because they are needed *to* decrypt: a file whose
    /// parameters were inside the ciphertext could only be opened by somebody
    /// who already knew them, which is nobody.
    pub kdf: Kdf,
    /// When it was written, seconds since the epoch.
    pub created: u64,
}

/// How the passphrase becomes a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kdf {
    /// The algorithm.
    pub algorithm: String,
    /// Memory cost, in KiB.
    pub memory_kib: u32,
    /// Time cost.
    pub iterations: u32,
    /// Parallelism.
    pub lanes: u32,
    /// The salt, base64.
    pub salt: String,
}

impl Default for Kdf {
    fn default() -> Self {
        Self {
            algorithm: "argon2id".to_owned(),
            // Chosen so an offline guess is expensive on the attacker's
            // hardware rather than merely inconvenient on the owner's. A
            // second on a laptop is a second; at 64 MiB it is also a second on
            // a GPU, which is the whole point of the memory cost.
            memory_kib: 64 * 1024,
            iterations: 3,
            lanes: 4,
            salt: String::new(),
        }
    }
}

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    /// Not an omt identity file.
    NotOurs,
    /// A version this build does not know.
    Version(u32),
    /// Truncated.
    Truncated {
        /// What was expected.
        wanted: usize,
        /// What was there.
        got: usize,
    },
    /// The header is not readable.
    BadHeader(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotOurs => write!(f, "this is not an omt identity file"),
            Self::Version(v) => write!(
                f,
                "this file is version {v}; this build of omt does not know it"
            ),
            Self::Truncated { wanted, got } => {
                write!(
                    f,
                    "the file is truncated: expected {wanted} bytes, found {got}"
                )
            }
            Self::BadHeader(why) => write!(f, "the header is unreadable: {why}"),
        }
    }
}

impl std::error::Error for FileError {}

/// Write the framing around a header and a ciphertext.
///
/// The header is returned as bytes as well, because it is the additional
/// authenticated data — the caller must pass exactly these bytes to the cipher,
/// not a re-serialisation of the same structure. Two serialisations of one
/// structure are not guaranteed to be the same bytes, and a tag computed over
/// one and checked against the other fails in a way that looks like corruption.
///
/// # Errors
/// Fails if the header cannot be serialised.
pub fn frame(header: &Header, ciphertext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), FileError> {
    let header_bytes =
        serde_json::to_vec(header).map_err(|e| FileError::BadHeader(e.to_string()))?;
    let mut out = Vec::with_capacity(MAGIC.len() + 4 + header_bytes.len() + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(
        &u32::try_from(header_bytes.len())
            .map_err(|_| FileError::BadHeader("the header is impossibly large".to_owned()))?
            .to_le_bytes(),
    );
    out.extend_from_slice(&header_bytes);
    out.extend_from_slice(ciphertext);
    Ok((out, header_bytes))
}

/// Read the framing, returning the header, its exact bytes, and the ciphertext.
///
/// # Errors
/// Fails on anything that is not this format at this version.
pub fn unframe(bytes: &[u8]) -> Result<(Header, Vec<u8>, Vec<u8>), FileError> {
    if bytes.len() < MAGIC.len() + 4 {
        return Err(FileError::Truncated {
            wanted: MAGIC.len() + 4,
            got: bytes.len(),
        });
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(FileError::NotOurs);
    }
    let length_at = MAGIC.len();
    let length = u32::from_le_bytes([
        bytes[length_at],
        bytes[length_at + 1],
        bytes[length_at + 2],
        bytes[length_at + 3],
    ]) as usize;
    let header_at = length_at + 4;
    let header_end = header_at.checked_add(length).ok_or(FileError::Truncated {
        wanted: usize::MAX,
        got: bytes.len(),
    })?;
    if bytes.len() < header_end {
        // A length field that promises more than the file holds is the shape
        // of both a truncated download and a hostile file, and neither should
        // reach the parser below it.
        return Err(FileError::Truncated {
            wanted: header_end,
            got: bytes.len(),
        });
    }
    let header_bytes = bytes[header_at..header_end].to_vec();
    let header: Header =
        serde_json::from_slice(&header_bytes).map_err(|e| FileError::BadHeader(e.to_string()))?;
    if header.v != 1 {
        return Err(FileError::Version(header.v));
    }
    Ok((header, header_bytes, bytes[header_end..].to_vec()))
}

/// What a person can be told about a file without unlocking it.
///
/// The reason the header is in the clear. Somebody with three backups and one
/// passphrase needs to know which file to try, and the moment they need this is
/// the moment they have lost every device that could have told them.
#[must_use]
pub fn describe(header: &Header) -> String {
    format!(
        "omt identity `{}` ({}), written {}",
        header.label, header.identity, header.created
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            v: 1,
            identity: "id-1".to_owned(),
            label: "Vincent's omt".to_owned(),
            kdf: Kdf {
                salt: "c2FsdA==".to_owned(),
                ..Kdf::default()
            },
            created: 1_800_000_000,
        }
    }

    #[test]
    fn a_file_round_trips_through_its_framing() {
        let (bytes, _) = frame(&header(), b"ciphertext").expect("frame");
        let (back, _, ciphertext) = unframe(&bytes).expect("unframe");
        assert_eq!(back, header());
        assert_eq!(ciphertext, b"ciphertext");
    }

    #[test]
    fn the_header_bytes_are_returned_rather_than_re_serialised() {
        // They are the additional authenticated data, and two serialisations
        // of one structure are not guaranteed to be the same bytes — a tag
        // computed over one and checked against the other fails in a way that
        // looks exactly like corruption.
        let (bytes, written) = frame(&header(), b"x").expect("frame");
        let (_, read, _) = unframe(&bytes).expect("unframe");
        assert_eq!(written, read);
    }

    #[test]
    fn somebody_holding_the_file_can_tell_what_it_is() {
        // The moment they need this is the moment they have lost every device
        // that could have told them.
        let (bytes, _) = frame(&header(), b"x").expect("frame");
        let (back, _, _) = unframe(&bytes).expect("unframe");
        let said = describe(&back);
        assert!(said.contains("Vincent's omt"), "{said}");
        assert!(said.contains("id-1"), "{said}");
    }

    #[test]
    fn the_kdf_parameters_are_readable_without_the_passphrase() {
        // A file whose parameters were inside the ciphertext could only be
        // opened by somebody who already knew them, which is nobody.
        let (bytes, _) = frame(&header(), b"x").expect("frame");
        let (back, _, _) = unframe(&bytes).expect("unframe");
        assert_eq!(back.kdf.algorithm, "argon2id");
        assert!(!back.kdf.salt.is_empty());
    }

    #[test]
    fn something_else_entirely_is_not_our_file() {
        assert_eq!(unframe(b"PK\x03\x04 and so on"), Err(FileError::NotOurs));
    }

    #[test]
    fn a_future_version_says_so_rather_than_failing_obscurely() {
        let mut future = header();
        future.v = 9;
        let (bytes, _) = frame(&future, b"x").expect("frame");
        assert_eq!(unframe(&bytes), Err(FileError::Version(9)));
    }

    #[test]
    fn a_length_promising_more_than_the_file_holds_is_refused() {
        // The shape of both a truncated download and a hostile file, and
        // neither should reach the parser below it.
        let (mut bytes, _) = frame(&header(), b"x").expect("frame");
        bytes[MAGIC.len()] = 0xFF;
        bytes[MAGIC.len() + 1] = 0xFF;
        assert!(matches!(unframe(&bytes), Err(FileError::Truncated { .. })));
    }

    #[test]
    fn a_truncated_file_does_not_panic_at_any_length() {
        let (bytes, _) = frame(&header(), b"ciphertext here").expect("frame");
        for cut in 0..bytes.len() {
            let _ = unframe(&bytes[..cut]);
        }
    }

    #[test]
    fn an_empty_file_is_reported_as_truncated_not_as_foreign() {
        // "Not an omt file" would send somebody looking for the right file
        // when what they have is the right file, cut short.
        assert!(matches!(unframe(b""), Err(FileError::Truncated { .. })));
    }

    #[test]
    fn a_corrupt_header_says_the_header_is_corrupt() {
        let (mut bytes, header_bytes) = frame(&header(), b"x").expect("frame");
        let at = MAGIC.len() + 4;
        bytes[at..at + header_bytes.len()].fill(b'{');
        assert!(matches!(unframe(&bytes), Err(FileError::BadHeader(_))));
    }

    #[test]
    fn every_failure_explains_itself_in_a_sentence() {
        // This file is read exactly once, by somebody who has lost everything
        // else. An error they cannot act on is the worst possible moment for
        // one.
        for error in [
            FileError::NotOurs,
            FileError::Version(9),
            FileError::Truncated { wanted: 10, got: 2 },
            FileError::BadHeader("x".to_owned()),
        ] {
            let said = error.to_string();
            assert!(said.len() > 20, "{said}");
        }
    }

    #[test]
    fn the_work_factor_is_expensive_on_an_attackers_hardware() {
        // Memory cost rather than iterations alone: a second on a laptop is a
        // second, and at 64 MiB it is also a second on a GPU.
        let kdf = Kdf::default();
        assert!(kdf.memory_kib >= 64 * 1024, "{}", kdf.memory_kib);
        assert!(kdf.iterations >= 3);
    }
}
