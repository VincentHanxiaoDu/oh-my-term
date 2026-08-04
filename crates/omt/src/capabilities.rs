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
    r.register::<AgentThreads, _>(AgentThreadsHandler(state))?;
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
