//! The capabilities this binary declares, and the dump codegen reads.

use anyhow::Result;
use omt_catalog::{
    CallContext, CapabilityError, CapabilityHandler, CapabilityRegistry, Decl, Effects, Intent,
    Kind, Parity, capability,
};
use omt_types::{Role, SessionId, WorkspaceId};
use serde::{Deserialize, Serialize};

use crate::state::State;

/// Input to `instance.info`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct InfoIn {}

/// What `instance.info` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InfoOut {
    /// The build's version.
    pub version: String,
    /// The protocol version it speaks.
    pub proto: u16,
}

capability! {
    /// What this instance is.
    pub struct InstanceInfo;
    input  = InfoIn,
    output = InfoOut,
    decl = Decl {
        name: "instance.info",
        group: "instance",
        verb: "info",
        title: "Instance info",
        aliases: &["version"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Version and protocol of this instance.",
    },
}

struct InstanceInfoHandler;

impl CapabilityHandler<InstanceInfo> for InstanceInfoHandler {
    fn call(&self, _ctx: &CallContext, _input: InfoIn) -> Result<InfoOut, CapabilityError> {
        Ok(InfoOut {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            proto: omt_proto::PROTO_VERSION,
        })
    }
}

/// Input to `instance.catalog`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CatalogIn {}

/// One entry of the catalog.
#[derive(Serialize, schemars::JsonSchema)]
pub struct CatalogEntry {
    /// Its dotted name.
    pub name: String,
    /// Its route.
    pub route: String,
    /// Minimum role.
    pub role: Role,
    /// What omt does when it runs.
    pub effects: Effects,
    /// One line about it.
    pub doc: String,
}

/// What `instance.catalog` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct CatalogOut {
    /// Every capability, in name order.
    pub entries: Vec<CatalogEntry>,
}

capability! {
    /// Everything this instance can do.
    pub struct InstanceCatalog;
    input  = CatalogIn,
    output = CatalogOut,
    decl = Decl {
        name: "instance.catalog",
        group: "instance",
        verb: "catalog",
        title: "List capabilities",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every capability this instance offers.",
    },
}

struct InstanceCatalogHandler;

impl CapabilityHandler<InstanceCatalog> for InstanceCatalogHandler {
    fn call(&self, _ctx: &CallContext, _input: CatalogIn) -> Result<CatalogOut, CapabilityError> {
        Ok(CatalogOut {
            entries: omt_catalog::linked_decls()
                .into_iter()
                .map(|d| CatalogEntry {
                    name: d.name.to_owned(),
                    route: d.route(),
                    role: d.role,
                    effects: d.effects,
                    doc: d.doc.to_owned(),
                })
                .collect(),
        })
    }
}

/// Input to `events.subscribe`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SubscribeIn {
    /// What to send.
    #[serde(default)]
    pub filter: omt_events::Filter,
}

/// What `events.subscribe` acknowledges.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SubscribeOut {
    /// The subscription's id.
    pub subscription: String,
}

capability! {
    /// Start receiving events.
    pub struct EventsSubscribe;
    input  = SubscribeIn,
    output = SubscribeOut,
    decl = Decl {
        name: "events.subscribe",
        group: "events",
        verb: "subscribe",
        title: "Subscribe to events",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Subscribe to the event stream.",
    },
}

struct EventsSubscribeHandler;

impl CapabilityHandler<EventsSubscribe> for EventsSubscribeHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        _input: SubscribeIn,
    ) -> Result<SubscribeOut, CapabilityError> {
        Ok(SubscribeOut {
            subscription: omt_types::ClientId::new().to_string(),
        })
    }
}

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

struct WorkspaceListHandler(State);

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

struct WorkspaceOpenHandler(State);

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

struct SessionListHandler(State);

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
                .map(|s| SessionSummary {
                    id: s.id.to_wire(),
                    workspace: s.workspace.to_wire(),
                    title: s.title.clone(),
                    state: describe_state(s.state),
                    cwd: s.cwd.clone(),
                })
                .collect(),
        })
    }
}

fn describe_state(state: omt_session::SessionState) -> String {
    match state {
        omt_session::SessionState::Starting => "starting".to_owned(),
        omt_session::SessionState::Running => "running".to_owned(),
        omt_session::SessionState::Exited { code } => match code {
            Some(c) => format!("exited:{c}"),
            None => "exited".to_owned(),
        },
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

struct SessionCloseHandler(State);

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

struct AgentThreadsHandler(State);

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
        let roster = instance
            .threads(id)
            .ok_or_else(|| CapabilityError::not_found(format!("no session {}", input.session)))?;

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

fn describe_agent_state(state: &omt_types::AgentState) -> String {
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

struct FsListHandler(State);

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

struct GitStatusHandler(State);

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

struct GitDiffHandler(State);

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

struct AgentInterruptHandler(State);

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

// ---------------------------------------------------------------------------
// Session input and geometry
// ---------------------------------------------------------------------------

/// Input to `session.write`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionWriteIn {
    /// Which session.
    pub session: String,
    /// What to type.
    pub text: String,
    /// The writer-token epoch the caller believed it held.
    ///
    /// Required, not optional. Input already in flight when the token changed
    /// hands must be rejected rather than landing in somebody else's command
    /// line, and a caller that could omit this would be a caller exempt from
    /// the check.
    pub epoch: u64,
}

/// What `session.write` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionWriteOut {
    /// How many bytes went out.
    ///
    /// Bytes written, not bytes delivered. The far side is a program omt does
    /// not own, so a successful write proves the pipe accepted it and nothing
    /// more.
    pub bytes: usize,
}

capability! {
    /// Type into a session.
    pub struct SessionWrite;
    input  = SessionWriteIn,
    output = SessionWriteOut,
    decl = Decl {
        name: "session.write",
        group: "session",
        verb: "write",
        title: "Type into a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_PTY,
        // Never replayed: re-sending the tail of a shell command is how a
        // retry becomes a disaster.
        intent: Some(Intent::RawStream),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Write input to a session, gated on the writer token's epoch.",
    },
}

struct SessionWriteHandler(State);

impl CapabilityHandler<SessionWrite> for SessionWriteHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionWriteIn,
    ) -> Result<SessionWriteOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;
        let bytes = instance
            .write_session_input(id, omt_session::Epoch(input.epoch), input.text.as_bytes())
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
        Ok(SessionWriteOut { bytes })
    }
}

/// Input to `session.resize`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionResizeIn {
    /// Which session.
    pub session: String,
    /// New width.
    pub cols: u16,
    /// New height.
    pub rows: u16,
}

/// What `session.resize` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionResizeOut {
    /// The size now in force.
    pub cols: u16,
    /// The size now in force.
    pub rows: u16,
}

capability! {
    /// Change a session's size.
    pub struct SessionResize;
    input  = SessionResizeIn,
    output = SessionResizeOut,
    decl = Decl {
        name: "session.resize",
        group: "session",
        verb: "resize",
        title: "Resize a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_PTY,
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Resize a session. The grid and the kernel are told together, so \
              a program is never handed a size the kernel disagrees with.",
    },
}

struct SessionResizeHandler(State);

impl CapabilityHandler<SessionResize> for SessionResizeHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionResizeIn,
    ) -> Result<SessionResizeOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;
        instance
            .runtime_mut(id)
            .ok_or_else(|| CapabilityError::not_found("no such session"))?
            .resize(input.cols, input.rows)
            .map_err(|e| CapabilityError::internal(e.to_string()))?;
        Ok(SessionResizeOut {
            cols: input.cols,
            rows: input.rows,
        })
    }
}

/// Input to `session.read`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionReadIn {
    /// Which session.
    pub session: String,
    /// How many lines of scrollback to include before the screen.
    #[serde(default)]
    pub history: u32,
}

/// What `session.read` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionReadOut {
    /// The visible screen, one string per row.
    pub screen: Vec<String>,
    /// Scrollback before it, oldest first.
    pub history: Vec<String>,
    /// Where the cursor is.
    pub cursor: (u16, u16),
    /// Whether a full-screen program is drawing.
    ///
    /// A client rendering a transcript needs to know: while this is true the
    /// program owns every cell and a line-oriented view of it is nonsense.
    pub alternate_screen: bool,
}

capability! {
    /// What is on a session's screen.
    pub struct SessionRead;
    input  = SessionReadIn,
    output = SessionReadOut,
    decl = Decl {
        name: "session.read",
        group: "session",
        verb: "read",
        title: "Read a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "The visible screen as text, optionally with scrollback.",
    },
}

struct SessionReadHandler(State);

impl CapabilityHandler<SessionRead> for SessionReadHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionReadIn,
    ) -> Result<SessionReadOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let instance = self.0.lock()?;
        let runtime = instance
            .runtime(id)
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;
        let terminal = runtime.terminal();

        // Bounded rather than "all of it": a client asking for a million lines
        // over a phone link would be answered, slowly, and time out.
        let want = (input.history as usize).min(MAX_HISTORY_LINES);
        let all: Vec<String> = terminal.scrollback().lines().map(|l| l.text()).collect();
        let history = all[all.len().saturating_sub(want)..].to_vec();

        Ok(SessionReadOut {
            screen: terminal.screen_text(),
            history,
            cursor: (terminal.grid().cursor.row, terminal.grid().cursor.col),
            alternate_screen: terminal.active() == omt_term::Which::Alternate,
        })
    }
}

/// The most scrollback one read returns.
const MAX_HISTORY_LINES: usize = 5_000;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Input to `config.get`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConfigGetIn {
    /// A dotted key, or absent for everything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// One resolved setting and where it came from.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ConfigValue {
    /// The dotted key.
    pub key: String,
    /// Its value.
    pub value: serde_json::Value,
    /// Which layer won.
    pub layer: String,
    /// The file it came from, where there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// What `config.get` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ConfigGetOut {
    /// The settings.
    pub values: Vec<ConfigValue>,
}

capability! {
    /// Read configuration, with provenance.
    pub struct ConfigGet;
    input  = ConfigGetIn,
    output = ConfigGetOut,
    decl = Decl {
        name: "config.get",
        group: "config",
        verb: "get",
        title: "Read configuration",
        aliases: &["config.sources"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::READS_FS,
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Resolved settings, each with the layer and file it came from — \
              which is what makes a surprising value traceable.",
    },
}

struct ConfigGetHandler(State);

impl CapabilityHandler<ConfigGet> for ConfigGetHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: ConfigGetIn,
    ) -> Result<ConfigGetOut, CapabilityError> {
        let resolved = self.0.config()?;
        let values = resolved
            .keys()
            .into_iter()
            .filter(|k| input.key.as_deref().is_none_or(|want| *k == want))
            .map(|k| {
                let provenance = resolved.source(k);
                ConfigValue {
                    key: k.to_owned(),
                    value: resolved.get(k).cloned().unwrap_or(serde_json::Value::Null),
                    layer: provenance
                        .map_or_else(|| "builtin".to_owned(), |p| format!("{:?}", p.layer)),
                    file: provenance.and_then(|p| p.file.clone()),
                }
            })
            .collect();
        Ok(ConfigGetOut { values })
    }
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// Input to `keys.cheatsheet`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct KeysIn {}

/// One line of the cheatsheet.
#[derive(Serialize, schemars::JsonSchema)]
pub struct KeyBinding {
    /// The chord, canonically spelled.
    pub chord: String,
    /// Which mode it applies in.
    pub mode: String,
    /// What it does, or why it is reserved.
    pub action: String,
    /// Whether the program underneath sees this key.
    pub reaches_program: bool,
}

/// What `keys.cheatsheet` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct KeysOut {
    /// Every binding, and every reserved key.
    pub bindings: Vec<KeyBinding>,
}

capability! {
    /// Every key, generated from the live keymap.
    pub struct KeysCheatsheet;
    input  = KeysIn,
    output = KeysOut,
    decl = Decl {
        name: "keys.cheatsheet",
        group: "keys",
        verb: "cheatsheet",
        title: "Keyboard reference",
        aliases: &["keys"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every binding and every reserved key, generated from the keymap \
              in force so it cannot go stale.",
    },
}

struct KeysHandler;

impl CapabilityHandler<KeysCheatsheet> for KeysHandler {
    fn call(&self, _ctx: &CallContext, _input: KeysIn) -> Result<KeysOut, CapabilityError> {
        Ok(KeysOut {
            bindings: omt_input::cheatsheet(&omt_input::defaults())
                .into_iter()
                .map(|e| KeyBinding {
                    chord: e.chord,
                    mode: format!("{:?}", e.mode).to_lowercase(),
                    action: e.action,
                    reaches_program: e.reaches_program,
                })
                .collect(),
        })
    }
}

fn session_id(text: &str) -> Result<SessionId, CapabilityError> {
    SessionId::from_wire(text)
        .ok_or_else(|| CapabilityError::invalid_input(format!("`{text}` is not a session id")))
}

/// Build the registry this binary serves.
///
/// # Errors
/// Fails if a declaration is invalid, duplicated, or has no handler — all at
/// startup, by name, rather than when a caller trips over it.
pub fn registry(state: State) -> Result<CapabilityRegistry> {
    let mut r = CapabilityRegistry::new();
    r.register::<InstanceInfo, _>(InstanceInfoHandler)?;
    r.register::<InstanceCatalog, _>(InstanceCatalogHandler)?;
    r.register::<EventsSubscribe, _>(EventsSubscribeHandler)?;
    r.register::<WorkspaceList, _>(WorkspaceListHandler(state.clone()))?;
    r.register::<WorkspaceOpen, _>(WorkspaceOpenHandler(state.clone()))?;
    r.register::<SessionList, _>(SessionListHandler(state.clone()))?;
    r.register::<SessionClose, _>(SessionCloseHandler(state.clone()))?;
    r.register::<AgentThreads, _>(AgentThreadsHandler(state.clone()))?;
    r.register::<FsList, _>(FsListHandler(state.clone()))?;
    r.register::<GitStatus, _>(GitStatusHandler(state.clone()))?;
    r.register::<GitDiff, _>(GitDiffHandler(state.clone()))?;
    r.register::<AgentInterrupt, _>(AgentInterruptHandler(state.clone()))?;
    r.register::<SessionWrite, _>(SessionWriteHandler(state.clone()))?;
    r.register::<SessionResize, _>(SessionResizeHandler(state.clone()))?;
    r.register::<SessionRead, _>(SessionReadHandler(state.clone()))?;
    r.register::<ConfigGet, _>(ConfigGetHandler(state))?;
    r.register::<KeysCheatsheet, _>(KeysHandler)?;
    r.seal()?;
    Ok(r)
}

/// The catalog as codegen consumes it.
///
/// # Errors
/// Fails if the registry cannot be built or the dump cannot be encoded.
pub fn dump() -> Result<String> {
    let registry = registry(State::default())?;
    let mut generator = schemars::SchemaGenerator::default();

    let entries: Vec<_> = registry
        .decls()
        .map(|d| {
            serde_json::json!({
                "name": d.name,
                "group": d.group,
                "verb": d.verb,
                "title": d.title,
                "aliases": d.aliases,
                "hidden": d.hidden,
                "kind": d.kind,
                "role": d.role,
                "effects": d.effects,
                "intent": d.intent,
                "parity": d.parity,
                "since": d.since,
                "doc": d.doc,
                "route": d.route(),
            })
        })
        .collect();

    let schemas = serde_json::json!({
        "InfoIn": generator.subschema_for::<InfoIn>(),
        "InfoOut": generator.subschema_for::<InfoOut>(),
        "CatalogIn": generator.subschema_for::<CatalogIn>(),
        "CatalogOut": generator.subschema_for::<CatalogOut>(),
        "SubscribeIn": generator.subschema_for::<SubscribeIn>(),
        "SubscribeOut": generator.subschema_for::<SubscribeOut>(),
    });

    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "proto": omt_proto::PROTO_VERSION,
        "capabilities": entries,
        "schemas": schemas,
    }))?)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_seals() {
        // Sealing is strict, so this failing means something is declared with
        // no handler — found here rather than by a caller.
        registry(State::default()).expect("the registry must seal");
    }

    #[test]
    fn the_dump_is_deterministic() {
        // Codegen output has to be byte-identical run to run, or every build
        // produces a spurious diff.
        assert_eq!(dump().expect("dump"), dump().expect("dump"));
    }

    #[test]
    fn every_declared_capability_reaches_the_dump() {
        let dump: serde_json::Value = serde_json::from_str(&dump().expect("dump")).expect("parse");
        let names: Vec<_> = dump["capabilities"]
            .as_array()
            .expect("capabilities is an array")
            .iter()
            .map(|c| c["name"].as_str().expect("name").to_owned())
            .collect();
        for expected in ["events.subscribe", "instance.catalog", "instance.info"] {
            assert!(
                names.contains(&expected.to_owned()),
                "{expected} missing from {names:?}"
            );
        }
    }

    #[test]
    fn the_dump_is_sorted() {
        let dump: serde_json::Value = serde_json::from_str(&dump().expect("dump")).expect("parse");
        let names: Vec<_> = dump["capabilities"]
            .as_array()
            .expect("array")
            .iter()
            .map(|c| c["name"].as_str().expect("name").to_owned())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn effects_appear_as_strings_in_the_dump() {
        // The wire form, all the way through to the generated artifact.
        let dump: serde_json::Value = serde_json::from_str(&dump().expect("dump")).expect("parse");
        let effects = &dump["capabilities"][0]["effects"];
        assert!(effects.is_array(), "expected an array, got {effects}");
    }
}
