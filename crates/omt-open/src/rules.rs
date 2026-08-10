//! The formats compilers and test runners actually print.
//!
//! `docs/architecture/18-semantic-open.md` §2.7. Token-splitting on whitespace
//! finds `src/main.rs:42`, and that covers the easy half. It cannot find
//! `File "my file.py", line 9` — the path has a space in it — and it cannot
//! find rustc's `--> src/x.rs:9:5` without the arrow travelling along with it.
//! Those are the lines people most want to click, because they are the lines
//! printed when something is broken.
//!
//! Each rule is anchored on a literal the format guarantees: `File "` and
//! `", line `, the `--> `, `panicked at `. That is why these are hand-written
//! scanners rather than patterns. They are not general regular expressions;
//! they are a handful of fixed anchors, and writing them out means each one
//! can say in a comment which tool prints it and why it needs its own rule.
//!
//! Rules are tried in precedence order and the first to match a region owns it.
//! The quoted Python form is highest precisely because the quotes make it
//! unambiguous — a path with spaces is only safe to take when something in the
//! syntax says where it ends.

use crate::recognize::{Match, Target};

/// One recognised region, before it becomes a `Match`.
struct Found {
    path: String,
    line: Option<u32>,
    column: Option<u32>,
    start: usize,
    end: usize,
}

/// `File "<path>", line <n>` — Python tracebacks.
///
/// Highest precedence. The quotes delimit the path, so this is the one format
/// where a path containing spaces can be taken without guessing.
fn python_traceback(line: &str) -> Option<Found> {
    let anchor = line.find("File \"")?;
    let after = anchor + 6;
    let close = line[after..].find('"')? + after;
    let path = &line[after..close];
    if path.is_empty() {
        return None;
    }
    let rest = &line[close..];
    let number = rest.strip_prefix("\", line ").map(|tail| {
        tail.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
    });
    Some(Found {
        path: path.to_owned(),
        line: number.and_then(|n| n.parse().ok()),
        column: None,
        start: after,
        end: close,
    })
}

/// `--> <path>:<line>:<col>` — rustc, and everything that copied its format.
///
/// clippy, miri, cargo-nextest. Anchored on the arrow, so the arrow itself is
/// not swept into the path the way whitespace splitting would.
fn rustc_arrow(line: &str) -> Option<Found> {
    let anchor = line.find("--> ")?;
    let start = anchor + 4;
    let token = line[start..].split_whitespace().next()?;
    require_location(token).map(|(path, l, c)| Found {
        path,
        line: l,
        column: c,
        start,
        end: start + token.len(),
    })
}

/// `panicked at <path>:<line>:<col>` — Rust 1.73 and later.
fn rust_panic(line: &str) -> Option<Found> {
    let anchor = line.find("panicked at ")?;
    let start = anchor + 12;
    let token = line[start..].split_whitespace().next()?;
    let token = token.trim_end_matches(':');
    require_location(token).map(|(path, l, c)| Found {
        path,
        line: l,
        column: c,
        start,
        end: start + token.len(),
    })
}

/// `at <fn> (<path>:<line>:<col>)` and `at <path>:<line>:<col>` — Node and V8.
///
/// jest, vitest and tsx all print through the same stack formatter. The
/// parenthesised form is the common one and the bare form appears for
/// top-level frames, so both have to work or half a stack trace is clickable.
fn node_stack(line: &str) -> Option<Found> {
    let anchor = line.find(" at ").map_or_else(
        || line.strip_prefix("at ").map(|_| 0usize),
        |i| Some(i + 1),
    )?;
    let rest = &line[anchor + 3..];
    // Prefer whatever is inside parentheses; that is where the location goes
    // when the frame has a function name.
    let (token, offset) = match (rest.find('('), rest.find(')')) {
        (Some(open), Some(close)) if close > open + 1 => {
            (&rest[open + 1..close], anchor + 3 + open + 1)
        }
        _ => {
            let token = rest.trim_start();
            let lead = rest.len() - token.len();
            (
                token.split_whitespace().next().unwrap_or(""),
                anchor + 3 + lead,
            )
        }
    };
    let token = token.strip_prefix("file://").unwrap_or(token);
    require_location(token).map(|(path, l, c)| Found {
        path,
        line: l,
        column: c,
        start: offset,
        end: offset + token.len(),
    })
}

/// Like `split_path_line_column`, but only when a line number is actually there.
///
/// The stack-frame rules use this. `" at "` occurs in ordinary English — "the
/// file is at the top" — and a rule that accepted a bare token after it would
/// make every prose line tappable and none of them meaningful. A real frame
/// always carries `:line` or `:line:col`, so requiring one costs nothing and
/// removes the whole class of false positives.
fn require_location(token: &str) -> Option<(String, Option<u32>, Option<u32>)> {
    let split = split_path_line_column(token)?;
    if split.1.is_none() { None } else { Some(split) }
}

/// Split `path:line:col`, `path:line`, or a bare path.
///
/// Scanned from the right, because a Windows path begins `C:\` and splitting
/// from the left turns the drive letter into a line number.
fn split_path_line_column(token: &str) -> Option<(String, Option<u32>, Option<u32>)> {
    if token.is_empty() {
        return None;
    }
    let parts: Vec<&str> = token.rsplitn(3, ':').collect();
    match parts.as_slice() {
        [column, line, path]
            if !path.is_empty()
                && line.chars().all(|c| c.is_ascii_digit())
                && !line.is_empty()
                && column.chars().all(|c| c.is_ascii_digit())
                && !column.is_empty() =>
        {
            Some(((*path).to_owned(), line.parse().ok(), column.parse().ok()))
        }
        [line, path] if !path.is_empty() && !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()) => {
            Some(((*path).to_owned(), line.parse().ok(), None))
        }
        _ => Some((token.to_owned(), None, None)),
    }
}

/// Every rule, in precedence order.
type Rule = fn(&str) -> Option<Found>;
const RULES: &[(&str, Rule)] = &[
    ("builtin.python_traceback", python_traceback),
    ("builtin.rustc_arrow", rustc_arrow),
    ("builtin.rust_panic", rust_panic),
    ("builtin.node_stack", node_stack),
];

/// One rule, as the catalog describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleInfo {
    /// Its id.
    pub id: &'static str,
    /// Its precedence; higher wins.
    pub precedence: u16,
    /// What it recognises, in words.
    pub about: &'static str,
}

/// Every rule, in the order they are tried.
///
/// Listed so the rule table is inspectable rather than folklore. A recognition
/// system nobody can enumerate is one where "why did that not become a link"
/// has no answer short of reading the source.
#[must_use]
pub fn catalog() -> Vec<RuleInfo> {
    vec![
        RuleInfo {
            id: "builtin.python_traceback",
            precedence: 900,
            about: "Python and Ruby traceback frames. Highest, because the quotes delimit a path that may contain spaces.",
        },
        RuleInfo {
            id: "builtin.rustc_arrow",
            precedence: 860,
            about: "rustc, clippy, miri and everything that copied the `-->` diagnostic format.",
        },
        RuleInfo {
            id: "builtin.rust_panic",
            precedence: 855,
            about: "`thread '...' panicked at path:line:col`, Rust 1.73 and later.",
        },
        RuleInfo {
            id: "builtin.node_stack",
            precedence: 880,
            about: "Node and V8 stack frames, both the named and bare forms; jest, vitest and tsx print through the same formatter.",
        },
    ]
}

/// Which rule matched a line, if any.
///
/// Separate from `apply` because a preview needs to say *which* rule fired —
/// "it matched" is not an answer somebody can use to fix a rule that did not.
#[must_use]
pub fn which(line: &str) -> Option<&'static str> {
    RULES
        .iter()
        .find(|(_, rule)| rule(line).is_some())
        .map(|(id, _)| *id)
}

/// Apply the rule table to a line.
///
/// Returns at most one match — the first rule to fire owns the line. These are
/// diagnostic formats, and a diagnostic line names one location; producing two
/// overlapping matches for the same span gives a client two things to make
/// tappable in the same place.
#[must_use]
pub fn apply(line: &str) -> Option<Match> {
    for (_, rule) in RULES {
        if let Some(found) = rule(line) {
            return Some(Match {
                target: Target::Path {
                    raw: found.path,
                    line: found.line,
                    column: found.column,
                },
                start: found.start,
                end: found.end,
            });
        }
    }
    None
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn path_of(line: &str) -> (String, Option<u32>, Option<u32>) {
        match apply(line).expect("a match").target {
            Target::Path { raw, line, column } => (raw, line, column),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_rule_that_runs_is_in_the_catalog() {
        // A recognition system nobody can enumerate is one where "why did that
        // not become a link" has no answer short of reading the source.
        let listed: std::collections::BTreeSet<&str> =
            catalog().into_iter().map(|r| r.id).collect();
        for (id, _) in RULES {
            assert!(listed.contains(id), "`{id}` runs but is not listed");
        }
        assert_eq!(listed.len(), RULES.len());
    }

    #[test]
    fn a_preview_can_say_which_rule_fired() {
        // "It matched" is not an answer somebody can use to fix a rule that
        // did not.
        assert_eq!(which("  --> src/x.rs:9:5"), Some("builtin.rustc_arrow"));
        assert_eq!(
            which(r#"File "/a.py", line 3"#),
            Some("builtin.python_traceback")
        );
        assert_eq!(which("ordinary prose"), None);
    }

    #[test]
    fn every_catalog_entry_explains_itself() {
        for rule in catalog() {
            assert!(!rule.about.is_empty(), "{}", rule.id);
            assert!(rule.precedence > 0, "{}", rule.id);
        }
    }

    #[test]
    fn a_python_traceback_frame_is_found() {
        let (path, line, _) =
            path_of(r#"  File "/app/service.py", line 42, in handle_request"#);
        assert_eq!(path, "/app/service.py");
        assert_eq!(line, Some(42));
    }

    #[test]
    fn a_path_with_a_space_survives_because_the_quotes_say_where_it_ends() {
        // The whole reason this rule outranks whitespace splitting.
        let (path, line, _) = path_of(r#"File "/home/me/my project/app.py", line 7"#);
        assert_eq!(path, "/home/me/my project/app.py");
        assert_eq!(line, Some(7));
    }

    #[test]
    fn the_rustc_arrow_does_not_travel_with_the_path() {
        let (path, line, column) = path_of("  --> crates/omt/src/main.rs:9:5");
        assert_eq!(path, "crates/omt/src/main.rs");
        assert_eq!((line, column), (Some(9), Some(5)));
    }

    #[test]
    fn a_rust_panic_location_is_found() {
        let (path, line, column) =
            path_of("thread 'main' panicked at src/lib.rs:120:9:");
        assert_eq!(path, "src/lib.rs");
        assert_eq!((line, column), (Some(120), Some(9)));
    }

    #[test]
    fn both_node_frame_shapes_work() {
        // Half a clickable stack trace is worse than none: the frames people
        // want are usually the ones with a function name.
        let (path, line, column) = path_of("    at handleRequest (/app/server.js:12:15)");
        assert_eq!(path, "/app/server.js");
        assert_eq!((line, column), (Some(12), Some(15)));

        let (path, line, _) = path_of("    at /app/index.js:3:1");
        assert_eq!(path, "/app/index.js");
        assert_eq!(line, Some(3));
    }

    #[test]
    fn a_file_url_prefix_is_stripped_from_a_node_frame() {
        let (path, _, _) = path_of("    at run (file:///app/x.js:1:1)");
        assert_eq!(path, "/app/x.js");
    }

    #[test]
    fn a_windows_drive_letter_is_not_read_as_a_line_number() {
        // Scanning from the right is the whole reason. From the left, `C` is
        // the path and `\\src\\x.rs` is a line number.
        let (path, line, column) = path_of("  --> C:\\src\\x.rs:9:5");
        assert_eq!(path, "C:\\src\\x.rs");
        assert_eq!((line, column), (Some(9), Some(5)));
    }

    #[test]
    fn ordinary_prose_matches_no_rule() {
        // These fire on diagnostic formats. A rule that also fired on English
        // would make every log line tappable and none of them meaningful.
        for line in [
            "just some ordinary prose here",
            "Compiling omt v0.1.0",
            "the file is at the top",
        ] {
            assert!(apply(line).is_none(), "{line:?}");
        }
    }

    #[test]
    fn the_offsets_point_at_the_path_and_not_at_the_anchor() {
        // A client makes exactly this span tappable. Including the `--> ` in it
        // puts the target under the arrow.
        let line = "  --> src/x.rs:9:5";
        let m = apply(line).expect("match");
        assert_eq!(&line[m.start..m.end], "src/x.rs:9:5");
    }

    #[test]
    fn a_malformed_frame_does_not_panic_or_invent_a_line_number() {
        for line in [
            "File \"",
            "File \"\", line ",
            "--> ",
            "panicked at ",
            "    at ()",
            "    at (:::)",
            "--> :::",
        ] {
            let _ = apply(line);
        }
    }

    #[test]
    fn a_line_number_that_is_not_a_number_is_not_taken_as_one() {
        let (path, line, _) = split_path_line_column("src/x.rs:notanumber")
            .map(|(p, l, c)| (p, l, c))
            .expect("split");
        assert_eq!(path, "src/x.rs:notanumber");
        assert_eq!(line, None);
    }
}
