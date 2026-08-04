//! Blobs: content-addressed, sniffed, and bounded.
//!
//! Content addressing is what makes transfer idempotent. A phone that loses its
//! connection halfway through an upload and retries produces the same digest,
//! so the second attempt is a no-op rather than a second copy — and a client
//! that already has a blob can say so by name rather than sending it again.

pub mod transfer;

pub use transfer::{CHUNK_BYTES, Progress, Receiver, TransferError, TransferPlan};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The largest blob accepted.
///
/// Bounded because the sender is remote: without a cap, one client can fill the
/// host's disk, and the failure lands on everybody else using it.
pub const MAX_BLOB: usize = 32 << 20;

/// What a blob is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// An image.
    Image,
    /// Text.
    Text,
    /// Anything else.
    Binary,
}

/// A stored blob's metadata. Never the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobMeta {
    /// Its content hash, which is its name.
    pub digest: String,
    /// How many bytes.
    pub size: usize,
    /// What it appears to be.
    pub kind: MediaKind,
    /// Its media type, where one could be determined.
    pub media_type: Option<String>,
    /// A suggested filename extension.
    pub extension: Option<String>,
}

/// Why a blob was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    /// Larger than the cap.
    #[error("blob of {size} bytes exceeds the {max}-byte limit")]
    TooLarge {
        /// How large it was.
        size: usize,
        /// The cap.
        max: usize,
    },
    /// Nothing there.
    #[error("an empty blob is not a blob")]
    Empty,
}

/// The content hash of some bytes, hex encoded.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Identify a blob from its leading bytes.
///
/// From the *content*, never from a filename. A remote client chooses the
/// filename, so trusting it would let `screenshot.png` be an executable — and
/// the extension omt then suggests would be a lie it told itself.
#[must_use]
pub fn sniff(bytes: &[u8]) -> (MediaKind, Option<&'static str>, Option<&'static str>) {
    const SIGNATURES: &[(&[u8], &str, &str)] = &[
        (
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
            "image/png",
            "png",
        ),
        (&[0xff, 0xd8, 0xff], "image/jpeg", "jpg"),
        (b"GIF87a", "image/gif", "gif"),
        (b"GIF89a", "image/gif", "gif"),
        (b"%PDF-", "application/pdf", "pdf"),
    ];

    for (magic, media_type, ext) in SIGNATURES {
        if bytes.starts_with(magic) {
            let kind = if media_type.starts_with("image/") {
                MediaKind::Image
            } else {
                MediaKind::Binary
            };
            return (kind, Some(media_type), Some(ext));
        }
    }
    // WebP and RIFF-based formats carry their tag at offset 8.
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return (MediaKind::Image, Some("image/webp"), Some("webp"));
    }
    if std::str::from_utf8(bytes).is_ok() {
        return (MediaKind::Text, Some("text/plain"), Some("txt"));
    }
    (MediaKind::Binary, None, None)
}

/// Describe a blob, refusing anything out of bounds.
///
/// # Errors
/// Fails if the blob is empty or larger than [`MAX_BLOB`].
pub fn describe(bytes: &[u8]) -> Result<BlobMeta, MediaError> {
    if bytes.is_empty() {
        return Err(MediaError::Empty);
    }
    if bytes.len() > MAX_BLOB {
        return Err(MediaError::TooLarge {
            size: bytes.len(),
            max: MAX_BLOB,
        });
    }
    let (kind, media_type, extension) = sniff(bytes);
    Ok(BlobMeta {
        digest: digest(bytes),
        size: bytes.len(),
        kind,
        media_type: media_type.map(str::to_owned),
        extension: extension.map(str::to_owned),
    })
}

/// A filename safe to write into a temp directory.
///
/// Built from the digest rather than from anything the sender said. A remote
/// client's filename is untrusted input, and one containing `../` or a NUL is a
/// path traversal wearing a helpful name.
#[must_use]
pub fn safe_filename(meta: &BlobMeta) -> String {
    let stem = &meta.digest[..16.min(meta.digest.len())];
    match &meta.extension {
        Some(ext) => format!("omt-{stem}.{ext}"),
        None => format!("omt-{stem}"),
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

    fn png() -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        v.extend_from_slice(&[0u8; 32]);
        v
    }

    #[test]
    fn the_same_bytes_always_get_the_same_name() {
        // What makes a retried upload a no-op rather than a second copy.
        assert_eq!(digest(b"abc"), digest(b"abc"));
        assert_ne!(digest(b"abc"), digest(b"abd"));
    }

    #[test]
    fn a_retry_produces_the_same_blob() {
        let a = describe(&png()).expect("describe");
        let b = describe(&png()).expect("describe again");
        assert_eq!(a, b);
    }

    #[test]
    fn a_png_is_recognized_from_its_content() {
        let meta = describe(&png()).expect("describe");
        assert_eq!(meta.kind, MediaKind::Image);
        assert_eq!(meta.media_type.as_deref(), Some("image/png"));
        assert_eq!(meta.extension.as_deref(), Some("png"));
    }

    #[test]
    fn every_image_format_a_screenshot_might_use_is_recognized() {
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0, 0, 0, 0];
        assert_eq!(sniff(&jpeg).0, MediaKind::Image);
        assert_eq!(sniff(b"GIF89a....").0, MediaKind::Image);
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(b"WEBPVP8 ");
        assert_eq!(sniff(&webp).0, MediaKind::Image);
    }

    #[test]
    fn text_is_recognized_as_text() {
        let meta = describe(b"hello, world").expect("describe");
        assert_eq!(meta.kind, MediaKind::Text);
    }

    #[test]
    fn arbitrary_bytes_are_binary_rather_than_guessed_at() {
        let meta = describe(&[0xde, 0xad, 0xbe, 0xef, 0xff, 0xfe]).expect("describe");
        assert_eq!(meta.kind, MediaKind::Binary);
        assert_eq!(meta.media_type, None, "no type is better than a wrong one");
    }

    #[test]
    fn a_filename_never_comes_from_the_sender() {
        // A remote client's filename is untrusted input, and one containing
        // `../` is a path traversal wearing a helpful name.
        let meta = describe(&png()).expect("describe");
        let name = safe_filename(&meta);
        assert!(name.starts_with("omt-"));
        assert!(name.ends_with(".png"));
        assert!(!name.contains('/') && !name.contains('\\') && !name.contains(".."));
    }

    #[test]
    fn a_filename_is_stable_for_the_same_content() {
        let a = safe_filename(&describe(&png()).expect("a"));
        let b = safe_filename(&describe(&png()).expect("b"));
        assert_eq!(a, b, "so a retry overwrites rather than accumulating");
    }

    #[test]
    fn an_oversized_blob_is_refused_with_both_numbers() {
        // The sender is remote; without a cap one client fills the host's disk
        // and everybody else pays for it.
        let huge = vec![0u8; MAX_BLOB + 1];
        let err = describe(&huge).expect_err("must refuse");
        assert!(
            matches!(err, MediaError::TooLarge { size, max } if size > max),
            "{err:?}"
        );
    }

    #[test]
    fn an_empty_blob_is_refused() {
        assert_eq!(describe(&[]), Err(MediaError::Empty));
    }

    #[test]
    fn a_blob_exactly_at_the_limit_is_accepted() {
        // Off-by-one at a boundary is how a legitimate file gets rejected.
        let at_limit = vec![b'a'; MAX_BLOB];
        assert!(describe(&at_limit).is_ok());
    }

    #[test]
    fn the_metadata_never_carries_the_bytes() {
        let meta = describe(&png()).expect("describe");
        let json = serde_json::to_string(&meta).expect("serialize");
        assert!(!json.contains("\"data\""), "{json}");
        assert!(json.contains(&meta.digest));
    }
}
