//! The running instance: one place that owns the tree, the ledger and the
//! event stream, and the one place that assigns sequence numbers.
//!
//! Sequence assignment being centralized is the point. Every durable client
//! position is a `(scope, seq)` pair, and two components that both minted
//! sequence numbers would produce two events with the same position and
//! different contents — which a resuming client cannot detect and cannot
//! recover from.

use std::collections::BTreeMap;

use omt_agent::{Ledger, MergeMachine, SourceReading};
use omt_events::{Event, EventPayload, EventSourceTag, ReplayWindow, ResumeOutcome};
use omt_session::{Instance as Tree, SessionKind, SessionMode};
use omt_types::{BindingId, Seq, SeqScope, SessionId, Timestamp, WorkspaceId};
use omt_util::SeqGenerator;

/// How much replay an instance keeps per scope.
///
/// Bounded, because a client that is in a tunnel for an hour must not be able
/// to make the daemon hold an hour of terminal output in memory. Past the
/// window a resume is answered with a resync rather than a partial stream.
pub const REPLAY_CAPACITY: usize = 4096;

/// One session's stream state.
struct Stream {
    seq: SeqGenerator,
    window: ReplayWindow,
}

impl Stream {
    fn new() -> Self {
        Self {
            seq: SeqGenerator::new(),
            window: ReplayWindow::new(REPLAY_CAPACITY),
        }
    }
}

/// Everything one daemon owns.
pub struct Instance {
    /// The session tree.
    pub tree: Tree,
    /// The interaction ledger.
    pub ledger: Ledger,
    streams: BTreeMap<SessionId, Stream>,
    instance_stream: Stream,
    merges: BTreeMap<BindingId, MergeMachine>,
}

impl Default for Instance {
    fn default() -> Self {
        Self::new()
    }
}

impl Instance {
    /// A fresh instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tree: Tree::default(),
            ledger: Ledger::new(),
            streams: BTreeMap::new(),
            instance_stream: Stream::new(),
            merges: BTreeMap::new(),
        }
    }

    /// Open a workspace.
    ///
    /// # Errors
    /// Fails if a limit is reached.
    pub fn open_workspace(&mut self, root: &str) -> Result<WorkspaceId, omt_session::TreeError> {
        self.tree.open_workspace(root)
    }

    /// Start a session and begin its stream.
    ///
    /// # Errors
    /// Fails if the workspace is unknown or a limit is reached.
    pub fn create_session(
        &mut self,
        workspace: WorkspaceId,
        kind: SessionKind,
        mode: SessionMode,
    ) -> Result<SessionId, omt_session::TreeError> {
        let id = self.tree.create_session(workspace, kind, mode)?;
        self.streams.insert(id, Stream::new());
        Ok(id)
    }

    /// Record an event against a session, assigning its position.
    ///
    /// The position is assigned *here* and nowhere else. Two components minting
    /// their own would produce two events at one position with different
    /// contents, which a resuming client has no way to notice.
    pub fn emit(
        &mut self,
        session: SessionId,
        source: EventSourceTag,
        payload: EventPayload,
    ) -> Option<Event> {
        let stream = self.streams.get_mut(&session)?;
        let event = Event {
            scope: SeqScope::Session { session },
            seq: Seq::new(stream.seq.next()),
            ts: Timestamp::now(),
            source,
            payload,
        };
        stream.window.push(event.clone());
        Some(event)
    }

    /// Record an instance-wide event.
    pub fn emit_instance(&mut self, source: EventSourceTag, payload: EventPayload) -> Event {
        let event = Event {
            scope: SeqScope::Instance,
            seq: Seq::new(self.instance_stream.seq.next()),
            ts: Timestamp::now(),
            source,
            payload,
        };
        self.instance_stream.window.push(event.clone());
        event
    }

    /// What a client that was last at `after` should receive.
    ///
    /// Returns `None` for a session that does not exist, which is different
    /// from an empty replay: a client asking about a closed session needs to be
    /// told it is gone rather than handed silence.
    pub fn resume(&self, session: SessionId, after: Seq) -> Option<ResumeOutcome> {
        Some(self.streams.get(&session)?.window.resume_from(after))
    }

    /// The instance stream's replay.
    #[must_use]
    pub fn resume_instance(&self, after: Seq) -> ResumeOutcome {
        self.instance_stream.window.resume_from(after)
    }

    /// The newest position a session has reached.
    #[must_use]
    pub fn position(&self, session: SessionId) -> Option<Seq> {
        self.streams
            .get(&session)
            .map(|s| Seq::new(s.seq.current()))
    }

    /// Feed a source reading into a binding's merge machine.
    pub fn observe_agent(&mut self, binding: BindingId, reading: SourceReading) {
        self.merges.entry(binding).or_default().observe(reading);
    }

    /// What a binding's agent is doing, as of now.
    pub fn agent_state(
        &mut self,
        binding: BindingId,
        now_ms: u64,
    ) -> Option<omt_types::AgentState> {
        Some(self.merges.get_mut(&binding)?.state(now_ms))
    }

    /// Why the merge machine thinks so.
    pub fn explain_agent(
        &mut self,
        binding: BindingId,
        now_ms: u64,
    ) -> Option<(omt_types::AgentState, omt_agent::Explanation)> {
        Some(self.merges.get_mut(&binding)?.explain(now_ms))
    }

    /// Close a session, ending its stream.
    ///
    /// # Errors
    /// Fails if the session is unknown.
    pub fn close_session(&mut self, id: SessionId) -> Result<(), omt_session::TreeError> {
        self.tree.close_session(id)?;
        // The stream goes with it. Keeping it would let a client resume a
        // session that no longer exists and receive silence rather than an
        // answer.
        self.streams.remove(&id);
        Ok(())
    }

    /// How many sessions have live streams.
    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.streams.len()
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
    use omt_events::ActivityGuess;
    use omt_types::{AgentState, Tier};

    fn instance() -> (Instance, SessionId) {
        let mut i = Instance::new();
        let ws = i.open_workspace("/w").expect("workspace");
        let s = i
            .create_session(ws, SessionKind::Shell, SessionMode::Pty)
            .expect("session");
        (i, s)
    }

    fn activity(state: ActivityGuess) -> EventPayload {
        EventPayload::Agent {
            event: Box::new(omt_events::AgentEvent {
                session: SessionId::new(),
                binding: BindingId::new(),
                agent: omt_types::AgentKind::Unknown,
                agent_version: None,
                agent_session: None,
                thread: None,
                seq: Seq::new(0),
                ts: Timestamp::now(),
                tier: Tier::Heuristic,
                payload: omt_events::AgentPayload::Activity { state },
            }),
        }
    }

    #[test]
    fn positions_within_a_session_are_consecutive() {
        // Every durable client position is a (scope, seq) pair, so a gap is
        // indistinguishable from a lost event.
        let (mut i, s) = instance();
        let a = i
            .emit(s, EventSourceTag::Heuristic, activity(ActivityGuess::Busy))
            .expect("emit");
        let b = i
            .emit(s, EventSourceTag::Heuristic, activity(ActivityGuess::Idle))
            .expect("emit");
        assert_eq!(b.seq.get(), a.seq.get() + 1);
    }

    #[test]
    fn two_sessions_number_independently() {
        // A shared counter would make one session's activity advance another's
        // position, and a client resuming the quiet one would skip events.
        let mut i = Instance::new();
        let ws = i.open_workspace("/w").expect("workspace");
        let a = i
            .create_session(ws, SessionKind::Shell, SessionMode::Pty)
            .expect("a");
        let b = i
            .create_session(ws, SessionKind::Shell, SessionMode::Pty)
            .expect("b");

        for _ in 0..5 {
            i.emit(a, EventSourceTag::Heuristic, activity(ActivityGuess::Busy));
        }
        let first_b = i
            .emit(b, EventSourceTag::Heuristic, activity(ActivityGuess::Busy))
            .expect("emit");
        assert_eq!(
            first_b.seq.get(),
            1,
            "b's stream starts at its own beginning, not where a is"
        );
    }

    #[test]
    fn the_instance_stream_is_separate_from_every_session() {
        let (mut i, s) = instance();
        i.emit(s, EventSourceTag::Heuristic, activity(ActivityGuess::Busy));
        let e = i.emit_instance(EventSourceTag::Heuristic, activity(ActivityGuess::Idle));
        assert_eq!(e.scope, SeqScope::Instance);
        assert_eq!(e.seq.get(), 1, "its own counter, from its own beginning");
    }

    #[test]
    fn an_event_carries_the_scope_it_was_numbered_in() {
        // Without it, a client holding a position cannot tell which counter it
        // refers to.
        let (mut i, s) = instance();
        let e = i
            .emit(s, EventSourceTag::Heuristic, activity(ActivityGuess::Busy))
            .expect("emit");
        assert_eq!(e.scope, SeqScope::Session { session: s });
    }

    #[test]
    fn a_client_within_the_window_gets_the_events_it_missed() {
        let (mut i, s) = instance();
        for _ in 0..5 {
            i.emit(s, EventSourceTag::Heuristic, activity(ActivityGuess::Busy));
        }
        // Five events occupy positions 1 through 5, so a client last at 2 is
        // owed three.
        let outcome = i.resume(s, Seq::new(2)).expect("session exists");
        let ResumeOutcome::Replayed { events } = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(
            events.iter().map(|e| e.seq.get()).collect::<Vec<_>>(),
            [3, 4, 5]
        );
    }

    #[test]
    fn a_client_past_the_window_is_told_to_resync_rather_than_given_a_gap() {
        // A partial stream a client cannot detect is worse than an explicit
        // "start again".
        let (mut i, s) = instance();
        for _ in 0..(REPLAY_CAPACITY + 100) {
            i.emit(s, EventSourceTag::Heuristic, activity(ActivityGuess::Busy));
        }
        let outcome = i.resume(s, Seq::new(0)).expect("session exists");
        assert!(
            matches!(outcome, ResumeOutcome::Resync { .. }),
            "{outcome:?}"
        );
    }

    #[test]
    fn resuming_a_session_that_never_existed_is_distinguishable_from_silence() {
        // A client asking about a closed session needs to be told it is gone.
        let i = Instance::new();
        assert!(i.resume(SessionId::new(), Seq::new(0)).is_none());
    }

    #[test]
    fn closing_a_session_ends_its_stream() {
        let (mut i, s) = instance();
        i.emit(s, EventSourceTag::Heuristic, activity(ActivityGuess::Busy));
        assert_eq!(i.stream_count(), 1);
        i.close_session(s).expect("close");
        assert_eq!(i.stream_count(), 0);
        assert!(
            i.resume(s, Seq::new(0)).is_none(),
            "and a resume says so rather than returning nothing"
        );
    }

    #[test]
    fn emitting_against_an_unknown_session_produces_nothing() {
        // Rather than minting a position in a stream nobody is reading.
        let mut i = Instance::new();
        assert!(
            i.emit(
                SessionId::new(),
                EventSourceTag::Heuristic,
                activity(ActivityGuess::Busy)
            )
            .is_none()
        );
    }

    #[test]
    fn an_agent_binding_reports_state_through_the_merge_machine() {
        let mut i = Instance::new();
        let binding = BindingId::new();
        i.observe_agent(
            binding,
            SourceReading {
                tier: Tier::Hook,
                state: AgentState::Working { detail: None },
                at: 1_000,
                authoritative: true,
            },
        );
        assert_eq!(
            i.agent_state(binding, 1_000),
            Some(AgentState::Working { detail: None })
        );
    }

    #[test]
    fn an_unknown_binding_has_no_state_rather_than_a_default() {
        // Reporting Idle for a binding nothing has observed would show "waiting
        // for you" about an agent that does not exist.
        let mut i = Instance::new();
        assert_eq!(i.agent_state(BindingId::new(), 0), None);
    }

    #[test]
    fn the_explanation_is_available_alongside_the_state() {
        let mut i = Instance::new();
        let binding = BindingId::new();
        i.observe_agent(
            binding,
            SourceReading {
                tier: Tier::Hook,
                state: AgentState::Idle,
                at: 0,
                authoritative: false,
            },
        );
        let (_, why) = i.explain_agent(binding, 0).expect("known binding");
        assert_eq!(why.winner, Tier::Hook);
    }

    #[test]
    fn a_sessions_position_is_readable_without_emitting() {
        let (mut i, s) = instance();
        i.emit(s, EventSourceTag::Heuristic, activity(ActivityGuess::Busy));
        let before = i.position(s).expect("position");
        assert_eq!(
            i.position(s).expect("position"),
            before,
            "reading is not a write"
        );
    }
}
