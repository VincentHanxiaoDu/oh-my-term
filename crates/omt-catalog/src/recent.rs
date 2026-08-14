//! The recent-results cache, and what a repeated request means.
//!
//! [03 §3.5](../../../docs/architecture/03-capability-catalog.md) puts this in
//! dispatch and says why: a retry arriving twice means something different for
//! each of D15's intent classes, and deciding that per transport means
//! deciding it several times and getting it wrong somewhere.
//!
//! The class that matters most is [`Intent::ExternallyConfirmed`].
//! [D15](../../../docs/architecture/decisions.md)'s rule — *never retry an
//! injection, by any actor except a human who can see the screen* — is
//! enforced here, once. A duplicate keystroke into an agent's UI is not a
//! duplicate row that dedup can absorb; it is a keystroke landing wherever
//! that agent happens to be now.

use std::collections::VecDeque;

use crate::decl::Intent;
use crate::registry::RequestId;

/// How many results are remembered.
///
/// Bounded, and small on purpose: this exists to absorb a reconnect's replay,
/// which happens within seconds. An unbounded cache would grow for the life of
/// an instance to serve a retry nobody is going to send.
pub const CAPACITY: usize = 256;

/// What dispatch should do with a request it has seen before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repeat {
    /// Run it. Either it is safe to repeat, or nothing was remembered.
    Execute,
    /// Serve what it produced last time.
    Replay,
    /// Refuse. Repeating it would do something a retry must never do.
    Refuse {
        /// What to tell the caller, which has to say *why* rather than just
        /// "conflict" — the caller is deciding whether to surface a retry
        /// button to a human, and "this cannot be retried by a machine" and
        /// "this collided" are different answers.
        reason: &'static str,
    },
}

/// What a repeated request id means for one intent class.
///
/// A pure function of the class and whether a result was remembered, so 03's
/// table is readable next to the table.
#[must_use]
pub fn repeat_for(intent: Option<&Intent>, remembered: bool) -> Repeat {
    match intent {
        // A query is safe by definition. Re-executing is also more correct
        // than replaying: the caller asked what is true now.
        None => Repeat::Execute,
        // The handler's own compare-and-set on `(identity, intent_id)` makes a
        // re-execution safe, so a miss is not a problem.
        Some(Intent::Cas) => {
            if remembered {
                Repeat::Replay
            } else {
                Repeat::Execute
            }
        }
        // On a miss, the handler's dedup returns the original entry, so this
        // reaches the same place by a longer route.
        Some(Intent::Append { .. }) => {
            if remembered {
                Repeat::Replay
            } else {
                Repeat::Execute
            }
        }
        // Never cached and never repeated. Resumption of a stream is `ack`,
        // which names a position — not a re-send, which would replay bytes the
        // far side already has.
        Some(Intent::RawStream) => Repeat::Refuse {
            reason: "a raw stream is resumed by acknowledging a position, never by repeating a call",
        },
        // D15, enforced once. A human who can see the screen may do this
        // again; nothing else may, and that includes a retry loop that means
        // well.
        Some(Intent::ExternallyConfirmed { .. }) => Repeat::Refuse {
            reason: "this delivers input to something omt does not control, and a repeat would land wherever that agent is now",
        },
        // Last-writer-wins: the compare-and-set on `version` produces a
        // visible loser, which is the whole point of the class.
        Some(Intent::LwwFreeText) => Repeat::Execute,
    }
}

/// A bounded set of recent results, keyed by request identity.
#[derive(Debug, Default, Clone)]
pub struct Recent {
    entries: VecDeque<(RequestId, serde_json::Value)>,
}

impl Recent {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What this request produced last time, if it is still remembered.
    #[must_use]
    pub fn get(&self, id: &RequestId) -> Option<&serde_json::Value> {
        self.entries.iter().find(|(k, _)| k == id).map(|(_, v)| v)
    }

    /// Remember a result, evicting the oldest if the cache is full.
    ///
    /// Recording a second result for the same id **replaces** the first rather
    /// than storing both: two answers under one key is a cache that returns
    /// whichever it finds first, and which one that is depends on eviction.
    pub fn insert(&mut self, id: RequestId, output: serde_json::Value) {
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == id) {
            slot.1 = output;
            return;
        }
        if self.entries.len() >= CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back((id, output));
    }

    /// How many results are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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
    use crate::decl::DedupKey;
    use omt_types::DeviceId;

    fn id(n: u64) -> RequestId {
        RequestId {
            device: DeviceId::new(),
            n,
        }
    }

    #[test]
    fn an_injection_is_never_repeated_by_a_machine() {
        // D15's rule, and the reason this lives in dispatch rather than in
        // each transport: a duplicate keystroke is not a duplicate row that
        // dedup can absorb, it is a keystroke landing wherever the agent
        // happens to be now.
        let intent = Intent::ExternallyConfirmed {
            confirm_within_ms: 5_000,
        };
        for remembered in [true, false] {
            assert!(
                matches!(repeat_for(Some(&intent), remembered), Repeat::Refuse { .. }),
                "an externally-confirmed call was repeatable"
            );
        }
    }

    #[test]
    fn a_raw_stream_is_resumed_rather_than_repeated() {
        // Repeating would re-send bytes the far side already has. Resumption
        // names a position instead.
        assert!(matches!(
            repeat_for(Some(&Intent::RawStream), true),
            Repeat::Refuse { .. }
        ));
    }

    #[test]
    fn a_refusal_says_why_rather_than_only_that_it_refused() {
        // The caller is deciding whether to offer a human a retry button, and
        // "a machine may not repeat this" is a different answer from "this
        // collided".
        let Repeat::Refuse { reason } = repeat_for(Some(&Intent::RawStream), false) else {
            panic!("expected a refusal");
        };
        assert!(!reason.is_empty());
    }

    #[test]
    fn a_query_is_re_executed_because_the_caller_asked_what_is_true_now() {
        assert_eq!(repeat_for(None, true), Repeat::Execute);
    }

    #[test]
    fn a_cas_replays_what_it_has_and_re_executes_what_it_does_not() {
        // The handler's own compare-and-set makes the miss safe, which is why
        // a bounded cache is enough rather than a durable one.
        assert_eq!(repeat_for(Some(&Intent::Cas), true), Repeat::Replay);
        assert_eq!(repeat_for(Some(&Intent::Cas), false), Repeat::Execute);
    }

    #[test]
    fn an_append_behaves_the_same_way_for_the_same_reason() {
        let intent = Intent::Append {
            dedup: DedupKey::IntentId,
            ttl_secs: 60,
        };
        assert_eq!(repeat_for(Some(&intent), true), Repeat::Replay);
        assert_eq!(repeat_for(Some(&intent), false), Repeat::Execute);
    }

    #[test]
    fn last_writer_wins_re_executes_so_the_loser_is_visible() {
        assert_eq!(
            repeat_for(Some(&Intent::LwwFreeText), true),
            Repeat::Execute
        );
    }

    #[test]
    fn a_result_comes_back_under_the_id_that_produced_it() {
        let mut recent = Recent::new();
        let (a, b) = (id(1), id(2));
        recent.insert(a, serde_json::json!({"n": 1}));
        recent.insert(b, serde_json::json!({"n": 2}));
        assert_eq!(recent.get(&a), Some(&serde_json::json!({"n": 1})));
        assert_eq!(recent.get(&b), Some(&serde_json::json!({"n": 2})));
    }

    #[test]
    fn a_second_result_for_one_id_replaces_rather_than_joins_it() {
        // Two answers under one key is a cache that returns whichever it finds
        // first, and which one that is depends on eviction order.
        let mut recent = Recent::new();
        let key = id(1);
        recent.insert(key, serde_json::json!("first"));
        recent.insert(key, serde_json::json!("second"));
        assert_eq!(recent.len(), 1);
        assert_eq!(recent.get(&key), Some(&serde_json::json!("second")));
    }

    #[test]
    fn the_cache_is_bounded_and_forgets_the_oldest_first() {
        // Unbounded, it would grow for the life of an instance to serve a
        // retry nobody is going to send.
        let mut recent = Recent::new();
        let first = id(0);
        recent.insert(first, serde_json::json!(0));
        for n in 1..=CAPACITY as u64 {
            recent.insert(id(n), serde_json::json!(n));
        }
        assert_eq!(recent.len(), CAPACITY);
        assert!(
            recent.get(&first).is_none(),
            "the oldest entry survived eviction"
        );
    }
}
