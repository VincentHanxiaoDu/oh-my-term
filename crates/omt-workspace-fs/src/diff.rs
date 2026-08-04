//! What changed, as git sees it.
//!
//! Parsed from git's own output rather than computed here. A diff omt produced
//! itself would disagree with the one the user gets from `git diff` — over
//! whitespace settings, over renames, over their own `diff.algorithm` — and a
//! review surface that disagrees with the command line is one nobody trusts
//! twice.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::git::GitError;

/// What happened to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
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
    /// Git could not merge it.
    Conflicted,
}

/// One file in a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Where it is now.
    pub path: String,
    /// Where it was, for a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// What happened.
    pub kind: ChangeKind,
    /// Lines added.
    pub added: u32,
    /// Lines removed.
    pub removed: u32,
    /// Whether git considers it binary.
    ///
    /// Reported rather than hidden: a surface that tried to render a binary
    /// diff shows nothing and looks broken, where one that says "binary" is
    /// simply correct.
    pub binary: bool,
}

/// One contiguous run of changed lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// Where it starts in the old file.
    pub old_start: u32,
    /// How many lines it covers there.
    pub old_lines: u32,
    /// Where it starts in the new file.
    pub new_start: u32,
    /// How many lines it covers.
    pub new_lines: u32,
    /// The lines themselves, prefixes and all.
    pub lines: Vec<String>,
}

/// Which diff to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffTarget {
    /// Changes not yet staged.
    Unstaged,
    /// Changes staged for the next commit.
    Staged,
    /// Everything since a commit.
    SinceCommit,
}

/// Summarize what changed.
///
/// # Errors
/// Fails if git cannot be run or the directory is not a repository.
pub fn changed_files(
    dir: &Path,
    target: DiffTarget,
    base: Option<&str>,
) -> Result<Vec<FileChange>, GitError> {
    let mut args = vec!["diff", "--numstat", "--find-renames"];
    match target {
        DiffTarget::Staged => args.push("--cached"),
        DiffTarget::SinceCommit => {}
        DiffTarget::Unstaged => {}
    }
    if let Some(base) = base {
        args.push(base);
    }
    let numstat = run(dir, &args)?;

    // A second call for the status letters. `--numstat` gives counts and
    // renames but not whether a file was added or deleted, and inferring that
    // from a zero count is wrong for an empty new file.
    let mut status_args = vec!["diff", "--name-status", "--find-renames"];
    if target == DiffTarget::Staged {
        status_args.push("--cached");
    }
    if let Some(base) = base {
        status_args.push(base);
    }
    let statuses = run(dir, &status_args)?;
    let kinds = parse_name_status(&statuses);

    Ok(parse_numstat(&numstat, &kinds))
}

/// The hunks of one file.
///
/// # Errors
/// Fails if git cannot be run.
pub fn hunks(
    dir: &Path,
    path: &str,
    target: DiffTarget,
    base: Option<&str>,
) -> Result<Vec<Hunk>, GitError> {
    let mut args = vec!["diff", "--unified=3"];
    if target == DiffTarget::Staged {
        args.push("--cached");
    }
    if let Some(base) = base {
        args.push(base);
    }
    // `--` so a path that looks like a revision is treated as a path. A file
    // called `main` would otherwise diff a branch.
    args.push("--");
    args.push(path);
    Ok(parse_hunks(&run(dir, &args)?))
}

/// Parse `--numstat`, which is `added\tremoved\tpath`.
#[must_use]
pub fn parse_numstat(
    text: &str,
    kinds: &[(String, ChangeKind, Option<String>)],
) -> Vec<FileChange> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let added = parts.next()?;
            let removed = parts.next()?;
            let path = parts.next()?;

            // Git writes `-` for a binary file rather than a count, which is
            // the only signal that it is one.
            let binary = added == "-" || removed == "-";
            let (kind, from) = kinds
                .iter()
                .find(|(p, _, _)| p == path)
                .map(|(_, k, f)| (*k, f.clone()))
                .unwrap_or((ChangeKind::Modified, None));

            Some(FileChange {
                path: path.to_owned(),
                from,
                kind,
                added: added.parse().unwrap_or(0),
                removed: removed.parse().unwrap_or(0),
                binary,
            })
        })
        .collect()
}

/// Parse `--name-status`.
#[must_use]
pub fn parse_name_status(text: &str) -> Vec<(String, ChangeKind, Option<String>)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let status = parts.next()?;
            let first = parts.next()?;
            let letter = status.chars().next()?;
            let kind = match letter {
                'A' => ChangeKind::Added,
                'D' => ChangeKind::Deleted,
                'R' => ChangeKind::Renamed,
                'C' => ChangeKind::Copied,
                'U' => ChangeKind::Conflicted,
                _ => ChangeKind::Modified,
            };
            // A rename writes both paths; the second is where it landed.
            match parts.next() {
                Some(to) => Some((to.to_owned(), kind, Some(first.to_owned()))),
                None => Some((first.to_owned(), kind, None)),
            }
        })
        .collect()
}

/// Parse unified-diff hunks.
#[must_use]
pub fn parse_hunks(text: &str) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::new();
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = parse_hunk_header(header) {
                out.push(hunk);
            }
            continue;
        }
        // Everything before the first `@@` is the file header, which a hunk
        // view does not want.
        if let Some(current) = out.last_mut()
            && (line.starts_with(' ')
                || line.starts_with('+')
                || line.starts_with('-')
                || line.is_empty())
        {
            current.lines.push(line.to_owned());
        }
    }
    out
}

fn parse_hunk_header(header: &str) -> Option<Hunk> {
    // `-old,count +new,count @@ context`
    let body = header.split(" @@").next()?;
    let mut parts = body.split_whitespace();
    let (old_start, old_lines) = parse_range(parts.next()?.strip_prefix('-')?)?;
    let (new_start, new_lines) = parse_range(parts.next()?.strip_prefix('+')?)?;
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        lines: Vec::new(),
    })
}

fn parse_range(text: &str) -> Option<(u32, u32)> {
    match text.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        // A range with no comma is one line, which git writes as just the
        // start. Defaulting to zero would render an empty hunk.
        None => Some((text.parse().ok()?, 1)),
    }
}

fn run(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| GitError::Unavailable(e.to_string()))?;
    if !out.status.success() {
        return Err(GitError::NotARepository(dir.display().to_string()));
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

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().expect("tempdir");
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .args(&args)
                .current_dir(d.path())
                .output()
                .expect("git");
        }
        d
    }

    fn commit(dir: &Path) {
        for args in [vec!["add", "-A"], vec!["commit", "-qm", "c"]] {
            Command::new("git")
                .args(&args)
                .current_dir(dir)
                .output()
                .expect("git");
        }
    }

    #[test]
    fn a_modified_file_reports_its_line_counts() {
        let d = repo();
        std::fs::write(d.path().join("a.txt"), "one\ntwo\nthree\n").expect("write");
        commit(d.path());
        std::fs::write(d.path().join("a.txt"), "one\nCHANGED\nthree\nfour\n").expect("write");

        let changes = changed_files(d.path(), DiffTarget::Unstaged, None).expect("diff");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "a.txt");
        assert_eq!(changes[0].kind, ChangeKind::Modified);
        assert_eq!(changes[0].added, 2);
        assert_eq!(changes[0].removed, 1);
    }

    #[test]
    fn an_added_file_is_added_not_modified() {
        // Inferring this from a zero removal count is wrong for an empty new
        // file, which is why the status letters are read separately.
        let d = repo();
        std::fs::write(d.path().join("a.txt"), "x\n").expect("write");
        commit(d.path());
        std::fs::write(d.path().join("new.txt"), "hello\n").expect("write");
        Command::new("git")
            .args(["add", "new.txt"])
            .current_dir(d.path())
            .output()
            .expect("git");

        let changes = changed_files(d.path(), DiffTarget::Staged, None).expect("diff");
        assert_eq!(changes[0].kind, ChangeKind::Added);
    }

    #[test]
    fn a_deleted_file_is_reported_as_deleted() {
        let d = repo();
        std::fs::write(d.path().join("a.txt"), "x\n").expect("write");
        commit(d.path());
        std::fs::remove_file(d.path().join("a.txt")).expect("remove");

        let changes = changed_files(d.path(), DiffTarget::Unstaged, None).expect("diff");
        assert_eq!(changes[0].kind, ChangeKind::Deleted);
    }

    #[test]
    fn staged_and_unstaged_are_different_answers() {
        // A review surface that conflated them would show the user changes
        // they had already staged as if they were still pending.
        let d = repo();
        std::fs::write(d.path().join("a.txt"), "one\n").expect("write");
        commit(d.path());

        std::fs::write(d.path().join("a.txt"), "staged\n").expect("write");
        Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(d.path())
            .output()
            .expect("git");
        std::fs::write(d.path().join("a.txt"), "staged then edited\n").expect("write");

        let staged = changed_files(d.path(), DiffTarget::Staged, None).expect("diff");
        let unstaged = changed_files(d.path(), DiffTarget::Unstaged, None).expect("diff");
        assert_eq!(staged.len(), 1);
        assert_eq!(unstaged.len(), 1);
    }

    #[test]
    fn a_clean_repository_has_no_changes() {
        let d = repo();
        std::fs::write(d.path().join("a.txt"), "x\n").expect("write");
        commit(d.path());
        assert!(
            changed_files(d.path(), DiffTarget::Unstaged, None)
                .expect("diff")
                .is_empty()
        );
    }

    #[test]
    fn hunks_carry_their_line_numbers_and_their_lines() {
        let d = repo();
        let original: String = (1..=20).map(|i| format!("line{i}\n")).collect();
        std::fs::write(d.path().join("a.txt"), &original).expect("write");
        commit(d.path());
        let edited = original.replace("line10\n", "CHANGED\n");
        std::fs::write(d.path().join("a.txt"), edited).expect("write");

        let hunks = hunks(d.path(), "a.txt", DiffTarget::Unstaged, None).expect("hunks");
        assert_eq!(hunks.len(), 1);
        assert!(hunks[0].old_start > 0);
        assert!(
            hunks[0].lines.iter().any(|l| l.starts_with("+CHANGED")),
            "{:?}",
            hunks[0].lines
        );
        assert!(hunks[0].lines.iter().any(|l| l.starts_with("-line10")));
    }

    #[test]
    fn a_path_that_looks_like_a_revision_is_treated_as_a_path() {
        // A file called `main` would otherwise diff a branch.
        let d = repo();
        std::fs::write(d.path().join("main"), "one\n").expect("write");
        commit(d.path());
        std::fs::write(d.path().join("main"), "two\n").expect("write");
        let hunks = hunks(d.path(), "main", DiffTarget::Unstaged, None).expect("hunks");
        assert_eq!(hunks.len(), 1, "the path was read as a revision");
    }

    #[test]
    fn a_binary_file_is_marked_rather_than_rendered() {
        // A surface that tried to render a binary diff shows nothing and looks
        // broken; one that says "binary" is simply correct.
        let numstat = "-\t-\timage.png\n12\t3\tsrc/main.rs\n";
        let changes = parse_numstat(numstat, &[]);
        assert!(changes[0].binary, "{changes:?}");
        assert!(!changes[1].binary);
        assert_eq!(changes[1].added, 12);
    }

    #[test]
    fn a_rename_keeps_both_names() {
        // Showing only the new name loses the thing a reviewer most needs to
        // see: that it is the same file.
        let statuses = "R100\told/path.rs\tnew/path.rs\n";
        let kinds = parse_name_status(statuses);
        assert_eq!(kinds[0].0, "new/path.rs");
        assert_eq!(kinds[0].1, ChangeKind::Renamed);
        assert_eq!(kinds[0].2.as_deref(), Some("old/path.rs"));
    }

    #[test]
    fn a_single_line_hunk_header_parses() {
        // Git writes `-5` rather than `-5,1` for a one-line range, and
        // defaulting the count to zero renders an empty hunk.
        let hunks = parse_hunks("@@ -5 +5,2 @@ fn main()\n-old\n+new\n+extra\n");
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 5);
        assert_eq!(hunks[0].old_lines, 1);
        assert_eq!(hunks[0].new_lines, 2);
    }

    #[test]
    fn the_file_header_is_not_mistaken_for_hunk_lines() {
        // `--- a/x` and `+++ b/x` start with the same characters as removed and
        // added lines, and including them puts them in the rendered diff.
        let text = "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
        let hunks = parse_hunks(text);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].lines, vec!["-a", "+b"]);
    }

    #[test]
    fn a_directory_outside_a_repository_says_so() {
        let d = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            changed_files(d.path(), DiffTarget::Unstaged, None),
            Err(GitError::NotARepository(_))
        ));
    }

    #[test]
    fn nothing_here_changes_the_repository() {
        let d = repo();
        std::fs::write(d.path().join("a.txt"), "one\n").expect("write");
        commit(d.path());
        std::fs::write(d.path().join("a.txt"), "two\n").expect("write");

        let before = crate::git::status(d.path()).expect("status");
        let _ = changed_files(d.path(), DiffTarget::Unstaged, None);
        let _ = hunks(d.path(), "a.txt", DiffTarget::Unstaged, None);
        assert_eq!(crate::git::status(d.path()).expect("status"), before);
    }
}
