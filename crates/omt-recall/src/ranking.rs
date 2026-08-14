//! Why one search result beat another.
//!
//! `docs/architecture/20-recall-and-usage.md` §4. A transparent weighted
//! product — no learning, no hidden state, and every factor printable.
//!
//! Transparency is the requirement, not a nicety. Search over somebody's own
//! terminal history is a place where a wrong answer is indistinguishable from
//! a right one: the result looks plausible, they act on it, and they find out
//! later. A ranking that cannot explain itself gives them no way to notice it
//! is wrong, and no way to fix it — the only remedy for an opaque ranker is to
//! stop trusting search, which is the same as not having it.
//!
//! A product rather than a sum, so a factor of zero is a veto and a factor of
//! one is an abstention. With a sum, "this failed" would still be outranked by
//! enough text overlap, and the failed command somebody is looking for would
//! sit below three successful ones that merely mention the word.

use serde::Serialize;

/// What kind of thing a result is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A command somebody ran.
    Command,
    /// Output from a command.
    Output,
    /// Something an agent said.
    AgentMessage,
    /// A file omt saw change.
    FileChange,
}

/// One candidate, with the facts ranking is computed from.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Its id.
    pub id: String,
    /// What kind of thing it is.
    pub kind: Kind,
    /// FTS5's `bm25`, which is negative and more-negative-is-better.
    pub bm25: f64,
    /// How many seconds ago.
    pub age_secs: u64,
    /// Whether it is in the workspace the search came from.
    pub in_scope: bool,
    /// The exit code, where there was one.
    pub exit: Option<i32>,
    /// Whether its session is still running.
    pub session_active: bool,
}

/// One factor's contribution, so a ranking can be read.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Factor {
    /// What it is called.
    pub name: String,
    /// What it multiplied by.
    pub value: f64,
    /// Why, in words.
    pub because: String,
}

/// A scored candidate and the arithmetic behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scored {
    /// Which candidate.
    pub id: String,
    /// The product of every factor.
    pub score: f64,
    /// Each factor, in the order they were applied.
    pub factors: Vec<Factor>,
}

/// How long it takes for recency to stop mattering, in seconds.
///
/// A week. Shorter and yesterday's command is already buried; longer and a
/// month-old result competes with one from this morning, which is not what
/// anybody means when they search their own history.
const RECENCY_HALF_LIFE: f64 = 7.0 * 24.0 * 60.0 * 60.0;

/// Normalise bm25 into `[1, 2]`.
///
/// FTS5 returns a negative number and more negative is better, which is a
/// footgun every caller hits once. Clamping into a bounded range also stops one
/// enormous text match from swamping every other factor — a result that matches
/// the words but failed, in another workspace, a month ago, should not win.
fn text_component(bm25: f64) -> f64 {
    1.0 + (-bm25).clamp(0.0, 20.0) / 20.0
}

/// Score one candidate, and record why.
#[must_use]
pub fn score(candidate: &Candidate) -> Scored {
    #[allow(clippy::cast_precision_loss, reason = "ages are small enough")]
    let age = candidate.age_secs as f64;

    let mut factors = Vec::new();
    let mut push = |name: &str, value: f64, because: String| {
        factors.push(Factor {
            name: name.to_owned(),
            value,
            because,
        });
        value
    };

    let text = push(
        "text",
        text_component(candidate.bm25),
        format!("bm25 {:.1}", candidate.bm25),
    );
    let recency = push(
        "recency",
        0.5f64.powf(age / RECENCY_HALF_LIFE).max(0.25),
        format!("{} days old", (age / 86_400.0).round() as i64),
    );
    let proximity = push(
        "proximity",
        if candidate.in_scope { 1.5 } else { 1.0 },
        if candidate.in_scope {
            "in this workspace".to_owned()
        } else {
            "another workspace".to_owned()
        },
    );
    let status = push(
        "status",
        match candidate.exit {
            Some(0) => 1.2,
            // A failure is not demoted much. Somebody searching their history
            // is often looking for the command that broke, and burying
            // failures would hide exactly that.
            Some(_) => 0.9,
            None => 1.0,
        },
        match candidate.exit {
            Some(0) => "succeeded".to_owned(),
            Some(code) => format!("exited {code}"),
            None => "no exit status".to_owned(),
        },
    );
    let activity = push(
        "session",
        if candidate.session_active { 1.3 } else { 1.0 },
        if candidate.session_active {
            "session still running".to_owned()
        } else {
            "session ended".to_owned()
        },
    );
    let kind = push(
        "kind",
        match candidate.kind {
            Kind::Command => 1.4,
            Kind::AgentMessage => 1.1,
            Kind::FileChange => 1.0,
            // Output is the bulk of what is indexed and the least often what
            // somebody meant. Weighted down rather than excluded, because when
            // it is what they meant, nothing else will do.
            Kind::Output => 0.8,
        },
        format!("{:?}", candidate.kind).to_lowercase(),
    );

    Scored {
        id: candidate.id.clone(),
        score: text * recency * proximity * status * activity * kind,
        factors,
    }
}

/// Score and order a set of candidates, best first.
#[must_use]
pub fn rank(candidates: &[Candidate]) -> Vec<Scored> {
    let mut out: Vec<Scored> = candidates.iter().map(score).collect();
    // Ties broken by id, so the order is total. An unstable order makes the
    // same query return different results on consecutive runs, which reads as
    // the index being broken.
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    out
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn candidate(id: &str) -> Candidate {
        Candidate {
            id: id.to_owned(),
            kind: Kind::Command,
            bm25: -10.0,
            age_secs: 0,
            in_scope: true,
            exit: Some(0),
            session_active: false,
        }
    }

    #[test]
    fn every_factor_is_reported_with_a_reason() {
        // The only remedy for a ranker that cannot explain itself is to stop
        // trusting search, which is the same as not having it.
        let s = score(&candidate("a"));
        assert_eq!(s.factors.len(), 6);
        for factor in &s.factors {
            assert!(!factor.because.is_empty(), "{}", factor.name);
        }
    }

    #[test]
    fn the_reported_factors_multiply_to_the_reported_score() {
        // A printed explanation that does not reconstruct the number is worse
        // than none: it looks like an audit and is not one.
        let s = score(&candidate("a"));
        let product: f64 = s.factors.iter().map(|f| f.value).product();
        assert!((product - s.score).abs() < 1e-9, "{product} vs {}", s.score);
    }

    #[test]
    fn something_more_recent_wins_all_else_equal() {
        let mut old = candidate("old");
        old.age_secs = 30 * 86_400;
        let ranked = rank(&[old, candidate("new")]);
        assert_eq!(ranked[0].id, "new");
    }

    #[test]
    fn recency_does_not_bury_an_old_result_entirely() {
        // A floor, because the thing somebody half-remembers is often months
        // old, and a factor that decays to nothing makes it unfindable however
        // well the text matches.
        let mut ancient = candidate("ancient");
        ancient.age_secs = 5 * 365 * 86_400;
        let s = score(&ancient);
        let recency = s
            .factors
            .iter()
            .find(|f| f.name == "recency")
            .expect("recency");
        assert!(recency.value >= 0.25, "{}", recency.value);
    }

    #[test]
    fn this_workspace_beats_another_one() {
        let mut elsewhere = candidate("elsewhere");
        elsewhere.in_scope = false;
        assert_eq!(rank(&[elsewhere, candidate("here")])[0].id, "here");
    }

    #[test]
    fn a_failure_is_demoted_gently_rather_than_buried() {
        // Somebody searching their history is often looking for the command
        // that broke.
        let mut failed = candidate("failed");
        failed.exit = Some(1);
        let s = score(&failed);
        let status = s
            .factors
            .iter()
            .find(|f| f.name == "status")
            .expect("status");
        assert!(status.value > 0.5, "{}", status.value);
        assert!(status.value < 1.2);
    }

    #[test]
    fn a_command_outranks_the_output_it_produced() {
        // Output is the bulk of what is indexed and the least often what
        // somebody meant.
        let mut output = candidate("output");
        output.kind = Kind::Output;
        assert_eq!(rank(&[output, candidate("command")])[0].id, "command");
    }

    #[test]
    fn output_is_weighted_down_rather_than_excluded() {
        // When it is what they meant, nothing else will do.
        let mut output = candidate("output");
        output.kind = Kind::Output;
        assert!(score(&output).score > 0.0);
    }

    #[test]
    fn a_huge_text_match_cannot_swamp_every_other_factor() {
        // A result that matches the words but failed, in another workspace, a
        // month ago, should not win.
        let mut wordy = candidate("wordy");
        wordy.bm25 = -10_000.0;
        wordy.in_scope = false;
        wordy.exit = Some(1);
        wordy.age_secs = 30 * 86_400;
        wordy.kind = Kind::Output;
        let ranked = rank(&[wordy, candidate("nearby")]);
        assert_eq!(ranked[0].id, "nearby");
    }

    #[test]
    fn the_order_is_total_so_the_same_query_gives_the_same_answer() {
        // An unstable order makes consecutive runs disagree, which reads as
        // the index being broken.
        let candidates = vec![candidate("b"), candidate("a"), candidate("c")];
        let once: Vec<String> = rank(&candidates).into_iter().map(|s| s.id).collect();
        for _ in 0..5 {
            let again: Vec<String> = rank(&candidates).into_iter().map(|s| s.id).collect();
            assert_eq!(again, once);
        }
        assert_eq!(once, ["a", "b", "c"]);
    }

    #[test]
    fn nothing_scores_zero_by_accident() {
        // A product means one zero is a veto. Nothing here is allowed to be a
        // veto, because every factor is a preference rather than a rule.
        let mut awkward = candidate("awkward");
        awkward.bm25 = 0.0;
        awkward.age_secs = u64::MAX / 2;
        awkward.in_scope = false;
        awkward.exit = Some(-1);
        awkward.kind = Kind::Output;
        assert!(score(&awkward).score > 0.0);
    }
}
