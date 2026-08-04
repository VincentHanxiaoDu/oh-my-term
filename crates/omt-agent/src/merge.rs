//! Merging sources by confidence, not by vote.
//!
//! Several sources watch one agent and they disagree. The resolution is not a
//! majority — a majority of guesses is still a guess. It is a ladder: the
//! highest tier that has spoken recently wins, and a lower tier may fill a gap
//! but may never contradict a live higher one.
//!
//! That single rule is what makes it safe to leave heuristics switched on. Without
//! it, a screen guess would eventually overwrite something a hook actually
//! observed, and nobody would be able to tell which reading they were looking
//! at.

use std::collections::BTreeMap;

use omt_types::{AgentState, Tier};

/// A monotonic millisecond clock reading.
///
/// Passed in rather than read here so the merge machine stays a pure function
/// of its inputs: freshness and debounce are the two things most likely to be
/// wrong, and neither is testable against a clock the test cannot move.
pub type Millis = u64;

/// How long a tier's word stands before it stops suppressing lower ones.
///
/// Per tier, because they fail differently. A hook that goes quiet for
/// twenty seconds is probably mid-tool-call and still correct; a screen
/// heuristic that goes quiet for twenty seconds knows nothing at all.
#[must_use]
pub const fn freshness_window(tier: Tier) -> Millis {
    match tier {
        Tier::Protocol | Tier::Hook => 30_000,
        Tier::Transcript => 15_000,
        Tier::Marker | Tier::Process => 10_000,
        Tier::Heuristic => 3_000,
    }
}

/// How long `Idle` must be agreed before it is believed, and how many times.
///
/// Only `Idle` is debounced. An agent between two tool calls looks idle for a
/// moment, and flickering "done" at someone watching from a phone is worse
/// than being a beat late. `Blocked` is deliberately *not* debounced: latency
/// there is the entire product.
const IDLE_DEBOUNCE_MS: Millis = 700;
const IDLE_CONFIRMATIONS: u32 = 3;

/// What one source last said.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceReading {
    /// Which tier it speaks at.
    pub tier: Tier,
    /// What it claimed.
    pub state: AgentState,
    /// When it claimed it.
    pub at: Millis,
    /// Whether this source reports the whole lifecycle rather than just
    /// identity. An authoritative source suspends the heuristic entirely —
    /// two sources of truth for one fact is a bug, not redundancy.
    pub authoritative: bool,
}

/// Why the merge machine decided what it did.
///
/// Carried rather than computed on demand, because a mis-detection nobody can
/// explain is a mis-detection nobody can report. This is what `agent.explain`
/// renders.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    /// The tier that won.
    pub winner: Tier,
    /// Every source considered, with what it said and whether it was fresh.
    pub considered: Vec<ConsideredSource>,
    /// A sentence a human can read.
    pub summary: String,
}

/// One source's part in a decision.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsideredSource {
    /// Its tier.
    pub tier: Tier,
    /// What it said.
    pub state: AgentState,
    /// How long ago.
    pub age_ms: Millis,
    /// Whether that is within its window.
    pub fresh: bool,
    /// Whether it was suspended by an authoritative higher tier.
    pub suspended: bool,
}

/// One binding's merged view.
#[derive(Debug, Clone, Default)]
pub struct MergeMachine {
    readings: BTreeMap<Tier, SourceReading>,
    state: Option<AgentState>,
    /// How many consecutive times a fresh source has said idle, and since when.
    idle_streak: u32,
    idle_since: Option<Millis>,
}

impl MergeMachine {
    /// A machine that has heard nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record what a source just said.
    pub fn observe(&mut self, reading: SourceReading) {
        self.readings.insert(reading.tier, reading);
    }

    /// The state as of now.
    ///
    /// Takes the time rather than reading a clock, so a test can advance past a
    /// freshness window without sleeping through it.
    pub fn state(&mut self, now: Millis) -> AgentState {
        let (winner, explanation) = self.decide(now);
        let _ = explanation;
        let resolved = self.apply_debounce(winner, now);
        self.state = Some(resolved.clone());
        resolved
    }

    /// The state, with the reasoning behind it.
    pub fn explain(&mut self, now: Millis) -> (AgentState, Explanation) {
        let (winner, explanation) = self.decide(now);
        let resolved = self.apply_debounce(winner, now);
        self.state = Some(resolved.clone());
        (resolved, explanation)
    }

    /// Whether an authoritative source is currently speaking.
    ///
    /// The heuristic source asks this before doing any work at all: when a hook
    /// is reporting the full lifecycle there is nothing for a screen guess to
    /// add, and everything for it to get wrong.
    #[must_use]
    pub fn heuristics_suspended(&self, now: Millis) -> bool {
        self.readings
            .values()
            .any(|r| r.authoritative && r.tier > Tier::Heuristic && is_fresh(r, now))
    }

    fn decide(&self, now: Millis) -> (AgentState, Explanation) {
        let suspend_heuristics = self.heuristics_suspended(now);

        let mut considered: Vec<ConsideredSource> = Vec::new();
        let mut winner: Option<&SourceReading> = None;

        // Highest tier first, so the first fresh one encountered is the answer
        // and everything below it is only recorded for the explanation.
        for reading in self.readings.values().rev() {
            let fresh = is_fresh(reading, now);
            let suspended = suspend_heuristics && reading.tier == Tier::Heuristic;
            considered.push(ConsideredSource {
                tier: reading.tier,
                state: reading.state.clone(),
                age_ms: now.saturating_sub(reading.at),
                fresh,
                suspended,
            });
            if fresh && !suspended && winner.is_none() {
                winner = Some(reading);
            }
        }

        let (state, summary) = match winner {
            Some(r) => (
                r.state.clone(),
                format!(
                    "{:?} won: it spoke {} ms ago, within its {} ms window",
                    r.tier,
                    now.saturating_sub(r.at),
                    freshness_window(r.tier)
                ),
            ),
            None if self.readings.is_empty() => (
                AgentState::Unknown,
                "no source has said anything yet".to_owned(),
            ),
            None => (
                AgentState::Unknown,
                "every source has gone silent past its freshness window".to_owned(),
            ),
        };

        (
            state.clone(),
            Explanation {
                winner: winner.map_or(Tier::Heuristic, |r| r.tier),
                considered,
                summary,
            },
        )
    }

    /// Hold back a transition to idle until it has been agreed for long enough.
    fn apply_debounce(&mut self, candidate: AgentState, now: Millis) -> AgentState {
        if !matches!(candidate, AgentState::Idle) {
            self.idle_streak = 0;
            self.idle_since = None;
            return candidate;
        }

        self.idle_streak += 1;
        let since = *self.idle_since.get_or_insert(now);
        let long_enough = now.saturating_sub(since) >= IDLE_DEBOUNCE_MS;
        let agreed_enough = self.idle_streak >= IDLE_CONFIRMATIONS;

        if long_enough && agreed_enough {
            return AgentState::Idle;
        }

        // Not yet convinced. Keep showing what was there, rather than
        // flickering "done" at somebody watching from a phone.
        match &self.state {
            Some(prior) if !matches!(prior, AgentState::Idle) => prior.clone(),
            Some(prior) => prior.clone(),
            // Nothing was there before, so there is nothing to hold — an agent
            // that starts idle is idle.
            None => AgentState::Idle,
        }
    }
}

fn is_fresh(reading: &SourceReading, now: Millis) -> bool {
    now.saturating_sub(reading.at) <= freshness_window(reading.tier)
}

/// How long an agent may say nothing before it is called wedged.
///
/// Well past the slowest tier's freshness window, because "every source went
/// quiet" is normal for a few seconds and alarming after a minute. Under it,
/// silence is a gap; over it, with the process still alive, it is a state.
pub const WEDGED_AFTER_MS: Millis = 90_000;

/// What omt can say about an agent that has gone quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Something has spoken recently.
    Live,
    /// Every source is stale, but not for long enough to be alarming.
    Quiet {
        /// How long since anything was heard.
        for_ms: Millis,
    },
    /// Nothing has spoken for a long time and the process is still there.
    ///
    /// The state `Unknown` cannot express. `Unknown` is honest but not
    /// actionable — it reads as "omt lost track", when what the user needs to
    /// know is "this agent is alive and has done nothing for two minutes",
    /// which is a thing they can interrupt.
    Wedged {
        /// How long since anything was heard.
        for_ms: Millis,
    },
    /// The process is gone, which is not a hang.
    Exited,
}

impl Liveness {
    /// Whether the user should be told.
    #[must_use]
    pub const fn needs_reporting(self) -> bool {
        matches!(self, Self::Wedged { .. })
    }
}

impl MergeMachine {
    /// Whether an agent has stopped saying anything while still running.
    ///
    /// Takes whether the process is alive, because that is the difference
    /// between a hang and an exit and this machine cannot see it: an agent that
    /// left is not wedged, and reporting it as such would have people
    /// interrupting a process that is not there.
    #[must_use]
    pub fn liveness(&self, now: Millis, process_alive: bool) -> Liveness {
        if !process_alive {
            return Liveness::Exited;
        }
        let newest = self.readings.values().map(|r| r.at).max();
        let Some(newest) = newest else {
            // Nothing has ever spoken. That is a starting agent, not a wedged
            // one, and calling it wedged would fire on every launch.
            return Liveness::Live;
        };
        let quiet_for = now.saturating_sub(newest);

        // Anything still inside its own window counts as live: a hook that
        // spoke twenty seconds ago is mid-tool-call, not silent.
        if self.readings.values().any(|r| is_fresh(r, now)) {
            return Liveness::Live;
        }
        if quiet_for >= WEDGED_AFTER_MS {
            Liveness::Wedged { for_ms: quiet_for }
        } else {
            Liveness::Quiet { for_ms: quiet_for }
        }
    }
}

/// Whether a state means a human is needed.
#[must_use]
pub const fn needs_human(state: &AgentState) -> bool {
    matches!(state, AgentState::Blocked { .. })
}

/// Whether a blocked state can actually be answered from a phone.
///
/// `Blocked` without an interaction means omt can see that something needs a
/// human but cannot render what. The surface says so and routes to the
/// terminal — an explicit degradation rather than an answering affordance that
/// does the wrong thing.
#[must_use]
pub const fn is_answerable(state: &AgentState) -> bool {
    matches!(
        state,
        AgentState::Blocked {
            interaction: Some(_),
            ..
        }
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use omt_types::{BlockReason, InteractionId};

    fn reading(tier: Tier, state: AgentState, at: Millis) -> SourceReading {
        SourceReading {
            tier,
            state,
            at,
            authoritative: false,
        }
    }

    fn working() -> AgentState {
        AgentState::Working { detail: None }
    }

    fn blocked() -> AgentState {
        AgentState::Blocked {
            reason: BlockReason::Permission,
            interaction: Some(InteractionId::new()),
        }
    }

    /// Settle an idle claim through the debounce, so tests about *other*
    /// things do not have to model it.
    fn settle_idle(m: &mut MergeMachine, tier: Tier, from: Millis) -> AgentState {
        let mut now = from;
        let mut last = AgentState::Unknown;
        for _ in 0..4 {
            m.observe(reading(tier, AgentState::Idle, now));
            last = m.state(now);
            now += 400;
        }
        last
    }

    #[test]
    fn the_highest_fresh_tier_wins() {
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Heuristic, AgentState::Idle, 1_000));
        m.observe(reading(Tier::Hook, working(), 1_000));
        assert_eq!(m.state(1_000), working());
    }

    #[test]
    fn a_lower_tier_may_not_contradict_a_live_higher_one() {
        // The rule that makes heuristics safe to leave enabled. A screen guess
        // arriving later must not overwrite what a hook observed.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 1_000));
        m.observe(reading(Tier::Heuristic, AgentState::Idle, 2_000));
        assert_eq!(
            m.state(2_000),
            working(),
            "the newer but weaker reading lost"
        );
    }

    #[test]
    fn a_lower_tier_may_fill_a_gap_the_higher_one_left() {
        // Filling a gap is allowed; contradicting is not. Once the hook is past
        // its window it stops suppressing anything.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 1_000));
        let late = 1_000 + freshness_window(Tier::Hook) + 1;
        m.observe(reading(Tier::Heuristic, blocked(), late));
        assert!(
            matches!(m.state(late), AgentState::Blocked { .. }),
            "the stale hook no longer suppresses"
        );
    }

    #[test]
    fn a_stale_source_stops_speaking_for_the_agent() {
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 0));
        let much_later = freshness_window(Tier::Hook) + 5_000;
        assert_eq!(
            m.state(much_later),
            AgentState::Unknown,
            "silence past the window is not a claim that nothing changed"
        );
    }

    #[test]
    fn freshness_windows_differ_by_how_a_tier_fails() {
        // A hook quiet for twenty seconds is mid-tool-call. A screen guess
        // quiet for twenty seconds knows nothing.
        assert!(freshness_window(Tier::Hook) > freshness_window(Tier::Transcript));
        assert!(freshness_window(Tier::Transcript) > freshness_window(Tier::Heuristic));
    }

    #[test]
    fn an_authoritative_source_suspends_the_heuristic_entirely() {
        // Two sources of truth for one fact is a bug, not redundancy.
        let mut m = MergeMachine::new();
        m.observe(SourceReading {
            authoritative: true,
            ..reading(Tier::Hook, working(), 1_000)
        });
        assert!(m.heuristics_suspended(1_000));

        m.observe(reading(Tier::Heuristic, blocked(), 1_000));
        assert_eq!(m.state(1_000), working());
        let (_, why) = m.explain(1_000);
        assert!(
            why.considered
                .iter()
                .any(|c| c.tier == Tier::Heuristic && c.suspended),
            "and the explanation says it was suspended: {why:?}"
        );
    }

    #[test]
    fn a_non_authoritative_hook_does_not_suspend_anything() {
        // A source reporting only session identity has no business silencing
        // the one watching for activity.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Marker, AgentState::Starting, 1_000));
        assert!(!m.heuristics_suspended(1_000));
    }

    #[test]
    fn a_block_is_never_debounced_because_latency_there_is_the_product() {
        let mut m = MergeMachine::new();
        settle_idle(&mut m, Tier::Hook, 0);
        m.observe(reading(Tier::Hook, blocked(), 5_000));
        assert!(
            matches!(m.state(5_000), AgentState::Blocked { .. }),
            "a human is needed now, not in 700 ms"
        );
    }

    #[test]
    fn idle_is_debounced_so_a_gap_between_tools_does_not_flicker() {
        // An agent between two tool calls looks idle for a moment. Showing
        // "done" and taking it back is worse than being a beat late.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 0));
        assert_eq!(m.state(0), working());

        m.observe(reading(Tier::Hook, AgentState::Idle, 100));
        assert_eq!(m.state(100), working(), "one sighting is not enough");
        m.observe(reading(Tier::Hook, AgentState::Idle, 300));
        assert_eq!(m.state(300), working(), "nor is it enough soon enough");
    }

    #[test]
    fn idle_is_believed_once_it_has_been_agreed_long_enough() {
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 0));
        m.state(0);
        let settled = settle_idle(&mut m, Tier::Hook, 100);
        assert_eq!(settled, AgentState::Idle);
    }

    #[test]
    fn work_resuming_mid_debounce_cancels_it() {
        // Otherwise a burst of tool calls would eventually be reported idle
        // just because the idle sightings accumulated.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 0));
        m.state(0);
        m.observe(reading(Tier::Hook, AgentState::Idle, 100));
        m.state(100);
        m.observe(reading(Tier::Hook, working(), 200));
        assert_eq!(m.state(200), working());
        m.observe(reading(Tier::Hook, AgentState::Idle, 1_500));
        assert_eq!(
            m.state(1_500),
            working(),
            "the streak restarted rather than carrying over"
        );
    }

    #[test]
    fn an_agent_that_starts_idle_is_idle() {
        // With nothing prior to hold, the debounce has nothing to protect.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, AgentState::Idle, 0));
        assert_eq!(m.state(0), AgentState::Idle);
    }

    #[test]
    fn nothing_heard_is_unknown_rather_than_idle() {
        // Idle is a claim. Reporting one from silence would show "waiting for
        // you" about an agent nobody has looked at.
        let mut m = MergeMachine::new();
        assert_eq!(m.state(0), AgentState::Unknown);
    }

    #[test]
    fn the_explanation_names_the_winner_and_everything_it_beat() {
        // Without this a mis-detection is unfalsifiable and every bug report
        // about it is useless.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Heuristic, AgentState::Idle, 1_000));
        m.observe(reading(Tier::Process, AgentState::Starting, 1_000));
        m.observe(reading(Tier::Hook, working(), 1_000));
        let (state, why) = m.explain(1_000);
        assert_eq!(state, working());
        assert_eq!(why.winner, Tier::Hook);
        assert_eq!(why.considered.len(), 3, "every source is accounted for");
        assert!(why.summary.contains("Hook"), "{}", why.summary);
        assert!(
            why.considered.iter().all(|c| c.fresh),
            "and each one's freshness is stated"
        );
    }

    #[test]
    fn the_explanation_reports_a_stale_source_as_stale() {
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 0));
        let late = freshness_window(Tier::Hook) + 1;
        let (_, why) = m.explain(late);
        let hook = why
            .considered
            .iter()
            .find(|c| c.tier == Tier::Hook)
            .expect("hook considered");
        assert!(!hook.fresh);
        assert!(hook.age_ms > freshness_window(Tier::Hook));
    }

    #[test]
    fn an_agent_that_has_gone_quiet_for_a_long_time_is_wedged_not_unknown() {
        // `Unknown` is honest but not actionable — it reads as "omt lost
        // track", when what the user needs is "alive, and has done nothing for
        // two minutes", which is a thing they can interrupt.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 0));
        let long_after = WEDGED_AFTER_MS + 1_000;
        assert_eq!(m.state(long_after), AgentState::Unknown);
        assert!(matches!(
            m.liveness(long_after, true),
            Liveness::Wedged { .. }
        ));
    }

    #[test]
    fn a_brief_silence_is_not_a_hang() {
        // Every source going quiet for a few seconds is normal.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 0));
        let a_little_after = freshness_window(Tier::Hook) + 1_000;
        assert!(matches!(
            m.liveness(a_little_after, true),
            Liveness::Quiet { .. }
        ));
        assert!(!m.liveness(a_little_after, true).needs_reporting());
    }

    #[test]
    fn a_source_still_inside_its_window_is_live() {
        // A hook that spoke twenty seconds ago is mid-tool-call, not silent.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 0));
        assert_eq!(m.liveness(20_000, true), Liveness::Live);
    }

    #[test]
    fn an_agent_that_exited_is_not_wedged() {
        // Reporting it as such would have people interrupting a process that
        // is not there.
        let mut m = MergeMachine::new();
        m.observe(reading(Tier::Hook, working(), 0));
        assert_eq!(m.liveness(WEDGED_AFTER_MS + 5_000, false), Liveness::Exited);
    }

    #[test]
    fn an_agent_that_has_never_spoken_is_starting_not_wedged() {
        // Otherwise this fires on every launch.
        let m = MergeMachine::new();
        assert_eq!(m.liveness(999_999, true), Liveness::Live);
    }

    #[test]
    fn only_a_wedge_is_worth_telling_the_user_about() {
        assert!(Liveness::Wedged { for_ms: 100_000 }.needs_reporting());
        assert!(!Liveness::Quiet { for_ms: 5_000 }.needs_reporting());
        assert!(!Liveness::Live.needs_reporting());
        assert!(!Liveness::Exited.needs_reporting());
    }

    #[test]
    fn a_block_without_an_interaction_is_not_answerable() {
        // omt can see something needs a human but cannot render what. The
        // surface must say so rather than offer a button that guesses.
        let seen = AgentState::Blocked {
            reason: BlockReason::Unspecified,
            interaction: None,
        };
        assert!(needs_human(&seen));
        assert!(!is_answerable(&seen), "no affordance for this one");
        assert!(is_answerable(&blocked()));
    }

    #[test]
    fn working_does_not_need_a_human() {
        assert!(!needs_human(&working()));
        assert!(!needs_human(&AgentState::Idle));
    }
}
