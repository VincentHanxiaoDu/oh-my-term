//! Putting parallel agent runs side by side.
//!
//! `docs/architecture/20-recall-and-usage.md` §9. Three worktrees, three
//! agents, one task, and a decision to make at the end of it.
//!
//! > **D-R5 — omt presents comparable facts side by side. It does not rank,
//! > score, recommend, or judge which result is better.**
//!
//! That is not modesty. Judging requires understanding the code, which requires
//! a model, and a plausible-looking wrong recommendation about which branch to
//! keep is exactly the failure that costs somebody real work. A comparison that
//! says "this one looks best" will be believed, and when it is wrong nobody
//! finds out until the other two worktrees are gone.
//!
//! So everything here is a fact with a source. The one piece of judgement the
//! module does allow itself is pointing at *disagreements* — a file git says
//! changed that the agent never mentioned, or the reverse — because a
//! disagreement is not an opinion about quality. It is a place where two
//! records of the same run differ, and that is worth a human's attention
//! regardless of which run they pick.

use std::collections::BTreeSet;

use serde::Serialize;

/// What one run did, as facts.
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// Which session.
    pub session: String,
    /// Its branch, where it had one.
    pub branch: Option<String>,
    /// The resolved base it is measured against.
    ///
    /// Displayed rather than assumed, because a diffstat against the wrong
    /// base is a number that looks authoritative and is meaningless — and
    /// nothing on screen would say which base it used.
    pub base_ref: Option<String>,
    /// Which agent.
    pub agent: Option<String>,
    /// How long it took, in seconds.
    pub elapsed_secs: u64,
    /// Files git reports changed.
    pub files_from_git: Vec<String>,
    /// Files the agent said it changed.
    pub files_from_agent: Vec<String>,
    /// Lines added, as git counts them.
    pub insertions: u32,
    /// Lines removed.
    pub deletions: u32,
    /// How many turns it took.
    pub turns: u32,
    /// How many times it stopped to ask.
    pub interactions: u32,
    /// How many of those nobody answered.
    pub interactions_timed_out: u32,
    /// How many commands exited non-zero.
    pub failed_blocks: u32,
    /// Tokens, as the agent reported them.
    ///
    /// Agent-reported only. omt does not count tokens itself, and a number
    /// omt invented would be compared against one an agent measured.
    pub tokens: Option<u64>,
    /// The agent's own summary, verbatim.
    pub summary: Option<String>,
}

/// One row of the comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    /// Which session.
    pub session: String,
    /// Its branch.
    pub branch: Option<String>,
    /// What it was measured against.
    pub base_ref: Option<String>,
    /// Which agent.
    pub agent: Option<String>,
    /// How long, in seconds.
    pub elapsed_secs: u64,
    /// How many files it touched, from both sources combined.
    pub files_touched: Vec<String>,
    /// Files the two records disagree about.
    pub files_disagreement: Vec<String>,
    /// Lines added.
    pub insertions: u32,
    /// Lines removed.
    pub deletions: u32,
    /// Turns.
    pub turns: u32,
    /// Times it stopped to ask.
    pub interactions: u32,
    /// How many of those went unanswered.
    pub interactions_timed_out: u32,
    /// Commands that exited non-zero.
    pub failed_blocks: u32,
    /// Tokens, as the agent reported them.
    pub tokens: Option<u64>,
    /// The agent's own summary, verbatim.
    pub summary: Option<String>,
}

/// A comparison of several runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Comparison {
    /// One row per run, in the order given.
    pub rows: Vec<Row>,
    /// Facts that are not comparable, and why.
    ///
    /// A comparison with a silently missing column reads as "these are the
    /// same" when it means "omt could not tell".
    pub caveats: Vec<String>,
}

/// Build a comparison. Presents; never judges.
#[must_use]
pub fn compare(runs: &[Run]) -> Comparison {
    let rows: Vec<Row> = runs
        .iter()
        .map(|run| {
            let git: BTreeSet<&String> = run.files_from_git.iter().collect();
            let agent: BTreeSet<&String> = run.files_from_agent.iter().collect();
            Row {
                session: run.session.clone(),
                branch: run.branch.clone(),
                base_ref: run.base_ref.clone(),
                agent: run.agent.clone(),
                elapsed_secs: run.elapsed_secs,
                files_touched: git.union(&agent).map(|s| (*s).clone()).collect(),
                // Symmetric difference, so it catches both directions: a file
                // git saw and the agent never mentioned, and a file the agent
                // claims it wrote that is not in the diff. The second is the
                // more alarming and the easier to miss.
                files_disagreement: git
                    .symmetric_difference(&agent)
                    .map(|s| (*s).clone())
                    .collect(),
                insertions: run.insertions,
                deletions: run.deletions,
                turns: run.turns,
                interactions: run.interactions,
                interactions_timed_out: run.interactions_timed_out,
                failed_blocks: run.failed_blocks,
                tokens: run.tokens,
                summary: run.summary.clone(),
            }
        })
        .collect();

    let mut caveats = Vec::new();

    // Different bases make the diffstats incomparable, and nothing on screen
    // would say so. This is the caveat most worth having: two numbers in the
    // same column that were measured against different things.
    let bases: BTreeSet<&Option<String>> = runs.iter().map(|r| &r.base_ref).collect();
    if bases.len() > 1 {
        caveats.push(
            "These runs were measured against different base refs, so the diff numbers are not comparable."
                .to_owned(),
        );
    }
    if runs.iter().any(|r| r.base_ref.is_none()) {
        caveats.push(
            "At least one run has no resolved base ref, so its diff numbers are against an unknown starting point."
                .to_owned(),
        );
    }
    if runs.iter().any(|r| r.tokens.is_none()) {
        caveats.push(
            "At least one agent did not report usage. Blank means unreported, not zero.".to_owned(),
        );
    }
    if rows.iter().any(|r| !r.files_disagreement.is_empty()) {
        caveats.push(
            "Some runs' git diff and agent-reported file lists disagree. That is worth looking at whichever run you pick."
                .to_owned(),
        );
    }

    Comparison { rows, caveats }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn run(session: &str) -> Run {
        Run {
            session: session.to_owned(),
            branch: Some(format!("work/{session}")),
            base_ref: Some("abc123".to_owned()),
            agent: Some("claude_code".to_owned()),
            elapsed_secs: 600,
            files_from_git: vec!["src/a.rs".to_owned()],
            files_from_agent: vec!["src/a.rs".to_owned()],
            insertions: 40,
            deletions: 10,
            turns: 8,
            interactions: 1,
            interactions_timed_out: 0,
            failed_blocks: 0,
            tokens: Some(120_000),
            summary: Some("Added the parser".to_owned()),
        }
    }

    #[test]
    fn every_run_becomes_a_row_in_the_order_given() {
        let c = compare(&[run("a"), run("b"), run("c")]);
        let sessions: Vec<&str> = c.rows.iter().map(|r| r.session.as_str()).collect();
        assert_eq!(sessions, ["a", "b", "c"]);
    }

    #[test]
    fn nothing_is_ranked_scored_or_recommended() {
        // D-R5. A comparison that says "this one looks best" will be believed,
        // and when it is wrong nobody finds out until the other two worktrees
        // are gone.
        let c = compare(&[run("a"), run("b")]);
        let json = serde_json::to_string(&c).expect("json");
        for word in ["score", "rank", "best", "recommend", "winner"] {
            assert!(!json.contains(word), "`{word}` appears in {json}");
        }
    }

    #[test]
    fn a_file_git_saw_and_the_agent_did_not_is_flagged() {
        let mut r = run("a");
        r.files_from_git.push("src/secret.rs".to_owned());
        let c = compare(&[r]);
        assert_eq!(c.rows[0].files_disagreement, ["src/secret.rs"]);
    }

    #[test]
    fn a_file_the_agent_claims_and_git_did_not_see_is_flagged_too() {
        // The more alarming direction and the easier to miss: the agent
        // believes it wrote something that is not in the diff.
        let mut r = run("a");
        r.files_from_agent.push("src/imagined.rs".to_owned());
        let c = compare(&[r]);
        assert_eq!(c.rows[0].files_disagreement, ["src/imagined.rs"]);
    }

    #[test]
    fn a_disagreement_earns_a_caveat_rather_than_sitting_in_a_column() {
        let mut r = run("a");
        r.files_from_agent.push("src/imagined.rs".to_owned());
        let c = compare(&[r]);
        assert!(
            c.caveats.iter().any(|x| x.contains("disagree")),
            "{:?}",
            c.caveats
        );
    }

    #[test]
    fn files_touched_is_the_union_of_both_records() {
        let mut r = run("a");
        r.files_from_git = vec!["one.rs".to_owned()];
        r.files_from_agent = vec!["two.rs".to_owned()];
        let c = compare(&[r]);
        assert_eq!(c.rows[0].files_touched, ["one.rs", "two.rs"]);
    }

    #[test]
    fn different_bases_make_the_numbers_incomparable_and_it_says_so() {
        // Two numbers in the same column measured against different things,
        // with nothing on screen to say so, is the caveat most worth having.
        let mut b = run("b");
        b.base_ref = Some("def456".to_owned());
        let c = compare(&[run("a"), b]);
        assert!(
            c.caveats.iter().any(|x| x.contains("different base")),
            "{:?}",
            c.caveats
        );
    }

    #[test]
    fn the_same_base_produces_no_such_caveat() {
        let c = compare(&[run("a"), run("b")]);
        assert!(!c.caveats.iter().any(|x| x.contains("different base")));
    }

    #[test]
    fn an_unreported_usage_is_called_unreported_not_zero() {
        // Blank meaning zero would have an agent that reports nothing look
        // like the cheapest one.
        let mut b = run("b");
        b.tokens = None;
        let c = compare(&[run("a"), b]);
        assert!(c.rows[1].tokens.is_none());
        assert!(
            c.caveats.iter().any(|x| x.contains("not zero")),
            "{:?}",
            c.caveats
        );
    }

    #[test]
    fn the_agents_own_summary_is_carried_verbatim() {
        // Rewriting it would be omt paraphrasing a claim it cannot check.
        let mut r = run("a");
        r.summary = Some("I refactored the lexer but the tests still fail".to_owned());
        let c = compare(&[r]);
        assert_eq!(
            c.rows[0].summary.as_deref(),
            Some("I refactored the lexer but the tests still fail")
        );
    }

    #[test]
    fn comparing_nothing_is_an_empty_comparison_rather_than_a_panic() {
        let c = compare(&[]);
        assert!(c.rows.is_empty());
        assert!(c.caveats.is_empty());
    }

    #[test]
    fn a_run_with_no_base_ref_says_its_numbers_are_against_an_unknown_start() {
        let mut r = run("a");
        r.base_ref = None;
        let c = compare(&[r]);
        assert!(
            c.caveats
                .iter()
                .any(|x| x.contains("unknown starting point")),
            "{:?}",
            c.caveats
        );
    }
}
