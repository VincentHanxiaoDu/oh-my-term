//! Instance → Workspace → Session, and the panes that look at them.
//!
//! The relationship most multiplexers get wrong is **pane → session is
//! many-to-one**. A pane is presentation: one session may be visible in several
//! panes, in several views, on several clients at once, and closing a pane never
//! kills anything. Sessions are stored flat and authoritative; panes only point
//! at them.

use std::collections::BTreeMap;

use omt_types::{AgentKind, ClientId, PaneId, SessionId, Timestamp, ViewId, WorkspaceId};

use crate::writer::{WriterPolicy, WriterState};

/// Where a session is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Spawned, not yet producing output.
    Starting,
    /// Running.
    Running,
    /// The process is gone.
    Exited {
        /// Its exit code, where one was observed.
        code: Option<i32>,
    },
}

/// What a session is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionKind {
    /// An interactive shell.
    Shell,
    /// One command.
    Command {
        /// Its argv.
        argv: Vec<String>,
    },
    /// An agent.
    Agent {
        /// Which one.
        kind: AgentKind,
    },
}

/// How a session is driven.
///
/// Chosen at creation and immutable for its life: a session cannot become a
/// different program halfway through, and pretending otherwise would leave
/// every observer's model of it wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// The agent's own CLI on a pty.
    Pty,
    /// Driven over a protocol.
    Native,
}

/// One logical terminal.
#[derive(Debug, Clone)]
pub struct Session {
    /// Its identity.
    pub id: SessionId,
    /// Which workspace it belongs to. Fixed at creation.
    pub workspace: WorkspaceId,
    /// Its title.
    pub title: String,
    /// What it is running.
    pub kind: SessionKind,
    /// How it is driven.
    pub mode: SessionMode,
    /// Where it is in its life.
    pub state: SessionState,
    /// Its live working directory, which is **not** its workspace.
    ///
    /// The workspace is identity and does not follow `cd`; this does. Conflating
    /// them makes "open a new session here" open one somewhere else.
    pub cwd: Option<String>,
    /// Who may type into it.
    pub writer: WriterState,
    /// Which clients are looking.
    pub viewers: Vec<ClientId>,
    /// When it started.
    pub created_at: Timestamp,
    /// When it ended.
    pub exited_at: Option<Timestamp>,
}

impl Session {
    /// A new session in a workspace.
    #[must_use]
    pub fn new(workspace: WorkspaceId, kind: SessionKind, mode: SessionMode) -> Self {
        Self {
            id: SessionId::new(),
            workspace,
            title: match &kind {
                SessionKind::Shell => "shell".to_owned(),
                SessionKind::Command { argv } => argv.first().cloned().unwrap_or_default(),
                SessionKind::Agent { kind } => format!("{kind:?}"),
            },
            kind,
            mode,
            state: SessionState::Starting,
            cwd: None,
            writer: WriterState::new(WriterPolicy::default()),
            viewers: Vec::new(),
            created_at: Timestamp::now(),
            exited_at: None,
        }
    }

    /// Whether the process is gone.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        matches!(self.state, SessionState::Exited { .. })
    }
}

/// A viewport onto a session.
///
/// Presentation only. It holds an id rather than the session itself, which is
/// what makes "closing a pane never kills a session" structural rather than a
/// rule somebody has to remember.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    /// Its identity, unique within a view.
    pub id: PaneId,
    /// What it is looking at.
    pub session: SessionId,
}

/// One arrangement of panes.
#[derive(Debug, Clone, Default)]
pub struct LayoutView {
    /// Its identity.
    pub id: ViewId,
    /// Its name.
    pub name: String,
    /// The panes in it.
    pub panes: Vec<Pane>,
    /// Which pane has focus.
    pub focus: Option<PaneId>,
    /// A pane shown alone, temporarily.
    pub zoom: Option<PaneId>,
}

impl LayoutView {
    /// A view with a name.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            id: ViewId::new(),
            name: name.to_owned(),
            ..Self::default()
        }
    }

    /// Add a pane looking at a session.
    pub fn add_pane(&mut self, session: SessionId) -> PaneId {
        let pane = Pane {
            id: PaneId::new(),
            session,
        };
        let id = pane.id;
        self.panes.push(pane);
        if self.focus.is_none() {
            self.focus = Some(id);
        }
        id
    }

    /// Remove a pane. The session it showed is untouched.
    pub fn remove_pane(&mut self, id: PaneId) -> bool {
        let before = self.panes.len();
        self.panes.retain(|p| p.id != id);
        if self.focus == Some(id) {
            // Focus moves to whatever is left rather than vanishing: a view
            // with panes and no focus swallows every keystroke.
            self.focus = self.panes.first().map(|p| p.id);
        }
        if self.zoom == Some(id) {
            self.zoom = None;
        }
        self.panes.len() != before
    }

    /// Every pane showing a session.
    #[must_use]
    pub fn panes_for(&self, session: SessionId) -> Vec<PaneId> {
        self.panes
            .iter()
            .filter(|p| p.session == session)
            .map(|p| p.id)
            .collect()
    }
}

/// A project root.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Its identity.
    pub id: WorkspaceId,
    /// Its canonical path, which *is* its identity.
    pub root: String,
    /// Its display name.
    pub name: String,
    /// Its layout views.
    pub views: BTreeMap<ViewId, LayoutView>,
    /// The view a client gets when it asks for nothing.
    pub primary: ViewId,
    /// Its sessions, in creation order.
    pub sessions: Vec<SessionId>,
    /// When it was opened.
    pub created_at: Timestamp,
}

impl Workspace {
    /// A workspace at a canonical path.
    #[must_use]
    pub fn new(root: &str) -> Self {
        let view = LayoutView::new("main");
        let primary = view.id;
        let name = root
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(root)
            .to_owned();
        let mut views = BTreeMap::new();
        views.insert(primary, view);
        Self {
            id: WorkspaceId::from_canonical_path(root),
            root: root.to_owned(),
            name,
            views,
            primary,
            sessions: Vec::new(),
            created_at: Timestamp::now(),
        }
    }
}

/// What went wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeError {
    /// No such workspace.
    #[error("no workspace {0}")]
    NoWorkspace(WorkspaceId),
    /// No such session.
    #[error("no session {0}")]
    NoSession(SessionId),
    /// No such view.
    #[error("no view {0}")]
    NoView(ViewId),
    /// A limit was reached.
    #[error("{what} limit of {limit} reached")]
    LimitReached {
        /// What was capped.
        what: &'static str,
        /// The cap.
        limit: usize,
    },
}

/// How much one instance may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceLimits {
    /// Most sessions.
    pub max_sessions: usize,
    /// Most workspaces.
    pub max_workspaces: usize,
}

impl Default for InstanceLimits {
    fn default() -> Self {
        Self {
            max_sessions: 256,
            max_workspaces: 64,
        }
    }
}

/// Everything one daemon holds.
#[derive(Debug, Clone)]
pub struct Instance {
    workspaces: BTreeMap<WorkspaceId, Workspace>,
    /// Flat and authoritative. Panes point in here; nothing owns a session but
    /// this map.
    sessions: BTreeMap<SessionId, Session>,
    limits: InstanceLimits,
}

impl Default for Instance {
    fn default() -> Self {
        Self::new(InstanceLimits::default())
    }
}

impl Instance {
    /// An empty instance.
    #[must_use]
    pub fn new(limits: InstanceLimits) -> Self {
        Self {
            workspaces: BTreeMap::new(),
            sessions: BTreeMap::new(),
            limits,
        }
    }

    /// Open a workspace, or return the one already open at that path.
    ///
    /// Idempotent by canonical path, because the path *is* the identity:
    /// opening the same directory twice must not produce two workspaces whose
    /// sessions cannot see each other.
    ///
    /// # Errors
    /// Fails if the workspace limit is reached.
    pub fn open_workspace(&mut self, root: &str) -> Result<WorkspaceId, TreeError> {
        // The id is *derived* from the path, so opening the same directory
        // twice cannot produce two workspaces even by accident — the
        // idempotency is structural rather than a lookup somebody could forget.
        let id = WorkspaceId::from_canonical_path(root);
        if self.workspaces.contains_key(&id) {
            return Ok(id);
        }
        if self.workspaces.len() >= self.limits.max_workspaces {
            return Err(TreeError::LimitReached {
                what: "workspace",
                limit: self.limits.max_workspaces,
            });
        }
        self.workspaces.insert(id, Workspace::new(root));
        Ok(id)
    }

    /// Start a session in a workspace.
    ///
    /// # Errors
    /// Fails if the workspace is unknown or the session limit is reached.
    pub fn create_session(
        &mut self,
        workspace: WorkspaceId,
        kind: SessionKind,
        mode: SessionMode,
    ) -> Result<SessionId, TreeError> {
        if !self.workspaces.contains_key(&workspace) {
            return Err(TreeError::NoWorkspace(workspace));
        }
        if self.sessions.len() >= self.limits.max_sessions {
            return Err(TreeError::LimitReached {
                what: "session",
                limit: self.limits.max_sessions,
            });
        }
        let session = Session::new(workspace, kind, mode);
        let id = session.id;
        self.sessions.insert(id, session);
        if let Some(ws) = self.workspaces.get_mut(&workspace) {
            ws.sessions.push(id);
        }
        Ok(id)
    }

    /// A session.
    #[must_use]
    pub fn session(&self, id: SessionId) -> Option<&Session> {
        self.sessions.get(&id)
    }

    /// A session, mutably.
    pub fn session_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        self.sessions.get_mut(&id)
    }

    /// A workspace.
    #[must_use]
    pub fn workspace(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.get(&id)
    }

    /// A workspace, mutably.
    pub fn workspace_mut(&mut self, id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.get_mut(&id)
    }

    /// Every session, in a stable order.
    #[must_use]
    pub fn sessions(&self) -> Vec<&Session> {
        self.sessions.values().collect()
    }

    /// Every workspace, in a stable order.
    #[must_use]
    pub fn workspaces(&self) -> Vec<&Workspace> {
        self.workspaces.values().collect()
    }

    /// Close a session for good, removing every pane that showed it.
    ///
    /// The opposite direction from closing a pane, and deliberately asymmetric:
    /// a pane going away is a view change, a session going away is the process
    /// ending.
    ///
    /// # Errors
    /// Fails if the session is unknown.
    pub fn close_session(&mut self, id: SessionId) -> Result<(), TreeError> {
        let session = self.sessions.remove(&id).ok_or(TreeError::NoSession(id))?;
        if let Some(ws) = self.workspaces.get_mut(&session.workspace) {
            ws.sessions.retain(|s| *s != id);
            for view in ws.views.values_mut() {
                for pane in view.panes_for(id) {
                    view.remove_pane(pane);
                }
            }
        }
        Ok(())
    }

    /// Every pane, in every view, showing a session.
    ///
    /// Many-to-one made concrete: this is what a "who is watching" query needs,
    /// and it is why a bare `PaneId` is never enough to identify anything
    /// across clients.
    #[must_use]
    pub fn panes_showing(&self, session: SessionId) -> Vec<(ViewId, PaneId)> {
        self.workspaces
            .values()
            .flat_map(|w| w.views.values())
            .flat_map(|v| v.panes_for(session).into_iter().map(move |p| (v.id, p)))
            .collect()
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

    fn instance() -> (Instance, WorkspaceId) {
        let mut i = Instance::default();
        let ws = i.open_workspace("/home/me/project").expect("open");
        (i, ws)
    }

    fn shell(i: &mut Instance, ws: WorkspaceId) -> SessionId {
        i.create_session(ws, SessionKind::Shell, SessionMode::Pty)
            .expect("create")
    }

    #[test]
    fn opening_the_same_path_twice_gives_one_workspace() {
        // The path is the identity. Two workspaces for one directory would give
        // it two sets of sessions that cannot see each other.
        let mut i = Instance::default();
        let a = i.open_workspace("/w").expect("first");
        let b = i.open_workspace("/w").expect("second");
        assert_eq!(a, b);
        assert_eq!(i.workspaces().len(), 1);
    }

    #[test]
    fn a_workspace_id_is_derived_from_its_path() {
        // Which is what makes opening the same directory twice structurally
        // one workspace rather than a lookup somebody could forget to do.
        assert_eq!(
            WorkspaceId::from_canonical_path("/w"),
            WorkspaceId::from_canonical_path("/w")
        );
        assert_ne!(
            WorkspaceId::from_canonical_path("/w"),
            WorkspaceId::from_canonical_path("/x")
        );
    }

    #[test]
    fn several_sessions_in_one_directory_is_the_normal_case() {
        // Agent, dev server and shell in one project — not an edge case.
        let (mut i, ws) = instance();
        for _ in 0..3 {
            shell(&mut i, ws);
        }
        assert_eq!(i.workspace(ws).expect("ws").sessions.len(), 3);
    }

    #[test]
    fn closing_a_pane_never_kills_a_session() {
        // The relationship most multiplexers get wrong. A pane is presentation.
        let (mut i, ws) = instance();
        let s = shell(&mut i, ws);
        let view = i.workspace(ws).expect("ws").primary;
        let pane = i
            .workspace_mut(ws)
            .expect("ws")
            .views
            .get_mut(&view)
            .expect("view")
            .add_pane(s);

        i.workspace_mut(ws)
            .expect("ws")
            .views
            .get_mut(&view)
            .expect("view")
            .remove_pane(pane);

        assert!(i.session(s).is_some(), "the session outlived its viewport");
    }

    #[test]
    fn one_session_can_be_shown_in_several_panes_at_once() {
        // Many-to-one, across views as well as within one.
        let (mut i, ws) = instance();
        let s = shell(&mut i, ws);
        let primary = i.workspace(ws).expect("ws").primary;
        let second = LayoutView::new("second");
        let second_id = second.id;
        {
            let w = i.workspace_mut(ws).expect("ws");
            w.views.insert(second_id, second);
            w.views.get_mut(&primary).expect("view").add_pane(s);
            w.views.get_mut(&primary).expect("view").add_pane(s);
            w.views.get_mut(&second_id).expect("view").add_pane(s);
        }
        let showing = i.panes_showing(s);
        assert_eq!(showing.len(), 3);
        assert_eq!(
            showing.iter().filter(|(v, _)| *v == second_id).count(),
            1,
            "including one in the other view"
        );
    }

    #[test]
    fn two_views_showing_one_session_hold_different_pane_ids() {
        // Which is why anything crossing a client boundary is keyed on
        // (view, session) and never on a bare pane id.
        let (mut i, ws) = instance();
        let s = shell(&mut i, ws);
        let primary = i.workspace(ws).expect("ws").primary;
        let other = LayoutView::new("other");
        let other_id = other.id;
        let (a, b) = {
            let w = i.workspace_mut(ws).expect("ws");
            w.views.insert(other_id, other);
            let a = w.views.get_mut(&primary).expect("view").add_pane(s);
            let b = w.views.get_mut(&other_id).expect("view").add_pane(s);
            (a, b)
        };
        assert_ne!(a, b);
    }

    #[test]
    fn closing_a_session_removes_every_pane_that_showed_it() {
        // The asymmetry: a pane going away is a view change, a session going
        // away is the process ending.
        let (mut i, ws) = instance();
        let s = shell(&mut i, ws);
        let view = i.workspace(ws).expect("ws").primary;
        i.workspace_mut(ws)
            .expect("ws")
            .views
            .get_mut(&view)
            .expect("view")
            .add_pane(s);

        i.close_session(s).expect("close");
        assert!(i.session(s).is_none());
        assert!(i.panes_showing(s).is_empty());
        assert!(!i.workspace(ws).expect("ws").sessions.contains(&s));
    }

    #[test]
    fn a_workspace_is_not_a_working_directory() {
        // The workspace is identity and does not follow `cd`; the cwd does.
        // Conflating them makes "open a session here" open one elsewhere.
        let (mut i, ws) = instance();
        let s = shell(&mut i, ws);
        i.session_mut(s).expect("session").cwd = Some("/home/me/project/src/deep".to_owned());
        assert_eq!(i.session(s).expect("session").workspace, ws);
        assert_eq!(i.workspace(ws).expect("ws").root, "/home/me/project");
    }

    #[test]
    fn removing_the_focused_pane_moves_focus_rather_than_dropping_it() {
        // A view with panes and no focus swallows every keystroke.
        let (mut i, ws) = instance();
        let a = shell(&mut i, ws);
        let b = shell(&mut i, ws);
        let view_id = i.workspace(ws).expect("ws").primary;
        let w = i.workspace_mut(ws).expect("ws");
        let view = w.views.get_mut(&view_id).expect("view");
        let pa = view.add_pane(a);
        view.add_pane(b);
        assert_eq!(view.focus, Some(pa));
        view.remove_pane(pa);
        assert!(view.focus.is_some());
        assert_ne!(view.focus, Some(pa));
    }

    #[test]
    fn removing_the_last_pane_leaves_no_focus() {
        let mut view = LayoutView::new("v");
        let p = view.add_pane(SessionId::new());
        view.remove_pane(p);
        assert_eq!(view.focus, None, "there is genuinely nothing to focus");
    }

    #[test]
    fn removing_a_zoomed_pane_clears_the_zoom() {
        // A zoom pointing at nothing would render an empty full-screen view.
        let mut view = LayoutView::new("v");
        let p = view.add_pane(SessionId::new());
        view.zoom = Some(p);
        view.remove_pane(p);
        assert_eq!(view.zoom, None);
    }

    #[test]
    fn a_session_cannot_be_created_in_a_workspace_that_does_not_exist() {
        let mut i = Instance::default();
        let err = i
            .create_session(
                WorkspaceId::from_canonical_path("/never-opened"),
                SessionKind::Shell,
                SessionMode::Pty,
            )
            .expect_err("must refuse");
        assert!(matches!(err, TreeError::NoWorkspace(_)));
    }

    #[test]
    fn the_session_limit_is_enforced_with_the_number_in_the_message() {
        // So the user is told what to raise rather than only that they hit
        // something.
        let mut i = Instance::new(InstanceLimits {
            max_sessions: 2,
            max_workspaces: 8,
        });
        let ws = i.open_workspace("/w").expect("open");
        shell(&mut i, ws);
        shell(&mut i, ws);
        let err = i
            .create_session(ws, SessionKind::Shell, SessionMode::Pty)
            .expect_err("must refuse");
        assert!(
            matches!(
                err,
                TreeError::LimitReached {
                    what: "session",
                    limit: 2
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn the_mode_is_fixed_at_creation() {
        // A session cannot become a different program halfway through; every
        // observer's model of it would be wrong from that moment.
        let (mut i, ws) = instance();
        let s = i
            .create_session(
                ws,
                SessionKind::Agent {
                    kind: AgentKind::ClaudeCode,
                },
                SessionMode::Native,
            )
            .expect("create");
        assert_eq!(i.session(s).expect("session").mode, SessionMode::Native);
    }

    #[test]
    fn a_session_gets_a_title_from_what_it_runs() {
        let (mut i, ws) = instance();
        let s = i
            .create_session(
                ws,
                SessionKind::Command {
                    argv: vec!["cargo".into(), "watch".into()],
                },
                SessionMode::Pty,
            )
            .expect("create");
        assert_eq!(i.session(s).expect("session").title, "cargo");
    }

    #[test]
    fn a_workspace_is_named_after_its_directory() {
        let mut i = Instance::default();
        let ws = i.open_workspace("/home/me/oh-my-term").expect("open");
        assert_eq!(i.workspace(ws).expect("ws").name, "oh-my-term");
    }

    #[test]
    fn closing_something_that_does_not_exist_says_so() {
        let mut i = Instance::default();
        assert!(matches!(
            i.close_session(SessionId::new()),
            Err(TreeError::NoSession(_))
        ));
    }
}
