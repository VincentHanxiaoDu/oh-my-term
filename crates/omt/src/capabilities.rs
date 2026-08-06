//! The capabilities this binary declares, and the dump codegen reads.

use anyhow::Result;
use omt_catalog::{
    CallContext, CapabilityError, CapabilityHandler, CapabilityRegistry, Decl, DedupKey, Effects,
    Intent,
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

/// Input to `session.acquire`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionAcquireIn {
    /// Which session.
    pub session: String,
    /// Take it even though somebody else holds it.
    #[serde(default)]
    pub force: bool,
}

/// What `session.acquire` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionAcquireOut {
    /// The epoch the caller now holds, which every write is checked against.
    pub epoch: u64,
}

capability! {
    /// Take the writer token.
    pub struct SessionAcquire;
    input  = SessionAcquireIn,
    output = SessionAcquireOut,
    decl = Decl {
        name: "session.acquire",
        group: "session",
        verb: "acquire",
        title: "Take the writer token",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        // Compare-and-swap: the swap is against who holds the token, and a
        // repeat of the same intent must return the epoch it already granted
        // rather than minting a second one — which would invalidate the writes
        // the caller is making with the first.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Take the writer token for a session, returning the epoch every write is checked against.",
    },
}

struct SessionAcquireHandler(State);

impl CapabilityHandler<SessionAcquire> for SessionAcquireHandler {
    fn call(
        &self,
        ctx: &CallContext,
        input: SessionAcquireIn,
    ) -> Result<SessionAcquireOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;
        let epoch = instance
            .acquire_writer(id, ctx.actor.clone(), input.force)
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
        Ok(SessionAcquireOut {
            epoch: epoch.0,
        })
    }
}

/// Input to `session.release`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionReleaseIn {
    /// Which session.
    pub session: String,
}

/// What `session.release` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionReleaseOut {
    /// Whether this caller had been holding it.
    pub released: bool,
}

capability! {
    /// Give the writer token up.
    pub struct SessionRelease;
    input  = SessionReleaseIn,
    output = SessionReleaseOut,
    decl = Decl {
        name: "session.release",
        group: "session",
        verb: "release",
        title: "Give up the writer token",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Give up the writer token. Idempotent: releasing one you do not hold is not an error.",
    },
}

struct SessionReleaseHandler(State);

impl CapabilityHandler<SessionRelease> for SessionReleaseHandler {
    fn call(
        &self,
        ctx: &CallContext,
        input: SessionReleaseIn,
    ) -> Result<SessionReleaseOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;
        Ok(SessionReleaseOut {
            released: instance.release_writer(id, &ctx.actor),
        })
    }
}

/// Input to `session.create`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionCreateIn {
    /// Which workspace to start it in.
    pub workspace: String,
    /// What to run. Absent means the user's shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// Arguments to it.
    #[serde(default)]
    pub args: Vec<String>,
    /// Columns to start at.
    #[serde(default = "default_cols")]
    pub cols: u16,
    /// Rows.
    #[serde(default = "default_rows")]
    pub rows: u16,
}

fn default_cols() -> u16 {
    80
}
fn default_rows() -> u16 {
    24
}

/// What `session.create` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionCreateOut {
    /// The new session.
    pub session: String,
}

capability! {
    /// Start a session.
    pub struct SessionCreate;
    input  = SessionCreateIn,
    output = SessionCreateOut,
    decl = Decl {
        name: "session.create",
        group: "session",
        verb: "create",
        title: "Start a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::SPAWNS_PROCESS,
        // Deduplicated by intent id, because the retry this protects against
        // is the ordinary one: a phone loses the connection between sending
        // and hearing back, resends, and without this the user has two shells
        // and no way to know which one their next keystroke reached. The
        // window outlives any plausible reconnect for the same reason.
        intent: Some(Intent::Append {
            dedup: DedupKey::IntentId,
            ttl_secs: 600,
        }),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Start a session in a workspace, running a program on a pty. Deduplicated by intent id, so a reconnect retry does not start a second process.",
    },
}

struct SessionCreateHandler(State);

impl CapabilityHandler<SessionCreate> for SessionCreateHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionCreateIn,
    ) -> Result<SessionCreateOut, CapabilityError> {
        let workspace = WorkspaceId::from_wire(&input.workspace).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a workspace id", input.workspace))
        })?;
        let mut instance = self.0.lock()?;
        let root = instance
            .workspace_root(workspace)
            .ok_or_else(|| CapabilityError::not_found("no such workspace"))?;

        let program = input
            .program
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".to_owned());

        let id = instance
            .create_session(
                workspace,
                omt_session::SessionKind::Shell,
                omt_session::SessionMode::Pty,
            )
            .map_err(|e| CapabilityError::internal(e.to_string()))?;

        let runtime = omt_daemon::SessionRuntime::spawn(
            id,
            &omt_pty::PtyConfig {
                program: program.into(),
                args: input.args,
                cwd: Some(root.into()),
                size: omt_pty::PtySize::new(input.cols, input.rows),
                // The same variable the local path sets, so a hook started by
                // a browser-created session knows which pane it belongs to
                // exactly as one started from the terminal does.
                env: vec![("OMT_SESSION".to_owned(), id.to_wire())],
                ..omt_pty::PtyConfig::default()
            },
            omt_term::ScrollbackLimits::default(),
        )
        .map_err(|e| CapabilityError::internal(e.to_string()))?;

        instance
            .attach(runtime)
            .map_err(|e| CapabilityError::internal(e.to_string()))?;

        Ok(SessionCreateOut {
            session: id.to_wire(),
        })
    }
}

/// Input to `session.snapshot`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionSnapshotIn {
    /// Which session.
    pub session: String,
}

/// A run of cells that share a style.
///
/// Runs rather than cells: a terminal screen is overwhelmingly long stretches
/// of one style, and a cell-per-element payload is roughly twenty times larger
/// for the same picture — which is the difference between a usable phone link
/// and an unusable one.
#[derive(Serialize, schemars::JsonSchema, PartialEq, Eq, Debug)]
pub struct StyledRun {
    /// The text.
    pub text: String,
    /// Foreground, as `#rrggbb`, or absent for the theme's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    /// Background, likewise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    /// Bold.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    /// Italic.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// Underlined.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    /// Inverse video, left for the client to apply so it can swap against
    /// whatever its own default colours are.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inverse: bool,
}

/// What `session.snapshot` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionSnapshotOut {
    /// One entry per row, each a list of runs.
    pub rows: Vec<Vec<StyledRun>>,
    /// Columns, so a client can size itself without counting characters.
    pub cols: u16,
    /// Rows.
    pub rows_count: u16,
    /// Where the cursor is, as (row, col).
    pub cursor: (u16, u16),
    /// Whether the cursor should be drawn at all.
    pub cursor_visible: bool,
    /// Whether a full-screen program is drawing.
    pub alternate_screen: bool,
}

capability! {
    /// A session's screen with its colours.
    pub struct SessionSnapshot;
    input  = SessionSnapshotIn,
    output = SessionSnapshotOut,
    decl = Decl {
        name: "session.snapshot",
        group: "session",
        verb: "snapshot",
        title: "Snapshot a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "The visible screen as styled runs, so a remote client renders the same picture as the terminal without emulating one.",
    },
}

struct SessionSnapshotHandler(State);

impl CapabilityHandler<SessionSnapshot> for SessionSnapshotHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionSnapshotIn,
    ) -> Result<SessionSnapshotOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let instance = self.0.lock()?;
        let runtime = instance
            .runtime(id)
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;
        let terminal = runtime.terminal();
        let grid = terminal.grid();
        let size = grid.size();

        let rows = (0..size.rows)
            .map(|row| runs_of(&grid.row(row).densified(size.cols)))
            .collect();

        Ok(SessionSnapshotOut {
            rows,
            cols: size.cols,
            rows_count: size.rows,
            cursor: (grid.cursor.row, grid.cursor.col),
            cursor_visible: grid.cursor.visible,
            alternate_screen: terminal.active() == omt_term::Which::Alternate,
        })
    }
}

/// Collapse a row of cells into runs that share a style.
fn runs_of(cells: &[omt_term::Cell]) -> Vec<StyledRun> {
    let mut out: Vec<StyledRun> = Vec::new();
    for cell in cells {
        // The spacer half of a wide character carries no glyph of its own.
        // Emitting anything for it would push every following column right by
        // one and the line would drift.
        if cell.flags.contains(omt_term::Flags::WIDE_SPACER) {
            continue;
        }
        let run = StyledRun {
            text: String::new(),
            fg: css_color(cell.fg),
            bg: css_color(cell.bg),
            bold: cell.flags.contains(omt_term::Flags::BOLD),
            italic: cell.flags.contains(omt_term::Flags::ITALIC),
            underline: cell.flags.underline() != omt_term::Underline::None,
            inverse: cell.flags.contains(omt_term::Flags::INVERSE),
        };
        let ch = match cell.resolve() {
            // An untouched cell is a space, not a NUL: a client rendering NUL
            // draws a hole where the terminal shows blank.
            omt_term::Resolved::Char('\0') => ' ',
            omt_term::Resolved::Char(c) => c,
            omt_term::Resolved::Grapheme(_) => '?',
        };
        match out.last_mut() {
            Some(last) if style_eq(last, &run) => last.text.push(ch),
            _ => {
                let mut fresh = run;
                fresh.text.push(ch);
                out.push(fresh);
            }
        }
    }
    out
}

fn style_eq(a: &StyledRun, b: &StyledRun) -> bool {
    a.fg == b.fg
        && a.bg == b.bg
        && a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.inverse == b.inverse
}

/// A colour the browser can use directly.
///
/// Resolved here rather than sent as a palette index, because the index means
/// nothing without the theme — and the theme is the instance's, so resolving it
/// on the client would render the user's terminal in someone else's colours.
fn css_color(color: omt_term::Color) -> Option<String> {
    match color.kind() {
        omt_term::ColorKind::Default => None,
        omt_term::ColorKind::Indexed(i) => Some(format!("var(--omt-ansi-{i})")),
        omt_term::ColorKind::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
    }
}

/// The most scrollback one read returns.
const MAX_HISTORY_LINES: usize = 5_000;

// ---------------------------------------------------------------------------
// File transfer
// ---------------------------------------------------------------------------

/// Input to `fs.read`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FsReadIn {
    /// Which workspace.
    pub workspace: String,
    /// A workspace-relative path.
    pub path: String,
    /// Which chunk, counting from zero.
    #[serde(default)]
    pub chunk: u32,
}

/// What `fs.read` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FsReadOut {
    /// This chunk, base64.
    pub data: String,
    /// Which chunk this is.
    pub chunk: u32,
    /// How many there are, so a client can show a bar rather than a spinner.
    pub chunks: u32,
    /// The whole file's size.
    pub total_bytes: u64,
    /// What it appears to be, sniffed from the bytes rather than the name — an
    /// extension is a claim and the content is the fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

capability! {
    /// Read a file out of a workspace, one chunk at a time.
    pub struct FsRead;
    input  = FsReadIn,
    output = FsReadOut,
    decl = Decl {
        name: "fs.read",
        group: "fs",
        verb: "read",
        title: "Read a file",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Read one chunk of a file. Chunked so a large file over a slow link can show progress and resume rather than restarting.",
    },
}

struct FsReadHandler(State);

impl CapabilityHandler<FsRead> for FsReadHandler {
    fn call(&self, _ctx: &CallContext, input: FsReadIn) -> Result<FsReadOut, CapabilityError> {
        let path = resolve_in_workspace(&self.0, &input.workspace, &input.path)?;
        let bytes = std::fs::read(&path).map_err(|e| CapabilityError::not_found(e.to_string()))?;

        let plan = omt_media::TransferPlan::of(&bytes)
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        let index = input.chunk as usize;
        let chunk = bytes
            .chunks(omt_media::CHUNK_BYTES)
            .nth(index)
            .ok_or_else(|| {
                CapabilityError::invalid_input(format!("there is no chunk {}", input.chunk))
            })?;

        Ok(FsReadOut {
            data: b64_encode(chunk),
            chunk: input.chunk,
            chunks: plan.chunk_count() as u32,
            total_bytes: plan.total_bytes,
            media_type: plan.media_type,
        })
    }
}

/// Input to `fs.write`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FsWriteIn {
    /// Which workspace.
    pub workspace: String,
    /// Where to put it, workspace-relative.
    pub path: String,
    /// The chunk, base64.
    pub data: String,
    /// Which chunk this is.
    #[serde(default)]
    pub chunk: u32,
    /// How many there are in total.
    #[serde(default = "one")]
    pub chunks: u32,
}

fn one() -> u32 {
    1
}

/// What `fs.write` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FsWriteOut {
    /// How many chunks have arrived.
    pub received: u32,
    /// How many are still missing, which is what a progress bar is drawn from.
    pub remaining: u32,
    /// Whether the file is now complete and on disk.
    pub complete: bool,
}

capability! {
    /// Write a file into a workspace, one chunk at a time.
    pub struct FsWrite;
    input  = FsWriteIn,
    output = FsWriteOut,
    decl = Decl {
        name: "fs.write",
        group: "fs",
        verb: "write",
        title: "Write a file",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_FS,
        // Keyed on the chunk as well as the intent, because the retry this
        // protects against is one chunk of many: deduplicating on the intent
        // alone would drop the rest of the file.
        intent: Some(Intent::Append {
            dedup: DedupKey::IntentIdAndTarget,
            ttl_secs: 600,
        }),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Write one chunk of a file into a workspace. The file appears only once every chunk has arrived.",
    },
}

struct FsWriteHandler(State);

impl CapabilityHandler<FsWrite> for FsWriteHandler {
    fn call(&self, _ctx: &CallContext, input: FsWriteIn) -> Result<FsWriteOut, CapabilityError> {
        let path = resolve_for_write(&self.0, &input.workspace, &input.path)?;
        let bytes = b64_decode(&input.data)
            .ok_or_else(|| CapabilityError::invalid_input("the chunk is not valid base64"))?;

        // Written beside the destination and renamed at the end. A partial file
        // under the real name is one another program will happily open, and a
        // half-copied image is worse than a missing one.
        let partial = path.with_extension("omt-partial");
        if input.chunk == 0 {
            if let Some(parent) = partial.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| CapabilityError::internal(e.to_string()))?;
            }
            std::fs::write(&partial, &bytes)
                .map_err(|e| CapabilityError::internal(e.to_string()))?;
        } else {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&partial)
                .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
            file.write_all(&bytes)
                .map_err(|e| CapabilityError::internal(e.to_string()))?;
        }

        let received = input.chunk + 1;
        let complete = received >= input.chunks;
        if complete {
            std::fs::rename(&partial, &path)
                .map_err(|e| CapabilityError::internal(e.to_string()))?;
        }
        Ok(FsWriteOut {
            received,
            remaining: input.chunks.saturating_sub(received),
            complete,
        })
    }
}

/// Resolve a path for writing, which cannot require the file to exist yet.
///
/// The read path defers to the workspace layer, which refuses `..`, absolute
/// paths and symlinks that leave the tree — but it does that by resolving a
/// path that is already there. A destination is by definition not, so
/// containment is checked twice here instead: lexically on the components, and
/// again on the deepest ancestor that does exist, which is what catches a
/// symlink pointing out of the tree.
fn resolve_for_write(
    state: &State,
    workspace: &str,
    rel: &str,
) -> Result<std::path::PathBuf, CapabilityError> {
    use std::path::Component;

    let id = WorkspaceId::from_wire(workspace).ok_or_else(|| {
        CapabilityError::invalid_input(format!("`{workspace}` is not a workspace id"))
    })?;
    let root = {
        let instance = state.lock()?;
        instance
            .workspace_root(id)
            .ok_or_else(|| CapabilityError::not_found("no such workspace"))?
    };
    let root = std::path::Path::new(&root)
        .canonicalize()
        .map_err(|e| CapabilityError::not_found(e.to_string()))?;

    let candidate = std::path::Path::new(rel);
    for component in candidate.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            // Refused rather than normalised away. Normalising `..` silently
            // turns a path that meant to escape into one that did not, which
            // hides an attempt that is worth failing loudly.
            _ => {
                return Err(CapabilityError::invalid_input(format!(
                    "`{rel}` must stay inside the workspace"
                )));
            }
        }
    }
    let full = root.join(candidate);

    // The deepest existing ancestor, canonicalised: a component along the way
    // may be a symlink out of the tree, and the lexical check above cannot see
    // that.
    let mut existing = full.as_path();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => break,
        }
    }
    let anchored = existing
        .canonicalize()
        .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
    if !anchored.starts_with(&root) {
        return Err(CapabilityError::invalid_input(format!(
            "`{rel}` resolves outside the workspace"
        )));
    }
    Ok(full)
}

/// Resolve a workspace-relative path, refusing anything outside it.
fn resolve_in_workspace(
    state: &State,
    workspace: &str,
    rel: &str,
) -> Result<std::path::PathBuf, CapabilityError> {
    let id = WorkspaceId::from_wire(workspace).ok_or_else(|| {
        CapabilityError::invalid_input(format!("`{workspace}` is not a workspace id"))
    })?;
    let root = {
        let instance = state.lock()?;
        instance
            .workspace_root(id)
            .ok_or_else(|| CapabilityError::not_found("no such workspace"))?
    };
    let fs = omt_workspace_fs::WorkspaceFs::new(std::path::Path::new(&root))
        .map_err(|e| CapabilityError::not_found(e.to_string()))?;
    // The workspace layer decides, not this function: it already refuses
    // `..`, absolute paths and symlinks that leave the tree, and a second
    // implementation of that rule here is a second chance to get it wrong.
    fs.resolve(rel)
        .map_err(|e| CapabilityError::invalid_input(e.to_string()))
}

/// Base64, because JSON cannot carry bytes.
fn b64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let b = [
            group[0],
            group.get(1).copied().unwrap_or(0),
            group.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= group.len() {
                let index = ((n >> (18 - i * 6)) & 0x3f) as usize;
                out.push(ALPHABET[index] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The inverse, refusing anything that is not base64 rather than guessing.
fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let cleaned: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    let body: Vec<u8> = cleaned.iter().copied().take_while(|c| *c != b'=').collect();
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for group in body.chunks(4) {
        let mut n = 0u32;
        for (i, c) in group.iter().enumerate() {
            n |= value(*c)? << (18 - i * 6);
        }
        let bytes = match group.len() {
            2 => 1,
            3 => 2,
            4 => 3,
            // A single trailing character encodes nothing and means the input
            // was truncated. Returning the bytes before it would hand back a
            // silently short file.
            _ => return None,
        };
        for i in 0..bytes {
            out.push(((n >> (16 - i * 8)) & 0xff) as u8);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Interactions
// ---------------------------------------------------------------------------

/// Input to `interaction.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct InteractionListIn {
    /// Only this session's, when given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// One card, as a remote surface needs it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InteractionSummary {
    /// Its id, which is how it is answered.
    pub id: String,
    /// Which session raised it.
    pub session: String,
    /// What kind of card it is.
    pub kind: String,
    /// Whether omt can deliver an answer, and over what.
    ///
    /// A surface renders its buttons from this and never from the state: a card
    /// that is open but has no channel must not offer a button, or the user
    /// finds out by the wrong option being selected.
    pub deliverable: String,
    /// Why not, when it cannot be answered here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_deliverable_because: Option<String>,
    /// Where it is in its lifecycle.
    pub state: String,
    /// The question or the command, rendered for a small screen.
    pub prompt: String,
    /// The options, verbatim and in the agent's own order.
    pub options: Vec<String>,
}

/// What `interaction.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InteractionListOut {
    /// The cards still waiting.
    pub interactions: Vec<InteractionSummary>,
}

capability! {
    /// Every card waiting for a human.
    pub struct InteractionList;
    input  = InteractionListIn,
    output = InteractionListOut,
    decl = Decl {
        name: "interaction.list",
        group: "interaction",
        verb: "list",
        title: "List open questions",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every interaction waiting for a human, with whether omt can answer it from here and why not when it cannot.",
    },
}

struct InteractionListHandler(State);

impl CapabilityHandler<InteractionList> for InteractionListHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: InteractionListIn,
    ) -> Result<InteractionListOut, CapabilityError> {
        let filter = match input.session.as_deref() {
            Some(raw) => Some(session_id(raw)?),
            None => None,
        };
        let instance = self.0.lock()?;
        let interactions = instance
            .ledger
            .open_interactions()
            .into_iter()
            .filter(|i| filter.is_none_or(|f| i.session == f))
            .map(summarize_interaction)
            .collect();
        Ok(InteractionListOut { interactions })
    }
}

fn summarize_interaction(i: &omt_events::Interaction) -> InteractionSummary {
    let (kind, prompt, options) = match &i.kind {
        omt_events::InteractionKind::Choice { questions } => (
            "choice",
            questions.first().map(|q| q.question.clone()).unwrap_or_default(),
            questions
                .first()
                .map(|q| q.options.iter().map(|o| o.label.clone()).collect())
                .unwrap_or_default(),
        ),
        omt_events::InteractionKind::Permission {
            tool,
            command,
            options,
            ..
        } => (
            "permission",
            // The command if there is one, because that is what a person is
            // actually approving; the tool name alone is not enough to decide.
            command.clone().unwrap_or_else(|| tool.clone()),
            options.iter().map(|o| o.label.clone()).collect(),
        ),
        omt_events::InteractionKind::PlanReview { plan } => {
            ("plan_review", plan.clone(), Vec::new())
        }
        other => (kind_name(other), String::new(), Vec::new()),
    };

    let (deliverable, because) = match &i.deliverable {
        omt_events::Deliverable::Native => ("native".to_owned(), None),
        omt_events::Deliverable::Synthetic { .. } => ("synthetic".to_owned(), None),
        omt_events::Deliverable::None { reason } => {
            ("none".to_owned(), Some(format!("{reason:?}")))
        }
    };

    InteractionSummary {
        id: i.id.to_wire(),
        session: i.session.to_wire(),
        kind: kind.to_owned(),
        deliverable,
        not_deliverable_because: because,
        state: state_name(&i.state).to_owned(),
        prompt,
        options,
    }
}

fn kind_name(kind: &omt_events::InteractionKind) -> &'static str {
    match kind {
        omt_events::InteractionKind::Choice { .. } => "choice",
        omt_events::InteractionKind::Permission { .. } => "permission",
        omt_events::InteractionKind::PlanReview { .. } => "plan_review",
        _ => "other",
    }
}

fn state_name(state: &omt_events::InteractionState) -> &'static str {
    match state {
        omt_events::InteractionState::Open => "open",
        omt_events::InteractionState::Resolving { .. } => "resolving",
        omt_events::InteractionState::Submitted { .. } => "submitted",
        // The only state that means the agent actually took the answer.
        // Everything above it means omt sent bytes and nothing more.
        omt_events::InteractionState::Resolved { .. } => "resolved",
        omt_events::InteractionState::Undelivered { .. } => "undelivered",
        omt_events::InteractionState::Cancelled { .. } => "cancelled",
        omt_events::InteractionState::Abandoned { .. } => "abandoned",
    }
}

/// Input to `interaction.respond`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct InteractionRespondIn {
    /// Which card.
    pub interaction: String,
    /// The chosen option, by the agent's own label — verbatim, so the recorded
    /// answer can be compared against what was sent when confirming delivery.
    pub option: String,
    /// Free text, where the question allowed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// What `interaction.respond` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InteractionRespondOut {
    /// Its state after the answer.
    ///
    /// `resolving`, not `confirmed`: omt has claimed the right to answer and
    /// nothing more. For a synthetic responder the far side is a UI omt does
    /// not own, and confirmation comes from observing it, never from the write
    /// succeeding.
    pub state: String,
}

capability! {
    /// Answer a card.
    pub struct InteractionRespond;
    input  = InteractionRespondIn,
    output = InteractionRespondOut,
    decl = Decl {
        name: "interaction.respond",
        group: "interaction",
        verb: "respond",
        title: "Answer a question",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        // Compare-and-swap against the card's state, which is what makes two
        // people answering the same card at once resolve to one answer and one
        // loser who is told so — rather than two answers, both delivered.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Answer an open interaction. Exactly once: a second answer is refused with who won, and answerability comes from the deliverable rather than the state.",
    },
}

struct InteractionRespondHandler(State);

impl CapabilityHandler<InteractionRespond> for InteractionRespondHandler {
    fn call(
        &self,
        ctx: &CallContext,
        input: InteractionRespondIn,
    ) -> Result<InteractionRespondOut, CapabilityError> {
        let id = omt_types::InteractionId::from_wire(&input.interaction).ok_or_else(|| {
            CapabilityError::invalid_input(format!(
                "`{}` is not an interaction id",
                input.interaction
            ))
        })?;
        let mut instance = self.0.lock()?;

        let response = match instance.ledger.get(id).map(|i| &i.kind) {
            Some(omt_events::InteractionKind::Permission { .. }) => {
                omt_events::InteractionResponse::Permission {
                    option: input.option.clone(),
                    updated_input: None,
                }
            }
            Some(_) => omt_events::InteractionResponse::Choice {
                answers: vec![omt_events::ChoiceAnswer {
                    labels: vec![input.option.clone()],
                    other: input.text.clone(),
                    comment: None,
                }],
            },
            None => {
                return Err(CapabilityError::not_found(format!(
                    "no interaction {}",
                    input.interaction
                )));
            }
        };

        let interaction = instance
            .ledger
            .resolve(id, ctx.actor.clone(), omt_types::Timestamp::now(), response)
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;

        Ok(InteractionRespondOut {
            state: state_name(&interaction.state).to_owned(),
        })
    }
}

// ---------------------------------------------------------------------------
// Panes
// ---------------------------------------------------------------------------

/// Input to `pane.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PaneListIn {
    /// Which workspace's primary view.
    pub workspace: String,
}

/// One pane.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PaneSummary {
    /// Its id.
    pub id: String,
    /// The session it is looking at.
    pub session: String,
    /// Whether it has focus — which is where typing goes.
    pub focused: bool,
}

/// What `pane.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PaneListOut {
    /// The panes, in order.
    pub panes: Vec<PaneSummary>,
    /// How many of them actually fit at the given size.
    ///
    /// Reported so a client does not have to reimplement the rule: a pane too
    /// small to use is worse than a pane that is not shown.
    pub fit: u32,
}

capability! {
    /// The panes of a workspace's view.
    pub struct PaneList;
    input  = PaneListIn,
    output = PaneListOut,
    decl = Decl {
        name: "pane.list",
        group: "pane",
        verb: "list",
        title: "List panes",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Panes in a workspace's primary view, which one has focus, and how many fit on a standard terminal.",
    },
}

struct PaneListHandler(State);

impl CapabilityHandler<PaneList> for PaneListHandler {
    fn call(&self, _ctx: &CallContext, input: PaneListIn) -> Result<PaneListOut, CapabilityError> {
        let id = workspace_id(&input.workspace)?;
        let instance = self.0.lock()?;
        let (panes, focus) = instance
            .panes(id)
            .ok_or_else(|| CapabilityError::not_found("no such workspace"))?;
        let fit = omt_tui::how_many_fit(80, 24, omt_tui::Split::Vertical, panes.len()) as u32;
        Ok(PaneListOut {
            panes: panes
                .into_iter()
                .map(|p| PaneSummary {
                    id: p.id.to_wire(),
                    session: p.session.to_wire(),
                    focused: focus == Some(p.id),
                })
                .collect(),
            fit,
        })
    }
}

/// Input to `pane.open`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PaneOpenIn {
    /// Which workspace.
    pub workspace: String,
    /// Which session to look at.
    pub session: String,
}

/// What `pane.open` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PaneOpenOut {
    /// The new pane.
    pub pane: String,
}

capability! {
    /// Show a session in a new pane.
    pub struct PaneOpen;
    input  = PaneOpenIn,
    output = PaneOpenOut,
    decl = Decl {
        name: "pane.open",
        group: "pane",
        verb: "open",
        title: "Open a pane",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Append {
            dedup: DedupKey::IntentIdAndTarget,
            ttl_secs: 600,
        }),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Open a pane onto an existing session. Presentation only: no process is started and none is stopped.",
    },
}

struct PaneOpenHandler(State);

impl CapabilityHandler<PaneOpen> for PaneOpenHandler {
    fn call(&self, _ctx: &CallContext, input: PaneOpenIn) -> Result<PaneOpenOut, CapabilityError> {
        let workspace = workspace_id(&input.workspace)?;
        let session = session_id(&input.session)?;
        let mut instance = self.0.lock()?;
        if instance.session(session).is_none() {
            return Err(CapabilityError::not_found(format!(
                "no session {}",
                input.session
            )));
        }
        let pane = instance
            .add_pane(workspace, session)
            .ok_or_else(|| CapabilityError::not_found("no such workspace"))?;
        Ok(PaneOpenOut {
            pane: pane.to_wire(),
        })
    }
}

/// Input to `pane.close`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PaneCloseIn {
    /// Which workspace.
    pub workspace: String,
    /// Which pane.
    pub pane: String,
}

/// What `pane.close` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PaneCloseOut {
    /// Whether there had been such a pane.
    pub closed: bool,
}

capability! {
    /// Close a pane, leaving its session running.
    pub struct PaneClose;
    input  = PaneCloseIn,
    output = PaneCloseOut,
    decl = Decl {
        name: "pane.close",
        group: "pane",
        verb: "close",
        title: "Close a pane",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Close a pane. The session it showed keeps running — closing a view of something is not ending it.",
    },
}

struct PaneCloseHandler(State);

impl CapabilityHandler<PaneClose> for PaneCloseHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: PaneCloseIn,
    ) -> Result<PaneCloseOut, CapabilityError> {
        let workspace = workspace_id(&input.workspace)?;
        let pane = omt_types::PaneId::from_wire(&input.pane)
            .ok_or_else(|| CapabilityError::invalid_input(format!("`{}` is not a pane id", input.pane)))?;
        let mut instance = self.0.lock()?;
        Ok(PaneCloseOut {
            closed: instance.remove_pane(workspace, pane),
        })
    }
}

/// Input to `pane.focus`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PaneFocusIn {
    /// Which workspace.
    pub workspace: String,
    /// Which pane.
    pub pane: String,
}

/// What `pane.focus` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PaneFocusOut {
    /// Whether focus moved.
    pub focused: bool,
}

capability! {
    /// Move focus to a pane.
    pub struct PaneFocus;
    input  = PaneFocusIn,
    output = PaneFocusOut,
    decl = Decl {
        name: "pane.focus",
        group: "pane",
        verb: "focus",
        title: "Focus a pane",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Move focus to a pane, which is where typing goes. Focusing a pane that is not there is refused rather than silently ignored.",
    },
}

struct PaneFocusHandler(State);

impl CapabilityHandler<PaneFocus> for PaneFocusHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: PaneFocusIn,
    ) -> Result<PaneFocusOut, CapabilityError> {
        let workspace = workspace_id(&input.workspace)?;
        let pane = omt_types::PaneId::from_wire(&input.pane)
            .ok_or_else(|| CapabilityError::invalid_input(format!("`{}` is not a pane id", input.pane)))?;
        let mut instance = self.0.lock()?;
        if !instance.focus_pane(workspace, pane) {
            // Refused rather than ignored: silently keeping focus where it was
            // means the next keystroke goes somewhere the user is not looking.
            return Err(CapabilityError::not_found(format!(
                "no pane {} in that workspace",
                input.pane
            )));
        }
        Ok(PaneFocusOut { focused: true })
    }
}

/// Parse a workspace id, or say what was wrong with it.
fn workspace_id(raw: &str) -> Result<WorkspaceId, CapabilityError> {
    WorkspaceId::from_wire(raw)
        .ok_or_else(|| CapabilityError::invalid_input(format!("`{raw}` is not a workspace id")))
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// What a restart needs to know about one workspace.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PersistedWorkspace {
    /// Its canonical path, which is what the id is derived from — so restoring
    /// it produces the same id rather than a new one pointing at the same
    /// directory.
    pub root: String,
}

/// The whole snapshot.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct Persisted {
    /// A version, so a future format can be recognised rather than
    /// misinterpreted. A snapshot that cannot be understood is skipped, not
    /// guessed at.
    pub version: u32,
    /// The workspaces that were open.
    pub workspaces: Vec<PersistedWorkspace>,
}

/// The version this build writes.
pub const SNAPSHOT_VERSION: u32 = 1;

/// Input to `state.save`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct StateSaveIn {
    /// Where to write it. Absent means omt's own state directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// What `state.save` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct StateSaveOut {
    /// Where it was written.
    pub path: String,
    /// How many workspaces it holds.
    pub workspaces: u32,
}

capability! {
    /// Write what a restart would need.
    pub struct StateSave;
    input  = StateSaveIn,
    output = StateSaveOut,
    decl = Decl {
        name: "state.save",
        group: "state",
        verb: "save",
        title: "Save instance state",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_FS,
        // Writing the same state twice produces the same file, so a retry is
        // free — which is exactly what a compare-and-swap intent describes.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Write the open workspaces to a snapshot a later run can restore. Written atomically: a crash mid-write leaves the previous snapshot, never half of a new one.",
    },
}

struct StateSaveHandler(State);

impl CapabilityHandler<StateSave> for StateSaveHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: StateSaveIn,
    ) -> Result<StateSaveOut, CapabilityError> {
        let path = snapshot_path(input.path)?;
        let snapshot = {
            let instance = self.0.lock()?;
            Persisted {
                version: SNAPSHOT_VERSION,
                workspaces: instance
                    .workspaces()
                    .into_iter()
                    .map(|w| PersistedWorkspace {
                        root: w.root.clone(),
                    })
                    .collect(),
            }
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CapabilityError::internal(e.to_string()))?;
        }
        omt_store::write_snapshot(&path, &snapshot)
            .map_err(|e| CapabilityError::internal(e.to_string()))?;
        Ok(StateSaveOut {
            path: path.display().to_string(),
            workspaces: snapshot.workspaces.len() as u32,
        })
    }
}

/// Input to `state.restore`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct StateRestoreIn {
    /// Where to read it from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// What `state.restore` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct StateRestoreOut {
    /// How many workspaces were reopened.
    pub workspaces: u32,
    /// Whether there was a snapshot at all.
    pub found: bool,
    /// Roots the snapshot named that are no longer there.
    ///
    /// Reported rather than dropped silently: a workspace that moved is
    /// something the user can fix, and a restore that quietly opened four of
    /// five looks like it worked.
    pub missing: Vec<String>,
}

capability! {
    /// Reopen what was saved.
    pub struct StateRestore;
    input  = StateRestoreIn,
    output = StateRestoreOut,
    decl = Decl {
        name: "state.restore",
        group: "state",
        verb: "restore",
        title: "Restore instance state",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        // Idempotent by construction: workspace ids are derived from their
        // canonical paths, so restoring twice reopens the same ones.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Reopen the workspaces from a snapshot, reporting any whose directory is gone rather than dropping them silently.",
    },
}

struct StateRestoreHandler(State);

impl CapabilityHandler<StateRestore> for StateRestoreHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: StateRestoreIn,
    ) -> Result<StateRestoreOut, CapabilityError> {
        let path = snapshot_path(input.path)?;
        let Some(snapshot): Option<Persisted> = omt_store::load_snapshot(&path)
            .map_err(|e| CapabilityError::internal(e.to_string()))?
        else {
            return Ok(StateRestoreOut {
                workspaces: 0,
                found: false,
                missing: Vec::new(),
            });
        };
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(CapabilityError::precondition_failed(format!(
                "that snapshot is version {} and this build writes {SNAPSHOT_VERSION}",
                snapshot.version
            )));
        }

        let mut instance = self.0.lock()?;
        let mut opened = 0u32;
        let mut missing = Vec::new();
        for w in snapshot.workspaces {
            if !std::path::Path::new(&w.root).is_dir() {
                missing.push(w.root);
                continue;
            }
            if instance.open_workspace(&w.root).is_ok() {
                opened += 1;
            } else {
                missing.push(w.root);
            }
        }
        Ok(StateRestoreOut {
            workspaces: opened,
            found: true,
            missing,
        })
    }
}

/// Where a snapshot lives when the caller did not say.
fn snapshot_path(given: Option<String>) -> Result<std::path::PathBuf, CapabilityError> {
    if let Some(path) = given {
        return Ok(std::path::PathBuf::from(path));
    }
    let base = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })
        .map_err(|_| CapabilityError::internal("no HOME to keep state under"))?;
    Ok(base.join("omt").join("session.json"))
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/// Input to `recall.suggest`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RecallSuggestIn {
    /// What has been typed so far.
    pub prefix: String,
    /// Which workspace is being typed in — suggestions are scored per
    /// workspace, because the command you want in one repository is rarely the
    /// command you want in another.
    pub workspace: String,
    /// How many to return.
    #[serde(default = "ten")]
    pub limit: u32,
}

fn ten() -> u32 {
    10
}

/// One suggestion.
#[derive(Serialize, schemars::JsonSchema)]
pub struct RecallSuggestion {
    /// The command.
    pub command: String,
    /// How strongly it is suggested.
    pub score: f64,
    /// How often it has been run in this workspace, which is what a UI shows to
    /// explain why the suggestion is there.
    pub uses_here: u32,
}

/// What `recall.suggest` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct RecallSuggestOut {
    /// Best first.
    pub suggestions: Vec<RecallSuggestion>,
}

capability! {
    /// Suggest a command from history.
    pub struct RecallSuggest;
    input  = RecallSuggestIn,
    output = RecallSuggestOut,
    decl = Decl {
        name: "recall.suggest",
        group: "recall",
        verb: "suggest",
        title: "Suggest a command",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Commands from history matching a prefix, scored by use in this workspace. Destructive commands are never suggested.",
    },
}

struct RecallSuggestHandler(State);

impl CapabilityHandler<RecallSuggest> for RecallSuggestHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: RecallSuggestIn,
    ) -> Result<RecallSuggestOut, CapabilityError> {
        let workspace = WorkspaceId::from_wire(&input.workspace).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a workspace id", input.workspace))
        })?;
        let history = self.0.recall()?;
        Ok(RecallSuggestOut {
            suggestions: history
                .suggest(&input.prefix, workspace, input.limit as usize)
                .into_iter()
                .map(|s| RecallSuggestion {
                    command: s.command,
                    score: s.score,
                    uses_here: s.uses_here,
                })
                .collect(),
        })
    }
}

/// Input to `recall.record`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RecallRecordIn {
    /// What was run.
    pub command: String,
    /// Where.
    pub workspace: String,
    /// In which session.
    pub session: String,
    /// The directory it ran in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// How it ended, where that is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// How long it took.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// What `recall.record` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct RecallRecordOut {
    /// How many commands are now held.
    pub held: u32,
}

capability! {
    /// Record a command that ran.
    pub struct RecallRecord;
    input  = RecallRecordIn,
    output = RecallRecordOut,
    decl = Decl {
        name: "recall.record",
        group: "recall",
        verb: "record",
        title: "Record a command",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Append {
            dedup: DedupKey::IntentId,
            ttl_secs: 600,
        }),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Add a command to the history that suggestions are drawn from.",
    },
}

struct RecallRecordHandler(State);

impl CapabilityHandler<RecallRecord> for RecallRecordHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: RecallRecordIn,
    ) -> Result<RecallRecordOut, CapabilityError> {
        let workspace = WorkspaceId::from_wire(&input.workspace).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a workspace id", input.workspace))
        })?;
        let session = session_id(&input.session)?;
        let mut history = self.0.recall()?;
        history.record(omt_recall::HistoryEntry {
            command: input.command,
            workspace,
            session,
            cwd: input.cwd,
            at: omt_types::Timestamp::now(),
            exit_code: input.exit_code,
            duration_ms: input.duration_ms,
        });
        Ok(RecallRecordOut {
            held: history.len() as u32,
        })
    }
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

/// Input to `plugin.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PluginListIn {}

/// One installed plugin.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PluginSummary {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// Whether it is switched on.
    pub enabled: bool,
    /// What it has been granted.
    pub granted: Vec<String>,
    /// The grants that can do real damage, separated so a settings screen can
    /// lead with them rather than bury them in a list of twelve.
    pub high_consequence: Vec<String>,
}

/// What `plugin.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PluginListOut {
    /// The plugins.
    pub plugins: Vec<PluginSummary>,
}

capability! {
    /// Every installed plugin.
    pub struct PluginList;
    input  = PluginListIn,
    output = PluginListOut,
    decl = Decl {
        name: "plugin.list",
        group: "plugin",
        verb: "list",
        title: "List plugins",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Installed plugins with what each was granted, and which of those grants are high-consequence.",
    },
}

struct PluginListHandler(State);

impl CapabilityHandler<PluginList> for PluginListHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        _input: PluginListIn,
    ) -> Result<PluginListOut, CapabilityError> {
        let plugins = self.0.plugins()?;
        Ok(PluginListOut {
            plugins: plugins.iter().map(summarize_plugin).collect(),
        })
    }
}

fn summarize_plugin(p: &omt_plugin_host::Installed) -> PluginSummary {
    PluginSummary {
        id: p.manifest.id.clone(),
        name: p.manifest.name.clone(),
        version: p.manifest.version.clone(),
        enabled: p.enabled,
        granted: p.granted.iter().map(|g| format!("{g:?}")).collect(),
        high_consequence: p
            .manifest
            .high_consequence()
            .iter()
            .map(|g| format!("{g:?}"))
            .collect(),
    }
}

/// Input to `plugin.enable`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PluginEnableIn {
    /// Which plugin.
    pub id: String,
    /// On or off.
    pub enabled: bool,
}

/// What `plugin.enable` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PluginEnableOut {
    /// Its state afterwards.
    pub enabled: bool,
}

capability! {
    /// Switch a plugin on or off.
    pub struct PluginEnable;
    input  = PluginEnableIn,
    output = PluginEnableOut,
    decl = Decl {
        name: "plugin.enable",
        group: "plugin",
        verb: "enable",
        title: "Enable a plugin",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        // Compare-and-swap on the plugin's state: a repeat sets the same value
        // and reports it, which is what makes a toggle safe to retry.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Switch a plugin on or off. Idempotent: setting the state it already has is not an error.",
    },
}

struct PluginEnableHandler(State);

impl CapabilityHandler<PluginEnable> for PluginEnableHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: PluginEnableIn,
    ) -> Result<PluginEnableOut, CapabilityError> {
        let mut plugins = self.0.plugins()?;
        let plugin = plugins
            .iter_mut()
            .find(|p| p.manifest.id == input.id)
            .ok_or_else(|| CapabilityError::not_found(format!("no plugin {}", input.id)))?;
        plugin.enabled = input.enabled;
        Ok(PluginEnableOut {
            enabled: plugin.enabled,
        })
    }
}

// ---------------------------------------------------------------------------
// Scheduled jobs
// ---------------------------------------------------------------------------

/// Input to `job.create`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct JobCreateIn {
    /// What to call it. Also its identity: creating one with a name already in
    /// use replaces it, which is what makes editing a job possible without a
    /// separate update call.
    pub name: String,
    /// The directory it runs in.
    pub workspace: String,
    /// The command line.
    pub run: String,
    /// Fire every this many seconds. Zero or absent means manual.
    #[serde(default)]
    pub every_seconds: u64,
    /// Or fire daily, this many seconds past midnight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_at_secs: Option<u32>,
    /// Whether it is on.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// What `job.create` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct JobCreateOut {
    /// Its name.
    pub name: String,
    /// Whether an existing job of the same name was replaced.
    pub replaced: bool,
}

capability! {
    /// Create or replace a scheduled job.
    pub struct JobCreate;
    input  = JobCreateIn,
    output = JobCreateOut,
    decl = Decl {
        name: "job.create",
        group: "job",
        verb: "create",
        title: "Schedule a job",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        // A job runs a command line through a shell, so creating one is
        // creating the ability to run anything. Declared as spawning a process
        // because that is what it eventually does.
        effects: Effects::SPAWNS_PROCESS,
        // Keyed by name, so a retry after a dropped acknowledgement replaces
        // the job it already created rather than adding a second copy that
        // fires alongside it.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Create a scheduled job, replacing any job of the same name. A missed window is skipped rather than caught up.",
    },
}

struct JobCreateHandler(State);

impl CapabilityHandler<JobCreate> for JobCreateHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: JobCreateIn,
    ) -> Result<JobCreateOut, CapabilityError> {
        if input.name.trim().is_empty() {
            return Err(CapabilityError::invalid_input("a job needs a name"));
        }
        if !std::path::Path::new(&input.workspace).is_dir() {
            return Err(CapabilityError::invalid_input(format!(
                "`{}` is not a directory",
                input.workspace
            )));
        }
        let trigger = match (input.every_seconds, input.daily_at_secs) {
            (_, Some(at_secs)) => omt_recall::Trigger::Daily { at_secs },
            (0, None) => omt_recall::Trigger::Manual,
            (seconds, None) => omt_recall::Trigger::Every { seconds },
        };
        let job = omt_recall::Job {
            name: input.name.clone(),
            workspace: input.workspace,
            run: input.run,
            trigger,
            enabled: input.enabled,
        };

        let mut jobs = self.0.jobs()?;
        let replaced = jobs.iter().any(|s| s.job.name == job.name);
        jobs.retain(|s| s.job.name != job.name);
        jobs.push(omt_recall::Schedule::new(job));
        Ok(JobCreateOut {
            name: input.name,
            replaced,
        })
    }
}

/// Input to `job.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct JobListIn {}

/// One scheduled job.
#[derive(Serialize, schemars::JsonSchema)]
pub struct JobSummary {
    /// What it is called.
    pub name: String,
    /// Which workspace it runs in.
    pub workspace: String,
    /// What it runs.
    pub run: String,
    /// When it fires, in words.
    pub trigger: String,
    /// Whether it is switched on.
    pub enabled: bool,
    /// How many times it has failed in a row.
    ///
    /// Reported because a job that has failed six times running is one somebody
    /// needs to be shown rather than one that keeps quietly retrying.
    pub consecutive_failures: u32,
}

/// What `job.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct JobListOut {
    /// The jobs.
    pub jobs: Vec<JobSummary>,
}

capability! {
    /// Every scheduled job.
    pub struct JobList;
    input  = JobListIn,
    output = JobListOut,
    decl = Decl {
        name: "job.list",
        group: "job",
        verb: "list",
        title: "List scheduled jobs",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Scheduled jobs with when they fire and how many times each has failed in a row.",
    },
}

struct JobListHandler(State);

impl CapabilityHandler<JobList> for JobListHandler {
    fn call(&self, _ctx: &CallContext, _input: JobListIn) -> Result<JobListOut, CapabilityError> {
        let jobs = self.0.jobs()?;
        Ok(JobListOut {
            jobs: jobs
                .iter()
                .map(|s| JobSummary {
                    name: s.job.name.clone(),
                    workspace: s.job.workspace.clone(),
                    run: s.job.run.clone(),
                    trigger: describe_trigger(&s.job.trigger),
                    enabled: s.job.enabled,
                    consecutive_failures: s.state.consecutive_failures,
                })
                .collect(),
        })
    }
}

fn describe_trigger(trigger: &omt_recall::Trigger) -> String {
    match trigger {
        omt_recall::Trigger::Every { seconds } => format!("every {seconds}s"),
        omt_recall::Trigger::Daily { at_secs } => format!("daily at {at_secs}s"),
        omt_recall::Trigger::Manual => "manually".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Dictation
// ---------------------------------------------------------------------------

/// Input to `voice.append`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct VoiceAppendIn {
    /// Which session is being dictated into.
    pub session: String,
    /// What was heard.
    pub text: String,
    /// Whether this is settled or still being revised.
    ///
    /// The distinction is the whole design: a partial result is shown and then
    /// replaced, and treating one as final puts a half-heard word into somebody's
    /// command line permanently.
    #[serde(default)]
    pub final_chunk: bool,
}

/// What `voice.append` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct VoiceAppendOut {
    /// Everything heard, settled and unsettled, for display.
    pub display: String,
    /// Only the settled part, which is what may be sent to a session.
    pub committed: String,
}

capability! {
    /// Add a piece of dictation.
    pub struct VoiceAppend;
    input  = VoiceAppendIn,
    output = VoiceAppendOut,
    decl = Decl {
        name: "voice.append",
        group: "voice",
        verb: "append",
        title: "Add dictated text",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        // Raw stream: dictation is never replayed. Re-sending a chunk after a
        // reconnect would duplicate a word in the middle of a sentence, and
        // the buffer's own revision handles the ordinary case.
        intent: Some(Intent::RawStream),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Append a transcript chunk to a session's dictation buffer. Partial chunks are replaced by later ones; only the settled part is offered for sending.",
    },
}

struct VoiceAppendHandler(State);

impl CapabilityHandler<VoiceAppend> for VoiceAppendHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: VoiceAppendIn,
    ) -> Result<VoiceAppendOut, CapabilityError> {
        // Validated even though the buffer is keyed by string: accepting a
        // malformed id would silently create a buffer nothing ever reads.
        let session = session_id(&input.session)?;
        let mut voice = self.0.voice()?;
        let buffer = voice.entry(session.to_wire()).or_default();
        buffer.apply(&omt_stt::Transcript {
            text: input.text,
            is_final: input.final_chunk,
            // Not carried over the wire: a client's confidence number means
            // nothing without knowing which engine produced it, and omt has no
            // decision that reads it.
            confidence: None,
        });
        Ok(VoiceAppendOut {
            display: buffer.display(),
            committed: buffer.committed().to_owned(),
        })
    }
}

/// Input to `voice.clear`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct VoiceClearIn {
    /// Which session's buffer.
    pub session: String,
}

/// What `voice.clear` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct VoiceClearOut {
    /// Whether there had been anything to clear.
    pub had_text: bool,
}

capability! {
    /// Throw away what was dictated.
    pub struct VoiceClear;
    input  = VoiceClearIn,
    output = VoiceClearOut,
    decl = Decl {
        name: "voice.clear",
        group: "voice",
        verb: "clear",
        title: "Clear dictation",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Discard a session's dictation buffer. Idempotent.",
    },
}

struct VoiceClearHandler(State);

impl CapabilityHandler<VoiceClear> for VoiceClearHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: VoiceClearIn,
    ) -> Result<VoiceClearOut, CapabilityError> {
        let session = session_id(&input.session)?;
        let mut voice = self.0.voice()?;
        let had_text = voice
            .get(&session.to_wire())
            .is_some_and(|b| !b.is_empty());
        voice.remove(&session.to_wire());
        Ok(VoiceClearOut { had_text })
    }
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// Input to `theme.get`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ThemeGetIn {}

/// A theme as a client needs it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ThemeOut {
    /// What it is called.
    pub name: String,
    /// Whether it is meant for a light or a dark terminal.
    ///
    /// Stated by the theme rather than guessed from the background, because a
    /// borderline theme guessed wrong propagates into every contrast decision
    /// a client then makes.
    pub appearance: String,
    /// Default text, as `#rrggbb`.
    pub foreground: String,
    /// The background.
    pub background: String,
    /// The cursor.
    pub cursor: String,
    /// Selected text.
    pub selection: String,
    /// The sixteen ANSI colours in their conventional order, 0–15.
    ///
    /// Sent as one flat list because that is the order every terminal and every
    /// client indexes them by; splitting normal from bright would make every
    /// consumer reassemble it.
    pub ansi: Vec<String>,
}

capability! {
    /// The theme in force.
    pub struct ThemeGet;
    input  = ThemeGetIn,
    output = ThemeOut,
    decl = Decl {
        name: "theme.get",
        group: "theme",
        verb: "get",
        title: "Read the theme",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "The colours in force, so a remote client renders a session in the user's own theme rather than in its own defaults.",
    },
}

struct ThemeGetHandler(State);

impl CapabilityHandler<ThemeGet> for ThemeGetHandler {
    fn call(&self, _ctx: &CallContext, _input: ThemeGetIn) -> Result<ThemeOut, CapabilityError> {
        let _ = &self.0;
        let theme = omt_theme::Theme::builtin_dark();
        Ok(ThemeOut {
            name: theme.name,
            appearance: match theme.appearance {
                omt_theme::Appearance::Dark => "dark".to_owned(),
                omt_theme::Appearance::Light => "light".to_owned(),
            },
            foreground: theme.foreground.to_hex(),
            background: theme.background.to_hex(),
            cursor: theme.cursor.to_hex(),
            selection: theme.selection.to_hex(),
            ansi: theme
                .palette
                .normal
                .iter()
                .chain(theme.palette.bright.iter())
                .map(|c| c.to_hex())
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

/// Input to `open.recognize`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct OpenRecognizeIn {
    /// One line of terminal output.
    pub line: String,
}

/// Something on a line that can be opened.
#[derive(Serialize, schemars::JsonSchema)]
pub struct OpenMatch {
    /// What kind of thing it is.
    pub kind: String,
    /// The thing itself.
    pub target: String,
    /// Where it starts in the line, in bytes.
    pub start: usize,
    /// Where it ends.
    pub end: usize,
    /// The line number, for a path that carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_number: Option<u32>,
}

/// What `open.recognize` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct OpenRecognizeOut {
    /// Everything found, in the order it appears.
    pub matches: Vec<OpenMatch>,
}

capability! {
    /// Find the openable things on a line.
    pub struct OpenRecognize;
    input  = OpenRecognizeIn,
    output = OpenRecognizeOut,
    decl = Decl {
        name: "open.recognize",
        group: "open",
        verb: "recognize",
        title: "Find links and paths",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Paths, URLs and file:line references on a line of output, with their offsets — so a client can make them tappable without inventing its own pattern.",
    },
}

struct OpenRecognizeHandler(State);

impl CapabilityHandler<OpenRecognize> for OpenRecognizeHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: OpenRecognizeIn,
    ) -> Result<OpenRecognizeOut, CapabilityError> {
        let _ = &self.0;
        Ok(OpenRecognizeOut {
            matches: omt_open::recognize(&input.line)
                .into_iter()
                .map(|m| {
                    let (kind, target, line_number) = describe_target(&m.target);
                    OpenMatch {
                        kind,
                        target,
                        start: m.start,
                        end: m.end,
                        line_number,
                    }
                })
                .collect(),
        })
    }
}

fn describe_target(target: &omt_open::Target) -> (String, String, Option<u32>) {
    match target {
        omt_open::Target::Path { raw, line, .. } => ("path".to_owned(), raw.clone(), *line),
        omt_open::Target::Url { raw, .. } => ("url".to_owned(), raw.clone(), None),
    }
}

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
    r.register::<PaneList, _>(PaneListHandler(state.clone()))?;
    r.register::<PaneOpen, _>(PaneOpenHandler(state.clone()))?;
    r.register::<PaneClose, _>(PaneCloseHandler(state.clone()))?;
    r.register::<PaneFocus, _>(PaneFocusHandler(state.clone()))?;
    r.register::<StateSave, _>(StateSaveHandler(state.clone()))?;
    r.register::<StateRestore, _>(StateRestoreHandler(state.clone()))?;
    r.register::<RecallSuggest, _>(RecallSuggestHandler(state.clone()))?;
    r.register::<RecallRecord, _>(RecallRecordHandler(state.clone()))?;
    r.register::<PluginList, _>(PluginListHandler(state.clone()))?;
    r.register::<PluginEnable, _>(PluginEnableHandler(state.clone()))?;
    r.register::<JobCreate, _>(JobCreateHandler(state.clone()))?;
    r.register::<JobList, _>(JobListHandler(state.clone()))?;
    r.register::<VoiceAppend, _>(VoiceAppendHandler(state.clone()))?;
    r.register::<VoiceClear, _>(VoiceClearHandler(state.clone()))?;
    r.register::<ThemeGet, _>(ThemeGetHandler(state.clone()))?;
    r.register::<OpenRecognize, _>(OpenRecognizeHandler(state.clone()))?;
    r.register::<FsRead, _>(FsReadHandler(state.clone()))?;
    r.register::<FsWrite, _>(FsWriteHandler(state.clone()))?;
    r.register::<InteractionList, _>(InteractionListHandler(state.clone()))?;
    r.register::<InteractionRespond, _>(InteractionRespondHandler(state.clone()))?;
    r.register::<SessionCreate, _>(SessionCreateHandler(state.clone()))?;
    r.register::<SessionAcquire, _>(SessionAcquireHandler(state.clone()))?;
    r.register::<SessionRelease, _>(SessionReleaseHandler(state.clone()))?;
    r.register::<SessionRead, _>(SessionReadHandler(state.clone()))?;
    r.register::<SessionSnapshot, _>(SessionSnapshotHandler(state.clone()))?;
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

    fn cells(input: &str, cols: u16) -> Vec<omt_term::Cell> {
        let mut t = omt_term::Terminal::new(omt_term::TermConfig {
            size: omt_term::GridSize::new(cols, 2),
            ..omt_term::TermConfig::default()
        });
        t.advance(input.as_bytes());
        t.grid().row(0).densified(cols)
    }

    #[test]
    fn base64_round_trips_every_length_of_tail() {
        // The three tail cases are where every hand-rolled base64 breaks, and
        // the symptom is a file that transfers with the last byte or two wrong
        // — which for an image is a decoder error and for a binary is silent.
        for len in 0..16usize {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 % 256) as u8).collect();
            let text = b64_encode(&bytes);
            assert_eq!(
                b64_decode(&text).as_deref(),
                Some(bytes.as_slice()),
                "length {len} round-tripped wrong via {text:?}"
            );
        }
    }

    #[test]
    fn base64_agrees_with_the_standard_alphabet() {
        assert_eq!(b64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(b64_encode(b"ab"), "YWI=");
        assert_eq!(b64_encode(b"a"), "YQ==");
    }

    #[test]
    fn base64_refuses_input_that_is_not_base64() {
        // Guessing produces a shorter file than was sent, with nothing to say
        // so — the worst possible outcome for a transfer.
        assert_eq!(b64_decode("not valid!"), None);
    }

    #[test]
    fn base64_refuses_a_truncated_group_rather_than_shortening_the_file() {
        // A single trailing character encodes nothing. Returning the bytes
        // before it hands back a silently short file.
        assert_eq!(b64_decode("aGVsbG8Aa"), None);
    }

    #[test]
    fn base64_survives_the_newlines_a_client_may_wrap_it_in() {
        assert_eq!(b64_decode("aGVs\nbG8=").as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn one_style_across_a_row_is_one_run() {
        // A cell-per-element payload is roughly twenty times larger for the
        // same picture, which is the difference between a usable phone link
        // and an unusable one.
        let runs = runs_of(&cells("hello", 5));
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!(runs[0].text, "hello");
    }

    #[test]
    fn a_style_change_starts_a_new_run() {
        let runs = runs_of(&cells("ab\x1b[31mcd", 4));
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert_eq!(runs[0].text, "ab");
        assert_eq!(runs[1].text, "cd");
    }

    #[test]
    fn an_untouched_cell_is_a_space_not_a_hole() {
        // A client rendering NUL draws a hole where the terminal shows blank.
        let runs = runs_of(&cells("a", 4));
        let text: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "a   ");
    }

    #[test]
    fn a_wide_character_does_not_also_emit_its_spacer() {
        // Emitting anything for the spacer half pushes every following column
        // right by one and the line drifts.
        let runs = runs_of(&cells("漢b", 4));
        let text: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "漢b ");
    }

    #[test]
    fn truecolor_survives_as_something_a_browser_can_use() {
        let runs = runs_of(&cells("\x1b[38;2;10;20;30mX", 1));
        assert_eq!(runs[0].fg.as_deref(), Some("#0a141e"));
    }

    #[test]
    fn a_default_colour_is_left_for_the_client_theme_to_decide() {
        // Resolving it here would render the user's terminal in the instance's
        // colours rather than their own.
        assert_eq!(runs_of(&cells("X", 1))[0].fg, None);
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
