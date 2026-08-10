//! Noticing that an agent has stopped making progress.
//!
//! `docs/architecture/20-recall-and-usage.md` §10.
//!
//! `working` for twenty-two minutes is indistinguishable from `working` for
//! twenty seconds, except by elapsed time. Every signal needed is already
//! counted; none of this reads the screen, which matters because screen-scraping
//! an agent's own rendering breaks every time that agent changes its output.
//!
//! Two decisions shape the whole detector.
//!
//! **Every reason that fired is reported, never just the first.** An agent that
//! has run the same failing command seven times *and* has burned tokens without
//! touching a file has two different problems, and being told only about the
//! repetition sends somebody looking in the wrong place.
//!
//! **Baselines are per binding and learned from that binding's own history.**
//! A fixed threshold is wrong for everybody: thirty seconds is an eternity for
//! a linter and a blink for a test suite. Comparing an agent against itself is
//! the only comparison that means anything, and it is why a slow agent does not
//! generate a permanent alarm.

use std::collections::VecDeque;

use serde::Serialize;

/// How sure the detector is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// One signal. Worth showing quietly, not worth interrupting for.
    Weak,
    /// Several signals agree, or one of them is unambiguous.
    Strong,
}

/// Why the detector thinks something is wrong.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Reason {
    /// Working far beyond this binding's own baseline.
    LongWorking {
        /// How long it has been working, in seconds.
        elapsed_secs: u64,
        /// What is normal for this binding.
        baseline_secs: u64,
        /// How many times over.
        ratio: f64,
    },
    /// The same call, over and over.
    RepeatedCall {
        /// What is being called.
        call: String,
        /// How many times.
        count: u32,
    },
    /// Consecutive failures.
    RepeatedFailure {
        /// How many in a row.
        count: u32,
        /// The most recent one.
        last: String,
    },
    /// Nothing at all for longer than this binding's usual gap.
    Silent {
        /// How long, in seconds.
        since_secs: u64,
        /// What is normal for this binding.
        baseline_secs: u64,
    },
    /// Tokens climbing with nothing to show for it.
    BurnWithoutProgress {
        /// How many tokens since the last file changed.
        tokens: u64,
        /// How long, in seconds.
        since_secs: u64,
    },
}

/// What the detector concluded.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Signal {
    /// Everything that fired.
    pub reasons: Vec<Reason>,
    /// How sure.
    pub confidence: Confidence,
    /// A line somebody can read.
    pub detail: String,
}

/// Thresholds, all tunable.
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    /// How many times over its own baseline counts as long.
    pub long_working_ratio: f64,
    /// How many identical calls in the window count as a loop.
    pub repeat_count: u32,
    /// How many consecutive failures count.
    pub failure_count: u32,
    /// How many times its own gap counts as silent.
    pub silence_ratio: f64,
    /// How many tokens without a file change counts as burn.
    pub burn_tokens: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            // Three, not two. Everything takes twice as long as usual
            // sometimes, and a detector that fires on that is one people mute.
            long_working_ratio: 3.0,
            repeat_count: 4,
            failure_count: 3,
            silence_ratio: 4.0,
            burn_tokens: 50_000,
        }
    }
}

/// What is known about one agent binding right now.
#[derive(Debug, Clone, Default)]
pub struct Observation {
    /// How long it has been working, in seconds.
    pub working_secs: u64,
    /// The usual working time for this binding.
    pub baseline_working_secs: u64,
    /// How long since anything happened.
    pub silent_secs: u64,
    /// The usual gap between events for this binding.
    pub baseline_gap_secs: u64,
    /// The recent tool calls, canonicalised, most recent last.
    pub recent_calls: VecDeque<String>,
    /// How many results in a row have failed.
    pub consecutive_failures: u32,
    /// The most recent failure, in words.
    pub last_failure: Option<String>,
    /// Tokens spent since a file last changed.
    pub tokens_since_file_change: u64,
    /// How long since a file last changed.
    pub since_file_change_secs: u64,
}

/// Look at one binding and say what, if anything, is wrong.
///
/// Returns `None` when nothing fired. An empty signal and no signal are the
/// same thing to a caller, and returning one would have every client check the
/// list length before deciding whether to show anything.
#[must_use]
pub fn examine(observation: &Observation, thresholds: &Thresholds) -> Option<Signal> {
    let mut reasons = Vec::new();

    // A baseline of zero means this binding has never completed a turn, so
    // there is nothing to compare against. Silence there is not evidence — it
    // is the first turn.
    if observation.baseline_working_secs > 0 {
        #[allow(clippy::cast_precision_loss, reason = "durations are small")]
        let ratio = observation.working_secs as f64 / observation.baseline_working_secs as f64;
        if ratio >= thresholds.long_working_ratio {
            reasons.push(Reason::LongWorking {
                elapsed_secs: observation.working_secs,
                baseline_secs: observation.baseline_working_secs,
                ratio,
            });
        }
    }

    if observation.baseline_gap_secs > 0 {
        #[allow(clippy::cast_precision_loss, reason = "durations are small")]
        let ratio = observation.silent_secs as f64 / observation.baseline_gap_secs as f64;
        if ratio >= thresholds.silence_ratio {
            reasons.push(Reason::Silent {
                since_secs: observation.silent_secs,
                baseline_secs: observation.baseline_gap_secs,
            });
        }
    }

    if let Some((call, count)) = most_repeated(&observation.recent_calls)
        && count >= thresholds.repeat_count
    {
        reasons.push(Reason::RepeatedCall { call, count });
    }

    if observation.consecutive_failures >= thresholds.failure_count {
        reasons.push(Reason::RepeatedFailure {
            count: observation.consecutive_failures,
            last: observation
                .last_failure
                .clone()
                .unwrap_or_else(|| "an error".to_owned()),
        });
    }

    if observation.tokens_since_file_change >= thresholds.burn_tokens {
        reasons.push(Reason::BurnWithoutProgress {
            tokens: observation.tokens_since_file_change,
            since_secs: observation.since_file_change_secs,
        });
    }

    if reasons.is_empty() {
        return None;
    }

    // Repetition and consecutive failure are unambiguous on their own: an
    // agent that ran the same thing four times, or failed three times running,
    // is not having a slow day. The time-based ones are circumstantial, so one
    // of those alone stays weak.
    let unambiguous = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::RepeatedCall { .. } | Reason::RepeatedFailure { .. }
        )
    });
    let confidence = if unambiguous || reasons.len() > 1 {
        Confidence::Strong
    } else {
        Confidence::Weak
    };

    Some(Signal {
        detail: describe(&reasons),
        reasons,
        confidence,
    })
}

/// The most-repeated recent call, and how many times.
fn most_repeated(calls: &VecDeque<String>) -> Option<(String, u32)> {
    let mut counts: std::collections::BTreeMap<&String, u32> = std::collections::BTreeMap::new();
    for call in calls {
        *counts.entry(call).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(call, count)| (*count, std::cmp::Reverse((*call).clone())))
        .map(|(call, count)| (call.clone(), count))
}

/// One line somebody can act on.
fn describe(reasons: &[Reason]) -> String {
    let parts: Vec<String> = reasons
        .iter()
        .map(|r| match r {
            Reason::LongWorking { elapsed_secs, ratio, .. } => {
                format!("working {}m, {ratio:.0}× its usual", elapsed_secs / 60)
            }
            Reason::RepeatedCall { call, count } => format!("{call} ×{count}"),
            Reason::RepeatedFailure { count, last } => {
                format!("{count} failures in a row, last: {last}")
            }
            Reason::Silent { since_secs, .. } => format!("silent for {}m", since_secs / 60),
            Reason::BurnWithoutProgress { tokens, .. } => {
                format!("{tokens} tokens with no file changed")
            }
        })
        .collect();
    parts.join("; ")
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn calls(names: &[&str]) -> VecDeque<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    fn healthy() -> Observation {
        Observation {
            working_secs: 20,
            baseline_working_secs: 30,
            silent_secs: 2,
            baseline_gap_secs: 5,
            recent_calls: calls(&["Read(a.rs)", "Edit(a.rs)", "Bash(cargo test)"]),
            consecutive_failures: 0,
            last_failure: None,
            tokens_since_file_change: 1_000,
            since_file_change_secs: 30,
        }
    }

    #[test]
    fn a_working_agent_is_not_a_stuck_one() {
        // The detector has to be quiet almost always, or it is a detector
        // people mute and then never hear from again.
        assert!(examine(&healthy(), &Thresholds::default()).is_none());
    }

    #[test]
    fn twice_as_long_as_usual_is_not_an_alarm() {
        // Everything takes twice as long sometimes.
        let mut o = healthy();
        o.working_secs = 65;
        assert!(examine(&o, &Thresholds::default()).is_none());
    }

    #[test]
    fn far_beyond_its_own_baseline_is() {
        let mut o = healthy();
        o.working_secs = 22 * 60;
        let signal = examine(&o, &Thresholds::default()).expect("fires");
        assert!(matches!(signal.reasons[0], Reason::LongWorking { .. }));
    }

    #[test]
    fn the_baseline_is_the_bindings_own_so_a_slow_agent_is_not_a_permanent_alarm() {
        // Thirty seconds is an eternity for a linter and a blink for a test
        // suite. A fixed threshold is wrong for everybody.
        let mut slow = healthy();
        slow.baseline_working_secs = 20 * 60;
        slow.working_secs = 25 * 60;
        assert!(examine(&slow, &Thresholds::default()).is_none());
    }

    #[test]
    fn a_first_turn_with_no_history_is_not_evidence_of_anything() {
        // A baseline of zero means the binding has never completed a turn.
        let mut fresh = healthy();
        fresh.baseline_working_secs = 0;
        fresh.baseline_gap_secs = 0;
        fresh.working_secs = 60 * 60;
        fresh.silent_secs = 60 * 60;
        assert!(examine(&fresh, &Thresholds::default()).is_none());
    }

    #[test]
    fn the_same_call_over_and_over_is_a_loop() {
        let mut o = healthy();
        o.recent_calls = calls(&[
            "Bash(cargo test)",
            "Bash(cargo test)",
            "Bash(cargo test)",
            "Bash(cargo test)",
        ]);
        let signal = examine(&o, &Thresholds::default()).expect("fires");
        assert!(signal.detail.contains("×4"), "{}", signal.detail);
    }

    #[test]
    fn every_reason_that_fired_is_reported_not_just_the_first() {
        // An agent looping *and* burning tokens has two problems, and being
        // told about one sends somebody looking in the wrong place.
        let mut o = healthy();
        o.recent_calls = calls(&["Bash(x)", "Bash(x)", "Bash(x)", "Bash(x)"]);
        o.consecutive_failures = 5;
        o.last_failure = Some("exit 1".to_owned());
        o.tokens_since_file_change = 90_000;
        let signal = examine(&o, &Thresholds::default()).expect("fires");
        assert_eq!(signal.reasons.len(), 3, "{:?}", signal.reasons);
    }

    #[test]
    fn one_circumstantial_signal_is_weak_and_an_unambiguous_one_is_strong() {
        // An agent that failed three times running is not having a slow day.
        let mut slow = healthy();
        slow.working_secs = 22 * 60;
        assert_eq!(
            examine(&slow, &Thresholds::default()).expect("fires").confidence,
            Confidence::Weak
        );

        let mut failing = healthy();
        failing.consecutive_failures = 3;
        assert_eq!(
            examine(&failing, &Thresholds::default())
                .expect("fires")
                .confidence,
            Confidence::Strong
        );
    }

    #[test]
    fn two_circumstantial_signals_together_are_strong() {
        let mut o = healthy();
        o.working_secs = 22 * 60;
        o.silent_secs = 10 * 60;
        let signal = examine(&o, &Thresholds::default()).expect("fires");
        assert_eq!(signal.reasons.len(), 2);
        assert_eq!(signal.confidence, Confidence::Strong);
    }

    #[test]
    fn burning_tokens_with_nothing_to_show_is_reported() {
        let mut o = healthy();
        o.tokens_since_file_change = 120_000;
        o.since_file_change_secs = 15 * 60;
        let signal = examine(&o, &Thresholds::default()).expect("fires");
        assert!(signal.detail.contains("no file changed"), "{}", signal.detail);
    }

    #[test]
    fn nothing_wrong_returns_nothing_rather_than_an_empty_signal() {
        // An empty signal and no signal are the same thing, and returning one
        // would have every client check a list length before showing anything.
        assert!(examine(&healthy(), &Thresholds::default()).is_none());
    }

    #[test]
    fn the_detail_line_names_what_is_actually_happening() {
        let mut o = healthy();
        o.consecutive_failures = 4;
        o.last_failure = Some("cargo test: 3 failed".to_owned());
        let signal = examine(&o, &Thresholds::default()).expect("fires");
        assert!(signal.detail.contains("cargo test"), "{}", signal.detail);
    }

    #[test]
    fn the_thresholds_are_tunable_rather_than_baked_in() {
        // Somebody whose agent legitimately retries will turn this down, and
        // the alternative is that they turn the whole feature off.
        let mut o = healthy();
        o.recent_calls = calls(&["Bash(x)", "Bash(x)"]);
        assert!(examine(&o, &Thresholds::default()).is_none());
        let eager = Thresholds {
            repeat_count: 2,
            ..Thresholds::default()
        };
        assert!(examine(&o, &eager).is_some());
    }
}
