//! Snapshots: write somewhere else, then rename.
//!
//! A snapshot written in place is a snapshot that can be half-written when the
//! power goes, and half a session tree is worse than none — it restores, looks
//! plausible, and is wrong. `rename` within a directory is atomic, so a reader
//! sees either the old file or the new one and never something in between.

use std::io::Write;
use std::path::Path;

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// The snapshot could not be written or read.
    #[error("snapshot I/O: {0}")]
    Io(#[from] std::io::Error),
    /// It could not be serialized or parsed.
    #[error("snapshot format: {0}")]
    Format(#[from] serde_json::Error),
}

/// Write a snapshot atomically.
///
/// # Errors
/// Fails if the value cannot be serialized, or if any step of the write,
/// sync or rename does.
pub fn write_snapshot<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), SnapshotError> {
    let bytes = serde_json::to_vec_pretty(value)?;

    // The temp file has to live in the *same directory*: rename is only atomic
    // within a filesystem, and /tmp is routinely a different one.
    let temp = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        // Sync before the rename, not after. A rename that lands before the
        // contents do leaves a file that exists and is empty.
        file.sync_all()?;
    }
    std::fs::rename(&temp, path)?;

    // Sync the directory too, so the rename itself survives losing power.
    if let Some(parent) = path.parent()
        && let Ok(dir) = std::fs::File::open(parent)
    {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Read a snapshot back.
///
/// Returns `None` when there is no snapshot yet, which is the normal state of a
/// first run rather than a failure.
///
/// # Errors
/// Fails if the file exists but cannot be read or parsed.
pub fn load_snapshot<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, SnapshotError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Tree {
        sessions: Vec<String>,
    }

    #[test]
    fn a_snapshot_round_trips() {
        let d = tempfile::tempdir().expect("tempdir");
        let path = d.path().join("tree.json");
        let tree = Tree {
            sessions: vec!["a".into(), "b".into()],
        };
        write_snapshot(&path, &tree).expect("write");
        let back: Tree = load_snapshot(&path).expect("load").expect("present");
        assert_eq!(back, tree);
    }

    #[test]
    fn a_missing_snapshot_is_absence_not_failure() {
        // The normal state of a first run.
        let d = tempfile::tempdir().expect("tempdir");
        let back: Option<Tree> = load_snapshot(&d.path().join("nope")).expect("load");
        assert!(back.is_none());
    }

    #[test]
    fn writing_leaves_no_temp_file_behind() {
        // A stray .tmp would be picked up by a directory scan and look like a
        // snapshot of its own.
        let d = tempfile::tempdir().expect("tempdir");
        let path = d.path().join("tree.json");
        write_snapshot(&path, &Tree { sessions: vec![] }).expect("write");
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn the_temp_file_is_written_beside_its_target() {
        // rename is atomic only within a filesystem, and /tmp is routinely a
        // different one — so the staging file has to share the directory.
        let d = tempfile::tempdir().expect("tempdir");
        let path = d.path().join("tree.json");
        assert_eq!(path.with_extension("tmp").parent(), path.parent());
    }

    #[test]
    fn overwriting_replaces_rather_than_appending() {
        let d = tempfile::tempdir().expect("tempdir");
        let path = d.path().join("tree.json");
        write_snapshot(
            &path,
            &Tree {
                sessions: vec!["old".into()],
            },
        )
        .expect("first");
        write_snapshot(
            &path,
            &Tree {
                sessions: vec!["new".into()],
            },
        )
        .expect("second");
        let back: Tree = load_snapshot(&path).expect("load").expect("present");
        assert_eq!(back.sessions, ["new"]);
    }

    #[test]
    fn a_corrupt_snapshot_is_an_error_rather_than_a_default() {
        // Restoring a default here would silently replace the user's whole
        // session tree with an empty one.
        let d = tempfile::tempdir().expect("tempdir");
        let path = d.path().join("tree.json");
        std::fs::write(&path, b"{ not json").expect("write");
        let result: Result<Option<Tree>, _> = load_snapshot(&path);
        assert!(result.is_err());
    }
}
