//! What git says about a workspace.
//!
//! Read by running `git`, not by parsing `.git` — the layout is an internal
//! detail that changes, worktrees and submodules make it stranger than it
//! looks, and the user's own config, includes and aliases are already loaded by
//! the binary they installed.
//!
//! Everything here is a **query**. Nothing commits, checks out, or fetches: a
//! terminal that quietly ran git commands on somebody's behalf is a terminal
//! that eventually loses somebody's work.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// What a workspace's repository looks like right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStatus {
    /// The branch, or `None` on a detached head.
    ///
    /// `None` rather than the commit, so a surface renders "detached" instead
    /// of a hash the user might mistake for a branch name.
    pub branch: Option<String>,
    /// The upstream it tracks, where it has one.
    pub upstream: Option<String>,
    /// Commits ahead of upstream.
    pub ahead: u32,
    /// Commits behind it.
    pub behind: u32,
    /// Files changed but not staged.
    pub modified: u32,
    /// Files staged.
    pub staged: u32,
    /// Files git does not know about.
    pub untracked: u32,
}

impl GitStatus {
    /// Whether there is anything uncommitted.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.modified > 0 || self.staged > 0 || self.untracked > 0
    }
}

/// Where a repository's code lives on the internet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forge {
    /// Its host, e.g. `github.com`.
    pub host: String,
    /// `owner/repo`.
    pub slug: String,
}

impl Forge {
    /// The URL for a branch's pull request search.
    ///
    /// A search rather than a guessed PR number: omt does not know whether one
    /// exists, and a link to a PR that is not there is worse than a link to
    /// the list that shows it does not.
    #[must_use]
    pub fn branch_url(&self, branch: &str) -> String {
        let encoded = branch.replace(' ', "%20");
        match self.host.as_str() {
            h if h.contains("github") => {
                format!("https://{}/{}/pull/{encoded}", self.host, self.slug)
            }
            h if h.contains("gitlab") => {
                format!(
                    "https://{}/{}/-/merge_requests?scope=all&search={encoded}",
                    self.host, self.slug
                )
            }
            _ => format!("https://{}/{}", self.host, self.slug),
        }
    }
}

/// Why a query failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitError {
    /// This directory is not in a repository.
    #[error("`{0}` is not inside a git repository")]
    NotARepository(String),
    /// git could not be run.
    #[error("could not run git: {0}")]
    Unavailable(String),
}

/// Ask git about a directory.
///
/// # Errors
/// Fails if git is not installed or the directory is not in a repository.
pub fn status(dir: &Path) -> Result<GitStatus, GitError> {
    // One porcelain call rather than five. `--branch` puts the branch, upstream
    // and divergence on the first line, so the whole status is one process
    // rather than one per field — which matters because this runs on a timer.
    let out = run(dir, &["status", "--porcelain=v2", "--branch"])?;

    let mut status = GitStatus {
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        modified: 0,
        staged: 0,
        untracked: 0,
    };

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            // git says `(detached)` rather than a name, and reporting that as a
            // branch would put a literal "(detached)" in the UI.
            status.branch = (rest != "(detached)").then(|| rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            status.upstream = Some(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let mut parts = rest.split_whitespace();
            status.ahead = parts
                .next()
                .and_then(|a| a.strip_prefix('+'))
                .and_then(|a| a.parse().ok())
                .unwrap_or(0);
            status.behind = parts
                .next()
                .and_then(|b| b.strip_prefix('-'))
                .and_then(|b| b.parse().ok())
                .unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("1 ").or_else(|| line.strip_prefix("2 ")) {
            // The XY field: staged on the left, unstaged on the right.
            let xy = rest.split_whitespace().next().unwrap_or("..");
            let mut chars = xy.chars();
            if chars.next().is_some_and(|c| c != '.') {
                status.staged += 1;
            }
            if chars.next().is_some_and(|c| c != '.') {
                status.modified += 1;
            }
        } else if line.starts_with("? ") {
            status.untracked += 1;
        }
    }

    Ok(status)
}

/// Where this repository's code is hosted, if anywhere recognisable.
///
/// # Errors
/// Fails if git cannot be run. A repository with no remote is `Ok(None)` —
/// that is a normal repository, not a failure.
pub fn forge(dir: &Path) -> Result<Option<Forge>, GitError> {
    let url = match run(dir, &["remote", "get-url", "origin"]) {
        Ok(u) => u.trim().to_owned(),
        // No `origin` is not an error; plenty of repositories have none.
        Err(_) => return Ok(None),
    };
    Ok(parse_remote(&url))
}

/// Turn a remote URL into a host and slug.
///
/// Both spellings, because both are in every developer's config and a tool that
/// only understood one would work for half of a team.
#[must_use]
pub fn parse_remote(url: &str) -> Option<Forge> {
    let url = url.trim().trim_end_matches(".git");

    // `git@host:owner/repo`
    if let Some(rest) = url.split_once('@').map(|(_, r)| r)
        && let Some((host, slug)) = rest.split_once(':')
        && !slug.is_empty()
        && !url.starts_with("http")
    {
        return Some(Forge {
            host: host.to_owned(),
            slug: slug.trim_start_matches('/').to_owned(),
        });
    }

    // `https://host/owner/repo`
    for prefix in ["https://", "http://", "ssh://"] {
        if let Some(rest) = url.strip_prefix(prefix) {
            // Strip any userinfo, which appears in URLs people paste from a
            // token-based clone and must not travel into a public link.
            let rest = rest.rsplit('@').next().unwrap_or(rest);
            let (host, slug) = rest.split_once('/')?;
            if slug.is_empty() {
                return None;
            }
            return Some(Forge {
                host: host.to_owned(),
                slug: slug.to_owned(),
            });
        }
    }
    None
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

    fn commit(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), "x").expect("write");
        for args in [vec!["add", "-A"], vec!["commit", "-qm", "c"]] {
            Command::new("git")
                .args(&args)
                .current_dir(dir)
                .output()
                .expect("git");
        }
    }

    #[test]
    fn a_clean_repository_reports_its_branch_and_nothing_else() {
        let d = repo();
        commit(d.path(), "a.txt");
        let s = status(d.path()).expect("status");
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert!(!s.is_dirty(), "{s:?}");
    }

    #[test]
    fn changes_are_counted_by_where_they_are() {
        let d = repo();
        commit(d.path(), "a.txt");

        std::fs::write(d.path().join("a.txt"), "changed").expect("write");
        std::fs::write(d.path().join("new.txt"), "new").expect("write");
        Command::new("git")
            .args(["add", "new.txt"])
            .current_dir(d.path())
            .output()
            .expect("git");
        std::fs::write(d.path().join("untracked.txt"), "u").expect("write");

        let s = status(d.path()).expect("status");
        assert_eq!(s.modified, 1, "{s:?}");
        assert_eq!(s.staged, 1, "{s:?}");
        assert_eq!(s.untracked, 1, "{s:?}");
        assert!(s.is_dirty());
    }

    #[test]
    fn a_detached_head_has_no_branch_rather_than_a_literal_word() {
        // Reporting git's own "(detached)" would put that string in the UI as
        // if it were a branch name.
        let d = repo();
        commit(d.path(), "a.txt");
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(d.path())
            .output()
            .expect("git");
        let sha = String::from_utf8_lossy(&head.stdout).trim().to_owned();
        Command::new("git")
            .args(["checkout", "-q", &sha])
            .current_dir(d.path())
            .output()
            .expect("git");

        assert_eq!(status(d.path()).expect("status").branch, None);
    }

    #[test]
    fn a_directory_outside_a_repository_says_so() {
        let d = tempfile::tempdir().expect("tempdir");
        assert!(matches!(status(d.path()), Err(GitError::NotARepository(_))));
    }

    #[test]
    fn a_repository_with_no_remote_is_not_a_failure() {
        // Plenty of repositories have none.
        let d = repo();
        commit(d.path(), "a.txt");
        assert_eq!(forge(d.path()).expect("forge"), None);
    }

    #[test]
    fn both_remote_spellings_are_understood() {
        // Both are in every developer's config; a tool that understood one
        // would work for half of a team.
        let ssh = parse_remote("git@github.com:owner/repo.git").expect("ssh form");
        let https = parse_remote("https://github.com/owner/repo.git").expect("https form");
        assert_eq!(ssh, https);
        assert_eq!(ssh.host, "github.com");
        assert_eq!(ssh.slug, "owner/repo");
    }

    #[test]
    fn a_token_in_a_clone_url_does_not_travel_into_a_link() {
        // People paste these from a CI setup, and the token must not end up in
        // a URL omt hands to a browser.
        let f = parse_remote("https://x-token:secret@github.com/owner/repo.git").expect("parse");
        assert_eq!(f.host, "github.com");
        assert!(!f.branch_url("main").contains("secret"), "{f:?}");
    }

    #[test]
    fn a_nested_group_survives() {
        let f = parse_remote("git@gitlab.com:group/subgroup/repo.git").expect("parse");
        assert_eq!(f.slug, "group/subgroup/repo");
    }

    #[test]
    fn an_unrecognisable_remote_is_none_rather_than_a_guess() {
        assert_eq!(parse_remote("/srv/git/local.git"), None);
        assert_eq!(parse_remote(""), None);
    }

    #[test]
    fn a_branch_link_points_at_a_search_rather_than_a_guessed_number() {
        // omt does not know whether a PR exists, and a link to one that is not
        // there is worse than a link to the list that shows it is not.
        let f = Forge {
            host: "github.com".to_owned(),
            slug: "owner/repo".to_owned(),
        };
        let url = f.branch_url("feature/thing");
        assert!(url.starts_with("https://github.com/owner/repo/"), "{url}");
        assert!(!url.contains("/pull/1"), "{url}");
    }

    #[test]
    fn a_host_omt_does_not_know_still_gets_a_usable_link() {
        let f = Forge {
            host: "git.internal.example".to_owned(),
            slug: "team/thing".to_owned(),
        };
        assert_eq!(
            f.branch_url("main"),
            "https://git.internal.example/team/thing"
        );
    }

    #[test]
    fn nothing_here_changes_the_repository() {
        // A terminal that quietly ran git commands on somebody's behalf is one
        // that eventually loses somebody's work.
        let d = repo();
        commit(d.path(), "a.txt");
        let before = status(d.path()).expect("status");
        let _ = forge(d.path());
        let _ = status(d.path());
        assert_eq!(status(d.path()).expect("status"), before);
    }
}
