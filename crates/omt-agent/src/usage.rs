//! What a session has spent, as the agent reported it.
//!
//! **omt never computes a price.** Prices change, plans differ, and a wrong
//! number on a screen is worse than no number — somebody makes a decision on
//! it. A cost appears here only when the agent itself stated one.

use omt_events::AgentPayload;

/// What one session has spent.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    /// Input tokens.
    pub input: u64,
    /// Output tokens.
    pub output: u64,
    /// Tokens read from cache.
    pub cache_read: u64,
    /// Tokens written to cache.
    pub cache_write: u64,
    /// Cost, only where the agent itself stated one.
    pub cost_usd: Option<f64>,
    /// How many reports this is the sum of.
    pub reports: u32,
}

impl Usage {
    /// Every token, however it was counted.
    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }

    /// Fold in one report.
    ///
    /// Agents report cumulatively or incrementally depending on the agent and
    /// the event, so this takes the *larger* of the two rather than adding: a
    /// cumulative report added to a running total double-counts everything
    /// that came before it, and the number grows quadratically over a session.
    pub fn absorb(&mut self, report: &Usage) {
        self.input = self.input.max(report.input);
        self.output = self.output.max(report.output);
        self.cache_read = self.cache_read.max(report.cache_read);
        self.cache_write = self.cache_write.max(report.cache_write);
        if let Some(cost) = report.cost_usd {
            // Also a maximum, and for the same reason.
            self.cost_usd = Some(self.cost_usd.map_or(cost, |c: f64| c.max(cost)));
        }
        self.reports += 1;
    }

    /// Read a usage report out of an agent payload.
    #[must_use]
    pub fn from_payload(payload: &AgentPayload) -> Option<Self> {
        match payload {
            AgentPayload::Usage {
                input,
                output,
                cache_read,
                cache_write,
                cost_usd,
            } => Some(Self {
                input: *input,
                output: *output,
                cache_read: *cache_read,
                cache_write: *cache_write,
                cost_usd: *cost_usd,
                reports: 1,
            }),
            _ => None,
        }
    }
}

/// What an agent said about a limit it is approaching.
///
/// Only ever what the agent reported. omt does not know anybody's plan, and
/// guessing a limit would produce a percentage that is confidently wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimit {
    /// The agent's own description of its status.
    pub status: String,
    /// When it said the limit resets.
    pub resets_at: Option<omt_types::Timestamp>,
}

/// How close to a limit a session is, where that can be said at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Headroom {
    /// The agent has not said anything about a limit.
    ///
    /// The common case, and it must stay distinguishable from "plenty left" —
    /// a UI that rendered silence as a full bar would be inventing the one
    /// number the user would act on.
    Unknown,
    /// The agent reported a limit.
    Reported(RateLimit),
}

impl Headroom {
    /// Whether a surface has something real to show.
    #[must_use]
    pub const fn is_known(&self) -> bool {
        matches!(self, Self::Reported(_))
    }
}

/// Usage across every session, and per session.
#[derive(Debug, Default, Clone)]
pub struct UsageLedger {
    per_session: std::collections::BTreeMap<omt_types::SessionId, Usage>,
    limits: std::collections::BTreeMap<omt_types::SessionId, RateLimit>,
}

impl UsageLedger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold an agent observation in.
    pub fn observe(&mut self, session: omt_types::SessionId, payload: &AgentPayload) {
        if let Some(report) = Usage::from_payload(payload) {
            self.per_session.entry(session).or_default().absorb(&report);
        }
        if let AgentPayload::RateLimit { status, resets_at } = payload {
            self.limits.insert(
                session,
                RateLimit {
                    status: status.clone(),
                    resets_at: *resets_at,
                },
            );
        }
    }

    /// What one session has spent.
    #[must_use]
    pub fn session(&self, session: omt_types::SessionId) -> Usage {
        self.per_session.get(&session).copied().unwrap_or_default()
    }

    /// What every session has spent together.
    #[must_use]
    pub fn total(&self) -> Usage {
        let mut out = Usage::default();
        for u in self.per_session.values() {
            // Summed rather than absorbed: these are different sessions, so
            // adding is right where taking a maximum was right within one.
            out.input += u.input;
            out.output += u.output;
            out.cache_read += u.cache_read;
            out.cache_write += u.cache_write;
            out.reports += u.reports;
            if let Some(c) = u.cost_usd {
                out.cost_usd = Some(out.cost_usd.unwrap_or(0.0) + c);
            }
        }
        out
    }

    /// What the agent said about this session's limits.
    #[must_use]
    pub fn headroom(&self, session: omt_types::SessionId) -> Headroom {
        self.limits
            .get(&session)
            .cloned()
            .map_or(Headroom::Unknown, Headroom::Reported)
    }

    /// How many sessions have reported anything.
    #[must_use]
    pub fn len(&self) -> usize {
        self.per_session.len()
    }

    /// Whether nothing has been reported.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.per_session.is_empty()
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
    use omt_types::SessionId;

    fn usage(input: u64, output: u64, cost: Option<f64>) -> AgentPayload {
        AgentPayload::Usage {
            input,
            output,
            cache_read: 0,
            cache_write: 0,
            cost_usd: cost,
        }
    }

    #[test]
    fn a_cumulative_report_does_not_double_count() {
        // The bug this shape exists to prevent: agents report cumulatively,
        // and adding a cumulative report to a running total makes the number
        // grow quadratically over a session.
        let mut ledger = UsageLedger::new();
        let s = SessionId::new();
        ledger.observe(s, &usage(100, 50, None));
        ledger.observe(s, &usage(200, 90, None));
        ledger.observe(s, &usage(300, 140, None));
        assert_eq!(ledger.session(s).input, 300);
        assert_eq!(ledger.session(s).output, 140);
    }

    #[test]
    fn an_out_of_order_report_does_not_move_the_number_backwards() {
        // Events arrive twice and out of order after a reconnect.
        let mut ledger = UsageLedger::new();
        let s = SessionId::new();
        ledger.observe(s, &usage(300, 140, None));
        ledger.observe(s, &usage(100, 50, None));
        assert_eq!(ledger.session(s).input, 300);
    }

    #[test]
    fn a_cost_appears_only_when_the_agent_stated_one() {
        // omt never computes a price: plans differ, prices change, and a wrong
        // number on a screen is worse than none because somebody acts on it.
        let mut ledger = UsageLedger::new();
        let s = SessionId::new();
        ledger.observe(s, &usage(1_000_000, 500_000, None));
        assert_eq!(
            ledger.session(s).cost_usd,
            None,
            "a cost was invented from token counts"
        );

        ledger.observe(s, &usage(1_000_000, 500_000, Some(1.25)));
        assert_eq!(ledger.session(s).cost_usd, Some(1.25));
    }

    #[test]
    fn sessions_are_summed_where_reports_within_one_are_not() {
        // Different sessions genuinely add; reports within one do not.
        let mut ledger = UsageLedger::new();
        let a = SessionId::new();
        let b = SessionId::new();
        ledger.observe(a, &usage(100, 10, Some(0.5)));
        ledger.observe(a, &usage(200, 20, Some(1.0)));
        ledger.observe(b, &usage(50, 5, Some(0.25)));

        assert_eq!(ledger.session(a).input, 200);
        assert_eq!(ledger.total().input, 250);
        assert_eq!(ledger.total().cost_usd, Some(1.25));
    }

    #[test]
    fn a_session_nothing_reported_is_zero_rather_than_missing() {
        // A surface should render "nothing yet", not fail to render.
        let ledger = UsageLedger::new();
        assert_eq!(ledger.session(SessionId::new()), Usage::default());
        assert!(ledger.is_empty());
    }

    #[test]
    fn silence_about_a_limit_is_distinguishable_from_plenty_left() {
        // A UI that rendered silence as a full bar would be inventing the one
        // number the user would act on.
        let mut ledger = UsageLedger::new();
        let s = SessionId::new();
        assert_eq!(ledger.headroom(s), Headroom::Unknown);
        assert!(!ledger.headroom(s).is_known());

        ledger.observe(
            s,
            &AgentPayload::RateLimit {
                status: "8% of your five-hour window remains".to_owned(),
                resets_at: None,
            },
        );
        assert!(ledger.headroom(s).is_known());
    }

    #[test]
    fn a_newer_limit_replaces_the_last() {
        let mut ledger = UsageLedger::new();
        let s = SessionId::new();
        for status in ["50% remains", "8% remains"] {
            ledger.observe(
                s,
                &AgentPayload::RateLimit {
                    status: status.to_owned(),
                    resets_at: None,
                },
            );
        }
        let Headroom::Reported(limit) = ledger.headroom(s) else {
            panic!("expected a report");
        };
        assert_eq!(limit.status, "8% remains");
    }

    #[test]
    fn every_kind_of_token_is_counted() {
        let mut u = Usage::default();
        u.absorb(&Usage {
            input: 1,
            output: 2,
            cache_read: 4,
            cache_write: 8,
            cost_usd: None,
            reports: 1,
        });
        assert_eq!(u.total_tokens(), 15);
    }

    #[test]
    fn a_payload_that_is_not_usage_is_ignored() {
        assert!(
            Usage::from_payload(&AgentPayload::Activity {
                state: omt_events::ActivityGuess::Busy
            })
            .is_none()
        );
    }
}
