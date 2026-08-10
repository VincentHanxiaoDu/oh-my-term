//! The list of commands, as distinct from the output they produced.
//!
//! `docs/architecture/05-session-model.md` §9. History is structured, durable
//! and searchable long after the output is gone — a command is forty bytes and
//! is the thing people actually search their own past for, where its output is
//! megabytes nobody reads twice.
//!
//! **It is populated from block closure, so it exists exactly where blocks
//! exist.** With shell integration it is complete. Without it, heuristic blocks
//! contribute *nothing*: a block opened from output alone has no reliable
//! command text, and the alternative — scraping the line above for something
//! that looks like a prompt — produces a history full of half-lines and
//! prompts-as-commands. An empty history is an honest degradation; a wrong one
//! is a trap, because `history.rerun` on a mis-scraped entry runs something
//! nobody typed.

use omt_types::{SessionId, Timestamp, WorkspaceId};
use serde::{Deserialize, Serialize};

/// Who ran it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    /// A person typed it.
    Human,
    /// An agent ran it.
    Agent,
    /// omt could not tell.
    ///
    /// Kept as a third answer rather than defaulting to `Human`: "I do not
    /// know" and "a person did this" are different claims, and the second one
    /// is the one somebody would rely on when auditing what an agent did.
    Unknown,
}

/// One command that ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Its identity.
    pub id: u64,
    /// What was run.
    pub command: String,
    /// Which workspace it ran in.
    pub workspace: Option<WorkspaceId>,
    /// Where it ran.
    pub cwd: Option<String>,
    /// Which branch was checked out.
    pub git_branch: Option<String>,
    /// How it ended, where that was seen.
    pub exit: Option<i32>,
    /// When it started.
    pub started_at: Timestamp,
    /// How long it took, in milliseconds.
    pub duration_ms: Option<u64>,
    /// Which session.
    pub session: SessionId,
    /// Which host, when it was not this one.
    pub host: Option<String>,
    /// Who ran it.
    pub attribution: Attribution,
}

impl Entry {
    /// Whether it succeeded, where that is known.
    #[must_use]
    pub const fn succeeded(&self) -> Option<bool> {
        match self.exit {
            Some(code) => Some(code == 0),
            None => None,
        }
    }
}

/// What part of history to look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Everything.
    Global,
    /// One workspace.
    Workspace(WorkspaceId),
    /// One session.
    Session(SessionId),
    /// One directory.
    Cwd(String),
}

/// How a filter matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// From the start.
    Prefix,
    /// Anywhere.
    Substring,
    /// In order, with gaps.
    Fuzzy,
}

/// Whether to collapse repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dedup {
    /// Show every run.
    None,
    /// One row per distinct command, the most recent kept.
    KeepNewest,
}

/// What to look for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Where to look.
    pub scope: Scope,
    /// What to look for.
    pub filter: Option<String>,
    /// How to match it.
    pub mode: MatchMode,
    /// Only commands that exited zero.
    pub only_successful: bool,
    /// Whether to collapse repeats.
    pub dedup: Dedup,
    /// Most rows to return.
    pub limit: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            scope: Scope::Global,
            filter: None,
            mode: MatchMode::Substring,
            only_successful: false,
            // The default a person means by "my history": one row per command,
            // newest kept. Twenty identical `cargo test` rows are twenty rows
            // of nothing.
            dedup: Dedup::KeepNewest,
            limit: 200,
        }
    }
}

/// Whether a candidate matches, in order with gaps.
fn fuzzy(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle
        .chars()
        .all(|wanted| chars.any(|c| c.eq_ignore_ascii_case(&wanted)))
}

fn matches(entry: &Entry, query: &Query) -> bool {
    let in_scope = match &query.scope {
        Scope::Global => true,
        Scope::Workspace(id) => entry.workspace == Some(*id),
        Scope::Session(id) => entry.session == *id,
        Scope::Cwd(path) => entry.cwd.as_deref() == Some(path.as_str()),
    };
    if !in_scope {
        return false;
    }
    // A command that has not finished has no exit code, and treating "not yet"
    // as "failed" would hide a running command from a filter meant to hide
    // failures.
    if query.only_successful && entry.succeeded() != Some(true) {
        return false;
    }
    match &query.filter {
        None => true,
        Some(filter) => {
            let command = entry.command.to_lowercase();
            let filter = filter.to_lowercase();
            match query.mode {
                MatchMode::Prefix => command.starts_with(&filter),
                MatchMode::Substring => command.contains(&filter),
                MatchMode::Fuzzy => fuzzy(&command, &filter),
            }
        }
    }
}

/// A session's history.
#[derive(Debug, Clone, Default)]
pub struct History {
    entries: Vec<Entry>,
    next_id: u64,
}

impl History {
    /// An empty history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many commands are recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether anything is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Record a closed block.
    ///
    /// Returns the id, or `None` when the block carries no command omt ever
    /// saw. That refusal is the honest degradation §9 describes: a heuristic
    /// block has no reliable command text, and inventing one from the screen
    /// would put something nobody typed into a list people re-run from.
    pub fn record(
        &mut self,
        attributed: bool,
        command: &str,
        session: SessionId,
        started_at: Timestamp,
    ) -> Option<u64> {
        if !attributed || command.trim().is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            command: command.to_owned(),
            workspace: None,
            cwd: None,
            git_branch: None,
            exit: None,
            started_at,
            duration_ms: None,
            session,
            host: None,
            attribution: Attribution::Unknown,
        });
        Some(id)
    }

    /// Fill in how a command ended.
    pub fn finish(&mut self, id: u64, exit: i32, duration_ms: u64) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.exit = Some(exit);
            entry.duration_ms = Some(duration_ms);
        }
    }

    /// The entry at an id.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Attach the context a block closure knows.
    pub fn attribute(&mut self, id: u64, attribution: Attribution) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.attribution = attribution;
        }
    }

    /// Search.
    ///
    /// Newest first, because the thing somebody wants is almost always
    /// something they ran recently.
    #[must_use]
    pub fn query(&self, query: &Query) -> Vec<&Entry> {
        let mut out: Vec<&Entry> = self
            .entries
            .iter()
            .rev()
            .filter(|entry| matches(entry, query))
            .collect();

        if query.dedup == Dedup::KeepNewest {
            let mut seen = std::collections::BTreeSet::new();
            out.retain(|entry| seen.insert(entry.command.clone()));
        }
        out.truncate(query.limit);
        out
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

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_seconds(seconds)
    }

    fn history() -> (History, SessionId) {
        let session = SessionId::new();
        let mut h = History::new();
        for (i, command) in ["cargo build", "cargo test", "ls", "cargo test"]
            .into_iter()
            .enumerate()
        {
            let id = h
                .record(true, command, session, at(i as i64))
                .expect("recorded");
            h.finish(id, i32::from(command == "ls"), 100);
        }
        (h, session)
    }

    #[test]
    fn a_marked_command_is_recorded() {
        let (h, _) = history();
        assert_eq!(h.len(), 4);
    }

    #[test]
    fn a_heuristic_block_contributes_nothing() {
        // It has no reliable command text, and scraping the line above for
        // something that looks like a prompt puts something nobody typed into a
        // list people re-run from.
        let mut h = History::new();
        assert_eq!(h.record(false, "looks like a command", SessionId::new(), at(0)), None);
        assert!(h.is_empty(), "an empty history is the honest answer");
    }

    #[test]
    fn a_marked_block_with_no_command_is_not_recorded_either() {
        // A mark with nothing after it is a prompt the user pressed enter on.
        let mut h = History::new();
        assert_eq!(h.record(true, "   ", SessionId::new(), at(0)), None);
    }

    #[test]
    fn history_comes_back_newest_first() {
        // What somebody wants is almost always something they ran recently.
        let (h, _) = history();
        let out = h.query(&Query {
            dedup: Dedup::None,
            ..Query::default()
        });
        assert_eq!(out[0].command, "cargo test");
        assert_eq!(out.last().expect("some").command, "cargo build");
    }

    #[test]
    fn repeats_collapse_to_the_most_recent_by_default() {
        // Twenty identical `cargo test` rows are twenty rows of nothing.
        let (h, _) = history();
        let out = h.query(&Query::default());
        let commands: Vec<&str> = out.iter().map(|e| e.command.as_str()).collect();
        assert_eq!(commands, vec!["cargo test", "ls", "cargo build"]);
    }

    #[test]
    fn every_run_is_available_when_asked_for() {
        let (h, _) = history();
        let out = h.query(&Query {
            dedup: Dedup::None,
            ..Query::default()
        });
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn a_substring_filter_finds_it_anywhere_in_the_command() {
        let (h, _) = history();
        let out = h.query(&Query {
            filter: Some("test".to_owned()),
            ..Query::default()
        });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].command, "cargo test");
    }

    #[test]
    fn a_prefix_filter_only_matches_the_start() {
        let (h, _) = history();
        let out = h.query(&Query {
            filter: Some("test".to_owned()),
            mode: MatchMode::Prefix,
            ..Query::default()
        });
        assert!(out.is_empty(), "`cargo test` does not start with `test`");
    }

    #[test]
    fn a_fuzzy_filter_matches_in_order_with_gaps() {
        let (h, _) = history();
        let out = h.query(&Query {
            filter: Some("crgt".to_owned()),
            mode: MatchMode::Fuzzy,
            ..Query::default()
        });
        assert!(out.iter().any(|e| e.command == "cargo test"), "{out:?}");
    }

    #[test]
    fn filtering_to_successes_hides_the_failures() {
        let (h, _) = history();
        let out = h.query(&Query {
            only_successful: true,
            dedup: Dedup::None,
            ..Query::default()
        });
        assert!(out.iter().all(|e| e.command != "ls"), "`ls` exited 1");
    }

    #[test]
    fn a_command_that_has_not_finished_is_not_a_success() {
        // Treating "not yet" as "failed" would be wrong in the other direction;
        // both are wrong, and the honest answer is that it is neither.
        let mut h = History::new();
        let session = SessionId::new();
        h.record(true, "sleep 100", session, at(0)).expect("recorded");
        let out = h.query(&Query {
            only_successful: true,
            ..Query::default()
        });
        assert!(out.is_empty());
        assert_eq!(h.get(0).expect("entry").succeeded(), None);
    }

    #[test]
    fn a_session_scope_leaves_out_everything_else() {
        let (mut h, session) = history();
        let other = SessionId::new();
        h.record(true, "elsewhere", other, at(9)).expect("recorded");
        let out = h.query(&Query {
            scope: Scope::Session(session),
            dedup: Dedup::None,
            ..Query::default()
        });
        assert!(out.iter().all(|e| e.session == session));
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn a_limit_is_honoured_after_deduplication_rather_than_before() {
        // Otherwise a limit of three over twenty identical commands returns one
        // row, and the user sees a list that looks empty.
        let mut h = History::new();
        let session = SessionId::new();
        for i in 0..20 {
            h.record(true, "cargo test", session, at(i)).expect("recorded");
        }
        h.record(true, "ls", session, at(21)).expect("recorded");
        let out = h.query(&Query {
            limit: 3,
            ..Query::default()
        });
        assert_eq!(out.len(), 2, "two distinct commands, not one row of twenty");
    }

    #[test]
    fn attribution_defaults_to_unknown_rather_than_to_a_person() {
        // "I do not know" and "a person did this" are different claims, and the
        // second is the one somebody would rely on when auditing an agent.
        let (h, _) = history();
        assert_eq!(h.get(0).expect("entry").attribution, Attribution::Unknown);
    }

    #[test]
    fn attribution_can_be_filled_in_once_it_is_known() {
        let (mut h, _) = history();
        h.attribute(0, Attribution::Agent);
        assert_eq!(h.get(0).expect("entry").attribution, Attribution::Agent);
    }
}
