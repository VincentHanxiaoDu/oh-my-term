//! Which agent is in this session, as distinct from what it is doing.
//!
//! `docs/architecture/06-agent-layer.md` §2. Two axes that must not be
//! conflated:
//!
//! - **Binding** — *which* agent occupies the session, with a lifetime.
//! - **State** — what that agent is doing right now.
//!
//! Conflating them into one polling loop is the obvious design and it is wrong
//! twice over. Binding changes are rare — seconds to hours — and state changes
//! are frequent, hundreds of milliseconds; one loop has to run at the fast rate
//! and then re-answers the slow question hundreds of times an hour for nothing.
//!
//! The second reason matters more. A binding carries an identity: the agent's
//! *own* session id, which everything else keys off. Transcript lookups,
//! resume, usage attribution, the interaction ledger — all of them need to know
//! they are still talking about the same conversation. If the binding is
//! recomputed alongside state, that identity is rediscovered hundreds of times
//! a minute and any flicker in detection silently re-keys the lot.

use omt_types::{AgentKind, Timestamp, Tier};
use serde::Serialize;

/// Which agent occupies a session, and since when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binding {
    /// Which agent.
    pub kind: AgentKind,
    /// The agent's own session id, where it has one.
    ///
    /// Its identity rather than omt's. Transcript lookups and resume are done
    /// against the agent's own records, and a synthetic id omt invented would
    /// be useless for both.
    pub agent_session: Option<String>,
    /// How confidently it was identified.
    pub tier: Tier,
    /// When this binding started.
    pub since: Timestamp,
}

/// What changed when a new observation arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// Nothing; the same agent is still here.
    Same,
    /// A different agent now occupies the session.
    Rebound,
    /// The same agent, but its own session id is now known.
    ///
    /// Not a rebind. The conversation did not change — omt simply learned its
    /// name, which happens when a transcript appears a moment after the
    /// process does. Treating it as a rebind would discard everything keyed to
    /// the binding for the sake of an improvement.
    Identified,
    /// The same agent, seen more confidently than before.
    Upgraded,
}

/// Whether an observation replaces the current binding, and what it means.
///
/// A binding is only replaced by a *different agent*. Everything else — a
/// better tier, a session id arriving late — refines it in place, because the
/// conversation is the same conversation.
#[must_use]
pub fn reconcile(current: Option<&Binding>, observed: &Binding) -> Change {
    let Some(current) = current else {
        return Change::Rebound;
    };
    if current.kind != observed.kind {
        return Change::Rebound;
    }
    if current.agent_session.is_none() && observed.agent_session.is_some() {
        return Change::Identified;
    }
    if observed.tier > current.tier {
        // Tiers are ordered weakest first, so a larger one is more confident.
        return Change::Upgraded;
    }
    Change::Same
}

/// Apply an observation to the current binding.
///
/// Returns the binding to hold and what happened. `since` is preserved across
/// everything but a rebind, which is what makes "how long has this agent been
/// here" a question with a stable answer rather than one that resets whenever
/// detection improves.
#[must_use]
pub fn apply(current: Option<Binding>, observed: Binding) -> (Binding, Change) {
    let change = reconcile(current.as_ref(), &observed);
    match (change, current) {
        (Change::Rebound, _) => (observed, change),
        (_, None) => (observed, Change::Rebound),
        (_, Some(mut held)) => {
            if let Some(session) = observed.agent_session {
                held.agent_session = Some(session);
            }
            if observed.tier > held.tier {
                held.tier = observed.tier;
            }
            (held, change)
        }
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

    fn binding(kind: AgentKind, session: Option<&str>, tier: Tier) -> Binding {
        Binding {
            kind,
            agent_session: session.map(str::to_owned),
            tier,
            since: Timestamp::from_unix_seconds(1_000),
        }
    }

    #[test]
    fn nothing_changes_when_the_same_agent_is_seen_again() {
        let held = binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Process);
        let (_, change) = apply(
            Some(held.clone()),
            binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Process),
        );
        assert_eq!(change, Change::Same);
    }

    #[test]
    fn a_different_agent_rebinds() {
        let (next, change) = apply(
            Some(binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Process)),
            binding(AgentKind::Codex, None, Tier::Process),
        );
        assert_eq!(change, Change::Rebound);
        assert_eq!(next.kind, AgentKind::Codex);
        assert_eq!(next.agent_session, None, "the old conversation's id does not carry over");
    }

    #[test]
    fn learning_the_agents_own_id_is_not_a_rebind() {
        // The conversation did not change — omt learned its name, which happens
        // when a transcript appears a moment after the process does. Treating
        // it as a rebind would discard everything keyed to the binding for the
        // sake of an improvement.
        let (next, change) = apply(
            Some(binding(AgentKind::ClaudeCode, None, Tier::Process)),
            binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Transcript),
        );
        assert_eq!(change, Change::Identified);
        assert_eq!(next.agent_session.as_deref(), Some("s-1"));
    }

    #[test]
    fn a_binding_keeps_its_start_time_when_it_is_only_refined() {
        // Otherwise "how long has this agent been here" resets whenever
        // detection improves, which is often and invisible.
        let held = binding(AgentKind::ClaudeCode, None, Tier::Process);
        let later = Binding {
            since: Timestamp::from_unix_seconds(9_999),
            ..binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Transcript)
        };
        let (next, _) = apply(Some(held.clone()), later);
        assert_eq!(next.since, held.since);
    }

    #[test]
    fn a_rebind_takes_the_new_start_time() {
        // It is a different agent, so the clock genuinely starts again.
        let held = binding(AgentKind::ClaudeCode, None, Tier::Process);
        let later = Binding {
            since: Timestamp::from_unix_seconds(9_999),
            ..binding(AgentKind::Codex, None, Tier::Process)
        };
        let (next, _) = apply(Some(held), later);
        assert_eq!(next.since, Timestamp::from_unix_seconds(9_999));
    }

    #[test]
    fn a_better_tier_upgrades_without_rebinding() {
        let (next, change) = apply(
            Some(binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Heuristic)),
            binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Protocol),
        );
        assert_eq!(change, Change::Upgraded);
        assert_eq!(next.tier, Tier::Protocol);
    }

    #[test]
    fn a_worse_tier_never_downgrades_a_binding() {
        // A heuristic glimpse arriving after a protocol handshake is not news.
        let (next, change) = apply(
            Some(binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Protocol)),
            binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Heuristic),
        );
        assert_eq!(change, Change::Same);
        assert_eq!(next.tier, Tier::Protocol);
    }

    #[test]
    fn an_agents_own_id_is_never_forgotten_once_known() {
        // Everything else keys off it — transcript lookups, resume, usage
        // attribution — and losing it re-keys the lot.
        let (next, _) = apply(
            Some(binding(AgentKind::ClaudeCode, Some("s-1"), Tier::Transcript)),
            binding(AgentKind::ClaudeCode, None, Tier::Process),
        );
        assert_eq!(next.agent_session.as_deref(), Some("s-1"));
    }

    #[test]
    fn the_first_observation_is_a_rebind() {
        // There is nothing to compare against, and calling it `Same` would
        // have a caller skip the work a new binding needs.
        let (_, change) = apply(None, binding(AgentKind::ClaudeCode, None, Tier::Process));
        assert_eq!(change, Change::Rebound);
    }
}
