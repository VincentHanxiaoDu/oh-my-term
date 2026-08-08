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
    runtimes: BTreeMap<SessionId, crate::SessionRuntime>,
    rosters: BTreeMap<SessionId, omt_agent::ThreadRoster>,
    /// Terminals for sessions restored from a snapshot, which have no pty.
    orphans: BTreeMap<SessionId, omt_term::Terminal>,
    /// What each session has spent, and what any agent said about its limits.
    ///
    /// Public because it is read straight through by the capability that
    /// reports it, and there is nothing to guard: it is an accumulator, and
    /// wrapping it in methods that only forward would be ceremony.
    pub usage: omt_agent::UsageLedger,
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
            runtimes: BTreeMap::new(),
            rosters: BTreeMap::new(),
            orphans: BTreeMap::new(),
            usage: omt_agent::UsageLedger::new(),
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

    /// Feed a source reading into a binding's merge machine.
    pub fn observe_agent(&mut self, binding: BindingId, reading: SourceReading) {
        self.merges.entry(binding).or_default().observe(reading);
    }

    /// Note what an agent reported about a session's spending.
    ///
    /// Named apart from `observe_agent` deliberately: that one feeds the state
    /// machine that decides what an agent is *doing*, and this one feeds the
    /// ledger of what it has *spent*. Two different questions, and a shared
    /// name would let a call meant for one silently reach the other.
    pub fn observe_usage(&mut self, session: SessionId, payload: &omt_events::AgentPayload) {
        self.usage.observe(session, payload);
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
        // The runtime goes with it, which is what actually stops the process:
        // dropping a Pty hangs its child up.
        self.runtimes.remove(&id);
        self.rosters.remove(&id);
        Ok(())
    }

    /// Attach a running process to a session that already exists.
    ///
    /// # Errors
    /// Fails if the session is unknown — a runtime with no session would emit
    /// events into a stream nothing is reading.
    pub fn attach(&mut self, runtime: crate::SessionRuntime) -> Result<(), omt_session::TreeError> {
        let id = runtime.id;
        if self.tree.session(id).is_none() {
            return Err(omt_session::TreeError::NoSession(id));
        }
        self.runtimes.insert(id, runtime);
        Ok(())
    }

    /// A session's runtime.
    #[must_use]
    pub fn runtime(&self, id: SessionId) -> Option<&crate::SessionRuntime> {
        self.runtimes.get(&id)
    }

    /// Whether a workspace is already open.
    #[must_use]
    pub fn workspace_exists(&self, id: WorkspaceId) -> bool {
        self.tree.workspace(id).is_some()
    }

    /// Every workspace.
    #[must_use]
    pub fn workspaces(&self) -> Vec<&omt_session::Workspace> {
        self.tree.workspaces()
    }

    /// Write input to a session, checked against the writer token.
    ///
    /// The token lives here rather than on the runtime because it is a property
    /// of the session — a remote client and the local TUI contend for the same
    /// one, and two copies would let both believe they held it.
    ///
    /// # Errors
    /// Fails if the session is unknown, or if the epoch is stale.
    pub fn write_session_input(
        &mut self,
        id: SessionId,
        epoch: omt_session::Epoch,
        bytes: &[u8],
    ) -> Result<usize, crate::RuntimeError> {
        let Some(session) = self.tree.session_mut(id) else {
            return Err(crate::RuntimeError::Io(format!("no session {id}")));
        };
        let mut writer = std::mem::take(&mut session.writer);
        let result = self.runtimes.get_mut(&id).map_or_else(
            || Err(crate::RuntimeError::Io(format!("no runtime for {id}"))),
            |runtime| runtime.write_input(&mut writer, epoch, now_ms(), bytes),
        );
        if let Some(session) = self.tree.session_mut(id) {
            session.writer = writer;
        }
        result
    }

    /// Take the writer token for a session.
    ///
    /// Returns the epoch the caller now holds. Every write is checked against
    /// it, so a client that reconnects and resumes typing with a stale epoch is
    /// rejected rather than landing in whatever the new holder is composing.
    ///
    /// # Errors
    /// Fails if the session is unknown, or if somebody else holds the token and
    /// `force` was not asked for.
    pub fn acquire_writer(
        &mut self,
        id: SessionId,
        by: omt_types::Actor,
        force: bool,
    ) -> Result<omt_session::Epoch, crate::RuntimeError> {
        let now = now_ms();
        let Some(session) = self.tree.session_mut(id) else {
            return Err(crate::RuntimeError::Io(format!("no session {id}")));
        };
        session
            .writer
            .acquire(by, now, force, false)
            .map_err(|e| crate::RuntimeError::Io(e.to_string()))
    }

    /// Give the writer token up.
    ///
    /// Idempotent: releasing a token this actor does not hold is not an error,
    /// because a client that already lost it on a takeover would otherwise see
    /// a failure for doing exactly the right thing.
    pub fn release_writer(&mut self, id: SessionId, by: &omt_types::Actor) -> bool {
        self.tree
            .session_mut(id)
            .is_some_and(|s| s.writer.release(by))
    }

    /// Bring a session back from a snapshot, with nothing behind it.
    ///
    /// Its pty is gone — one does not survive the process that opened it — so
    /// this restores everything that did: the title, the directory, and the
    /// screen as it stood. The result is readable, searchable and copyable, and
    /// refuses writes rather than accepting bytes nothing will ever read.
    pub fn restore_orphan(
        &mut self,
        workspace: WorkspaceId,
        title: &str,
        cwd: Option<&str>,
        screen: &[String],
    ) -> Option<SessionId> {
        let id = self
            .tree
            .create_session(workspace, omt_session::SessionKind::Shell, SessionMode::Pty)
            .ok()?;
        self.streams.insert(id, Stream::new());

        // A terminal with no pty behind it, holding what the session last
        // showed. Sized to the content rather than to a default, so a restored
        // screen is not padded out to eighty columns of nothing.
        let cols = screen
            .iter()
            .map(|l| u16::try_from(l.chars().count()).unwrap_or(u16::MAX))
            .max()
            .unwrap_or(80)
            .max(1);
        let rows = u16::try_from(screen.len()).unwrap_or(u16::MAX).max(1);
        let mut terminal = omt_term::Terminal::new(omt_term::TermConfig {
            size: omt_term::GridSize::new(cols, rows),
            ..omt_term::TermConfig::default()
        });
        for (i, line) in screen.iter().enumerate() {
            if i > 0 {
                terminal.advance(b"\r\n");
            }
            terminal.advance(line.as_bytes());
        }
        self.orphans.insert(id, terminal);

        if let Some(session) = self.tree.session_mut(id) {
            session.title = title.to_owned();
            session.cwd = cwd.map(str::to_owned);
            session.state = omt_session::SessionState::Orphaned;
        }
        Some(id)
    }

    /// Put a process behind a session that has none.
    ///
    /// The id is kept, so every pane, every card and every client still
    /// referring to this session keeps working. The old screen is written into
    /// the new terminal above a separator: what you were looking at when the
    /// daemon went down is usually why you are restarting it.
    ///
    /// # Errors
    /// Fails if the process cannot be started.
    pub fn respawn(
        &mut self,
        id: SessionId,
        config: &omt_pty::PtyConfig,
    ) -> Result<(), crate::RuntimeError> {
        let previous = self
            .orphans
            .remove(&id)
            .map(|t| t.screen_text())
            .unwrap_or_default();

        let runtime =
            crate::SessionRuntime::spawn(id, config, omt_term::ScrollbackLimits::default())
                .map_err(|e| crate::RuntimeError::Io(e.to_string()))?;
        self.runtimes.insert(id, runtime);
        if let Some(session) = self.tree.session_mut(id) {
            session.state = omt_session::SessionState::Starting;
        }

        if let Some(rt) = self.runtimes.get_mut(&id) {
            for line in previous.iter().filter(|l| !l.trim().is_empty()) {
                rt.terminal_mut().advance(line.as_bytes());
                rt.terminal_mut().advance(b"\r\n");
            }
            if !previous.is_empty() {
                // A visible break, because output from before a restart mixed
                // into output from after it is worse than losing it.
                rt.terminal_mut()
                    .advance("\r\n\u{2500}\u{2500} restarted \u{2500}\u{2500}\r\n".as_bytes());
            }
        }
        Ok(())
    }

    /// The terminal of a restored session, which has no runtime.
    #[must_use]
    pub fn orphan_terminal(&self, id: SessionId) -> Option<&omt_term::Terminal> {
        self.orphans.get(&id)
    }

    /// Put a pane in a workspace's primary view.
    ///
    /// The pane holds the session's id rather than the session, which is what
    /// makes "closing a pane never kills a session" structural instead of a
    /// rule somebody has to remember.
    pub fn add_pane(
        &mut self,
        workspace: WorkspaceId,
        session: SessionId,
    ) -> Option<omt_types::PaneId> {
        let ws = self.tree.workspace_mut(workspace)?;
        let primary = ws.primary;
        ws.views.get_mut(&primary).map(|v| v.add_pane(session))
    }

    /// Remove a pane. The session it showed keeps running.
    pub fn remove_pane(&mut self, workspace: WorkspaceId, pane: omt_types::PaneId) -> bool {
        let Some(ws) = self.tree.workspace_mut(workspace) else {
            return false;
        };
        let primary = ws.primary;
        ws.views
            .get_mut(&primary)
            .is_some_and(|v| v.remove_pane(pane))
    }

    /// Move focus to a pane.
    pub fn focus_pane(&mut self, workspace: WorkspaceId, pane: omt_types::PaneId) -> bool {
        let Some(ws) = self.tree.workspace_mut(workspace) else {
            return false;
        };
        let primary = ws.primary;
        ws.views.get_mut(&primary).is_some_and(|v| {
            if v.panes.iter().any(|p| p.id == pane) {
                v.focus = Some(pane);
                true
            } else {
                false
            }
        })
    }

    /// The panes of a workspace's primary view, in order, with the focused one.
    #[must_use]
    pub fn panes(
        &self,
        workspace: WorkspaceId,
    ) -> Option<(Vec<omt_session::Pane>, Option<omt_types::PaneId>)> {
        let ws = self.tree.workspace(workspace)?;
        let view = ws.views.get(&ws.primary)?;
        Some((view.panes.clone(), view.focus))
    }

    /// One session, mutably.
    pub fn session_mut(&mut self, id: SessionId) -> Option<&mut omt_session::Session> {
        self.tree.session_mut(id)
    }

    /// A workspace's canonical root.
    #[must_use]
    pub fn workspace_root(&self, id: WorkspaceId) -> Option<String> {
        self.tree.workspace(id).map(|w| w.root.clone())
    }

    /// Every session.
    #[must_use]
    pub fn sessions(&self) -> Vec<&omt_session::Session> {
        self.tree.sessions()
    }

    /// A session's thread roster — its main thread and each subagent.
    #[must_use]
    pub fn threads(&self, id: SessionId) -> Option<&omt_agent::ThreadRoster> {
        self.rosters.get(&id)
    }

    /// Fold an agent observation into a session's roster.
    pub fn observe_thread(&mut self, id: SessionId, event: &omt_events::AgentEvent) {
        self.rosters.entry(id).or_default().observe(event);
    }

    /// A session, from the tree.
    #[must_use]
    pub fn session(&self, id: SessionId) -> Option<&omt_session::Session> {
        self.tree.session(id)
    }

    /// A session's runtime, mutably.
    pub fn runtime_mut(&mut self, id: SessionId) -> Option<&mut crate::SessionRuntime> {
        self.runtimes.get_mut(&id)
    }

    /// Move one session forward and turn what happened into events.
    ///
    /// The one place a host action becomes an event with a position on it. Two
    /// callers doing this independently would number the same observation
    /// twice.
    ///
    /// # Errors
    /// Fails if the session has no runtime, or on an I/O error that is not the
    /// end of the stream.
    pub fn pump_session(&mut self, id: SessionId) -> Result<Vec<Event>, crate::RuntimeError> {
        let Some(runtime) = self.runtimes.get_mut(&id) else {
            return Ok(Vec::new());
        };
        let pumped = runtime.pump()?;

        // Collected before emitting, because emitting borrows self mutably and
        // the runtime is borrowed from the same place.
        let mut pending: Vec<(EventSourceTag, EventPayload)> = Vec::new();
        if pumped.bytes > 0 {
            pending.push((
                EventSourceTag::Core,
                EventPayload::Terminal(omt_events::TerminalEvent::Output {
                    bytes: pumped.bytes as u64,
                }),
            ));
        }
        for action in &pumped.actions {
            pending.extend(runtime.event_for(action));
        }
        if let Some(status) = pumped.exited {
            pending.push((
                EventSourceTag::Core,
                EventPayload::SessionTree(omt_events::SessionTreeEvent::SessionClosed {
                    session: id,
                    code: match status {
                        omt_pty::ExitStatus::Code(c) => Some(c),
                        // A signalled process has no exit code, and inventing
                        // one would make "killed" indistinguishable from a
                        // program that returned that number.
                        omt_pty::ExitStatus::Signal(_) => None,
                    },
                }),
            ));
        }

        let mut out = Vec::new();
        for (source, payload) in pending {
            out.extend(self.emit(id, source, payload));
        }
        if pumped.exited.is_some() {
            // The process is gone; the session record should say so before any
            // client asks.
            if let Some(session) = self.tree.session_mut(id) {
                session.state = omt_session::SessionState::Exited {
                    code: pumped.exited.and_then(|s| match s {
                        omt_pty::ExitStatus::Code(c) => Some(c),
                        omt_pty::ExitStatus::Signal(_) => None,
                    }),
                };
                session.exited_at = Some(Timestamp::now());
            }
        }
        Ok(out)
    }

    /// Move every attached session forward.
    ///
    /// # Errors
    /// Fails on the first session that errors, having already emitted whatever
    /// the ones before it produced.
    pub fn pump_all(&mut self) -> Result<Vec<Event>, crate::RuntimeError> {
        let ids: Vec<SessionId> = self.runtimes.keys().copied().collect();
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.pump_session(id)?);
        }
        Ok(out)
    }

    /// How many sessions have live streams.
    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }
}

/// Milliseconds since this process started.
///
/// Monotonic rather than wall-clock: the writer token's idle timeout must not
/// move because somebody's clock was corrected.
fn now_ms() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
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
