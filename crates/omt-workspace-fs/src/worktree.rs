//! Worktrees, as git makes them.
//!
//! `git worktree` is invoked rather than reimplemented. Maintaining omt's own
//! notion of what is checked out where would be a second source of truth about
//! the same fact, and git's own `worktree list` is the one thing that can never
//! disagree with reality.
//!
//! A worktree needs no identity of its own: `WorkspaceId` is derived from a
//! canonical path, so a worktree *is* a workspace the moment it exists. Nothing
//! in the session tree or the capability surface needs a case for it.

use std::path::Path;
use std::process::Command;

use crate::git::GitError;

/// One worktree of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Its absolute path.
    pub path: String,
    /// The branch checked out in it, where one is.
    ///
    /// `None` for a detached HEAD, which is a real state and not an error —
    /// `git worktree add --detach` produces one deliberately.
    pub branch: Option<String>,
    /// The commit it is at.
    pub head: Option<String>,
    /// Whether this is the repository's original checkout.
    ///
    /// Reported because removing it is not possible and a UI offering to is a
    /// UI whose button fails.
    pub is_main: bool,
}

/// Why a worktree operation was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorktreeError {
    /// git said no.
    #[error("git refused: {0}")]
    Git(String),
    /// Something is already at that path.
    #[error("`{path}` already exists")]
    Exists {
        /// Where.
        path: String,
    },
    /// It has changes that removing would discard.
    ///
    /// The counts are in the message, not only in the refusal. "It has
    /// uncommitted changes" is a sentence somebody forces past without
    /// looking, because it is what git says about every worktree they have
    /// ever removed on purpose; "4 modified, 2 untracked" is a number they can
    /// recognise as either their own afternoon or nothing they care about.
    /// That is the whole difference between a confirmation and a formality —
    /// and a worktree is usually an agent's output, which is unrecoverable.
    #[error(
        "`{path}` has uncommitted changes that removing would discard: {modified} modified, {staged} staged, {untracked} untracked. Pass force to remove it anyway."
    )]
    Dirty {
        /// Which worktree.
        path: String,
        /// Tracked files changed and not staged.
        modified: u32,
        /// Changes staged and not committed.
        staged: u32,
        /// Files git is not tracking at all.
        untracked: u32,
    },
    /// The repository could not be read.
    #[error(transparent)]
    Repository(#[from] GitError),
}

/// Every worktree of the repository containing `dir`.
///
/// # Errors
/// Fails if git cannot be run or the directory is not in a repository.
pub fn list(dir: &Path) -> Result<Vec<Worktree>, WorktreeError> {
    // `--porcelain` rather than the human format: the human one aligns columns
    // and a path with a space in it becomes ambiguous. This one is one field
    // per line and never is.
    let out = run(dir, &["worktree", "list", "--porcelain"])?;
    let mut worktrees = Vec::new();
    let mut current: Option<Worktree> = None;

    for line in out.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(done) = current.take() {
                worktrees.push(done);
            }
            current = Some(Worktree {
                path: path.to_owned(),
                branch: None,
                head: None,
                // The first one listed is always the main checkout.
                is_main: worktrees.is_empty(),
            });
        } else if let Some(head) = line.strip_prefix("HEAD ")
            && let Some(w) = current.as_mut()
        {
            w.head = Some(head.to_owned());
        } else if let Some(branch) = line.strip_prefix("branch ")
            && let Some(w) = current.as_mut()
        {
            // `refs/heads/x` is how git spells it; nobody wants to read that
            // in a list.
            w.branch = Some(branch.trim_start_matches("refs/heads/").to_owned());
        }
    }
    if let Some(done) = current {
        worktrees.push(done);
    }
    Ok(worktrees)
}

/// Add a worktree at `path` for `branch`.
///
/// Creates the branch when it does not exist and checks it out when it does —
/// both are things people mean by "give me a worktree for this branch", and
/// failing on one of them would make the caller ask twice.
///
/// # Errors
/// Fails if the path is taken, or if git refuses.
pub fn add(dir: &Path, path: &str, branch: &str) -> Result<Worktree, WorktreeError> {
    if Path::new(path).exists() {
        return Err(WorktreeError::Exists {
            path: path.to_owned(),
        });
    }

    // `-B` creates or resets the branch to HEAD. Tried first because the common
    // case is a new branch; if the branch is checked out elsewhere git refuses
    // and the fallback checks it out without moving it.
    let created = run(dir, &["worktree", "add", "-b", branch, path]);
    if created.is_err() {
        run(dir, &["worktree", "add", path, branch])?;
    }

    list(dir)?
        .into_iter()
        .find(|w| same_path(&w.path, path))
        .ok_or_else(|| WorktreeError::Git("the worktree was not created".to_owned()))
}

/// Remove a worktree.
///
/// Refuses when there are uncommitted changes unless `force` is given. Not a
/// prompt: omt has no place to ask from, and a capability that blocked on a
/// question would be unanswerable from a phone.
///
/// # Errors
/// Fails if the worktree has changes and `force` was not given, or if git
/// refuses.
pub fn remove(dir: &Path, path: &str, force: bool) -> Result<(), WorktreeError> {
    if !force {
        let (modified, staged, untracked) = uncommitted(Path::new(path));
        if modified + staged + untracked > 0 {
            return Err(WorktreeError::Dirty {
                path: path.to_owned(),
                modified,
                staged,
                untracked,
            });
        }
    }
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(path);
    run(dir, &args)?;
    Ok(())
}

/// What a worktree has uncommitted: modified, staged, untracked.
///
/// A failure to read reports one modified file rather than none: refusing to
/// remove something omt could not inspect is the safe direction, and the
/// caller can still force it. Reported as counts rather than as a yes/no,
/// because the refusal has to say what is at stake to be worth reading.
fn uncommitted(path: &Path) -> (u32, u32, u32) {
    crate::git::status(path).map_or((1, 0, 0), |s| (s.modified, s.staged, s.untracked))
}

/// Compare two paths without requiring both to exist.
fn same_path(a: &str, b: &str) -> bool {
    let canonical = |p: &str| {
        std::fs::canonicalize(p)
            .map(|c| c.display().to_string())
            .unwrap_or_else(|_| p.to_owned())
    };
    a == b || canonical(a) == canonical(b)
}

fn run(dir: &Path, args: &[&str]) -> Result<String, WorktreeError> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| WorktreeError::Git(e.to_string()))?;
    if !out.status.success() {
        // git's own message, verbatim. Replacing it with omt's would lose the
        // detail that says which of a dozen reasons this was.
        return Err(WorktreeError::Git(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    /// A repository with one commit, which is the least git will list.
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let run_git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("git");
        };
        run_git(&["init", "-q", "-b", "main"]);
        run_git(&["config", "user.email", "test@example.com"]);
        run_git(&["config", "user.name", "test"]);
        std::fs::write(dir.path().join("a.txt"), "one\n").expect("write");
        run_git(&["add", "."]);
        run_git(&["commit", "-qm", "first"]);
        dir
    }

    #[test]
    fn a_fresh_repository_lists_its_own_checkout() {
        let dir = repo();
        let worktrees = list(dir.path()).expect("list");
        assert_eq!(worktrees.len(), 1);
        assert!(
            worktrees[0].is_main,
            "the original checkout is not marked as the main one"
        );
        assert_eq!(worktrees[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn adding_a_worktree_creates_its_branch() {
        let dir = repo();
        let path = dir.path().join("wt-feature");
        let added = add(dir.path(), &path.display().to_string(), "feature").expect("add");

        assert_eq!(added.branch.as_deref(), Some("feature"));
        assert!(
            path.join("a.txt").is_file(),
            "the checkout has no files in it"
        );
        assert_eq!(list(dir.path()).expect("list").len(), 2);
    }

    #[test]
    fn a_worktree_is_not_the_main_one() {
        // Removing the main checkout is impossible, and a UI offering to is a
        // UI whose button fails.
        let dir = repo();
        let path = dir.path().join("wt");
        add(dir.path(), &path.display().to_string(), "b").expect("add");
        let added = list(dir.path())
            .expect("list")
            .into_iter()
            .find(|w| !w.is_main)
            .expect("a second worktree");
        assert_eq!(added.branch.as_deref(), Some("b"));
    }

    #[test]
    fn a_path_that_exists_is_refused_rather_than_overwritten() {
        let dir = repo();
        let path = dir.path().join("taken");
        std::fs::create_dir(&path).expect("mkdir");
        let err = add(dir.path(), &path.display().to_string(), "b")
            .expect_err("an existing path was accepted");
        assert!(matches!(err, WorktreeError::Exists { .. }), "{err}");
    }

    #[test]
    fn an_existing_branch_is_checked_out_rather_than_failing() {
        // Both "make me a branch" and "give me this branch" are things people
        // mean by asking for a worktree, and failing on one makes them ask
        // twice.
        let dir = repo();
        Command::new("git")
            .args(["branch", "existing"])
            .current_dir(dir.path())
            .output()
            .expect("git branch");

        let path = dir.path().join("wt-existing");
        let added = add(dir.path(), &path.display().to_string(), "existing").expect("add");
        assert_eq!(added.branch.as_deref(), Some("existing"));
    }

    #[test]
    fn a_clean_worktree_is_removed() {
        let dir = repo();
        let path = dir.path().join("wt-clean");
        let p = path.display().to_string();
        add(dir.path(), &p, "clean").expect("add");
        remove(dir.path(), &p, false).expect("remove");
        assert_eq!(list(dir.path()).expect("list").len(), 1);
    }

    #[test]
    fn a_worktree_with_changes_is_refused_rather_than_losing_them() {
        // The whole point of the check: an agent's output is usually why the
        // worktree was wanted, and deleting it on a tap is unrecoverable.
        let dir = repo();
        let path = dir.path().join("wt-dirty");
        let p = path.display().to_string();
        add(dir.path(), &p, "dirty").expect("add");
        std::fs::write(path.join("a.txt"), "changed\n").expect("write");

        let err = remove(dir.path(), &p, false).expect_err("changes were discarded");
        assert!(matches!(err, WorktreeError::Dirty { .. }), "{err}");
        assert!(path.is_dir(), "it was removed anyway");
    }

    #[test]
    fn force_removes_a_worktree_with_changes() {
        let dir = repo();
        let path = dir.path().join("wt-forced");
        let p = path.display().to_string();
        add(dir.path(), &p, "forced").expect("add");
        std::fs::write(path.join("a.txt"), "changed\n").expect("write");

        remove(dir.path(), &p, true).expect("forced remove");
        assert_eq!(list(dir.path()).expect("list").len(), 1);
    }

    #[test]
    fn git_own_refusal_survives_rather_than_being_replaced() {
        // git says which of a dozen reasons it refused. Replacing that with
        // omt's own wording loses the only useful part.
        let dir = repo();
        let err = remove(dir.path(), "/definitely/not/a/worktree", true)
            .expect_err("a nonexistent worktree was removed");
        let WorktreeError::Git(message) = err else {
            panic!("expected git's own message");
        };
        assert!(!message.is_empty(), "git's message was thrown away");
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod what_a_refusal_says {
    use super::*;

    #[test]
    fn a_refusal_names_what_would_be_lost() {
        // "It has uncommitted changes" is what git says about every worktree
        // anybody has ever removed on purpose, so it is a sentence people
        // force past. A worktree is usually an agent's output and removing it
        // is unrecoverable, which makes the counts the whole point.
        let refusal = WorktreeError::Dirty {
            path: "/w/feature".to_owned(),
            modified: 4,
            staged: 1,
            untracked: 12,
        };
        let said = refusal.to_string();
        assert!(said.contains("/w/feature"), "{said}");
        assert!(said.contains("4 modified"), "{said}");
        assert!(said.contains("1 staged"), "{said}");
        assert!(said.contains("12 untracked"), "{said}");
        // And how to proceed anyway, in the same breath. A refusal with no way
        // past it is one somebody works around outside omt.
        assert!(said.to_lowercase().contains("force"), "{said}");
    }

    #[test]
    fn a_worktree_that_cannot_be_read_is_treated_as_dirty() {
        // The safe direction. Refusing to remove something omt could not
        // inspect costs one forced call; the other way costs the work.
        let (modified, staged, untracked) = uncommitted(Path::new("/no/such/worktree"));
        assert!(modified + staged + untracked > 0);
    }
}
