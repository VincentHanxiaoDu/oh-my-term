//! The event envelope, its closed vocabularies, and the bus.

use omt_types::{
    ClientId, InteractionId, SessionId, Seq, SeqScope, SessionMode, Timestamp, WorkspaceId,
};
use serde::{Deserialize, Serialize};

use crate::agent::AgentEvent;
use crate::interaction::Interaction;

/// Which family an event belongs to.
///
/// Closed, so a client can filter a subscription by kind and know it is not
/// silently missing a family nobody told it about. An event with no kind would
/// be unsubscribable, which is why every payload has one.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Terminal output, blocks and grid changes.
    Terminal,
    /// Agent observations.
    Agent,
    /// Interaction lifecycle.
    Interaction,
    /// Sessions, workspaces and layouts.
    SessionTree,
    /// Who is attached and what they are doing.
    Presence,
    /// Configuration changes.
    Config,
    /// Plugin lifecycle.
    Plugin,
    /// Who did what.
    Audit,
    /// Files and version control.
    WorkspaceFs,
    /// The instance itself.
    Instance,
}

/// Which observation produced an event.
///
/// One closed set, shared by every kind. `heuristic` rather than `pty`, because
/// the name should say how much to trust it, not where it came from.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EventSourceTag {
    /// Screen or bell inference.
    Heuristic,
    /// Process and environment inspection.
    Process,
    /// omt's injected correlation or OSC backchannel.
    Marker,
    /// The agent's own session file.
    Transcript,
    /// The agent's own hook system.
    Hook,
    /// The agent's own structured protocol.
    Protocol,
    /// omt itself — a state change it caused.
    Core,
    /// A filesystem watcher.
    Fs,
    /// A plugin.
    Plugin,
}

/// One event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Event {
    /// Which stream, and therefore which sequence counts it.
    pub scope: SeqScope,
    /// Position within that scope.
    pub seq: Seq,
    /// When.
    pub ts: Timestamp,
    /// What produced it.
    pub source: EventSourceTag,
    /// What happened.
    pub payload: EventPayload,
}

impl Event {
    /// The family this event belongs to.
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        self.payload.kind()
    }
}

/// What an event says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventPayload {
    /// Terminal output arrived.
    ///
    /// Byte frames travel on the binary path rather than here; this is the
    /// signal that something changed, for a client that is not streaming.
    Terminal(TerminalEvent),
    /// An agent observation.
    Agent {
        /// The observation. `omt-proto` wraps it; the shape belongs to
        /// `agent`, and the envelope copies rather than recomputes its
        /// identifiers.
        event: Box<AgentEvent>,
    },
    /// An interaction changed state.
    ///
    /// One transition event carrying the whole interaction, rather than four
    /// narrow ones: a client that missed an earlier transition still ends up
    /// correct, which matters because missing one is the normal case on a
    /// phone.
    Interaction {
        /// The interaction as it now stands.
        interaction: Box<Interaction>,
    },
    /// The session tree changed.
    SessionTree(SessionTreeEvent),
    /// Presence changed.
    Presence(PresenceEvent),
    /// The instance changed.
    Instance(InstanceEvent),
}

impl EventPayload {
    /// Which family this is.
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        match self {
            Self::Terminal(_) => EventKind::Terminal,
            Self::Agent { .. } => EventKind::Agent,
            Self::Interaction { .. } => EventKind::Interaction,
            Self::SessionTree(_) => EventKind::SessionTree,
            Self::Presence(_) => EventKind::Presence,
            Self::Instance(_) => EventKind::Instance,
        }
    }
}

/// Something happened in a terminal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TerminalEvent {
    /// Output was produced.
    Output {
        /// How many bytes, so a client can decide whether to fetch.
        bytes: u64,
    },
    /// The session was resized.
    Resized {
        /// New width.
        cols: u16,
        /// New height.
        rows: u16,
    },
    /// A command block opened or closed.
    BlockChanged {
        /// Which block.
        block: omt_types::BlockId,
        /// Whether it is still open.
        open: bool,
    },
    /// Scrollback was appended to.
    HistoryAppended {
        /// How many lines.
        lines: u64,
    },
}

/// Something happened to the session tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SessionTreeEvent {
    /// A session was created.
    SessionCreated {
        /// Which.
        session: SessionId,
        /// In which workspace.
        workspace: WorkspaceId,
        /// How it runs its agent.
        mode: SessionMode,
    },
    /// A session ended.
    SessionClosed {
        /// Which.
        session: SessionId,
        /// Its exit code, where there was one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
    },
    /// A layout changed.
    LayoutChanged {
        /// In which workspace.
        workspace: WorkspaceId,
    },
}

/// Something happened to presence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum PresenceEvent {
    /// A client attached.
    ClientAttached {
        /// Which.
        client: ClientId,
    },
    /// A client went away.
    ClientDetached {
        /// Which.
        client: ClientId,
    },
    /// The writer token changed hands.
    WriterChanged {
        /// For which session.
        session: SessionId,
        /// Who holds it now, if anyone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        holder: Option<ClientId>,
    },
    /// An actor's read mark moved.
    ReadMarkChanged {
        /// For which session.
        session: SessionId,
        /// Up to where.
        seq: Seq,
    },
}

/// Something happened to the instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum InstanceEvent {
    /// Configuration changed.
    ConfigChanged {
        /// Which layer.
        layer: String,
    },
    /// The instance is degraded.
    Degraded {
        /// What is wrong.
        detail: String,
    },
    /// A long-running job made progress.
    JobProgress {
        /// Which job.
        job: omt_types::JobId,
        /// How far, 0.0 to 1.0, where it can be known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fraction: Option<f32>,
    },
}

/// What a subscriber wants.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Filter {
    /// Only these kinds. Empty means all.
    #[serde(default)]
    pub kinds: Vec<EventKind>,
    /// Only these sessions. Empty means all.
    #[serde(default)]
    pub sessions: Vec<SessionId>,
}

impl Filter {
    /// Whether an event matches.
    #[must_use]
    pub fn matches(&self, event: &Event) -> bool {
        if !self.kinds.is_empty() && !self.kinds.contains(&event.kind()) {
            return false;
        }
        if !self.sessions.is_empty() {
            let SeqScope::Session { session } = event.scope else {
                return false;
            };
            if !self.sessions.contains(&session) {
                return false;
            }
        }
        true
    }
}

/// Why a subscriber was told it fell behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LagReason {
    /// The requested position is older than what is retained.
    WindowExceeded,
    /// The subscriber could not keep up.
    SubscriberSlow,
    /// The instance restarted.
    Restart,
}

/// The outcome of asking to resume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ResumeOutcome {
    /// Everything asked for is here.
    Replayed {
        /// The events, in order.
        events: Vec<Event>,
    },
    /// The gap is unrecoverable; rebuild from a snapshot.
    ///
    /// Announced rather than papered over: a client must never be left
    /// believing it is current when it is not.
    Resync {
        /// Why.
        reason: LagReason,
        /// How many events were lost, so the UI can say how much.
        dropped: u64,
        /// Where the stream now stands.
        from: Seq,
    },
}

/// A bounded replay window over one scope.
///
/// Deliberately bounded: an unbounded buffer trades a visible failure for an
/// invisible one, and the invisible one is the instance running out of memory
/// because a phone was in a tunnel.
#[derive(Debug)]
pub struct ReplayWindow {
    events: std::collections::VecDeque<Event>,
    capacity: usize,
    dropped: u64,
}

impl ReplayWindow {
    /// A window holding at most `capacity` events.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            events: std::collections::VecDeque::with_capacity(capacity.min(1024)),
            capacity,
            dropped: 0,
        }
    }

    /// Record an event, evicting the oldest if full.
    pub fn push(&mut self, event: Event) {
        if self.events.len() == self.capacity {
            self.events.pop_front();
            self.dropped += 1;
        }
        self.events.push_back(event);
    }

    /// How many events are retained.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether nothing is retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Everything after `after`, or an instruction to resynchronize.
    #[must_use]
    pub fn resume_from(&self, after: Seq) -> ResumeOutcome {
        let oldest = self.events.front().map(|e| e.seq);
        match oldest {
            // The window has moved past what was asked for. Say so, with how
            // much was lost, rather than quietly returning a partial answer.
            Some(o) if o.get() > after.get() + 1 => ResumeOutcome::Resync {
                reason: LagReason::WindowExceeded,
                dropped: self.dropped,
                from: o,
            },
            _ => ResumeOutcome::Replayed {
                events: self
                    .events
                    .iter()
                    .filter(|e| e.seq.get() > after.get())
                    .cloned()
                    .collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(session: SessionId, n: u64) -> Event {
        Event {
            scope: SeqScope::Session { session },
            seq: Seq::new(n),
            ts: Timestamp::UNIX_EPOCH,
            source: EventSourceTag::Core,
            payload: EventPayload::Terminal(TerminalEvent::Output { bytes: 1 }),
        }
    }

    #[test]
    fn every_payload_reports_a_kind() {
        // An event with no kind would be unsubscribable.
        let s = SessionId::new();
        assert_eq!(ev(s, 1).kind(), EventKind::Terminal);
    }

    #[test]
    fn resume_returns_exactly_what_follows() {
        let s = SessionId::new();
        let mut w = ReplayWindow::new(10);
        for n in 1..=5 {
            w.push(ev(s, n));
        }
        let ResumeOutcome::Replayed { events } = w.resume_from(Seq::new(3)) else {
            panic!("expected a replay");
        };
        let seqs: Vec<_> = events.iter().map(|e| e.seq.get()).collect();
        assert_eq!(seqs, [4, 5], "no gap, no duplication");
    }

    #[test]
    fn resume_from_zero_returns_everything_retained() {
        let s = SessionId::new();
        let mut w = ReplayWindow::new(10);
        for n in 1..=3 {
            w.push(ev(s, n));
        }
        let ResumeOutcome::Replayed { events } = w.resume_from(Seq::ZERO) else {
            panic!("expected a replay");
        };
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn a_gap_is_announced_never_silent() {
        // The client must never be left believing it is current when it is not.
        let s = SessionId::new();
        let mut w = ReplayWindow::new(3);
        for n in 1..=10 {
            w.push(ev(s, n));
        }
        let ResumeOutcome::Resync { reason, dropped, from } = w.resume_from(Seq::new(1)) else {
            panic!("expected a resync instruction");
        };
        assert_eq!(reason, LagReason::WindowExceeded);
        assert_eq!(dropped, 7, "the client is told how much it missed");
        assert_eq!(from.get(), 8);
    }

    #[test]
    fn the_window_is_bounded() {
        let s = SessionId::new();
        let mut w = ReplayWindow::new(4);
        for n in 1..=100 {
            w.push(ev(s, n));
        }
        assert_eq!(w.len(), 4, "an unbounded buffer would be an invisible failure");
    }

    #[test]
    fn resume_at_the_boundary_replays_rather_than_resyncs() {
        // Off-by-one here would send a client to a snapshot it did not need.
        let s = SessionId::new();
        let mut w = ReplayWindow::new(3);
        for n in 1..=5 {
            w.push(ev(s, n));
        }
        // window holds 3,4,5; asking for "after 2" is exactly satisfiable
        assert!(matches!(w.resume_from(Seq::new(2)), ResumeOutcome::Replayed { .. }));
        assert!(matches!(w.resume_from(Seq::new(1)), ResumeOutcome::Resync { .. }));
    }

    #[test]
    fn a_filter_selects_by_kind() {
        let s = SessionId::new();
        let f = Filter { kinds: vec![EventKind::Agent], sessions: vec![] };
        assert!(!f.matches(&ev(s, 1)));
        let f = Filter { kinds: vec![EventKind::Terminal], sessions: vec![] };
        assert!(f.matches(&ev(s, 1)));
    }

    #[test]
    fn a_filter_selects_by_session() {
        let a = SessionId::new();
        let b = SessionId::new();
        let f = Filter { kinds: vec![], sessions: vec![a] };
        assert!(f.matches(&ev(a, 1)));
        assert!(!f.matches(&ev(b, 1)));
    }

    #[test]
    fn an_empty_filter_matches_everything() {
        let s = SessionId::new();
        assert!(Filter::default().matches(&ev(s, 1)));
    }

    #[test]
    fn events_round_trip() {
        let s = SessionId::new();
        let e = ev(s, 1);
        let json = serde_json::to_string(&e).expect("serialize");
        let back: Event = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, back);
    }

    #[test]
    fn an_interaction_event_carries_the_whole_object() {
        // So a client that missed an earlier transition still converges.
        use crate::interaction::*;
        let i = Interaction {
            id: InteractionId::new(),
            session: SessionId::new(),
            binding: omt_types::BindingId::new(),
            kind: InteractionKind::Text {
                prompt: "?".into(),
                placeholder: None,
                multiline: false,
            },
            deliverable: Deliverable::Native,
            state: InteractionState::Open,
            opened_at: Timestamp::UNIX_EPOCH,
            expires_at: None,
        };
        let p = EventPayload::Interaction { interaction: Box::new(i.clone()) };
        assert_eq!(p.kind(), EventKind::Interaction);
        let json = serde_json::to_string(&p).expect("serialize");
        let back: EventPayload = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
    }
}
