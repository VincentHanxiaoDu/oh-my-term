//! Command history: what was run, where, and how it ended.
//!
//! Ranking is by *recency plus frequency in this directory*, not by frequency
//! alone. A command run fifty times in another project is not what the user
//! wants here, and a history that offers it is one they stop reading.

pub mod backpressure;
pub mod compare;
pub mod policy;
pub mod ranking;
pub mod redact;
pub mod schedule;
pub mod stuck;
pub mod timeline;

pub use schedule::{FAILURES_BEFORE_DISABLING, Job, JobState, Schedule, Skipped, Trigger};

use std::collections::BTreeMap;

use omt_types::{SessionId, Timestamp, WorkspaceId};
use serde::{Deserialize, Serialize};

/// One thing that was run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// The command, verbatim.
    pub command: String,
    /// Where it ran.
    pub workspace: WorkspaceId,
    /// Which session.
    pub session: SessionId,
    /// The directory it ran in, which is not the workspace.
    pub cwd: Option<String>,
    /// When it started.
    pub at: Timestamp,
    /// How it ended, where a shell reported it.
    pub exit_code: Option<i32>,
    /// How long it took.
    pub duration_ms: Option<u64>,
    /// Who ran it.
    ///
    /// Defaults to `Unknown` rather than to a person: "I do not know" and "a
    /// person did this" are different claims, and the second is the one
    /// somebody would rely on when auditing what an agent did.
    #[serde(default)]
    pub attribution: Attribution,
}

/// Who ran a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    /// omt could not tell.
    #[default]
    Unknown,
    /// A person typed it.
    Human,
    /// An agent ran it.
    Agent,
}

/// How a history filter matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchMode {
    /// From the start.
    Prefix,
    /// Anywhere.
    #[default]
    Substring,
    /// In order, with gaps.
    Fuzzy,
}

/// Which part of history to look at.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum HistoryScope {
    /// Everything.
    #[default]
    Global,
    /// One workspace.
    Workspace(WorkspaceId),
    /// One session.
    Session(SessionId),
    /// One directory.
    Cwd(String),
}

/// Whether to collapse repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dedup {
    /// Show every run.
    None,
    /// One row per distinct command, the most recent kept.
    ///
    /// The default, because it is what a person means by "my history": twenty
    /// identical `cargo test` rows are twenty rows of nothing.
    #[default]
    KeepNewest,
}

/// What to look for in history.
///
/// 20 §12.2. `history.query` is the instance-scoped query and the only
/// implementation; `session.history` and `workspace.history` are pre-scoped
/// conveniences over it, so that a session-scoped client does not have to
/// build a global query and narrow it. If they ever diverge, this one is right.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HistoryQuery {
    /// Where to look.
    pub scope: HistoryScope,
    /// What to look for.
    pub filter: Option<String>,
    /// How to match it.
    pub mode: MatchMode,
    /// Only commands that exited zero.
    pub only_successful: bool,
    /// Whether to collapse repeats.
    pub dedup: Dedup,
    /// Most rows to return. Zero means the default.
    pub limit: usize,
}

impl HistoryEntry {
    /// Whether this command succeeded.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(self.exit_code, Some(0))
    }
}

/// A ranked suggestion.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// The command.
    pub command: String,
    /// Its score. Higher is better; comparable only within one query.
    pub score: f64,
    /// How many times it has been run in the queried workspace.
    pub uses_here: u32,
}

/// Commands never worth suggesting back.
///
/// A recalled `rm -rf` is a keystroke away from running again, and history is
/// the one place where being helpful and being dangerous are the same action.
#[must_use]
pub fn is_dangerous(command: &str) -> bool {
    let c = command.trim();

    // A prefix match on `rm -rf /` would catch `rm -rf /tmp/scratch`, which is
    // an ordinary thing to run. What makes it dangerous is the *target*, so the
    // target is what is checked.
    if let Some(rest) = c
        .strip_prefix("rm -rf ")
        .or_else(|| c.strip_prefix("rm -fr "))
        .or_else(|| c.strip_prefix("rm -r -f "))
    {
        let target = rest.trim();
        if matches!(
            target,
            "/" | "/*" | "~" | "~/" | "~/*" | "$HOME" | "$HOME/*"
        ) {
            return true;
        }
    }

    c.contains(":(){")
        || c.starts_with("mkfs")
        || (c.starts_with("dd if=") && c.contains("of=/dev/"))
}

/// Everything an instance remembers being run.
#[derive(Debug, Default)]
pub struct History {
    entries: Vec<HistoryEntry>,
    limit: usize,
}

/// Whether a candidate matches, in order with gaps.
fn fuzzy(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle
        .chars()
        .all(|wanted| chars.any(|c| c.eq_ignore_ascii_case(&wanted)))
}

impl History {
    /// A history holding at most `limit` entries.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            entries: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Record a command.
    ///
    /// Secrets are not filtered here on purpose: a command line the user typed
    /// is theirs, and a filter that guessed would be both incomplete and
    /// surprising. Redaction is a policy decision made where the data leaves.
    pub fn record(&mut self, entry: HistoryEntry) {
        self.entries.push(entry);
        if self.entries.len() > self.limit {
            let excess = self.entries.len() - self.limit;
            self.entries.drain(..excess);
        }
    }

    /// How many entries are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Whether a block's command belongs in history at all.
    ///
    /// 05 §9: history is populated from block closure, so it exists exactly
    /// where blocks exist. A block opened from output alone has no reliable
    /// command text, and the alternative — scraping the line above for
    /// something that looks like a prompt — puts text nobody typed into a list
    /// people re-run from. An empty history is an honest degradation; a wrong
    /// one is a trap.
    #[must_use]
    pub fn is_recordable(attributed: bool, command: &str) -> bool {
        attributed && !command.trim().is_empty()
    }

    /// Search history.
    ///
    /// Newest first, because what somebody wants is almost always something
    /// they ran recently.
    #[must_use]
    pub fn query(&self, query: &HistoryQuery) -> Vec<&HistoryEntry> {
        let limit = if query.limit == 0 { 200 } else { query.limit };
        let mut out: Vec<&HistoryEntry> = self
            .entries
            .iter()
            .rev()
            .filter(|entry| {
                let in_scope = match &query.scope {
                    HistoryScope::Global => true,
                    HistoryScope::Workspace(id) => entry.workspace == *id,
                    HistoryScope::Session(id) => entry.session == *id,
                    HistoryScope::Cwd(path) => entry.cwd.as_deref() == Some(path.as_str()),
                };
                if !in_scope {
                    return false;
                }
                // A command that has not finished has no exit code. Treating
                // "not yet" as "failed" would hide a running command from a
                // filter meant to hide failures.
                if query.only_successful && entry.exit_code != Some(0) {
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
            })
            .collect();

        if query.dedup == Dedup::KeepNewest {
            let mut seen = std::collections::BTreeSet::new();
            out.retain(|entry| seen.insert(entry.command.clone()));
        }
        // Truncated after deduplication rather than before: a limit of three
        // over twenty identical commands would otherwise return one row and
        // look like an empty list.
        out.truncate(limit);
        out
    }

    /// Remove every entry matching a query, and say how many went.
    ///
    /// The targeted counterpart to a store purge: somebody who typed a secret
    /// into a shell wants that line gone, not their whole history.
    pub fn forget(&mut self, query: &HistoryQuery) -> usize {
        let doomed: std::collections::BTreeSet<String> = self
            .query(&HistoryQuery {
                dedup: Dedup::None,
                limit: usize::MAX,
                ..query.clone()
            })
            .into_iter()
            .map(|e| e.command.clone())
            .collect();
        let before = self.entries.len();
        self.entries.retain(|e| !doomed.contains(&e.command));
        before - self.entries.len()
    }

    /// Suggest commands for a prefix, best first.
    ///
    /// Scored by frequency *in this workspace* plus recency. A command run
    /// fifty times somewhere else is not an answer to "what do I run here".
    #[must_use]
    pub fn suggest(&self, prefix: &str, workspace: WorkspaceId, limit: usize) -> Vec<Suggestion> {
        let mut counts: BTreeMap<&str, (u32, i128)> = BTreeMap::new();
        for e in &self.entries {
            if !e.command.starts_with(prefix) || is_dangerous(&e.command) {
                continue;
            }
            // A command that failed is a worse suggestion than one that worked,
            // but not a disqualifying one: half of shell use is retrying
            // something after fixing it.
            let entry = counts.entry(e.command.as_str()).or_insert((0, 0));
            if e.workspace == workspace {
                entry.0 += 1;
            }
            entry.1 = entry.1.max(e.at.unix_millis());
        }

        let newest = counts.values().map(|(_, t)| *t).max().unwrap_or(0);
        let mut out: Vec<Suggestion> = counts
            .into_iter()
            .map(|(command, (uses_here, last))| {
                // Recency decays over roughly a week, so an old favourite is
                // still reachable but does not outrank what was just used.
                let age_days = ((newest - last) as f64) / 86_400_000.0;
                let recency = 1.0 / (1.0 + age_days);
                Suggestion {
                    command: command.to_owned(),
                    score: f64::from(uses_here).mul_add(1.0, recency),
                    uses_here,
                }
            })
            .collect();

        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                // A stable tiebreak, so two runs of the same query do not
                // reorder the list under the user's fingers.
                .then_with(|| a.command.cmp(&b.command))
        });
        out.truncate(limit);
        out
    }
}

/// Merging results from several instances.
///
/// `docs/architecture/20-recall-and-usage.md` §7. Somebody with a laptop and
/// two boxes has three histories, and the question "when did I run that" does
/// not care which machine it was on.
///
/// Merging is a client-side concern because the alternative is a shared index,
/// and a shared index means every instance's output leaves the machine it was
/// produced on. That is a much larger promise than "search works across your
/// boxes", and it is not one this feature needs.
pub mod federate {
    use super::Suggestion;

    /// One instance's answer to the same query.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Answer {
        /// Which instance.
        pub instance: String,
        /// Whether it answered at all.
        ///
        /// An instance that timed out is not an instance with no results, and
        /// showing them the same way turns "the box is down" into "you never
        /// ran that".
        pub reachable: bool,
        /// What it found.
        pub suggestions: Vec<Suggestion>,
    }

    /// A merged result, with the instance it came from.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Merged {
        /// The command.
        pub command: String,
        /// Its combined score.
        pub score: f64,
        /// Which instances have it, so somebody can tell local from remote.
        pub instances: Vec<String>,
    }

    /// What a merge could not include.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Gap {
        /// Which instance.
        pub instance: String,
        /// What to say about it.
        pub why: String,
    }

    /// Merge several instances' answers.
    ///
    /// The same command from two machines is one row that names both, not two
    /// rows. Two rows would fill a palette with duplicates of whatever somebody
    /// runs everywhere, which is most of what they run.
    #[must_use]
    pub fn merge(answers: &[Answer]) -> (Vec<Merged>, Vec<Gap>) {
        let mut by_command: std::collections::BTreeMap<&str, (f64, Vec<String>)> =
            std::collections::BTreeMap::new();
        let mut gaps = Vec::new();

        for answer in answers {
            if !answer.reachable {
                gaps.push(Gap {
                    instance: answer.instance.clone(),
                    why: format!(
                        "`{}` did not answer, so its history is missing from these results.",
                        answer.instance
                    ),
                });
                continue;
            }
            for suggestion in &answer.suggestions {
                let entry = by_command
                    .entry(suggestion.command.as_str())
                    .or_insert((0.0, Vec::new()));
                // Summed rather than maxed: a command run on three machines is
                // more of a habit than one run once on the fastest.
                entry.0 += suggestion.score;
                entry.1.push(answer.instance.clone());
            }
        }

        let mut merged: Vec<Merged> = by_command
            .into_iter()
            .map(|(command, (score, mut instances))| {
                instances.sort();
                instances.dedup();
                Merged {
                    command: command.to_owned(),
                    score,
                    instances,
                }
            })
            .collect();
        merged.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.command.cmp(&b.command))
        });
        (merged, gaps)
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
    #[test]
    fn history_is_deduplicated_so_a_habit_is_one_row() {
        // Otherwise a palette fills with copies of whatever somebody runs
        // fifty times a day, which is most of what they run.
        // The helpers below already build these; duplicating a literal here
        // would drift the moment `HistoryEntry` gains a field.
        let mut h = History::new(100);
        let here = workspace("/repo");
        for i in 0..20 {
            h.record(entry("cargo test", here, 1_000 + i));
        }
        let out = h.suggest("cargo", here, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].uses_here, 20);
    }

    #[test]
    fn merging_two_instances_gives_one_row_naming_both() {
        use federate::{Answer, merge};
        let suggestion = |command: &str, score: f64| Suggestion {
            command: command.to_owned(),
            score,
            uses_here: 1,
        };
        let (merged, gaps) = merge(&[
            Answer {
                instance: "laptop".to_owned(),
                reachable: true,
                suggestions: vec![suggestion("cargo test", 2.0)],
            },
            Answer {
                instance: "box".to_owned(),
                reachable: true,
                suggestions: vec![suggestion("cargo test", 1.0)],
            },
        ]);
        assert!(gaps.is_empty());
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].instances, ["box", "laptop"]);
        assert!((merged[0].score - 3.0).abs() < 1e-9, "summed, not maxed");
    }

    #[test]
    fn an_instance_that_did_not_answer_is_a_gap_not_an_absence() {
        // Showing them the same way turns "the box is down" into "you never
        // ran that", and somebody retypes a command they already have.
        use federate::{Answer, merge};
        let (merged, gaps) = merge(&[
            Answer {
                instance: "laptop".to_owned(),
                reachable: true,
                suggestions: vec![Suggestion {
                    command: "cargo test".to_owned(),
                    score: 1.0,
                    uses_here: 1,
                }],
            },
            Answer {
                instance: "box".to_owned(),
                reachable: false,
                suggestions: Vec::new(),
            },
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(gaps.len(), 1);
        assert!(gaps[0].why.contains("did not answer"), "{}", gaps[0].why);
    }

    #[test]
    fn merging_nothing_is_empty_rather_than_a_panic() {
        let (merged, gaps) = federate::merge(&[]);
        assert!(merged.is_empty());
        assert!(gaps.is_empty());
    }

    #[test]
    fn a_merge_is_ordered_totally_so_the_palette_does_not_jump() {
        use federate::{Answer, merge};
        let answers = vec![Answer {
            instance: "a".to_owned(),
            reachable: true,
            suggestions: vec![
                Suggestion { command: "b".to_owned(), score: 1.0, uses_here: 1 },
                Suggestion { command: "a".to_owned(), score: 1.0, uses_here: 1 },
            ],
        }];
        let once = merge(&answers).0;
        for _ in 0..5 {
            assert_eq!(merge(&answers).0, once);
        }
    }


    fn entry(command: &str, workspace: WorkspaceId, secs: i64) -> HistoryEntry {
        HistoryEntry {
            attribution: Attribution::Unknown,
            command: command.to_owned(),
            workspace,
            session: SessionId::new(),
            cwd: None,
            at: Timestamp::from_unix_seconds(secs),
            exit_code: Some(0),
            duration_ms: Some(10),
        }
    }

    fn workspace(path: &str) -> WorkspaceId {
        WorkspaceId::from_canonical_path(path)
    }

    #[test]
    fn a_command_run_here_outranks_one_run_elsewhere() {
        // The whole reason ranking is workspace-aware.
        let here = workspace("/here");
        let there = workspace("/there");
        let mut h = History::new(100);
        for i in 0..20 {
            h.record(entry("cargo build", there, 1_000 + i));
        }
        h.record(entry("cargo test", here, 1_050));

        let s = h.suggest("cargo", here, 5);
        assert_eq!(s[0].command, "cargo test", "{s:?}");
    }

    #[test]
    fn frequency_here_beats_a_single_recent_use() {
        let here = workspace("/here");
        let mut h = History::new(100);
        for i in 0..5 {
            h.record(entry("cargo test", here, 1_000 + i));
        }
        h.record(entry("cargo doc", here, 2_000));
        let s = h.suggest("cargo", here, 5);
        assert_eq!(s[0].command, "cargo test");
    }

    #[test]
    fn the_order_is_stable_across_identical_queries() {
        // A list that reorders under the user's fingers is one they misclick.
        let here = workspace("/here");
        let mut h = History::new(100);
        h.record(entry("git status", here, 1_000));
        h.record(entry("git stash", here, 1_000));
        assert_eq!(h.suggest("git", here, 5), h.suggest("git", here, 5));
    }

    #[test]
    fn a_prefix_that_matches_nothing_suggests_nothing() {
        let here = workspace("/here");
        let mut h = History::new(10);
        h.record(entry("ls", here, 1));
        assert!(h.suggest("kubectl", here, 5).is_empty());
    }

    #[test]
    fn a_destructive_command_is_never_suggested_back() {
        // History is the one place where being helpful and being dangerous are
        // the same action: a recalled `rm -rf /` is one keystroke from running.
        let here = workspace("/here");
        let mut h = History::new(10);
        h.record(entry("rm -rf /", here, 1));
        h.record(entry("rm -rf /tmp/scratch", here, 2));
        let s = h.suggest("rm", here, 5);
        assert!(!s.iter().any(|x| x.command == "rm -rf /"), "{s:?}");
        assert!(
            s.iter().any(|x| x.command == "rm -rf /tmp/scratch"),
            "but an ordinary rm is still useful: {s:?}"
        );
    }

    #[test]
    fn the_dangerous_list_covers_the_classics() {
        assert!(is_dangerous("rm -rf /"));
        assert!(is_dangerous("  rm -fr /  "));
        assert!(
            is_dangerous("rm -rf ~"),
            "the other one people actually run"
        );
        assert!(is_dangerous("rm -rf /*"));
        assert!(is_dangerous(":(){ :|:& };:"));
        assert!(is_dangerous("mkfs.ext4 /dev/sda"));
        assert!(is_dangerous("dd if=/dev/zero of=/dev/sda"));
        assert!(!is_dangerous("rm -rf node_modules"));
        assert!(
            !is_dangerous("rm -rf /tmp/scratch"),
            "an absolute path is not by itself dangerous, and suppressing it \
             would hide most of what anyone runs rm for"
        );
        assert!(!is_dangerous("dd if=in.img of=out.img"));
    }

    #[test]
    fn history_is_bounded_and_drops_the_oldest() {
        let here = workspace("/here");
        let mut h = History::new(3);
        for i in 0..10 {
            h.record(entry(&format!("cmd{i}"), here, i));
        }
        assert_eq!(h.len(), 3);
        assert_eq!(h.entries()[0].command, "cmd7", "the oldest went first");
    }

    #[test]
    fn a_failed_command_is_still_recorded() {
        // Half of shell use is retrying something after fixing it.
        let here = workspace("/here");
        let mut h = History::new(10);
        h.record(HistoryEntry {
            exit_code: Some(1),
            ..entry("cargo build", here, 1)
        });
        assert!(!h.entries()[0].succeeded());
        assert_eq!(h.suggest("cargo", here, 5).len(), 1);
    }

    #[test]
    fn a_command_with_no_exit_code_did_not_succeed() {
        // Unknown is not success. Treating it as such would show a green tick
        // next to something that may still be running.
        let here = workspace("/here");
        let e = HistoryEntry {
            exit_code: None,
            ..entry("sleep 100", here, 1)
        };
        assert!(!e.succeeded());
    }

    #[test]
    fn suggestions_respect_the_limit() {
        let here = workspace("/here");
        let mut h = History::new(100);
        for i in 0..20 {
            h.record(entry(&format!("git cmd{i}"), here, i));
        }
        assert_eq!(h.suggest("git", here, 5).len(), 5);
    }

    #[test]
    fn an_empty_history_suggests_nothing_rather_than_failing() {
        let h = History::new(10);
        assert!(h.is_empty());
        assert!(h.suggest("", workspace("/x"), 5).is_empty());
    }

    fn queryable(
        command: &str,
        session: SessionId,
        workspace: WorkspaceId,
        exit: Option<i32>,
        at: i64,
    ) -> HistoryEntry {
        HistoryEntry {
            command: command.to_owned(),
            workspace,
            session,
            cwd: None,
            at: Timestamp::from_unix_seconds(at),
            exit_code: exit,
            duration_ms: Some(10),
            attribution: Attribution::Unknown,
        }
    }

    fn queried() -> (History, SessionId, WorkspaceId) {
        let session = SessionId::new();
        let workspace = WorkspaceId::from_canonical_path("/tmp/p");
        let mut h = History::new(100);
        for (i, (command, exit)) in [
            ("cargo build", Some(0)),
            ("cargo test", Some(0)),
            ("ls", Some(1)),
            ("cargo test", Some(0)),
        ]
        .into_iter()
        .enumerate()
        {
            h.record(queryable(command, session, workspace, exit, i as i64));
        }
        (h, session, workspace)
    }

    #[test]
    fn a_heuristic_block_is_not_recordable() {
        // It has no reliable command text, and scraping the line above for
        // something prompt-shaped puts text nobody typed into a list people
        // re-run from.
        assert!(!History::is_recordable(false, "looks like a command"));
        assert!(!History::is_recordable(true, "   "));
        assert!(History::is_recordable(true, "cargo test"));
    }

    #[test]
    fn history_comes_back_newest_first() {
        let (h, _, _) = queried();
        let out = h.query(&HistoryQuery {
            dedup: Dedup::None,
            ..HistoryQuery::default()
        });
        assert_eq!(out[0].command, "cargo test");
        assert_eq!(out.last().expect("some").command, "cargo build");
    }

    #[test]
    fn repeats_collapse_to_the_most_recent_by_default() {
        // Twenty identical `cargo test` rows are twenty rows of nothing.
        let (h, _, _) = queried();
        let commands: Vec<&str> = h
            .query(&HistoryQuery::default())
            .iter()
            .map(|e| e.command.as_str())
            .collect();
        assert_eq!(commands, vec!["cargo test", "ls", "cargo build"]);
    }

    #[test]
    fn a_prefix_filter_only_matches_the_start() {
        let (h, _, _) = queried();
        assert!(
            h.query(&HistoryQuery {
                filter: Some("test".to_owned()),
                mode: MatchMode::Prefix,
                ..HistoryQuery::default()
            })
            .is_empty(),
            "`cargo test` does not start with `test`"
        );
    }

    #[test]
    fn a_fuzzy_filter_matches_in_order_with_gaps() {
        let (h, _, _) = queried();
        let out = h.query(&HistoryQuery {
            filter: Some("crgt".to_owned()),
            mode: MatchMode::Fuzzy,
            ..HistoryQuery::default()
        });
        assert!(out.iter().any(|e| e.command == "cargo test"), "{out:?}");
    }

    #[test]
    fn a_command_that_has_not_finished_is_not_a_success() {
        // Treating "not yet" as "failed" is wrong in the other direction; the
        // honest answer is that it is neither.
        let mut h = History::new(10);
        h.record(queryable("sleep 100", SessionId::new(), WorkspaceId::from_canonical_path("/tmp/p"), None, 0));
        assert!(
            h.query(&HistoryQuery {
                only_successful: true,
                ..HistoryQuery::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn a_session_scope_leaves_out_everything_else() {
        let (mut h, session, workspace) = queried();
        h.record(queryable("elsewhere", SessionId::new(), workspace, Some(0), 9));
        let out = h.query(&HistoryQuery {
            scope: HistoryScope::Session(session),
            dedup: Dedup::None,
            ..HistoryQuery::default()
        });
        assert!(out.iter().all(|e| e.session == session));
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn a_limit_is_honoured_after_deduplication_rather_than_before() {
        // Otherwise a limit of three over twenty identical commands returns one
        // row and the list looks empty.
        let workspace = WorkspaceId::from_canonical_path("/tmp/p");
        let session = SessionId::new();
        let mut h = History::new(100);
        for i in 0..20 {
            h.record(queryable("cargo test", session, workspace, Some(0), i));
        }
        h.record(queryable("ls", session, workspace, Some(0), 21));
        let out = h.query(&HistoryQuery {
            limit: 3,
            ..HistoryQuery::default()
        });
        assert_eq!(out.len(), 2, "two distinct commands, not one row of twenty");
    }

    #[test]
    fn forgetting_removes_every_run_of_a_matching_command() {
        // Somebody who typed a secret into a shell wants that line gone, not
        // their whole history — and gone means every time they ran it.
        let (mut h, _, _) = queried();
        let removed = h.forget(&HistoryQuery {
            filter: Some("cargo test".to_owned()),
            ..HistoryQuery::default()
        });
        assert_eq!(removed, 2, "both runs went");
        assert!(h.query(&HistoryQuery::default()).iter().all(|e| e.command != "cargo test"));
        assert_eq!(h.len(), 2, "and nothing else did");
    }

    #[test]
    fn attribution_defaults_to_unknown_rather_than_to_a_person() {
        let (h, _, _) = queried();
        assert_eq!(h.entries()[0].attribution, Attribution::Unknown);
    }
}
