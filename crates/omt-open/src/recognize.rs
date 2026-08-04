//! Finding paths, URLs and error locations in terminal output.
//!
//! Two rules the whole module is built around.
//!
//! **Nothing is matched across a soft wrap.** Half a URL is not a URL, and a
//! link that appears and vanishes as a window is resized is worse than no link
//! at all.
//!
//! **Recognition is pure.** Nothing here touches the filesystem, resolves a
//! symlink or opens anything. It reports what the *text* looks like; whether a
//! path exists, and whether the user may open it, are decisions made later with
//! the answers to different questions.

use serde::{Deserialize, Serialize};

/// Something in the output that could be acted on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// A filesystem path, possibly with a line and column.
    Path {
        /// The path as written.
        raw: String,
        /// A line number, where the text carried one.
        line: Option<u32>,
        /// A column.
        column: Option<u32>,
    },
    /// A URL.
    Url {
        /// The URL as written.
        raw: String,
    },
}

/// Where in a line something was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// What it is.
    pub target: Target,
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset one past the last.
    pub end: usize,
}

/// Schemes a recognized URL may use.
///
/// An allow-list rather than a deny-list. The interesting schemes are the ones
/// nobody thought of: `file:`, `javascript:`, and every custom scheme some
/// installed application registered. A deny-list is a list of the attacks
/// somebody already had.
pub const ALLOWED_SCHEMES: &[&str] = &["http", "https", "ftp", "ftps", "mailto", "ssh", "git"];

/// Whether a scheme may be turned into an openable target.
#[must_use]
pub fn scheme_allowed(scheme: &str) -> bool {
    ALLOWED_SCHEMES.contains(&scheme.to_lowercase().as_str())
}

/// Trailing characters that are almost never part of the thing.
///
/// A URL at the end of a sentence picks up the full stop; one in parentheses
/// picks up the bracket. Trimming these is why a link is clickable rather than
/// almost-clickable.
const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?', '\'', '"', ')', ']', '}'];

/// Leading characters that are almost never part of the thing.
///
/// A URL in parentheses or quoted in a log message. Without this the opening
/// bracket travels with the link and it resolves to nothing.
const LEADING_PUNCTUATION: &[char] = &['(', '[', '{', '\'', '"', '<'];

/// Find every actionable target in one line of text.
///
/// Takes a single line by construction: a caller cannot accidentally pass two
/// joined by a soft wrap, because that is exactly the mistake that produces
/// half-URLs.
#[must_use]
pub fn recognize(line: &str) -> Vec<Match> {
    let mut out = Vec::new();
    let mut index = 0usize;

    for token in line.split_whitespace() {
        // Find the token's real offset rather than assuming, since runs of
        // whitespace collapse in the iterator.
        let Some(found) = line[index..].find(token) else {
            continue;
        };
        let start = index + found;
        index = start + token.len();

        let lead = token.len() - token.trim_start_matches(LEADING_PUNCTUATION).len();
        let trimmed = token
            .trim_start_matches(LEADING_PUNCTUATION)
            .trim_end_matches(TRAILING_PUNCTUATION);
        if trimmed.is_empty() {
            continue;
        }
        let start = start + lead;
        let end = start + trimmed.len();

        if let Some(url) = as_url(trimmed) {
            out.push(Match {
                target: url,
                start,
                end,
            });
            continue;
        }
        if let Some(path) = as_path(trimmed) {
            out.push(Match {
                target: path,
                start,
                end,
            });
        }
    }

    out
}

fn as_url(token: &str) -> Option<Target> {
    let (scheme, rest) = token.split_once("://").or_else(|| token.split_once(':'))?;
    if rest.is_empty() || !scheme_allowed(scheme) {
        return None;
    }
    // A scheme has to look like one: `C:\Users` and `error:` are not URLs, and
    // treating them as such is how a Windows path becomes a clickable link to
    // nowhere.
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-')
        || scheme.len() < 2
    {
        return None;
    }
    Some(Target::Url {
        raw: token.to_owned(),
    })
}

fn as_path(token: &str) -> Option<Target> {
    // Anything carrying a scheme is a URL or nothing. Without this,
    // `data:text/html;base64,...` contains a slash and reads as a relative
    // path — a rejected scheme must not come back through the other door.
    if let Some((scheme, _)) = token.split_once(':')
        && scheme.len() >= 2
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '+' || c == '-')
    {
        return None;
    }

    // `path:line:col`, `path:line`, or a bare path. Split from the right, so a
    // path that itself contains a colon is not mangled.
    let (path, line, column) = split_location(token);

    let looks_like_path = path.starts_with('/')
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with('~')
        || (path.contains('/') && !path.starts_with('-'));

    if !looks_like_path || path.len() < 2 {
        return None;
    }

    Some(Target::Path {
        raw: path.to_owned(),
        line,
        column,
    })
}

/// Split `path:line:col` into its parts.
///
/// Only a *trailing run of numbers* is a location. Anything else stays part of
/// the path: silently dropping a trailing segment because it followed a colon
/// would turn `/etc/hosts:something` into a path the user did not write, and a
/// path that is quietly wrong is worse than one that is obviously unopenable.
fn split_location(token: &str) -> (&str, Option<u32>, Option<u32>) {
    // Peel at most two numeric segments off the end.
    let (head, last) = match token.rsplit_once(':') {
        Some((h, l)) if !h.is_empty() => (h, l),
        _ => return (token, None, None),
    };
    let Ok(last_n) = last.parse::<u32>() else {
        return (token, None, None);
    };

    match head.rsplit_once(':') {
        Some((path, mid)) if !path.is_empty() => match mid.parse::<u32>() {
            // `path:line:col`
            Ok(line) => (path, Some(line), Some(last_n)),
            // `path:line`, where the path itself contains a colon.
            Err(_) => (head, Some(last_n), None),
        },
        // `path:line`
        _ => (head, Some(last_n), None),
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

    fn targets(line: &str) -> Vec<Target> {
        recognize(line).into_iter().map(|m| m.target).collect()
    }

    fn path(raw: &str, line: Option<u32>, column: Option<u32>) -> Target {
        Target::Path {
            raw: raw.to_owned(),
            line,
            column,
        }
    }

    fn url(raw: &str) -> Target {
        Target::Url {
            raw: raw.to_owned(),
        }
    }

    #[test]
    fn a_rust_error_location_is_recognized_with_line_and_column() {
        // The case this exists for: jumping from a compiler error to the line.
        assert_eq!(
            targets("  --> crates/omt-term/src/lib.rs:42:9"),
            [path("crates/omt-term/src/lib.rs", Some(42), Some(9))]
        );
    }

    #[test]
    fn a_path_with_only_a_line_is_recognized() {
        assert_eq!(
            targets("src/main.rs:17: warning"),
            [path("src/main.rs", Some(17), None)]
        );
    }

    #[test]
    fn an_absolute_path_is_recognized_without_a_location() {
        assert_eq!(targets("see /etc/hosts"), [path("/etc/hosts", None, None)]);
    }

    #[test]
    fn a_relative_path_needs_a_marker_to_be_one() {
        // Otherwise every English word in the output becomes a link.
        assert_eq!(
            targets("./config.toml"),
            [path("./config.toml", None, None)]
        );
        assert_eq!(
            targets("../sibling/file.rs"),
            [path("../sibling/file.rs", None, None)]
        );
        assert!(targets("just some ordinary prose here").is_empty());
    }

    #[test]
    fn a_url_is_recognized() {
        assert_eq!(
            targets("see https://example.com/a/b"),
            [url("https://example.com/a/b")]
        );
    }

    #[test]
    fn trailing_punctuation_is_not_part_of_the_link() {
        // A URL at the end of a sentence, or in parentheses. Getting this wrong
        // is why links are almost-clickable in most terminals.
        assert_eq!(
            targets("see https://example.com/page."),
            [url("https://example.com/page")]
        );
        assert_eq!(
            targets("(https://example.com/page)"),
            [url("https://example.com/page")]
        );
    }

    #[test]
    fn a_dangerous_scheme_is_not_recognized() {
        // An allow-list, because the interesting schemes are the ones nobody
        // thought of — a deny-list is a list of the attacks somebody already
        // had.
        assert!(targets("javascript:alert(1)").is_empty());
        assert!(targets("data:text/html;base64,AAAA").is_empty());
        assert!(targets("file:///etc/passwd").is_empty());
        assert!(!scheme_allowed("javascript"));
        assert!(scheme_allowed("https"));
        assert!(scheme_allowed("HTTPS"), "and case does not smuggle one in");
    }

    #[test]
    fn a_windows_drive_letter_is_not_a_url() {
        // `C:\Users\me` would otherwise become a clickable link to nowhere.
        assert!(
            !targets("C:\\Users\\me")
                .iter()
                .any(|t| matches!(t, Target::Url { .. })),
            "{:?}",
            targets("C:\\Users\\me")
        );
    }

    #[test]
    fn a_bare_word_with_a_colon_is_not_a_url() {
        assert!(targets("error: something went wrong").is_empty());
        assert!(targets("note: see above").is_empty());
    }

    #[test]
    fn several_targets_in_one_line_are_all_found() {
        let found = targets("edit src/a.rs:1 then see https://example.com");
        assert_eq!(found.len(), 2);
        assert!(matches!(found[0], Target::Path { .. }));
        assert!(matches!(found[1], Target::Url { .. }));
    }

    #[test]
    fn offsets_point_at_the_text_that_was_matched() {
        // A renderer underlines by offset; an offset that is off by one
        // underlines the wrong character.
        let line = "  --> src/main.rs:3:1";
        let m = &recognize(line)[0];
        assert_eq!(&line[m.start..m.end], "src/main.rs:3:1");
    }

    #[test]
    fn offsets_survive_runs_of_whitespace() {
        // The naive implementation assumes single spaces and drifts.
        let line = "a    b    /etc/hosts";
        let m = &recognize(line)[0];
        assert_eq!(&line[m.start..m.end], "/etc/hosts");
    }

    #[test]
    fn a_path_containing_a_colon_is_not_mangled() {
        // Splitting from the left would cut this in the middle.
        assert_eq!(
            targets("/tmp/weird:name/file.txt:9"),
            [path("/tmp/weird:name/file.txt", Some(9), None)]
        );
    }

    #[test]
    fn a_trailing_non_numeric_segment_stays_part_of_the_path() {
        // Dropping it would produce a path the user never wrote, and a path
        // that is quietly wrong is worse than one that obviously will not open.
        assert_eq!(
            targets("/etc/hosts:something"),
            [path("/etc/hosts:something", None, None)]
        );
    }

    #[test]
    fn recognition_touches_nothing_on_disk() {
        // Recognition is pure: whether a path exists is a different question,
        // asked later, and answering it here would stat every word of every
        // line of output.
        let found = targets("/this/path/definitely/does/not/exist.rs:1:1");
        assert_eq!(
            found,
            [path(
                "/this/path/definitely/does/not/exist.rs",
                Some(1),
                Some(1)
            )],
            "a nonexistent path is still recognized as one"
        );
    }

    #[test]
    fn an_empty_line_yields_nothing() {
        assert!(recognize("").is_empty());
        assert!(recognize("      ").is_empty());
    }

    #[test]
    fn a_home_relative_path_is_recognized() {
        assert_eq!(
            targets("~/.config/omt/config.toml"),
            [path("~/.config/omt/config.toml", None, None)]
        );
    }
}
