//! Feeding the event stream to connected clients.
//!
//! `events.subscribe` minted a subscription id and returned it, and the socket
//! server accepted a `subscribe` message and replied with silence — which is
//! correct, because a subscription is answered by what arrives afterwards. What
//! was missing is the afterwards: nothing ever sent an event, on any transport.
//!
//! So every client polled. The web client asked for a fresh screen every 150ms
//! whether or not anything had changed, which is both the reason the terminal
//! felt laggy and the reason an idle phone kept a radio busy.

use std::collections::BTreeMap;

use omt_server::{EventCursor, EventSource, EventsSince};
use omt_types::SessionId;

use crate::state::State;

/// Reads events out of the running instance for the socket server.
pub struct InstanceEvents(pub State);

impl EventSource for InstanceEvents {
    fn since(&self, cursor: &mut EventCursor) -> EventsSince {
        let Ok(instance) = self.0.lock() else {
            // A poisoned lock is reported by the capability path, which says it
            // in an answer somebody asked for. Saying it again once per tick
            // would drown the connection.
            return EventsSince::default();
        };

        let mut out = EventsSince::default();

        // The instance stream first: a session opening is what tells a client
        // there is a new session to follow, and delivering its events before
        // that arrives would have the client place them nowhere.
        collect(
            instance.resume_instance(omt_types::Seq::new(cursor.instance)),
            "instance",
            &mut cursor.instance,
            &mut out,
        );

        // Every session, including ones this client has never seen: a client
        // that subscribed before a session existed must still be sent its
        // events, and it has no position to ask about one it does not know.
        let sessions: Vec<SessionId> = instance.sessions().iter().map(|s| s.id).collect();
        let mut seen: BTreeMap<String, u64> = BTreeMap::new();
        for id in sessions {
            let key = id.to_wire();
            let mut at = cursor.sessions.get(&key).copied().unwrap_or(0);
            if let Some(outcome) = instance.resume(id, omt_types::Seq::new(at)) {
                collect(outcome, &key, &mut at, &mut out);
            }
            seen.insert(key, at);
        }
        // Positions for sessions that are gone go with them, or the cursor
        // grows for the lifetime of the connection.
        cursor.sessions = seen;

        out
    }
}

/// Fold one stream's replay into the batch, advancing the position.
fn collect(outcome: omt_events::ResumeOutcome, scope: &str, at: &mut u64, out: &mut EventsSince) {
    match outcome {
        omt_events::ResumeOutcome::Replayed { events } => {
            if let Some(last) = events.last() {
                *at = last.seq.get();
            }
            out.events.extend(events);
        }
        omt_events::ResumeOutcome::Resync {
            reason,
            dropped,
            from,
        } => {
            // Said out loud. A client left believing it is current when it is
            // not will render a screen that is quietly wrong, and there is no
            // later moment at which it finds out.
            out.resync.push(omt_proto::Resync {
                scope_key: scope.to_owned(),
                reason,
                dropped,
                from,
            });
            // Continue from where the stream now begins rather than replaying a
            // window that no longer reaches back. The client rebuilds from a
            // snapshot; this keeps it from being told the same thing every tick.
            *at = from.get();
        }
    }
}
