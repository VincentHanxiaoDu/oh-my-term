//! Workspaces, Sessions, Agents, Files, Git.
//!
//! Split out of one 6454-line module, which P1's own file-length rule
//! calls a design smell and which nothing measured until the check that
//! enforces it was written.

use super::*;

// ---------------------------------------------------------------------------
// Workspaces
// ---------------------------------------------------------------------------

/// Input to `workspace.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceListIn {}

/// One workspace, as a client sees it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct WorkspaceSummary {
    /// Its id.
    pub id: String,
    /// Its canonical root, which is its identity.
    pub root: String,
    /// Its display name.
    pub name: String,
    /// How many sessions it holds.
    pub sessions: usize,
}

/// What `workspace.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct WorkspaceListOut {
    /// Every workspace, in a stable order.
    pub workspaces: Vec<WorkspaceSummary>,
}

capability! {
    /// Every workspace this instance has open.
    pub struct WorkspaceList;
    input  = WorkspaceListIn,
    output = WorkspaceListOut,
    decl = Decl {
        name: "workspace.list",
        group: "workspace",
        verb: "list",
        title: "List workspaces",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every workspace this instance has open, with its session count.",
    },
}

pub(crate) struct WorkspaceListHandler(pub(crate) State);

impl CapabilityHandler<WorkspaceList> for WorkspaceListHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        _input: WorkspaceListIn,
    ) -> Result<WorkspaceListOut, CapabilityError> {
        let instance = self.0.lock()?;
        Ok(WorkspaceListOut {
            workspaces: instance
                .workspaces()
                .into_iter()
                .map(|w| WorkspaceSummary {
                    id: w.id.to_wire(),
                    root: w.root.clone(),
                    name: w.name.clone(),
                    sessions: w.sessions.len(),
                })
                .collect(),
        })
    }
}

/// Input to `workspace.open`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceOpenIn {
    /// The canonical path to open.
    pub root: String,
}

/// What `workspace.open` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct WorkspaceOpenOut {
    /// The workspace's id.
    pub id: String,
    /// Whether it was already open.
    ///
    /// Reported rather than hidden: the id is derived from the path, so opening
    /// twice is one workspace by construction, and a caller that expected to
    /// create something should be able to see that it did not.
    pub already_open: bool,
}

capability! {
    /// Open a workspace, or return the one already open there.
    pub struct WorkspaceOpen;
    input  = WorkspaceOpenIn,
    output = WorkspaceOpenOut,
    decl = Decl {
        name: "workspace.open",
        group: "workspace",
        verb: "open",
        title: "Open a workspace",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Open a workspace at a canonical path. Idempotent: the id is \
              derived from the path.",
    },
}

pub(crate) struct WorkspaceOpenHandler(pub(crate) State);

impl CapabilityHandler<WorkspaceOpen> for WorkspaceOpenHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: WorkspaceOpenIn,
    ) -> Result<WorkspaceOpenOut, CapabilityError> {
        let mut instance = self.0.lock()?;
        let existing = WorkspaceId::from_canonical_path(&input.root);
        let already_open = instance.workspace_exists(existing);
        let id = instance
            .open_workspace(&input.root)
            .map_err(|e| CapabilityError::internal(e.to_string()))?;
        Ok(WorkspaceOpenOut {
            id: id.to_wire(),
            already_open,
        })
    }
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

/// Input to `session.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionListIn {
    /// Only sessions in this workspace, where given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

/// One session, as a client sees it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionSummary {
    /// Its id.
    pub id: String,
    /// Which workspace it belongs to.
    pub workspace: String,
    /// Its title.
    pub title: String,
    /// Where it is in its life.
    pub state: String,
    /// Its live working directory, which is not its workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// How many agent threads are in it.
    ///
    /// Here rather than left to a second call per session, because it is what
    /// every surface reads to decide whether it is looking at an agent — which
    /// screen a tap opens, and whether the conversation is offered at all. A
    /// client that has to ask separately gets it wrong until it has asked.
    pub threads: usize,
    /// How many of those are waiting for a human.
    pub blocked: usize,
}

/// What `session.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionListOut {
    /// Every matching session.
    pub sessions: Vec<SessionSummary>,
}

capability! {
    /// Every session, or every session in one workspace.
    pub struct SessionList;
    input  = SessionListIn,
    output = SessionListOut,
    decl = Decl {
        name: "session.list",
        group: "session",
        verb: "list",
        title: "List sessions",
        aliases: &["ls"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every session this instance holds, optionally filtered to one \
              workspace.",
    },
}

pub(crate) struct SessionListHandler(pub(crate) State);

impl CapabilityHandler<SessionList> for SessionListHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionListIn,
    ) -> Result<SessionListOut, CapabilityError> {
        let filter = match input.workspace.as_deref() {
            Some(w) => Some(WorkspaceId::from_wire(w).ok_or_else(|| {
                CapabilityError::invalid_input(format!("`{w}` is not a workspace id"))
            })?),
            None => None,
        };
        let instance = self.0.lock()?;
        Ok(SessionListOut {
            sessions: instance
                .sessions()
                .into_iter()
                .filter(|s| filter.is_none_or(|w| s.workspace == w))
                .map(|s| {
                    let roster = instance.threads(s.id);
                    SessionSummary {
                        id: s.id.to_wire(),
                        workspace: s.workspace.to_wire(),
                        title: s.title.clone(),
                        state: describe_state(s.state),
                        cwd: s.cwd.clone(),
                        threads: roster.map_or(0, omt_agent::ThreadRoster::len),
                        blocked: roster.map_or(0, |r| r.awaiting_human().len()),
                    }
                })
                .collect(),
        })
    }
}

pub(crate) fn describe_state(state: omt_session::SessionState) -> String {
    match state {
        omt_session::SessionState::Starting => "starting".to_owned(),
        omt_session::SessionState::Running => "running".to_owned(),
        omt_session::SessionState::Exited { code } => match code {
            Some(c) => format!("exited:{c}"),
            None => "exited".to_owned(),
        },
        // Named rather than folded into "exited": a client has to be able to
        // tell "the program finished" from "the daemon restarted and this can
        // be brought back".
        omt_session::SessionState::Orphaned => "orphaned".to_owned(),
    }
}

/// Input to `session.close`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionCloseIn {
    /// Which session.
    pub session: String,
}

/// What `session.close` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionCloseOut {
    /// Whether it was there to close.
    ///
    /// Closing something already gone is success, not an error: a client that
    /// retried after a dropped acknowledgement must not be told it failed.
    pub closed: bool,
}

capability! {
    /// End a session and stop its process.
    pub struct SessionClose;
    input  = SessionCloseIn,
    output = SessionCloseOut,
    decl = Decl {
        name: "session.close",
        group: "session",
        verb: "close",
        title: "Close a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::DESTRUCTIVE,
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "End a session and stop the process on it. Idempotent.",
    },
}

pub(crate) struct SessionCloseHandler(pub(crate) State);

impl CapabilityHandler<SessionClose> for SessionCloseHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionCloseIn,
    ) -> Result<SessionCloseOut, CapabilityError> {
        let id = SessionId::from_wire(&input.session).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a session id", input.session))
        })?;
        let mut instance = self.0.lock()?;
        Ok(SessionCloseOut {
            closed: instance.close_session(id).is_ok(),
        })
    }
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

/// Input to `agent.threads`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentThreadsIn {
    /// Which session's threads.
    pub session: String,
}

/// One thread — the session's own, or a subagent it spawned.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ThreadSummary {
    /// The agent's own id for it.
    pub id: String,
    /// Whether it is a subagent.
    pub is_subagent: bool,
    /// What it is doing.
    pub state: String,
    /// What it is working on, where it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Cards it raised that are still waiting.
    pub open_interactions: Vec<String>,
}

/// What `agent.threads` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AgentThreadsOut {
    /// Every thread, blocked ones first.
    pub threads: Vec<ThreadSummary>,
    /// How many need a human.
    pub blocked: usize,
}

capability! {
    /// Every thread in a session, including the subagents it spawned.
    pub struct AgentThreads;
    input  = AgentThreadsIn,
    output = AgentThreadsOut,
    decl = Decl {
        name: "agent.threads",
        group: "agent",
        verb: "threads",
        title: "List agent threads",
        aliases: &["subagents"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every thread in a session — the main one and each subagent — with \
              what it is doing and any card it raised.",
    },
}

pub(crate) struct AgentThreadsHandler(pub(crate) State);

impl CapabilityHandler<AgentThreads> for AgentThreadsHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: AgentThreadsIn,
    ) -> Result<AgentThreadsOut, CapabilityError> {
        let id = SessionId::from_wire(&input.session).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a session id", input.session))
        })?;
        let instance = self.0.lock()?;
        // A session that exists is not an error, whether or not an agent is in
        // it. Only a session that does not exist is.
        //
        // These were the same branch, so every plain shell answered `no session
        // <id>` about a session sitting right there in the list — and a client
        // that trusted it would tell somebody their session had gone.
        if instance.session(id).is_none() {
            return Err(CapabilityError::not_found(format!(
                "no session {}",
                input.session
            )));
        }
        let Some(roster) = instance.threads(id) else {
            // No agent has bound to it. An empty roster is the honest answer:
            // there are no threads, which is different from not knowing.
            return Ok(AgentThreadsOut {
                threads: Vec::new(),
                blocked: 0,
            });
        };

        let mut threads: Vec<ThreadSummary> = roster
            .threads()
            .into_iter()
            .map(|t| ThreadSummary {
                id: t.id.clone(),
                is_subagent: t.is_subagent,
                state: describe_agent_state(&t.state),
                label: t.label.clone(),
                open_interactions: t.open_interactions.iter().map(|i| i.to_wire()).collect(),
            })
            .collect();
        // Blocked first, for the same reason the grid sorts that way: spawn
        // order buries the one thread that needs somebody.
        threads.sort_by_key(|t| usize::from(!t.state.starts_with("blocked")));

        Ok(AgentThreadsOut {
            blocked: roster.summary().blocked,
            threads,
        })
    }
}

pub(crate) fn describe_agent_state(state: &omt_types::AgentState) -> String {
    match state {
        omt_types::AgentState::Starting => "starting".to_owned(),
        omt_types::AgentState::Idle => "idle".to_owned(),
        omt_types::AgentState::Working { .. } => "working".to_owned(),
        omt_types::AgentState::Blocked { .. } => "blocked".to_owned(),
        omt_types::AgentState::Exited { .. } => "exited".to_owned(),
        omt_types::AgentState::Unknown => "unknown".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

/// Input to `fs.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FsListIn {
    /// Which workspace.
    pub workspace: String,
    /// A workspace-relative path. Empty means the root.
    #[serde(default)]
    pub path: String,
}

/// One entry.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FsEntry {
    /// Its name.
    pub name: String,
    /// Its workspace-relative path, which a client sends back.
    pub rel: String,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// Whether it is a symlink — reported, not followed silently.
    pub is_symlink: bool,
    /// Its size, for files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// What `fs.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FsListOut {
    /// The entries, directories first.
    pub entries: Vec<FsEntry>,
}

capability! {
    /// List a directory inside a workspace.
    pub struct FsList;
    input  = FsListIn,
    output = FsListOut,
    decl = Decl {
        name: "fs.list",
        group: "fs",
        verb: "list",
        title: "List files",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::READS_FS,
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "List a directory inside a workspace. Paths are workspace-relative \
              and cannot escape it.",
    },
}

pub(crate) struct FsListHandler(pub(crate) State);

impl CapabilityHandler<FsList> for FsListHandler {
    fn call(&self, _ctx: &CallContext, input: FsListIn) -> Result<FsListOut, CapabilityError> {
        let id = WorkspaceId::from_wire(&input.workspace).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a workspace id", input.workspace))
        })?;
        let root = {
            let instance = self.0.lock()?;
            instance
                .workspace_root(id)
                .ok_or_else(|| CapabilityError::not_found("no such workspace"))?
        };
        let fs = omt_workspace_fs::WorkspaceFs::new(std::path::Path::new(&root))
            .map_err(|e| CapabilityError::not_found(e.to_string()))?;
        let entries = fs
            .list(&input.path)
            // The workspace layer's own refusal, verbatim: it already
            // distinguishes "outside the workspace" from "not there", and
            // flattening them here would make a typo look like an attack.
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(FsListOut {
            entries: entries
                .into_iter()
                .map(|e| FsEntry {
                    name: e.name,
                    rel: e.rel,
                    is_dir: e.is_dir,
                    is_symlink: e.is_symlink,
                    size: e.size,
                })
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

/// Input to `git.status`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitStatusIn {
    /// Which workspace.
    pub workspace: String,
}

/// What `git.status` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct GitStatusOut {
    /// The branch, or absent on a detached head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Commits ahead of upstream.
    pub ahead: u32,
    /// Commits behind it.
    pub behind: u32,
    /// Files changed but not staged.
    pub modified: u32,
    /// Files staged.
    pub staged: u32,
    /// Files git does not know about.
    pub untracked: u32,
    /// Whether anything is uncommitted.
    pub dirty: bool,
}

capability! {
    /// What git says about a workspace.
    pub struct GitStatus;
    input  = GitStatusIn,
    output = GitStatusOut,
    decl = Decl {
        name: "git.status",
        group: "git",
        verb: "status",
        title: "Git status",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::READS_FS,
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Branch, divergence and working-tree state. Reads only — nothing \
              here commits, checks out or fetches.",
    },
}

pub(crate) struct GitStatusHandler(pub(crate) State);

impl CapabilityHandler<GitStatus> for GitStatusHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: GitStatusIn,
    ) -> Result<GitStatusOut, CapabilityError> {
        let id = WorkspaceId::from_wire(&input.workspace).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a workspace id", input.workspace))
        })?;
        let root = {
            let instance = self.0.lock()?;
            instance
                .workspace_root(id)
                .ok_or_else(|| CapabilityError::not_found("no such workspace"))?
        };
        let status = omt_workspace_fs::status(std::path::Path::new(&root))
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
        Ok(GitStatusOut {
            branch: status.branch,
            ahead: status.ahead,
            behind: status.behind,
            modified: status.modified,
            staged: status.staged,
            untracked: status.untracked,
            dirty: status.modified > 0 || status.staged > 0 || status.untracked > 0,
        })
    }
}

/// Input to `git.diff`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitDiffIn {
    /// Which workspace.
    pub workspace: String,
    /// Whether to take the staged diff rather than the unstaged one.
    #[serde(default)]
    pub staged: bool,
}

/// One changed file.
#[derive(Serialize, schemars::JsonSchema)]
pub struct DiffFile {
    /// Where it is now.
    pub path: String,
    /// Where it was, for a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// What happened to it.
    pub kind: String,
    /// Lines added.
    pub added: u32,
    /// Lines removed.
    pub removed: u32,
    /// Whether git considers it binary.
    pub binary: bool,
}

/// What `git.diff` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct GitDiffOut {
    /// The changed files.
    pub files: Vec<DiffFile>,
}

capability! {
    /// What changed in a workspace.
    pub struct GitDiff;
    input  = GitDiffIn,
    output = GitDiffOut,
    decl = Decl {
        name: "git.diff",
        group: "git",
        verb: "diff",
        title: "Git diff",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::READS_FS,
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Which files changed, with line counts. Staged and unstaged are \
              separate answers.",
    },
}

pub(crate) struct GitDiffHandler(pub(crate) State);

impl CapabilityHandler<GitDiff> for GitDiffHandler {
    fn call(&self, _ctx: &CallContext, input: GitDiffIn) -> Result<GitDiffOut, CapabilityError> {
        let id = WorkspaceId::from_wire(&input.workspace).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a workspace id", input.workspace))
        })?;
        let root = {
            let instance = self.0.lock()?;
            instance
                .workspace_root(id)
                .ok_or_else(|| CapabilityError::not_found("no such workspace"))?
        };
        let target = if input.staged {
            omt_workspace_fs::DiffTarget::Staged
        } else {
            omt_workspace_fs::DiffTarget::Unstaged
        };
        let files = omt_workspace_fs::changed_files(std::path::Path::new(&root), target, None)
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
        Ok(GitDiffOut {
            files: files
                .into_iter()
                .map(|f| DiffFile {
                    path: f.path,
                    from: f.from,
                    kind: format!("{:?}", f.kind).to_lowercase(),
                    added: f.added,
                    removed: f.removed,
                    binary: f.binary,
                })
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

/// Input to `agent.interrupt`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentInterruptIn {
    /// Which session.
    pub session: String,
}

/// What `agent.interrupt` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AgentInterruptOut {
    /// How it was interrupted.
    pub method: String,
}

capability! {
    /// Stop what an agent is doing.
    pub struct AgentInterrupt;
    input  = AgentInterruptIn,
    output = AgentInterruptOut,
    decl = Decl {
        name: "agent.interrupt",
        group: "agent",
        verb: "interrupt",
        title: "Interrupt an agent",
        aliases: &["stop"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_PTY,
        // Raw bytes toward a process: never replayed, because re-sending an
        // interrupt lands wherever the agent has got to by then.
        intent: Some(Intent::RawStream),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Stop an agent. For an agent with no protocol of its own this is \
              the entire remote control surface.",
    },
}

pub(crate) struct AgentInterruptHandler(pub(crate) State);

impl CapabilityHandler<AgentInterrupt> for AgentInterruptHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: AgentInterruptIn,
    ) -> Result<AgentInterruptOut, CapabilityError> {
        let id = SessionId::from_wire(&input.session).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a session id", input.session))
        })?;
        let mut instance = self.0.lock()?;
        let runtime = instance
            .runtime_mut(id)
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;

        // Ctrl-C rather than a per-agent key, until the binding says which
        // agent this is: a control character is position-independent by
        // construction, so it is safe without inferring anything about what is
        // on screen.
        runtime
            .write_bytes(b"\x03")
            .map_err(|e| CapabilityError::internal(e.to_string()))?;
        Ok(AgentInterruptOut {
            method: "ctrl-c".to_owned(),
        })
    }
}
