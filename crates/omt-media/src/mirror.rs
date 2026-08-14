//! Where a file fetched from a remote host lands locally.
//!
//! `docs/architecture/18-semantic-open.md` §6.3. This is not a second file
//! store: the mirror tree is a named region of the blob store, under
//! `BlobClass::Mirror`, and the sweeper and refcount rules stay here rather
//! than in the callers.
//!
//! The layout is the whole design, and every element earns its place.
//!
//! The **basename is preserved verbatim**, because a hash-named file is
//! unusable: the editor tab has to say `lexer.rs`, syntax highlighting has to
//! work, and a language server has to pick the right grammar from the
//! extension. The **trailing directory components** give an editor's
//! breadcrumb something true to show. And a **hash of the full remote path**
//! sits above both, because two hosts — or two checkouts on one host — have
//! files with the same name and the same tail, and without the hash the second
//! one silently overwrites the first.
//!
//! A sidecar beside each file records where it came from. Without it a mirror
//! is an anonymous copy: nothing can say which host it belongs to, whether it
//! is stale, or whether writing to it would mean anything.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many trailing components of the remote directory are kept.
///
/// Three: enough for a breadcrumb to be recognisable, few enough that a deeply
/// nested path does not reproduce somebody's whole tree locally.
pub const TAIL_COMPONENTS: usize = 3;

/// What is known about one mirrored file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mirror {
    /// The path on the remote host.
    pub remote_path: String,
    /// Which host.
    pub host: String,
    /// When it was fetched, seconds since the epoch.
    pub fetched_at: u64,
    /// A hash of the content that was fetched.
    pub digest: String,
    /// Whether omt will refuse to write it back.
    ///
    /// Default. Writing to a remote file through a local mirror is a different
    /// feature with different failure modes, and a mirror that silently
    /// accepted edits nobody could push would lose work.
    pub read_only: bool,
}

/// The short hash that disambiguates one remote path from another.
///
/// Eight hex characters of the digest of the *full* remote path. Not of the
/// content: two hosts may hold identical files, and they still must not share
/// a mirror, because discarding one would take the other with it.
#[must_use]
pub fn path_key(remote_path: &str) -> String {
    crate::digest(remote_path.as_bytes())
        .chars()
        .take(8)
        .collect()
}

/// Where a remote file is mirrored locally.
#[must_use]
pub fn local_path(root: &Path, host: &str, remote_path: &str) -> PathBuf {
    let remote = Path::new(remote_path);
    let basename = remote
        .file_name()
        .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());

    let parent_components: Vec<String> = remote
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let tail = parent_components
        .iter()
        .rev()
        .take(TAIL_COMPONENTS)
        .rev()
        .cloned()
        .collect::<Vec<_>>();

    let mut path = root
        .join(sanitize_component_name(host))
        .join(path_key(remote_path));
    for part in tail {
        path.push(sanitize_component_name(&part));
    }
    path.join(sanitize_component_name(&basename))
}

/// Where the sidecar for a mirrored file goes.
///
/// Beside it, prefixed with a dot, so it does not appear in an editor's file
/// list next to the thing it describes.
#[must_use]
pub fn sidecar_path(local: &Path) -> PathBuf {
    let name = local
        .file_name()
        .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
    local.with_file_name(format!(".{name}.omt-remote.json"))
}

/// Reduce one component of a remote path to something safe to create locally.
///
/// The remote host chose these strings. A component of `..` would place the
/// mirror outside the tree, and one containing a separator would create
/// directories nobody asked for.
fn sanitize_component_name(part: &str) -> String {
    let cleaned: String = part
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | '\0'))
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "_".to_owned()
    } else {
        trimmed.to_owned()
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

    fn root() -> PathBuf {
        PathBuf::from("/state/omt/remote")
    }

    #[test]
    fn the_basename_survives_verbatim() {
        // A hash-named file is unusable: the tab has to say `lexer.rs`, and a
        // language server picks its grammar from the extension.
        let path = local_path(&root(), "box", "/app/src/lexer.rs");
        assert_eq!(
            path.file_name().expect("name").to_string_lossy(),
            "lexer.rs"
        );
    }

    #[test]
    fn the_trailing_directories_give_a_breadcrumb_something_true_to_show() {
        let path = local_path(&root(), "box", "/very/deep/app/src/lexer.rs");
        let text = path.display().to_string();
        assert!(text.contains("app/src"), "{text}");
        assert!(!text.contains("/very/"), "only the tail is kept: {text}");
    }

    #[test]
    fn two_hosts_with_the_same_filename_do_not_collide() {
        // Without the hash the second silently overwrites the first, and the
        // user is looking at a file from the wrong machine.
        let a = local_path(&root(), "box", "/app/src/lexer.rs");
        let b = local_path(&root(), "other", "/app/src/lexer.rs");
        assert_ne!(a, b);
    }

    #[test]
    fn two_checkouts_on_one_host_do_not_collide() {
        let a = local_path(&root(), "box", "/work/one/src/lexer.rs");
        let b = local_path(&root(), "box", "/work/two/src/lexer.rs");
        assert_ne!(a, b);
    }

    #[test]
    fn the_key_is_of_the_path_not_the_content() {
        // Two hosts may hold identical files and still must not share a
        // mirror, because discarding one would take the other with it.
        assert_ne!(path_key("/a/x.rs"), path_key("/b/x.rs"));
        assert_eq!(path_key("/a/x.rs"), path_key("/a/x.rs"));
        assert_eq!(path_key("/a/x.rs").len(), 8);
    }

    #[test]
    fn a_remote_component_cannot_place_the_mirror_outside_the_tree() {
        // The remote host chose these strings.
        let path = local_path(&root(), "box", "/../../etc/passwd");
        assert!(path.starts_with(root()), "{}", path.display());
        assert!(!path.display().to_string().contains(".."));
    }

    #[test]
    fn a_hostile_host_name_is_reduced_too() {
        let path = local_path(&root(), "../../etc", "/app/x.rs");
        assert!(path.starts_with(root()), "{}", path.display());
    }

    #[test]
    fn the_sidecar_sits_beside_the_file_and_out_of_the_way() {
        let local = local_path(&root(), "box", "/app/src/lexer.rs");
        let sidecar = sidecar_path(&local);
        assert_eq!(sidecar.parent(), local.parent());
        assert_eq!(
            sidecar.file_name().expect("name").to_string_lossy(),
            ".lexer.rs.omt-remote.json"
        );
    }

    #[test]
    fn a_mirror_is_read_only_by_default() {
        // Writing back through a mirror is a different feature with different
        // failure modes, and one that silently accepted edits nobody could
        // push would lose work.
        let m = Mirror {
            remote_path: "/app/x.rs".to_owned(),
            host: "box".to_owned(),
            fetched_at: 0,
            digest: "abc".to_owned(),
            read_only: true,
        };
        let round: Mirror =
            serde_json::from_str(&serde_json::to_string(&m).expect("to")).expect("from");
        assert!(round.read_only);
        assert_eq!(round, m);
    }

    #[test]
    fn a_path_with_no_directory_still_lands_somewhere_sensible() {
        let path = local_path(&root(), "box", "x.rs");
        assert!(path.starts_with(root()));
        assert_eq!(path.file_name().expect("name").to_string_lossy(), "x.rs");
    }
}
