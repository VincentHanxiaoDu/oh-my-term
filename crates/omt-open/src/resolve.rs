//! Turning something recognised into something concrete, or an honest failure.
//!
//! `docs/architecture/18-semantic-open.md` §3 and §4.
//!
//! Recognition is pure and says what the *text* looks like. Resolution is where
//! the filesystem is consulted, and where the two questions recognition
//! deliberately refused to answer get answered: does this exist, and what can
//! be done with it.
//!
//! The rule that shapes everything here: **resolution never silently succeeds
//! with a guess.** A path that resolved against the wrong root is worse than
//! one that did not resolve, because the first opens somebody else's file and
//! the second says it could not find yours. So the root that was used is part
//! of the answer, and a path that escapes every root is refused rather than
//! clamped.
//!
//! Actions are *offers*, ranked, never performed here. Ranking and doing are
//! separated because the ranking has to be cheap enough to run on every match
//! on screen, and because a client that shows a menu needs the list before
//! anybody has chosen.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

/// Whether the thing is actually there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Existence {
    /// It is a file.
    File,
    /// It is a directory.
    Directory,
    /// It is not there.
    Missing,
    /// Not checked, because checking would have meant a network round trip.
    ///
    /// Distinguished from `Missing` deliberately. "I did not look" and "it is
    /// not there" lead to different next steps, and collapsing them turns a
    /// slow remote into a confident wrong answer.
    Unchecked,
}

/// Why a target could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The path climbed out of every root it was tried against.
    EscapesRoots {
        /// What was asked for.
        requested: String,
    },
    /// There was nowhere to resolve it against.
    NoRoots,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EscapesRoots { requested } => write!(
                f,
                "`{requested}` points outside every open workspace, so omt will not open it"
            ),
            Self::NoRoots => write!(f, "there is no workspace to resolve a relative path against"),
        }
    }
}

impl std::error::Error for ResolveError {}

/// One thing that could be done with a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionOffer {
    /// Which handler is offering.
    pub handler: String,
    /// What to call it in a menu.
    pub label: String,
    /// Higher is a better default.
    pub rank: i16,
    /// Whether it changes anything, so a confirm sheet can be decided without
    /// running it.
    pub mutates: bool,
}

/// A concrete, existence-checked target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolved {
    /// The absolute path.
    pub path: String,
    /// Which root it resolved against, so a wrong answer is visible rather
    /// than merely wrong.
    pub root: Option<String>,
    /// A line, where the text carried one.
    pub line: Option<u32>,
    /// Whether it is there.
    pub existence: Existence,
    /// What can be done, best first.
    pub actions: Vec<ActionOffer>,
}

/// Lexically normalise a path without touching the filesystem.
///
/// `..` is resolved against the accumulated components rather than by asking
/// the OS, because asking requires the path to exist — and the interesting case
/// is a path that does not exist yet, which is exactly when a traversal would
/// slip through an existence-gated check.
#[must_use]
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    let rooted = path.has_root();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !out.pop() || out.as_os_str() == "/" {
                    // On an absolute path `/..` is `/`, which is what the
                    // kernel does; keeping the `..` would report legitimate
                    // paths as escaping. On a relative one the `..` is kept, so
                    // the caller can see it climbed out rather than having it
                    // silently flattened into a path inside the root.
                    if rooted {
                        out = PathBuf::from("/");
                    } else {
                        out.push("..");
                    }
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether a normalised path is inside a root.
#[must_use]
pub fn within(root: &Path, candidate: &Path) -> bool {
    let root = lexical_normalize(root);
    let candidate = lexical_normalize(candidate);
    candidate.starts_with(&root)
}

/// What can be done with a resolved path.
///
/// Ranked rather than chosen. The best handler is the default, and the rest
/// still appear in a menu — a target with exactly one action and no way to see
/// the others is one somebody has to leave omt to act on differently.
#[must_use]
pub fn offers(existence: Existence, line: Option<u32>) -> Vec<ActionOffer> {
    let mut out = Vec::new();
    match existence {
        Existence::File | Existence::Unchecked => {
            out.push(ActionOffer {
                handler: "editor".to_owned(),
                // The line is in the label because "open at line 42" and "open"
                // are different enough that somebody choosing between them
                // should not have to remember which one carries the position.
                label: line.map_or_else(
                    || "Open in editor".to_owned(),
                    |l| format!("Open in editor at line {l}"),
                ),
                rank: 100,
                mutates: false,
            });
            out.push(ActionOffer {
                handler: "explorer".to_owned(),
                label: "Reveal in explorer".to_owned(),
                rank: 60,
                mutates: false,
            });
            out.push(ActionOffer {
                handler: "clipboard".to_owned(),
                label: "Copy path".to_owned(),
                rank: 40,
                mutates: false,
            });
        }
        Existence::Directory => {
            out.push(ActionOffer {
                handler: "explorer".to_owned(),
                label: "Open in explorer".to_owned(),
                rank: 100,
                mutates: false,
            });
            out.push(ActionOffer {
                handler: "session".to_owned(),
                label: "New session here".to_owned(),
                rank: 80,
                // Starting a shell is a real effect, and saying so here is
                // what lets a phone decide to confirm without running it.
                mutates: true,
            });
            out.push(ActionOffer {
                handler: "clipboard".to_owned(),
                label: "Copy path".to_owned(),
                rank: 40,
                mutates: false,
            });
        }
        // Nothing to open. Copying the path is still useful — it is usually
        // what somebody does next when a build names a file that is not there.
        Existence::Missing => out.push(ActionOffer {
            handler: "clipboard".to_owned(),
            label: "Copy path".to_owned(),
            rank: 40,
            mutates: false,
        }),
    }
    out.sort_by(|a, b| b.rank.cmp(&a.rank));
    out
}

/// Resolve a recognised path against the roots that are open.
///
/// # Errors
/// Refuses a path that escapes every root, and a relative path with no root to
/// resolve it against.
pub fn resolve(
    raw: &str,
    line: Option<u32>,
    roots: &[PathBuf],
    check_existence: bool,
) -> Result<Resolved, ResolveError> {
    let requested = Path::new(raw);

    if requested.is_absolute() {
        let normalised = lexical_normalize(requested);
        // An absolute path is taken at face value only when it is inside a
        // root. Otherwise a build log naming `/etc/shadow` becomes a link.
        let root = roots.iter().find(|r| within(r, &normalised));
        if root.is_none() && !roots.is_empty() {
            return Err(ResolveError::EscapesRoots {
                requested: raw.to_owned(),
            });
        }
        return Ok(finish(normalised, root.cloned(), line, check_existence));
    }

    if roots.is_empty() {
        return Err(ResolveError::NoRoots);
    }

    // Tried against each root in order. The first that contains an existing
    // file wins; if none does, the first root is used so the answer still says
    // where it looked.
    let mut fallback: Option<(PathBuf, PathBuf)> = None;
    for root in roots {
        let joined = lexical_normalize(&root.join(requested));
        if !within(root, &joined) {
            continue;
        }
        if joined.exists() {
            return Ok(finish(joined, Some(root.clone()), line, check_existence));
        }
        if fallback.is_none() {
            fallback = Some((joined, root.clone()));
        }
    }

    match fallback {
        Some((path, root)) => Ok(finish(path, Some(root), line, check_existence)),
        None => Err(ResolveError::EscapesRoots {
            requested: raw.to_owned(),
        }),
    }
}

fn finish(
    path: PathBuf,
    root: Option<PathBuf>,
    line: Option<u32>,
    check_existence: bool,
) -> Resolved {
    let existence = if !check_existence {
        Existence::Unchecked
    } else if path.is_dir() {
        Existence::Directory
    } else if path.is_file() {
        Existence::File
    } else {
        Existence::Missing
    };
    Resolved {
        path: path.display().to_string(),
        root: root.map(|r| r.display().to_string()),
        line,
        actions: offers(existence, line),
        existence,
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

    fn workspace() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src/main.rs"), "fn main() {}").expect("write");
        (dir, root)
    }

    #[test]
    fn a_relative_path_resolves_against_the_root_and_says_which() {
        // A path resolved against the wrong root opens somebody else's file.
        // Naming the root is what makes a wrong answer visible.
        let (_d, root) = workspace();
        let r = resolve("src/main.rs", Some(42), &[root.clone()], true).expect("resolve");
        assert_eq!(r.existence, Existence::File);
        assert_eq!(r.root.as_deref(), Some(root.display().to_string().as_str()));
        assert_eq!(r.line, Some(42));
    }

    #[test]
    fn a_traversal_out_of_every_root_is_refused_not_clamped() {
        let (_d, root) = workspace();
        let err = resolve("../../../../etc/passwd", None, &[root], true).expect_err("refuse");
        assert!(matches!(err, ResolveError::EscapesRoots { .. }));
    }

    #[test]
    fn an_absolute_path_outside_the_roots_is_refused() {
        // Otherwise a build log naming /etc/shadow becomes a link somebody can
        // tap.
        let (_d, root) = workspace();
        let err = resolve("/etc/shadow", None, &[root], true).expect_err("refuse");
        assert!(matches!(err, ResolveError::EscapesRoots { .. }));
    }

    #[test]
    fn normalisation_does_not_need_the_path_to_exist() {
        // The interesting traversal is one to a file that is not there yet,
        // which is exactly when an existence-gated check lets it through.
        // Absolute: `/..` is `/`, as the kernel has it, so this lands on the
        // real target — and the root check then refuses it.
        assert_eq!(
            lexical_normalize(Path::new("/a/b/../../../etc/passwd")),
            Path::new("/etc/passwd")
        );
        // Relative: the climb is kept, so an escape is visible instead of
        // being flattened into something inside the root.
        assert_eq!(
            lexical_normalize(Path::new("a/../../etc/passwd")),
            Path::new("../etc/passwd")
        );
    }

    #[test]
    fn climbing_above_the_start_is_kept_rather_than_flattened() {
        // Flattening `..` away would turn an escape into a path inside the
        // root, which is the silent version of this failure.
        assert_eq!(lexical_normalize(Path::new("../x")), Path::new("../x"));
        assert!(!within(Path::new("/root"), Path::new("/root/../etc")));
    }

    #[test]
    fn a_missing_file_says_missing_rather_than_failing() {
        let (_d, root) = workspace();
        let r = resolve("src/absent.rs", None, &[root], true).expect("resolve");
        assert_eq!(r.existence, Existence::Missing);
    }

    #[test]
    fn not_looking_is_distinguished_from_not_being_there() {
        // Collapsing them turns a slow remote into a confident wrong answer.
        let (_d, root) = workspace();
        let r = resolve("src/absent.rs", None, &[root], false).expect("resolve");
        assert_eq!(r.existence, Existence::Unchecked);
    }

    #[test]
    fn a_relative_path_with_no_root_is_an_honest_failure() {
        assert_eq!(
            resolve("src/main.rs", None, &[], true),
            Err(ResolveError::NoRoots)
        );
    }

    #[test]
    fn the_first_root_that_actually_has_the_file_wins() {
        let (_a, first) = workspace();
        let (_b, second) = workspace();
        std::fs::remove_file(first.join("src/main.rs")).expect("remove");
        let r = resolve("src/main.rs", None, &[first, second.clone()], true).expect("resolve");
        assert_eq!(r.root.as_deref(), Some(second.display().to_string().as_str()));
        assert_eq!(r.existence, Existence::File);
    }

    #[test]
    fn actions_are_ordered_best_first() {
        let out = offers(Existence::File, Some(9));
        assert_eq!(out[0].handler, "editor");
        assert!(out.windows(2).all(|w| w[0].rank >= w[1].rank));
    }

    #[test]
    fn the_line_appears_in_the_label_that_carries_it() {
        // "Open at line 42" and "Open" are different enough that somebody
        // choosing should not have to remember which carries the position.
        assert!(offers(Existence::File, Some(42))[0].label.contains("42"));
        assert!(!offers(Existence::File, None)[0].label.contains("line"));
    }

    #[test]
    fn a_directory_offers_different_things_than_a_file() {
        let out = offers(Existence::Directory, None);
        assert_eq!(out[0].handler, "explorer");
        assert!(out.iter().any(|a| a.handler == "session"));
    }

    #[test]
    fn anything_that_changes_something_says_so_without_being_run() {
        // Which is what lets a phone decide to show a confirm sheet.
        let session = offers(Existence::Directory, None)
            .into_iter()
            .find(|a| a.handler == "session")
            .expect("offered");
        assert!(session.mutates);
        assert!(offers(Existence::File, None).iter().all(|a| !a.mutates));
    }

    #[test]
    fn a_missing_file_still_offers_something_useful() {
        // Copying the path is what somebody does next when a build names a
        // file that is not there.
        let out = offers(Existence::Missing, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].handler, "clipboard");
    }
}
