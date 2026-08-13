//! What changed in a file, structured.
//!
//! Here rather than in the crate that produces it, per
//! [D-7](../../../docs/architecture/decisions.md): `omt-workspace-fs` computes
//! a diff and lives at L2, and [06 §5](../../../docs/architecture/06-agent-layer.md)
//! types a permission card's diff as one — an L1 type. Two crates naming the
//! same thing is the rule that puts it at L0.
//!
//! **Structured hunks on the wire, never a unified-diff string**
//! ([15 §3.2](../../../docs/architecture/15-workspace-explorer.md)). Returning
//! `git diff` output verbatim forces every client to implement a patch parser
//! to render anything but a `<pre>`; three surfaces would mean three parsers
//! and three sets of off-by-one bugs. It is parsed once, here, and line
//! numbers ship with it — which is also what turns "jump to `file:line`" into
//! something a client can do without guessing.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A workspace-relative path: `/`-separated, no leading or trailing slash.
///
/// A newtype because the invariant is the point. An absolute path or a
/// `..` segment reaching outside the workspace is the confinement bug
/// [15 §9](../../../docs/architecture/15-workspace-explorer.md) exists to
/// prevent, and a `String` parameter cannot say which of the two it holds.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct RelPath(String);

impl RelPath {
    /// Normalize a path into a workspace-relative one.
    ///
    /// Backslashes become slashes, empty and `.` segments are dropped, and the
    /// leading and trailing slashes go. `..` is **kept** rather than resolved:
    /// resolving it here would silently turn an escape attempt into a valid
    /// path, and the caller that owns the workspace root is the only thing
    /// that can decide what is inside it.
    #[must_use]
    pub fn new(path: impl AsRef<str>) -> Self {
        let joined = path
            .as_ref()
            .replace('\\', "/")
            .split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .collect::<Vec<_>>()
            .join("/");
        Self(joined)
    }

    /// Read it back.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether it tries to leave the workspace.
    ///
    /// Asked rather than assumed: [`Self::new`] deliberately does not resolve
    /// `..`, so somebody has to check, and a method is harder to forget than a
    /// convention.
    #[must_use]
    pub fn escapes(&self) -> bool {
        self.0.split('/').any(|s| s == "..")
    }

    /// The final segment.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for RelPath {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for RelPath {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// What happened to one file.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// Nothing.
    #[default]
    Unmodified,
    /// It is new.
    Added,
    /// It changed.
    Modified,
    /// It is gone.
    Deleted,
    /// It moved.
    Renamed,
    /// It was copied.
    Copied,
    /// It became a different kind of thing — a file where a symlink was.
    TypeChanged,
}

/// How many lines moved.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct LineStat {
    /// Lines added.
    pub added: u32,
    /// Lines removed.
    pub removed: u32,
}

/// Where a file stands, on both axes at once.
///
/// Two independent fields rather than one status, because a file can be staged
/// *and* further modified, and a single value has to pick one of the two
/// truths to report.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VcsFileState {
    /// What the index says.
    pub index: ChangeKind,
    /// What the working tree says.
    pub worktree: ChangeKind,
    /// Whether git could not merge it.
    pub conflicted: bool,
    /// Whether git has never been told about it.
    pub untracked: bool,
    /// Whether it is ignored.
    pub ignored: bool,
    /// How many lines moved. `None` when binary or not computed — which is not
    /// the same as zero, and a surface showing `+0 −0` for a binary file is
    /// stating something false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<LineStat>,
    /// Where a rename or copy came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orig_rel: Option<RelPath>,
}

/// What a diff is taken against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiffBase {
    /// Working tree against the index — what `git diff` shows.
    Worktree,
    /// Index against `HEAD` — what `git diff --cached` shows.
    Index,
    /// Against a named revision.
    Commit {
        /// The revision, as the user wrote it.
        rev: String,
    },
}

/// Which sides of a diff end without a newline.
///
/// Carried rather than dropped because git carries it: a change that consists
/// only of adding a trailing newline is a real change, and a diff that could
/// not express it would render as empty.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EofNewline {
    /// Both sides end with one.
    #[default]
    Both,
    /// Only the old side does.
    OldOnly,
    /// Only the new side does.
    NewOnly,
    /// Neither does.
    Neither,
}

/// Why a diff's contents were withheld.
///
/// [15 §9.4](../../../docs/architecture/15-workspace-explorer.md) — the reason
/// travels so a surface can say which rule applied rather than showing an
/// unexplained blank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RedactReason {
    /// It matched the secret redactor from 13 §8.
    Secret,
    /// The path is denied to this caller.
    Confined,
    /// A policy named it.
    Policy {
        /// Which rule, so the user can find it.
        rule: String,
    },
}

/// Which side of a diff a line belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    /// Unchanged, shown for context.
    Context,
    /// Only in the new file.
    Added,
    /// Only in the old file.
    Removed,
}

/// One line of a diff, with its numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiffLine {
    /// Which side it is on.
    pub kind: DiffLineKind,
    /// Its number in the old file, where it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_no: Option<u32>,
    /// Its number in the new file, where it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_no: Option<u32>,
    /// The text, **without** git's `+`/`-`/space prefix.
    ///
    /// The prefix is `kind`, and carrying it in the text too would make every
    /// client strip a character before rendering — which is the patch-parsing
    /// this type exists to stop.
    pub text: String,
    /// Byte ranges within `text` that differ from the line it pairs with.
    ///
    /// Computed here so all three surfaces highlight identically
    /// ([15 §7.4](../../../docs/architecture/15-workspace-explorer.md)) rather
    /// than each inventing its own word-diff and disagreeing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intra: Vec<IntraRange>,
}

/// A byte range inside a line.
///
/// A struct rather than `Range<u32>` because `Range` is not `Copy`, does not
/// implement `JsonSchema`, and serializes to a shape with no obvious wire
/// spelling.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct IntraRange {
    /// First byte, inclusive.
    pub start: u32,
    /// Last byte, exclusive.
    pub end: u32,
}

/// One contiguous run of changed lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Hunk {
    /// Where it starts in the old file.
    pub old_start: u32,
    /// How many lines it covers there.
    pub old_lines: u32,
    /// Where it starts in the new file.
    pub new_start: u32,
    /// How many lines it covers.
    pub new_lines: u32,
    /// The section heading git puts after the `@@`, where it found one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// The lines, in order. The order is what makes a diff readable, which is
    /// why this is one list and not an added list beside a removed one.
    pub lines: Vec<DiffLine>,
}

/// The body of a diff, or why there is not one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiffBody {
    /// Text, as hunks.
    Text {
        /// The hunks, in file order.
        hunks: Vec<Hunk>,
        /// The path on the old side.
        old_path: RelPath,
        /// The path on the new side, which differs for a rename.
        new_path: RelPath,
        /// Which sides end without a newline.
        #[serde(default)]
        eof_newline: EofNewline,
    },
    /// Binary. Reported rather than hidden: a surface that tried to render it
    /// shows nothing and looks broken, where one that says "binary" is simply
    /// correct.
    Binary {
        /// Size before.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        old_size: Option<u64>,
        /// Size after.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_size: Option<u64>,
    },
    /// Over the configured size limit. Carries the stat so a surface can say
    /// "+18402 −3, too large to display" and offer to read the file instead.
    TooLarge {
        /// How big it is.
        bytes: u64,
        /// How many lines moved.
        lines: LineStat,
    },
    /// Withheld, with the reason.
    Redacted {
        /// Which rule applied.
        reason: RedactReason,
    },
}

/// The diff of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FileDiff {
    /// Which file.
    pub rel: RelPath,
    /// What it was compared against.
    pub base: DiffBase,
    /// Where it stands.
    pub state: VcsFileState,
    /// The change itself.
    pub body: DiffBody,
    /// Whether hunks were dropped to fit a limit.
    ///
    /// Separate from [`DiffBody::TooLarge`], which is the whole file refused.
    /// This is a partial answer, and a surface that could not tell them apart
    /// would present an incomplete diff as a complete one.
    #[serde(default)]
    pub truncated: bool,
}

/// The byte ranges in which two lines differ.
///
/// A common prefix and a common suffix, which is what a reader's eye does
/// anyway. Deliberately not a word-level or Myers diff: those disagree with
/// each other over what "the same word" means, and the value here is that
/// every surface highlights the *same* bytes, not that the highlight is
/// minimal.
#[must_use]
pub fn intra_ranges(old: &str, new: &str) -> (Vec<IntraRange>, Vec<IntraRange>) {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let prefix = a
        .iter()
        .zip(b)
        .take_while(|(x, y)| x == y)
        .count()
        .min(a.len())
        .min(b.len());
    // Suffix measured from the ends, stopping before it would overlap the
    // prefix — otherwise two identical lines produce a range covering the line
    // twice.
    let max_suffix = a.len().min(b.len()) - prefix;
    let suffix = (0..max_suffix)
        .take_while(|i| a[a.len() - 1 - i] == b[b.len() - 1 - i])
        .count();

    let range = |len: usize| {
        if prefix + suffix >= len {
            // One side is entirely contained in the other; there is nothing on
            // this side to mark.
            Vec::new()
        } else {
            vec![IntraRange {
                start: prefix as u32,
                end: (len - suffix) as u32,
            }]
        }
    };
    (range(a.len()), range(b.len()))
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_is_normalized_but_never_silently_resolved() {
        assert_eq!(RelPath::new("/src//lib.rs").as_str(), "src/lib.rs");
        assert_eq!(RelPath::new("./src/./lib.rs").as_str(), "src/lib.rs");
        assert_eq!(RelPath::new("src\\lib.rs").as_str(), "src/lib.rs");
        // `..` is kept. Resolving it here would turn an escape attempt into a
        // valid-looking path before anything had a chance to refuse it.
        let up = RelPath::new("../../etc/passwd");
        assert_eq!(up.as_str(), "../../etc/passwd");
        assert!(up.escapes());
        assert!(!RelPath::new("src/lib.rs").escapes());
    }

    #[test]
    fn a_diff_line_carries_its_numbers_and_not_gits_prefix() {
        // The prefix is `kind`. Carrying it in the text too would make every
        // client strip a character, which is the patch-parsing this type
        // exists to stop.
        let line = DiffLine {
            kind: DiffLineKind::Added,
            old_no: None,
            new_no: Some(42),
            text: "let x = 1;".to_owned(),
            intra: Vec::new(),
        };
        assert!(!line.text.starts_with('+'));
        let json = serde_json::to_string(&line).expect("serialize");
        assert!(json.contains(r#""kind":"added""#), "{json}");
        // An absent number is absent rather than null: a client rendering a
        // gutter must be able to tell "no number here" from "number zero".
        assert!(!json.contains("old_no"), "{json}");
    }

    #[test]
    fn a_binary_diff_says_so_rather_than_arriving_empty() {
        let body = DiffBody::Binary {
            old_size: Some(120),
            new_size: None,
        };
        let json = serde_json::to_string(&body).expect("serialize");
        assert!(json.contains(r#""type":"binary""#), "{json}");
    }

    #[test]
    fn a_truncated_diff_is_not_the_same_as_a_refused_one() {
        // A partial answer presented as a complete one is the failure this
        // distinction prevents.
        let refused = DiffBody::TooLarge {
            bytes: 4_000_000,
            lines: LineStat {
                added: 18402,
                removed: 3,
            },
        };
        let partial = FileDiff {
            rel: RelPath::new("src/lib.rs"),
            base: DiffBase::Worktree,
            state: VcsFileState::default(),
            body: DiffBody::Text {
                hunks: vec![],
                old_path: RelPath::new("src/lib.rs"),
                new_path: RelPath::new("src/lib.rs"),
                eof_newline: EofNewline::Both,
            },
            truncated: true,
        };
        assert!(partial.truncated);
        assert!(!matches!(partial.body, DiffBody::TooLarge { .. }));
        assert!(matches!(refused, DiffBody::TooLarge { .. }));
    }

    #[test]
    fn intra_ranges_mark_only_what_differs() {
        let (old, new) = intra_ranges("let x = 1;", "let x = 22;");
        assert_eq!(old, vec![IntraRange { start: 8, end: 9 }]);
        assert_eq!(new, vec![IntraRange { start: 8, end: 10 }]);
    }

    #[test]
    fn identical_lines_have_nothing_to_mark() {
        // The overlap case: a naive prefix-plus-suffix count would cover the
        // line twice and every surface would highlight the whole row.
        let (old, new) = intra_ranges("same", "same");
        assert!(old.is_empty(), "{old:?}");
        assert!(new.is_empty(), "{new:?}");
    }

    #[test]
    fn a_pure_insertion_marks_only_the_side_that_grew() {
        let (old, new) = intra_ranges("ac", "abc");
        assert!(old.is_empty(), "{old:?}");
        assert_eq!(new, vec![IntraRange { start: 1, end: 2 }]);
    }

    #[test]
    fn intra_ranges_never_run_past_the_line() {
        // A range whose end exceeds the text would panic any client that
        // sliced with it.
        for (a, b) in [("", "x"), ("x", ""), ("", ""), ("abc", "xyz")] {
            let (old, new) = intra_ranges(a, b);
            for r in old {
                assert!(r.end as usize <= a.len(), "{r:?} past {a:?}");
                assert!(r.start <= r.end);
            }
            for r in new {
                assert!(r.end as usize <= b.len(), "{r:?} past {b:?}");
                assert!(r.start <= r.end);
            }
        }
    }

    #[test]
    fn a_files_state_reports_both_axes() {
        // Staged *and* further modified is a real state, and one status field
        // has to pick which of the two truths to tell.
        let state = VcsFileState {
            index: ChangeKind::Modified,
            worktree: ChangeKind::Modified,
            ..VcsFileState::default()
        };
        let json = serde_json::to_string(&state).expect("serialize");
        assert!(json.contains(r#""index":"modified""#), "{json}");
        assert!(json.contains(r#""worktree":"modified""#), "{json}");
        // No line stat is absent rather than zero: `+0 −0` for a binary file
        // states something false.
        assert!(!json.contains("lines"), "{json}");
    }

    #[test]
    fn every_type_here_round_trips() {
        let diff = FileDiff {
            rel: RelPath::new("src/lib.rs"),
            base: DiffBase::Commit {
                rev: "HEAD~2".to_owned(),
            },
            state: VcsFileState::default(),
            body: DiffBody::Text {
                hunks: vec![Hunk {
                    old_start: 1,
                    old_lines: 2,
                    new_start: 1,
                    new_lines: 3,
                    header: Some("fn main".to_owned()),
                    lines: vec![DiffLine {
                        kind: DiffLineKind::Context,
                        old_no: Some(1),
                        new_no: Some(1),
                        text: "fn main() {".to_owned(),
                        intra: Vec::new(),
                    }],
                }],
                old_path: RelPath::new("src/lib.rs"),
                new_path: RelPath::new("src/lib.rs"),
                eof_newline: EofNewline::Neither,
            },
            truncated: false,
        };
        let json = serde_json::to_string(&diff).expect("serialize");
        let back: FileDiff = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, diff);
    }
}
