//! Reading an agent's own files without trusting them.
//!
//! A transcript or state file is written by another program, in a directory a
//! repository can often influence. Parsing one unbounded is three separate ways
//! to take the daemon down — a huge file, a deeply nested document, or a
//! pathological one that is small on disk and enormous once parsed.
//!
//! All three are bounded here, **before** the parser sees anything, because a
//! parser that has already begun is a parser that has already allocated.

use std::path::Path;

/// The largest agent-written file read whole.
pub const MAX_FILE_BYTES: usize = 4 << 20;

/// The most structural tokens a document may contain.
///
/// Bounds the *parsed* size, which the byte count does not: `[[[[…]]]]` is a
/// few kilobytes on disk and a very different thing in memory.
pub const MAX_STRUCTURAL_TOKENS: usize = 1_000_000;

/// How deep nesting may go.
///
/// A recursive-descent parser turns depth into stack, so an attacker-influenced
/// document with ten thousand open brackets is a stack overflow — which aborts
/// rather than unwinding, taking every other session with it.
pub const MAX_NESTING_DEPTH: usize = 128;

/// Why a file was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BoundedError {
    /// Larger than the byte cap.
    #[error("`{path}` is {size} bytes, over the {max}-byte limit")]
    TooLarge {
        /// Which file.
        path: String,
        /// Its size.
        size: u64,
        /// The cap.
        max: usize,
    },
    /// More structure than the cap allows.
    #[error("that document has more than {max} structural tokens")]
    TooComplex {
        /// The cap.
        max: usize,
    },
    /// Nested deeper than the cap allows.
    #[error("that document nests deeper than {max}")]
    TooDeep {
        /// The cap.
        max: usize,
    },
    /// It could not be read.
    #[error("could not read `{path}`: {detail}")]
    Io {
        /// Which file.
        path: String,
        /// What happened.
        detail: String,
    },
    /// It is not the shape it claimed.
    #[error("malformed: {0}")]
    Malformed(String),
}

/// Read a file, refusing one that is too large **without reading it**.
///
/// The size is checked from the metadata first. Reading and then measuring
/// would mean a hostile file had already been in memory by the time it was
/// rejected, which is the failure the cap exists to prevent.
///
/// # Errors
/// Fails if the file is missing, unreadable, or over [`MAX_FILE_BYTES`].
pub fn read_bounded(path: &Path) -> Result<String, BoundedError> {
    let meta = std::fs::metadata(path).map_err(|e| BoundedError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    })?;
    let size = meta.len();
    if size > MAX_FILE_BYTES as u64 {
        return Err(BoundedError::TooLarge {
            path: path.display().to_string(),
            size,
            max: MAX_FILE_BYTES,
        });
    }
    std::fs::read_to_string(path).map_err(|e| BoundedError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    })
}

/// Check a document's shape before parsing it.
///
/// A scan over bytes rather than a parse: the point is to decide whether
/// parsing is safe, and a check that itself parses has already done the
/// dangerous thing.
///
/// # Errors
/// Fails if the text has more structure or deeper nesting than the caps allow.
pub fn check_structure(text: &str) -> Result<(), BoundedError> {
    let mut depth = 0usize;
    let mut tokens = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for b in text.bytes() {
        if in_string {
            // Brackets inside a string are text, and a scanner that counted
            // them would reject ordinary documents containing JSON in a field.
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => {
                in_string = true;
                tokens += 1;
            }
            b'{' | b'[' => {
                depth += 1;
                tokens += 1;
                if depth > MAX_NESTING_DEPTH {
                    return Err(BoundedError::TooDeep {
                        max: MAX_NESTING_DEPTH,
                    });
                }
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                tokens += 1;
            }
            b',' | b':' => tokens += 1,
            _ => {}
        }
        if tokens > MAX_STRUCTURAL_TOKENS {
            return Err(BoundedError::TooComplex {
                max: MAX_STRUCTURAL_TOKENS,
            });
        }
    }
    Ok(())
}

/// Read and parse an agent's JSON, with every bound applied first.
///
/// # Errors
/// Fails if the file is too large, too complex, too deeply nested, unreadable,
/// or not valid JSON.
pub fn read_json(path: &Path) -> Result<serde_json::Value, BoundedError> {
    let text = read_bounded(path)?;
    check_structure(&text)?;
    serde_json::from_str(&text).map_err(|e| BoundedError::Malformed(e.to_string()))
}

/// Parse one line of a JSON-lines transcript, bounded.
///
/// # Errors
/// Fails if the line is over-structured or is not valid JSON.
pub fn parse_line(line: &str) -> Result<serde_json::Value, BoundedError> {
    check_structure(line)?;
    serde_json::from_str(line).map_err(|e| BoundedError::Malformed(e.to_string()))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, contents: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, contents).expect("write");
        p
    }

    #[test]
    fn an_ordinary_transcript_reads_fine() {
        let d = tempfile::tempdir().expect("tempdir");
        let p = write(d.path(), "t.json", r#"{"role":"assistant","text":"hello"}"#);
        let v = read_json(&p).expect("read");
        assert_eq!(v["role"], "assistant");
    }

    #[test]
    fn an_oversized_file_is_refused_without_being_read() {
        // Reading and then measuring means a hostile file was already in
        // memory by the time it was rejected.
        let d = tempfile::tempdir().expect("tempdir");
        let p = write(d.path(), "big.json", &"x".repeat(MAX_FILE_BYTES + 1));
        let err = read_bounded(&p).expect_err("must refuse");
        assert!(
            matches!(err, BoundedError::TooLarge { size, max, .. } if size as usize > max),
            "{err:?}"
        );
    }

    #[test]
    fn a_file_exactly_at_the_limit_is_accepted() {
        // Off-by-one at a boundary rejects a legitimate transcript.
        let d = tempfile::tempdir().expect("tempdir");
        let p = write(d.path(), "edge.json", &"x".repeat(MAX_FILE_BYTES));
        assert!(read_bounded(&p).is_ok());
    }

    #[test]
    fn a_deeply_nested_document_is_refused_before_parsing() {
        // Depth becomes stack in a recursive-descent parser, and a stack
        // overflow aborts rather than unwinding — taking every other session
        // with it.
        let bomb = "[".repeat(MAX_NESTING_DEPTH + 10);
        let err = check_structure(&bomb).expect_err("must refuse");
        assert!(matches!(err, BoundedError::TooDeep { .. }), "{err:?}");
    }

    #[test]
    fn nesting_within_the_limit_is_allowed() {
        let ok = format!("{}{}", "[".repeat(10), "]".repeat(10));
        assert!(check_structure(&ok).is_ok());
    }

    #[test]
    fn a_document_that_is_small_on_disk_but_enormous_parsed_is_refused() {
        // The case the byte cap does not catch.
        let mut wide = String::from("[");
        for _ in 0..MAX_STRUCTURAL_TOKENS {
            wide.push_str("1,");
        }
        wide.push(']');
        assert!(
            matches!(check_structure(&wide), Err(BoundedError::TooComplex { .. })),
            "a token bomb was accepted"
        );
    }

    #[test]
    fn brackets_inside_a_string_are_text_not_structure() {
        // Agents put JSON inside string fields constantly. Counting those
        // brackets would reject ordinary transcripts.
        let text = format!(r#"{{"tool_input":"{}"}}"#, "[".repeat(500));
        assert!(
            check_structure(&text).is_ok(),
            "an ordinary field was refused"
        );
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_string() {
        // Getting this wrong makes the scanner think it left the string and
        // start counting the brackets after it.
        let text = r#"{"text":"she said \"[[[[\" and left"}"#;
        assert!(check_structure(text).is_ok());
    }

    #[test]
    fn a_missing_file_is_an_error_rather_than_an_empty_document() {
        // Returning an empty value would make "the agent said nothing"
        // indistinguishable from "the file is not there".
        let d = tempfile::tempdir().expect("tempdir");
        let err = read_json(&d.path().join("nope.json")).expect_err("must fail");
        assert!(matches!(err, BoundedError::Io { .. }), "{err:?}");
    }

    #[test]
    fn malformed_json_is_reported_rather_than_swallowed() {
        let d = tempfile::tempdir().expect("tempdir");
        let p = write(d.path(), "bad.json", "{ not json");
        assert!(matches!(read_json(&p), Err(BoundedError::Malformed(_))));
    }

    #[test]
    fn a_transcript_line_is_bounded_the_same_way() {
        // JSON-lines transcripts are read a line at a time, and one line is as
        // capable of carrying a bomb as a whole file.
        assert!(parse_line(r#"{"a":1}"#).is_ok());
        assert!(matches!(
            parse_line(&"[".repeat(MAX_NESTING_DEPTH + 1)),
            Err(BoundedError::TooDeep { .. })
        ));
    }
}
