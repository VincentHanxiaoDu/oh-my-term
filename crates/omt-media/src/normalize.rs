//! Making an image safe to hand on.
//!
//! `docs/architecture/09-ssh-and-media.md` §4.1 and §8. A screenshot pasted
//! from a phone carries EXIF, and EXIF carries GPS. Passing that to an agent —
//! which may well send it to a model API — leaks where somebody was standing,
//! and it happens with no visible sign that anything was attached beyond the
//! picture they meant to share.
//!
//! So metadata is stripped on the way in, not on the way out. Stripping at the
//! point of use means every point of use has to remember, and one of them will
//! not.
//!
//! What is *not* done here is re-encoding. Decoding and re-encoding an image
//! to normalise it means running a decoder over untrusted bytes, which is the
//! largest attack surface in this whole document. Cutting metadata segments out
//! of the container is a byte-level operation over a length-prefixed structure,
//! and it does not require understanding a single pixel.

/// Which segments of a JPEG carry metadata rather than image data.
///
/// APP1 is EXIF and XMP, APP2 is ICC and FlashPix, APP13 is IPTC — which is
/// where camera software puts everything it could not fit elsewhere. Comments
/// go too: they are free text somebody may not know is there.
const METADATA_MARKERS: &[u8] = &[
    0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xEB, 0xEC, 0xED, 0xEE, 0xEF,
    0xFE,
];

/// Whether these bytes look like a JPEG.
#[must_use]
pub fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// Whether these bytes look like a PNG.
#[must_use]
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}

/// Remove metadata segments from a JPEG, leaving the image untouched.
///
/// Walks the segment chain rather than searching for byte patterns: a search
/// would eventually match inside compressed scan data and corrupt the image,
/// and it would do so only for some pictures, which is the worst way for this
/// to fail.
#[must_use]
pub fn strip_jpeg_metadata(bytes: &[u8]) -> Vec<u8> {
    if !is_jpeg(bytes) {
        return bytes.to_vec();
    }
    let mut out: Vec<u8> = vec![0xFF, 0xD8];
    let mut i = 2usize;
    while i + 3 < bytes.len() {
        if bytes[i] != 0xFF {
            // Out of step with the segment structure. Rather than guess, keep
            // the remainder verbatim: a corrupted image is a worse outcome
            // than metadata that survived in a file omt could not parse.
            out.extend_from_slice(&bytes[i..]);
            return out;
        }
        let marker = bytes[i + 1];
        // Start of scan: everything after it is entropy-coded image data with
        // no further segment headers to walk.
        if marker == 0xDA {
            out.extend_from_slice(&bytes[i..]);
            return out;
        }
        let length = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        if length < 2 || i + 2 + length > bytes.len() {
            out.extend_from_slice(&bytes[i..]);
            return out;
        }
        if !METADATA_MARKERS.contains(&marker) {
            out.extend_from_slice(&bytes[i..i + 2 + length]);
        }
        i += 2 + length;
    }
    if i < bytes.len() {
        out.extend_from_slice(&bytes[i..]);
    }
    out
}

/// Which PNG chunks carry text or timestamps rather than image data.
const PNG_METADATA: &[&[u8; 4]] = &[b"tEXt", b"zTXt", b"iTXt", b"tIME", b"eXIf"];

/// Remove metadata chunks from a PNG.
#[must_use]
pub fn strip_png_metadata(bytes: &[u8]) -> Vec<u8> {
    if !is_png(bytes) {
        return bytes.to_vec();
    }
    let mut out: Vec<u8> = bytes[..8].to_vec();
    let mut i = 8usize;
    while i + 8 <= bytes.len() {
        let length = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]])
            as usize;
        let Some(kind) = bytes.get(i + 4..i + 8) else {
            break;
        };
        let end = match i.checked_add(12).and_then(|x| x.checked_add(length)) {
            Some(end) if end <= bytes.len() => end,
            // Truncated or lying about its own length. Keep the rest verbatim
            // rather than producing a file that is neither the original nor a
            // valid PNG.
            _ => {
                out.extend_from_slice(&bytes[i..]);
                return out;
            }
        };
        let is_metadata = PNG_METADATA.iter().any(|m| m.as_slice() == kind);
        if !is_metadata {
            out.extend_from_slice(&bytes[i..end]);
        }
        i = end;
    }
    out
}

/// Strip whatever metadata this image carries.
#[must_use]
pub fn normalize(bytes: &[u8]) -> Vec<u8> {
    if is_jpeg(bytes) {
        strip_jpeg_metadata(bytes)
    } else if is_png(bytes) {
        strip_png_metadata(bytes)
    } else {
        // Not a container omt understands, so nothing is claimed about it.
        // Silently returning it unchanged is right; silently calling it
        // stripped would not be.
        bytes.to_vec()
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

    /// A JPEG with an APP1 EXIF segment holding something identifiable.
    fn jpeg_with_exif() -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        let payload = b"Exif\0\0GPSLatitude=51.5074";
        let length = (payload.len() + 2) as u16;
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(payload);
        // A quantisation table, which must survive.
        let dqt = b"\x00\x01\x02\x03";
        out.extend_from_slice(&[0xFF, 0xDB]);
        out.extend_from_slice(&((dqt.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(dqt);
        // Start of scan, then image data.
        out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
        out.extend_from_slice(b"IMAGEDATA");
        out
    }

    fn png_with_text() -> Vec<u8> {
        let mut out: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let chunk = |kind: &[u8; 4], data: &[u8]| {
            let mut c = (data.len() as u32).to_be_bytes().to_vec();
            c.extend_from_slice(kind);
            c.extend_from_slice(data);
            c.extend_from_slice(&[0, 0, 0, 0]);
            c
        };
        out.extend(chunk(b"IHDR", &[0; 13]));
        out.extend(chunk(b"tEXt", b"Author\0somebody@example.com"));
        out.extend(chunk(b"IDAT", b"PIXELS"));
        out.extend(chunk(b"IEND", b""));
        out
    }

    #[test]
    fn gps_does_not_survive_a_paste() {
        // The whole point. Passing this to an agent leaks where somebody was
        // standing, with no visible sign anything was attached beyond the
        // picture they meant to share.
        let stripped = normalize(&jpeg_with_exif());
        let text = String::from_utf8_lossy(&stripped);
        assert!(!text.contains("GPSLatitude"), "{text:?}");
        assert!(!text.contains("Exif"));
    }

    #[test]
    fn the_image_itself_survives_stripping() {
        let stripped = normalize(&jpeg_with_exif());
        let text = String::from_utf8_lossy(&stripped);
        assert!(text.contains("IMAGEDATA"), "the scan data is still there");
        assert!(stripped.starts_with(&[0xFF, 0xD8]), "still a JPEG");
        assert!(
            stripped.windows(2).any(|w| w == [0xFF, 0xDB]),
            "the quantisation table must survive or the image will not decode"
        );
    }

    #[test]
    fn png_text_chunks_are_removed_and_pixels_are_not() {
        let stripped = normalize(&png_with_text());
        let text = String::from_utf8_lossy(&stripped);
        assert!(!text.contains("example.com"), "{text:?}");
        assert!(text.contains("PIXELS"));
        assert!(text.contains("IHDR"));
        assert!(text.contains("IEND"));
    }

    #[test]
    fn everything_after_the_start_of_scan_is_left_alone() {
        // Searching for byte patterns instead of walking the segment chain
        // would eventually match inside compressed scan data and corrupt the
        // image — for some pictures only, which is the worst way to fail.
        let mut jpeg = jpeg_with_exif();
        jpeg.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x08, 1, 2, 3, 4, 5, 6]);
        let stripped = normalize(&jpeg);
        assert!(
            stripped.ends_with(&[0xFF, 0xE1, 0x00, 0x08, 1, 2, 3, 4, 5, 6]),
            "bytes that merely look like a marker were rewritten"
        );
    }

    #[test]
    fn something_that_is_not_an_image_is_returned_unchanged() {
        // Nothing is claimed about it. Silently calling it stripped would be
        // the lie.
        let bytes = b"just some text".to_vec();
        assert_eq!(normalize(&bytes), bytes);
    }

    #[test]
    fn a_truncated_image_does_not_panic_or_produce_rubbish() {
        // Untrusted input, so the interesting cases are the malformed ones.
        for cut in 1..jpeg_with_exif().len() {
            let _ = normalize(&jpeg_with_exif()[..cut]);
        }
        for cut in 1..png_with_text().len() {
            let _ = normalize(&png_with_text()[..cut]);
        }
    }

    #[test]
    fn a_chunk_that_lies_about_its_length_is_survived() {
        let mut png = png_with_text();
        // Claim a chunk is four gigabytes long.
        png[8] = 0xFF;
        png[9] = 0xFF;
        png[10] = 0xFF;
        png[11] = 0xFF;
        let out = normalize(&png);
        assert!(!out.is_empty(), "no panic, and no empty file");
    }

    #[test]
    fn stripping_twice_changes_nothing_the_second_time() {
        let once = normalize(&jpeg_with_exif());
        assert_eq!(normalize(&once), once);
    }
}
