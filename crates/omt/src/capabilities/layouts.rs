//! Layouts, Panes, Persistence.
//!
//! Split out of one 6454-line module, which P1's own file-length rule
//! calls a design smell and which nothing measured until the check that
//! enforces it was written.

use super::*;

// ---------------------------------------------------------------------------
// Layouts
// ---------------------------------------------------------------------------

/// One pane in a launch configuration.
#[derive(Serialize, Deserialize, schemars::JsonSchema, Clone)]
pub struct PaneTemplate {
    /// What to run. Absent means the user's shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// Where to run it, relative to the workspace. Absent means its root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// What to call it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Input to `layout.open`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayoutOpenIn {
    /// Which workspace.
    pub workspace: String,
    /// The panes, in the order they should appear.
    ///
    /// A flat list rather than a recursive split tree, and the reason is
    /// worth stating: omt's own geometry tiles a flat list, so a tree would be
    /// a shape the renderer immediately flattens. When the renderer grows
    /// nested splits this grows with it, and not before — a schema that
    /// describes something nothing can draw is a promise to nobody.
    pub panes: Vec<PaneTemplate>,
}

/// What became of one pane.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PaneOutcome {
    /// Its position in the request, so a caller can say *which* one.
    pub index: u32,
    /// The session, when it started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Why it did not, when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `layout.open` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct LayoutOpenOut {
    /// One entry per requested pane, in order.
    ///
    /// Every pane, not just the failures: a layout that half-opened and said
    /// "error" leaves somebody to work out which of six panes is missing.
    pub panes: Vec<PaneOutcome>,
    /// How many started.
    pub opened: u32,
}

capability! {
    /// Open a workspace as a layout.
    pub struct LayoutOpen;
    input  = LayoutOpenIn,
    output = LayoutOpenOut,
    decl = Decl {
        name: "layout.open",
        group: "layout",
        verb: "open",
        title: "Open a layout",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::SPAWNS_PROCESS,
        intent: Some(Intent::Append {
            dedup: DedupKey::IntentIdAndTarget,
            ttl_secs: 600,
        }),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Start several sessions as a layout, reporting the outcome of every pane rather than only that something failed.",
    },
}

pub(crate) struct LayoutOpenHandler(pub(crate) State);

impl CapabilityHandler<LayoutOpen> for LayoutOpenHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: LayoutOpenIn,
    ) -> Result<LayoutOpenOut, CapabilityError> {
        let workspace = workspace_id(&input.workspace)?;
        if input.panes.is_empty() {
            return Err(CapabilityError::invalid_input("a layout needs a pane"));
        }

        let root = {
            let instance = self.0.lock()?;
            instance
                .workspace_root(workspace)
                .ok_or_else(|| CapabilityError::not_found("no such workspace"))?
        };

        let mut outcomes = Vec::with_capacity(input.panes.len());
        let mut opened = 0u32;
        for (index, pane) in input.panes.iter().enumerate() {
            // Resolved through the workspace so a template cannot point a pane
            // outside it — a layout file is something people share, and a
            // shared file that opens a shell in somebody's home directory is a
            // different kind of thing from a layout.
            let cwd = match pane.cwd.as_deref() {
                Some(rel) => match resolve_for_write(&self.0, &input.workspace, rel) {
                    Ok(path) if path.is_dir() => path.display().to_string(),
                    Ok(path) => {
                        outcomes.push(PaneOutcome {
                            index: index as u32,
                            session: None,
                            error: Some(format!("{} is not a directory", path.display())),
                        });
                        continue;
                    }
                    Err(e) => {
                        outcomes.push(PaneOutcome {
                            index: index as u32,
                            session: None,
                            error: Some(e.message.clone()),
                        });
                        continue;
                    }
                },
                None => root.clone(),
            };

            match self.start_pane(workspace, pane, &cwd) {
                Ok(session) => {
                    opened += 1;
                    outcomes.push(PaneOutcome {
                        index: index as u32,
                        session: Some(session),
                        error: None,
                    });
                }
                Err(e) => outcomes.push(PaneOutcome {
                    index: index as u32,
                    session: None,
                    error: Some(e.message),
                }),
            }
        }

        Ok(LayoutOpenOut {
            panes: outcomes,
            opened,
        })
    }
}

impl LayoutOpenHandler {
    /// Start one pane's session and put a pane on it.
    fn start_pane(
        &self,
        workspace: WorkspaceId,
        pane: &PaneTemplate,
        cwd: &str,
    ) -> Result<String, CapabilityError> {
        let program = pane
            .program
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".to_owned());

        let mut instance = self.0.lock()?;
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
                args: Vec::new(),
                cwd: Some(cwd.into()),
                size: omt_pty::PtySize::new(default_cols(), default_rows()),
                env: vec![("OMT_SESSION".to_owned(), id.to_wire())],
                ..omt_pty::PtyConfig::default()
            },
            crate::terminal_bits::session_limits(&self.0),
        )
        .map_err(|e| CapabilityError::internal(e.to_string()))?;
        instance
            .attach(runtime)
            .map_err(|e| CapabilityError::internal(e.to_string()))?;

        if let Some(title) = &pane.title
            && let Some(session) = instance.session_mut(id)
        {
            session.title = title.clone();
        }
        instance.add_pane(workspace, id);
        Ok(id.to_wire())
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

pub(crate) struct PaneListHandler(pub(crate) State);

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

pub(crate) struct PaneOpenHandler(pub(crate) State);

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

pub(crate) struct PaneCloseHandler(pub(crate) State);

impl CapabilityHandler<PaneClose> for PaneCloseHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: PaneCloseIn,
    ) -> Result<PaneCloseOut, CapabilityError> {
        let workspace = workspace_id(&input.workspace)?;
        let pane = omt_types::PaneId::from_wire(&input.pane).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a pane id", input.pane))
        })?;
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

pub(crate) struct PaneFocusHandler(pub(crate) State);

impl CapabilityHandler<PaneFocus> for PaneFocusHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: PaneFocusIn,
    ) -> Result<PaneFocusOut, CapabilityError> {
        let workspace = workspace_id(&input.workspace)?;
        let pane = omt_types::PaneId::from_wire(&input.pane).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a pane id", input.pane))
        })?;
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
pub(crate) fn workspace_id(raw: &str) -> Result<WorkspaceId, CapabilityError> {
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

/// What a restart needs to know about one session.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PersistedSession {
    /// Its workspace's canonical path, which is what the id derives from.
    pub workspace_root: String,
    /// Its title.
    pub title: String,
    /// What it was running, so "restart" respawns the same thing rather than a
    /// default shell somebody has to reconfigure.
    pub program: String,
    /// Where it was running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// The screen as it stood, so a restored session is readable, searchable
    /// and copyable even though nothing is behind it.
    #[serde(default)]
    pub screen: Vec<String>,
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
    /// The sessions that were open.
    ///
    /// Their processes do not survive — a pty dies with the daemon that opened
    /// it — so these come back orphaned: readable, with a restart offered.
    #[serde(default)]
    pub sessions: Vec<PersistedSession>,
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

/// The context the daemon's own bookkeeping runs under.
///
/// `Actor::Local` with the operator role, because this *is* the machine acting
/// on its own behalf. Given a name rather than constructed inline so the two
/// callers below cannot drift into claiming different authority.
pub(crate) fn daemon_context() -> CallContext {
    CallContext {
        actor: omt_types::Actor::Local,
        role: Role::Operator,
        request: omt_catalog::RequestId {
            device: omt_types::DeviceId::new(),
            n: 0,
        },
        // No intent: nothing retries this. The timer fires again in thirty
        // seconds, which is a better answer than a dedup entry that has to be
        // kept for as long as a retry is plausible.
        intent: None,
    }
}

/// Write a snapshot, without going through a capability call.
///
/// The daemon's own autosave uses this. Sharing the body rather than
/// duplicating it means the file the timer writes is byte-identical to the one
/// `state.save` writes — two writers with two formats is a snapshot that only
/// one of them can read.
///
/// # Errors
/// Fails if the file cannot be written.
pub fn save_to(state: &State, path: &std::path::Path) -> Result<StateSaveOut, CapabilityError> {
    StateSaveHandler(state.clone()).call(
        &daemon_context(),
        StateSaveIn {
            path: Some(path.display().to_string()),
        },
    )
}

/// Read a snapshot back, without going through a capability call.
///
/// # Errors
/// Fails if the file exists and cannot be understood.
pub fn restore_from(
    state: &State,
    path: &std::path::Path,
) -> Result<StateRestoreOut, CapabilityError> {
    StateRestoreHandler(state.clone()).call(
        &daemon_context(),
        StateRestoreIn {
            path: Some(path.display().to_string()),
        },
    )
}

pub(crate) struct StateSaveHandler(pub(crate) State);

impl CapabilityHandler<StateSave> for StateSaveHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: StateSaveIn,
    ) -> Result<StateSaveOut, CapabilityError> {
        let path = snapshot_path(input.path)?;
        let snapshot = {
            let instance = self.0.lock()?;
            let roots: std::collections::BTreeMap<_, _> = instance
                .workspaces()
                .into_iter()
                .map(|w| (w.id, w.root.clone()))
                .collect();
            let sessions = instance
                .sessions()
                .iter()
                .filter_map(|s| {
                    Some(PersistedSession {
                        workspace_root: roots.get(&s.workspace)?.clone(),
                        title: s.title.clone(),
                        program: match &s.kind {
                            omt_session::SessionKind::Command { argv } => argv
                                .first()
                                .cloned()
                                .unwrap_or_else(|| "/bin/sh".to_owned()),
                            _ => std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()),
                        },
                        cwd: s.cwd.clone(),
                        screen: Vec::new(),
                    })
                })
                .collect::<Vec<_>>();
            let sessions = sessions
                .into_iter()
                .zip(instance.sessions().iter().map(|s| s.id))
                .map(|(mut p, id)| {
                    // The screen, so a restored session can be read. Taken here
                    // rather than in the loop above because the runtime lookup
                    // needs the id and the session borrow is already held.
                    if let Some(rt) = instance.runtime(id) {
                        p.screen = rt.terminal().screen_text();
                    }
                    p
                })
                .collect();
            Persisted {
                version: SNAPSHOT_VERSION,
                workspaces: roots
                    .values()
                    .map(|root| PersistedWorkspace { root: root.clone() })
                    .collect(),
                sessions,
            }
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CapabilityError::internal(e.to_string()))?;
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
    /// How many sessions came back, orphaned.
    pub sessions: u32,
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

pub(crate) struct StateRestoreHandler(pub(crate) State);

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
                sessions: 0,
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

        // Sessions come back orphaned. Their processes are gone — a pty dies
        // with the daemon that opened it — but everything else survived, so
        // they are readable and offer a restart rather than vanishing.
        let mut orphaned = 0u32;
        for p in snapshot.sessions {
            if !std::path::Path::new(&p.workspace_root).is_dir() {
                continue;
            }
            let workspace = omt_types::WorkspaceId::from_canonical_path(&p.workspace_root);
            if instance
                .restore_orphan(workspace, &p.title, p.cwd.as_deref(), &p.screen)
                .is_some()
            {
                orphaned += 1;
            }
        }

        Ok(StateRestoreOut {
            workspaces: opened,
            sessions: orphaned,
            found: true,
            missing,
        })
    }
}

/// Where a snapshot lives when the caller did not say.
pub(crate) fn snapshot_path(given: Option<String>) -> Result<std::path::PathBuf, CapabilityError> {
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
