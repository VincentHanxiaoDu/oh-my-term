//! omt's own log, which did not exist.
//!
//! The `log.*` settings have been in the reference and the settings screen
//! since either existed — level, file, max size, redaction — and nothing
//! logged anything: no subsystem, no call sites, and every `eprintln!` was
//! what it was. Four settings whose whole meaning is "there is something
//! behind them" is the vocabulary-without-a-consumer shape again.
//!
//! What exists now is deliberately small: a level, an optional file with
//! rotation, and redaction. It is not a framework — there is no logger crate
//! to configure, no span plumbing, and the places that say something *to the
//! person at the terminal* (the reason omt closed, the crash note) stay where
//! a person reads them. This is the layer *under* those.

use std::io::Write as _;
use std::path::PathBuf;

/// The levels a line can be written at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    /// Something is broken or will be.
    Error,
    /// Something may need a look.
    Warn,
    /// What happened, the default.
    Info,
    /// Why, for the session being chased.
    Debug,
    /// Everything, for the performance question.
    Trace,
}

impl Level {
    /// From the setting's spelling; anything else is the default.
    fn from_setting(name: &str) -> Self {
        match name.trim().trim_matches('"') {
            "error" => Self::Error,
            "warn" => Self::Warn,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => Self::Info,
        }
    }
}

/// The log, built once from the settings that configured nothing.
#[derive(Debug, Clone)]
pub(crate) struct Diag {
    level: Level,
    file: Option<PathBuf>,
    max_bytes: u64,
    redact: bool,
}

impl Diag {
    /// From the configuration.
    ///
    /// A file that cannot be written is not a reason to refuse to start: the
    /// log is the thing that says what happened *after* the refusal, so a
    /// refused log file must cost nothing but the lines it would have held.
    pub(crate) fn from_state(state: &crate::state::State) -> Self {
        let configured = crate::terminal_bits::setting(state, "log.file");
        let file = match configured.trim().trim_matches('"').trim() {
            "" => None,
            path => Some(PathBuf::from(path)),
        };
        let max_bytes = crate::terminal_bits::byte_setting(state, "log.max_size")
            .filter(|n| *n > 0)
            .unwrap_or(32 << 20);
        Self {
            level: Level::from_setting(&crate::terminal_bits::setting(state, "log.level")),
            // A path outside the configured home is refused, exactly as the
            // shell installer refuses one: a terminal should not write
            // wherever a file says.
            file,
            max_bytes,
            redact: crate::terminal_bits::setting(state, "log.redact_secrets").contains("true"),
        }
    }

    /// Write a line, if the level admits it.
    pub(crate) fn log(&self, at: Level, message: &str) {
        if at > self.level {
            return;
        }
        let line = format!("[{at:?}] {message}");
        let line = if self.redact { redact(&line) } else { line };
        match &self.file {
            Some(path) => append_rotating(path, self.max_bytes, &line),
            None => eprintln!("{line}"),
        }
    }
}

/// Append a line to the log, rotating the file when it is over the cap.
///
/// The old file becomes `<name>.1` and a fresh one starts: a log that grows
/// forever is how a disk fills, and HANDOFF §5 lists exactly that as one of
/// the ways this machine has been stopped before.
fn append_rotating(path: &PathBuf, max_bytes: u64, line: &str) {
    let needs_rotation = std::fs::metadata(path)
        .map(|m| m.len() + line.len() as u64 > max_bytes)
        .unwrap_or(false);
    if needs_rotation {
        let rotated = path.with_extension("log.1");
        let _ = std::fs::rename(path, rotated);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// Replace the values of known secret shapes.
///
/// Conservative by shape rather than by name: `token=abc`, `Authorization:
/// Bearer abc`, `x-api-key: abc` are shapes, not spellings, so a secret in a
/// setting renamed tomorrow is still caught. The value is replaced, never the
/// shape — what happened has to stay readable. The first eight characters
/// survive, which is enough to tell two secrets apart and nothing more.
fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(non_ws) = rest.find(|c: char| !c.is_whitespace()) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..non_ws]);
        rest = &rest[non_ws..];
        let word_end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let word = &rest[..word_end];
        out.push_str(word);
        rest = &rest[word_end..];
        if word.is_empty() {
            if rest.is_empty() {
                break;
            }
            out.push_str(&rest[..1]);
            rest = &rest[1..];
            continue;
        }
        let secret = is_secret_name(word);
        let t = rest.trim_start_matches(' ');
        let skip = rest.len() - t.len();
        let value = t
            .strip_prefix('=')
            .or_else(|| t.strip_prefix(": "))
            .filter(|_| secret);
        if let Some(value) = value {
            out.push_str(&rest[..skip]);
            out.push(if rest[skip..].starts_with('=') {
                '='
            } else {
                ':'
            });
            if rest[skip..].starts_with(": ") {
                out.push(' ');
            }
            let end = value.find([',', '\n', '\r']).unwrap_or(value.len());
            let shown = end.min(8);
            out.push_str(&value[..shown]);
            if end > 8 {
                out.push('…');
            }
            rest = &value[end..];
        } else {
            out.push_str(&rest[..skip]);
            rest = &rest[skip..];
            if rest.is_empty() {
                break;
            }
            out.push_str(&rest[..1]);
            rest = &rest[1..];
        }
    }
    out
}

/// Whether a word names a secret.
fn is_secret_name(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "token" | "secret" | "password" | "passwd" | "api_key" | "apikey" | "key"
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::{Diag, Level, append_rotating, redact};

    #[test]
    fn a_line_under_the_level_is_never_written() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("omt.log");
        let diag = Diag {
            level: Level::Error,
            file: Some(path.clone()),
            max_bytes: 1 << 20,
            redact: false,
        };
        diag.log(Level::Info, "quiet");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap_or_default()
                .is_empty(),
            "an info line was written under error level"
        );
        diag.log(Level::Error, "loud");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap_or_default()
                .contains("loud")
        );
    }

    #[test]
    fn the_file_rotates_at_the_cap() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("omt.log");
        append_rotating(&path, 20, "1234567890");
        append_rotating(&path, 20, "1234567890");
        append_rotating(&path, 20, "1234567890");
        append_rotating(&path, 20, "1234567890");
        let rotated = path.with_extension("log.1");
        assert!(
            rotated.exists(),
            "the log never rotated and grew past its cap"
        );
    }

    #[test]
    fn secret_shapes_are_redacted_by_value() {
        let out = redact("token=abcdef1234567890 and api_key: supersekrit, rest");
        assert!(!out.contains("abcdef1234567890"), "{out}");
        assert!(!out.contains("supersekrit"), "{out}");
        // The shape survives, so the line still reads.
        assert!(out.contains("token="), "{out}");
    }

    #[test]
    fn ordinary_lines_are_untouched() {
        let out = redact("the pane said: hello world, twice");
        assert!(out.contains("hello world"), "{out}");
    }
}
