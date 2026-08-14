//! Who is listening, what they want, and where they are in the stream.
//!
//! Three things that were each implemented and none of which were joined: the
//! [`ReplayWindow`] knew how to answer "everything after this position", the
//! [`Filter`] knew "does this subscriber want this event", and the
//! [`Subscription`] knew "is this subscriber keeping up". Every caller wired
//! them together itself, which is three chances to get the order wrong and no
//! single place that says what a subscription *is*.
//!
//! The order matters and is the whole content of this module: **filter, then
//! account.** A subscriber that asked for one session must not be charged for
//! another session's firehose, and must not be disconnected for falling behind
//! on events it never wanted. Accounting first is the bug this exists to make
//! unwritable.

use std::collections::BTreeMap;

use omt_types::Seq;

use crate::backpressure::{Delivery, Policy, Subscription};
use crate::envelope::{Event, Filter, ReplayWindow, ResumeOutcome};

/// A subscriber's handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubscriberId(u64);

impl SubscriberId {
    /// Its numeric form, for a log line.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// What happened to one subscriber when an event was published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fanout {
    /// It did not want this event.
    Filtered,
    /// It was delivered.
    Delivered,
    /// It was dropped because the subscriber is behind, and the class allows
    /// dropping. Only terminal bytes are that class.
    Dropped {
        /// How much never went over the wire, so a client can say "catching
        /// up" rather than silently showing less.
        dropped_bytes: u64,
    },
    /// The subscriber is too far behind to continue.
    Disconnect {
        /// Why, in words a client can render. A disconnection with no stated
        /// reason is indistinguishable from a network fault, and the two want
        /// different things from the user.
        reason: &'static str,
    },
}

/// The events an instance has produced, and who is listening.
#[derive(Debug)]
pub struct EventBus {
    window: ReplayWindow,
    subscribers: BTreeMap<SubscriberId, Subscriber>,
    next_id: u64,
}

#[derive(Debug)]
struct Subscriber {
    filter: Filter,
    flow: Subscription,
}

impl EventBus {
    /// A bus retaining `capacity` events for replay.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            window: ReplayWindow::new(capacity),
            subscribers: BTreeMap::new(),
            next_id: 0,
        }
    }

    /// Add a subscriber.
    pub fn subscribe(&mut self, filter: Filter, policy: Policy) -> SubscriberId {
        let id = SubscriberId(self.next_id);
        self.next_id += 1;
        self.subscribers.insert(
            id,
            Subscriber {
                filter,
                flow: Subscription::new(policy),
            },
        );
        id
    }

    /// Remove a subscriber.
    ///
    /// Returns whether there was one. A caller that disconnects the same
    /// subscriber twice has a bug worth seeing, and a silent `()` hides it.
    pub fn unsubscribe(&mut self, id: SubscriberId) -> bool {
        self.subscribers.remove(&id).is_some()
    }

    /// How many are listening.
    #[must_use]
    pub fn subscribers(&self) -> usize {
        self.subscribers.len()
    }

    /// Publish an event, and say what each subscriber made of it.
    ///
    /// The event enters the replay window whatever the subscribers do: a
    /// subscriber that was disconnected for falling behind will reconnect and
    /// ask to resume, and the window is what answers that.
    pub fn publish(&mut self, event: Event) -> Vec<(SubscriberId, Fanout)> {
        self.window.push(event.clone());
        let mut out = Vec::with_capacity(self.subscribers.len());
        for (id, sub) in &mut self.subscribers {
            // Filter *then* account. A subscriber watching one quiet session
            // must not be disconnected because another session is a firehose,
            // and charging it for events it never asked for is exactly how
            // that would happen.
            if !sub.filter.matches(&event) {
                out.push((*id, Fanout::Filtered));
                continue;
            }
            let fanout = match sub.flow.offer_event() {
                Delivery::Queued => Fanout::Delivered,
                // Terminal bytes are the lossy class and events are not, so
                // an event queue never coalesces or resyncs — these arms exist
                // because `Delivery` describes both queues, and treating an
                // unreachable one as a delivery would hide a real change.
                Delivery::Coalesced { dropped_bytes } | Delivery::Resynced { dropped_bytes } => {
                    Fanout::Dropped { dropped_bytes }
                }
                Delivery::Overloaded => Fanout::Disconnect {
                    reason: "too far behind on events that cannot be dropped",
                },
            };
            out.push((*id, fanout));
        }
        out
    }

    /// Note that a subscriber caught up on `events` of its queue.
    pub fn flushed(&mut self, id: SubscriberId, events: usize) {
        if let Some(sub) = self.subscribers.get_mut(&id) {
            sub.flow.flushed(events);
        }
    }

    /// Everything a subscriber missed after a position, or an instruction to
    /// resynchronize.
    ///
    /// Filtered by that subscriber's own filter, which is the half a caller
    /// wiring the window up by hand kept forgetting: an unfiltered replay
    /// hands a phone every session's events on reconnect, which is both a
    /// privacy question and the largest burst it will ever see.
    #[must_use]
    pub fn resume(&self, id: SubscriberId, after: Seq) -> ResumeOutcome {
        let outcome = self.window.resume_from(after);
        let Some(sub) = self.subscribers.get(&id) else {
            return outcome;
        };
        match outcome {
            ResumeOutcome::Replayed { events } => ResumeOutcome::Replayed {
                events: events
                    .into_iter()
                    .filter(|e| sub.filter.matches(e))
                    .collect(),
            },
            resync => resync,
        }
    }

    /// The replay window, for a caller that needs it directly.
    #[must_use]
    pub const fn window(&self) -> &ReplayWindow {
        &self.window
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
    use crate::envelope::{EventPayload, EventSourceTag};
    use omt_types::{SeqScope, SessionId, Timestamp};

    fn event(scope: SessionId, seq: u64) -> Event {
        Event {
            seq: Seq::new(seq),
            scope: SeqScope::Session { session: scope },
            ts: Timestamp::from_unix_seconds(0),
            source: EventSourceTag::Core,
            payload: EventPayload::Instance(crate::InstanceEvent::Degraded {
                detail: "test".to_owned(),
            }),
        }
    }

    fn for_scope(scope: SessionId) -> Filter {
        Filter {
            sessions: vec![scope],
            ..Filter::default()
        }
    }

    #[test]
    fn a_subscriber_is_only_offered_what_it_asked_for() {
        let mut bus = EventBus::new(64);
        let (mine, theirs) = (SessionId::new(), SessionId::new());
        let me = bus.subscribe(for_scope(mine), Policy::default());

        let out = bus.publish(event(theirs, 1));
        assert_eq!(out, vec![(me, Fanout::Filtered)]);
        let out = bus.publish(event(mine, 2));
        assert_eq!(out, vec![(me, Fanout::Delivered)]);
    }

    #[test]
    fn a_quiet_subscriber_is_not_charged_for_a_noisy_session() {
        // Filter then account. Accounting first would disconnect somebody
        // watching one quiet session because a different one is a firehose —
        // which is the exact failure per-subscription backpressure exists to
        // prevent, reintroduced by getting the order wrong.
        let mut bus = EventBus::new(4096);
        let (quiet, noisy) = (SessionId::new(), SessionId::new());
        let watcher = bus.subscribe(for_scope(quiet), Policy::default());

        for seq in 0..2000 {
            for (_, fanout) in bus.publish(event(noisy, seq)) {
                assert_eq!(fanout, Fanout::Filtered, "charged for another session");
            }
        }
        assert_eq!(
            bus.publish(event(quiet, 9999)),
            vec![(watcher, Fanout::Delivered)],
            "a subscriber that received nothing was treated as behind"
        );
    }

    #[test]
    fn a_resume_is_filtered_by_the_subscribers_own_filter() {
        // An unfiltered replay hands a phone every session's events on
        // reconnect: both a privacy question and the largest burst it will
        // ever see.
        let mut bus = EventBus::new(64);
        let (mine, theirs) = (SessionId::new(), SessionId::new());
        let me = bus.subscribe(for_scope(mine), Policy::default());
        bus.publish(event(mine, 1));
        bus.publish(event(theirs, 2));
        bus.publish(event(mine, 3));

        let ResumeOutcome::Replayed { events } = bus.resume(me, Seq::new(0)) else {
            panic!("expected a replay");
        };
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(events.iter().all(|e| e.scope == SeqScope::Session { session: mine }));
    }

    #[test]
    fn resuming_past_the_window_says_so_rather_than_answering_partially() {
        let mut bus = EventBus::new(4);
        let scope = SessionId::new();
        let me = bus.subscribe(for_scope(scope), Policy::default());
        for seq in 1..=10 {
            bus.publish(event(scope, seq));
        }
        assert!(
            matches!(bus.resume(me, Seq::new(0)), ResumeOutcome::Resync { .. }),
            "a partial answer was returned as a complete one"
        );
    }

    #[test]
    fn an_event_enters_the_window_whatever_the_subscribers_did() {
        // A subscriber disconnected for falling behind will reconnect and ask
        // to resume; the window is what answers that. Retaining only what was
        // delivered would guarantee the resume fails.
        let mut bus = EventBus::new(64);
        let scope = SessionId::new();
        bus.publish(event(scope, 1));
        assert_eq!(bus.window().len(), 1);
    }

    #[test]
    fn unsubscribing_twice_is_visible_rather_than_silent() {
        let mut bus = EventBus::new(8);
        let id = bus.subscribe(Filter::default(), Policy::default());
        assert!(bus.unsubscribe(id));
        assert!(!bus.unsubscribe(id), "a double disconnect looked fine");
        assert_eq!(bus.subscribers(), 0);
    }

    #[test]
    fn a_disconnection_carries_a_reason() {
        // Indistinguishable from a network fault otherwise, and the two want
        // different things from the user.
        let mut bus = EventBus::new(65536);
        let scope = SessionId::new();
        let me = bus.subscribe(for_scope(scope), Policy::default());
        let mut reason = None;
        for seq in 0..100_000 {
            for (_, fanout) in bus.publish(event(scope, seq)) {
                if let Fanout::Disconnect { reason: r } = fanout {
                    reason = Some(r);
                }
            }
            if reason.is_some() {
                break;
            }
        }
        let reason = reason.expect("a subscriber that never reads is never disconnected");
        assert!(!reason.is_empty());
        // And it is the subscriber that was disconnected, not the bus.
        assert_eq!(bus.subscribers(), 1, "{me:?}");
    }
}
