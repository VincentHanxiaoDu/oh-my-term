//! The workspace file tree.
//!
//! One property matters more than everything else here: **nothing resolves
//! outside the workspace root**. A file tree is exposed to remote clients, so a
//! path that escapes is a remote arbitrary-file-read — and `..` is not the only
//! way out. A symlink pointing at `/` is the one people forget, which is why
//! containment is checked against the *canonical* path and not against the
//! string the caller sent.

pub mod git;

pub use git::{Forge, GitError, GitStatus, forge, parse_remote, status};

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One entry in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Its name, without any directory part.
    pub name: String,
    /// Its path relative to the workspace root, always using `/`.
    pub rel: String,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// Whether it is a symlink.
    ///
    /// Reported rather than followed silently, so a client can show that this
    /// entry is a pointer and the user is not surprised by where it leads.
    pub is_symlink: bool,
    /// Its size, for files.
    pub size: Option<u64>,
}

/// Why a path was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FsError {
    /// The path resolves outside the workspace root.
    #[error("`{path}` is outside the workspace")]
    Escapes {
        /// What was asked for.
        path: String,
    },
    /// Nothing is there.
    #[error("`{path}` does not exist")]
    NotFound {
        /// What was asked for.
        path: String,
    },
    /// The filesystem refused.
    #[error("{0}")]
    Io(String),
}

/// A rooted view of a directory tree.
#[derive(Debug, Clone)]
pub struct WorkspaceFs {
    root: PathBuf,
}

impl WorkspaceFs {
    /// Open a workspace at a root.
    ///
    /// The root is canonicalized once, here, so every later containment check
    /// compares two real paths rather than two strings.
    ///
    /// # Errors
    /// Fails if the root does not exist.
    pub fn new(root: &Path) -> Result<Self, FsError> {
        let root = root.canonicalize().map_err(|_| FsError::NotFound {
            path: root.display().to_string(),
        })?;
        Ok(Self { root })
    }

    /// The canonical root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a workspace-relative path to a real one, refusing escapes.
    ///
    /// # Errors
    /// Fails if the path escapes the root or does not exist.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, FsError> {
        // Reject obvious escapes before touching the filesystem, so a probe
        // cannot be used to learn which paths exist outside the root.
        if Path::new(rel).is_absolute() {
            return Err(FsError::Escapes {
                path: rel.to_owned(),
            });
        }
        let mut depth: i32 = 0;
        for component in Path::new(rel).components() {
            match component {
                Component::ParentDir => depth -= 1,
                Component::Normal(_) => depth += 1,
                Component::CurDir => {}
                Component::RootDir | Component::Prefix(_) => {
                    return Err(FsError::Escapes {
                        path: rel.to_owned(),
                    });
                }
            }
            if depth < 0 {
                return Err(FsError::Escapes {
                    path: rel.to_owned(),
                });
            }
        }

        let joined = self.root.join(rel);
        let canonical = joined.canonicalize().map_err(|_| FsError::NotFound {
            path: rel.to_owned(),
        })?;

        // The check that matters: a symlink inside the workspace can point
        // anywhere, and only the canonical path reveals it. Checking the
        // string the caller sent would miss every one of them.
        if !canonical.starts_with(&self.root) {
            return Err(FsError::Escapes {
                path: rel.to_owned(),
            });
        }
        Ok(canonical)
    }

    /// List a directory.
    ///
    /// # Errors
    /// Fails if the path escapes, does not exist, or cannot be read.
    pub fn list(&self, rel: &str) -> Result<Vec<Entry>, FsError> {
        let dir = self.resolve(rel)?;
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| FsError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| FsError::Io(e.to_string()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            // `symlink_metadata` deliberately: `metadata` follows the link, so
            // a broken symlink would vanish from the listing entirely rather
            // than being shown as what it is.
            let meta = entry
                .path()
                .symlink_metadata()
                .map_err(|e| FsError::Io(e.to_string()))?;
            let is_symlink = meta.file_type().is_symlink();
            let is_dir = if is_symlink {
                entry.path().metadata().is_ok_and(|m| m.is_dir())
            } else {
                meta.is_dir()
            };
            let child_rel = if rel.is_empty() || rel == "." {
                name.clone()
            } else {
                format!("{}/{name}", rel.trim_end_matches('/'))
            };
            out.push(Entry {
                name,
                rel: child_rel,
                is_dir,
                is_symlink,
                size: (!is_dir).then_some(meta.len()),
            });
        }
        // Directories first, then by name — stable, so a client's list does not
        // reshuffle between two identical requests.
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    }

    /// Whether a name is one a listing hides by default.
    ///
    /// `.git` is excluded because it is enormous and nobody browses it, not
    /// because it is secret — a client that asks for it explicitly still gets
    /// it, since hiding something a user can reach another way only confuses.
    #[must_use]
    pub fn is_noise(name: &str) -> bool {
        matches!(name, ".git" | "node_modules" | "target" | ".DS_Store")
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

    fn workspace() -> (tempfile::TempDir, WorkspaceFs) {
        let d = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(d.path().join("src")).expect("mkdir");
        std::fs::write(d.path().join("src/main.rs"), b"fn main() {}").expect("write");
        std::fs::write(d.path().join("README.md"), b"# hi").expect("write");
        let fs = WorkspaceFs::new(d.path()).expect("open");
        (d, fs)
    }

    #[test]
    fn a_listing_shows_what_is_there() {
        let (_d, fs) = workspace();
        let names: Vec<String> = fs
            .list("")
            .expect("list")
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(names.contains(&"src".to_owned()));
        assert!(names.contains(&"README.md".to_owned()));
    }

    #[test]
    fn directories_come_first_and_the_order_is_stable() {
        // A client's tree must not reshuffle between two identical requests.
        let (_d, fs) = workspace();
        let first = fs.list("").expect("list");
        let second = fs.list("").expect("list");
        assert_eq!(first, second);
        assert!(first[0].is_dir);
    }

    #[test]
    fn dot_dot_cannot_escape_the_root() {
        // The obvious one, rejected before the filesystem is touched so a probe
        // cannot be used to learn what exists outside.
        let (_d, fs) = workspace();
        for attempt in ["..", "../..", "src/../..", "../etc/passwd"] {
            assert!(
                matches!(fs.resolve(attempt), Err(FsError::Escapes { .. })),
                "{attempt} was not refused"
            );
        }
    }

    #[test]
    fn an_absolute_path_is_refused() {
        let (_d, fs) = workspace();
        assert!(matches!(
            fs.resolve("/etc/passwd"),
            Err(FsError::Escapes { .. })
        ));
    }

    #[test]
    fn a_symlink_out_of_the_workspace_is_refused() {
        // The one people forget. `..` is not the only way out, and only the
        // canonical path reveals this — checking the caller's string would miss
        // every symlink escape.
        let (d, fs) = workspace();
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret"), b"...").expect("write");

        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path().join("secret"), d.path().join("link"))
            .expect("symlink");

        assert!(
            matches!(fs.resolve("link"), Err(FsError::Escapes { .. })),
            "a symlink walked straight out of the workspace"
        );
    }

    #[test]
    fn a_symlink_inside_the_workspace_is_allowed() {
        // Containment, not paranoia: a link that stays inside is a normal file.
        let (d, fs) = workspace();
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.path().join("README.md"), d.path().join("readme-link"))
            .expect("symlink");
        assert!(fs.resolve("readme-link").is_ok());
    }

    #[test]
    fn a_symlink_is_reported_as_one() {
        // So a client can show that this entry is a pointer rather than
        // surprising the user with where it leads.
        let (d, fs) = workspace();
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.path().join("README.md"), d.path().join("link")).expect("ln");
        let entry = fs
            .list("")
            .expect("list")
            .into_iter()
            .find(|e| e.name == "link")
            .expect("present");
        assert!(entry.is_symlink);
    }

    #[test]
    fn a_broken_symlink_still_appears_in_the_listing() {
        // Following the link to decide would make it vanish entirely, and a
        // file the user can see in their own terminal but not in omt is worse
        // than one shown as broken.
        let (d, fs) = workspace();
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.path().join("nonexistent"), d.path().join("broken"))
            .expect("ln");
        assert!(
            fs.list("")
                .expect("list")
                .iter()
                .any(|e| e.name == "broken"),
            "the broken link disappeared"
        );
    }

    #[test]
    fn a_missing_path_is_not_found_rather_than_an_escape() {
        // Two different problems, and conflating them would make a typo look
        // like an attack in the log.
        let (_d, fs) = workspace();
        assert!(matches!(
            fs.resolve("src/nope.rs"),
            Err(FsError::NotFound { .. })
        ));
    }

    #[test]
    fn relative_paths_in_a_listing_are_workspace_relative() {
        // A client sends these back, so they must round-trip.
        let (_d, fs) = workspace();
        let entry = fs
            .list("src")
            .expect("list")
            .into_iter()
            .find(|e| e.name == "main.rs")
            .expect("present");
        assert_eq!(entry.rel, "src/main.rs");
        assert!(fs.resolve(&entry.rel).is_ok(), "and resolve again");
    }

    #[test]
    fn noise_is_named_rather_than_guessed_at() {
        assert!(WorkspaceFs::is_noise(".git"));
        assert!(WorkspaceFs::is_noise("node_modules"));
        assert!(!WorkspaceFs::is_noise("src"));
        assert!(
            !WorkspaceFs::is_noise(".env"),
            "hidden is not the same as noise"
        );
    }

    #[test]
    fn opening_a_root_that_does_not_exist_fails_immediately() {
        assert!(matches!(
            WorkspaceFs::new(Path::new("/definitely/not/here")),
            Err(FsError::NotFound { .. })
        ));
    }

    #[test]
    fn a_file_reports_its_size_and_a_directory_does_not() {
        let (_d, fs) = workspace();
        for entry in fs.list("").expect("list") {
            assert_eq!(
                entry.size.is_some(),
                !entry.is_dir,
                "{} reported the wrong shape",
                entry.name
            );
        }
    }
}
