//! History, Plugins, Scheduled jobs.
//!
//! Split out of one 6454-line module, which P1's own file-length rule
//! calls a design smell and which nothing measured until the check that
//! enforces it was written.

use super::*;

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

pub(crate) fn ten() -> u32 {
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

pub(crate) struct RecallSuggestHandler(pub(crate) State);

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

pub(crate) struct RecallRecordHandler(pub(crate) State);

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
            // Unknown unless a caller says otherwise: "I do not know" and "a
            // person did this" are different claims, and the second is the one
            // somebody would rely on when auditing what an agent did.
            attribution: omt_recall::Attribution::Unknown,
        });
        Ok(RecallRecordOut {
            held: history.len() as u32,
        })
    }
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

/// Input to `plugin.install`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PluginInstallIn {
    /// Its stable id.
    pub id: String,
    /// Its display name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// What it says it does.
    #[serde(default)]
    pub description: String,
    /// What it is asking for.
    pub permissions: Vec<String>,
    /// The program to run, with arguments. Empty for a plugin with nothing to
    /// run — a theme or a keymap is a real plugin and must not be made to
    /// invent an executable.
    #[serde(default)]
    pub entry: Vec<String>,
    /// What is actually granted.
    ///
    /// Separate from `permissions` on purpose: what a plugin asks for and what
    /// it is allowed are different things, and collapsing them would make every
    /// install a grant of everything requested.
    #[serde(default)]
    pub grant: Vec<String>,
}

/// What `plugin.install` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PluginInstallOut {
    /// The plugin.
    pub id: String,
    /// What it was granted.
    pub granted: Vec<String>,
    /// What it asked for and did not get, so a UI can say so rather than
    /// leaving the user to compare two lists.
    pub refused: Vec<String>,
}

capability! {
    /// Install a plugin.
    pub struct PluginInstall;
    input  = PluginInstallIn,
    output = PluginInstallOut,
    decl = Decl {
        name: "plugin.install",
        group: "plugin",
        verb: "install",
        title: "Install a plugin",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        // Admin, not Operator. 13 §4 is explicit that Operator cannot
        // install plugins — a plugin outlives the afternoon somebody was
        // handed a credential for, and it runs with omt's own reach.
        role: Role::Admin,
        effects: Effects::SPAWNS_PROCESS,
        // Keyed by the plugin id: installing the same plugin twice replaces it
        // rather than running two copies with the same namespace.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Install a plugin with an explicit grant. What it asks for and what it receives are separate, and what was refused is reported.",
    },
}

pub(crate) struct PluginInstallHandler(pub(crate) State);

impl CapabilityHandler<PluginInstall> for PluginInstallHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: PluginInstallIn,
    ) -> Result<PluginInstallOut, CapabilityError> {
        let asked = parse_permissions(&input.permissions)?;
        let granted = parse_permissions(&input.grant)?;
        // Nothing may be granted that was not declared. A plugin that receives
        // a permission it never asked for is one whose manifest no longer
        // describes what it can do, which is the document a user reads before
        // trusting it.
        if let Some(extra) = granted.difference(&asked).next() {
            return Err(CapabilityError::invalid_input(format!(
                "{extra:?} was granted but never declared"
            )));
        }

        let manifest = omt_plugin_host::Manifest {
            id: input.id.clone(),
            name: input.name,
            version: input.version,
            permissions: asked.clone(),
            description: input.description,
            entry: input.entry,
        };
        manifest
            .validate()
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;

        let refused: Vec<String> = asked
            .difference(&granted)
            .map(|p| permission_name(*p).to_owned())
            .collect();
        let installed = omt_plugin_host::Installed::new(manifest, granted.clone());

        let mut plugins = self.0.plugins()?;
        plugins.retain(|p| p.manifest.id != input.id);
        plugins.push(installed);

        Ok(PluginInstallOut {
            id: input.id,
            granted: granted
                .iter()
                .map(|p| permission_name(*p).to_owned())
                .collect(),
            refused,
        })
    }
}

/// A permission's name on the wire.
///
/// The inverse of `parse_permissions`, and the reason it exists: these were
/// written out with `{:?}`, so a plugin installed with `read_sessions` was
/// listed back as `ReadSessions`. A client that read the list and re-sent it —
/// which is exactly what an "edit this plugin's grants" screen does — was told
/// its own values were not permissions omt knows.
pub(crate) fn permission_name(permission: omt_plugin_host::Permission) -> &'static str {
    match permission {
        omt_plugin_host::Permission::ReadSessions => "read_sessions",
        omt_plugin_host::Permission::WriteInput => "write_input",
        omt_plugin_host::Permission::ReadWorkspace => "read_workspace",
        omt_plugin_host::Permission::WriteWorkspace => "write_workspace",
        omt_plugin_host::Permission::Network => "network",
        omt_plugin_host::Permission::SpawnProcess => "spawn_process",
    }
}

/// Turn permission names on the wire into the real thing.
pub(crate) fn parse_permissions(
    names: &[String],
) -> Result<std::collections::BTreeSet<omt_plugin_host::Permission>, CapabilityError> {
    names
        .iter()
        .map(|n| match n.as_str() {
            "read_sessions" => Ok(omt_plugin_host::Permission::ReadSessions),
            "write_input" => Ok(omt_plugin_host::Permission::WriteInput),
            "read_workspace" => Ok(omt_plugin_host::Permission::ReadWorkspace),
            "write_workspace" => Ok(omt_plugin_host::Permission::WriteWorkspace),
            "network" => Ok(omt_plugin_host::Permission::Network),
            "spawn_process" => Ok(omt_plugin_host::Permission::SpawnProcess),
            // Refused rather than ignored: silently dropping a permission omt
            // does not recognise would install a plugin with less authority
            // than its manifest claims, and the mismatch surfaces later as a
            // capability that mysteriously fails.
            other => Err(CapabilityError::invalid_input(format!(
                "`{other}` is not a permission omt knows"
            ))),
        })
        .collect()
}

/// Input to `plugin.start`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PluginStartIn {
    /// Which plugin.
    pub id: String,
    /// The socket to hand it. Absent means omt's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket: Option<String>,
}

/// What `plugin.start` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PluginStartOut {
    /// The process id, so it can be found and stopped.
    pub pid: u32,
    /// The role its token carries — derived from what it was granted, never
    /// from what it asked for.
    pub role: String,
}

capability! {
    /// Start a plugin's process.
    pub struct PluginStart;
    input  = PluginStartIn,
    output = PluginStartOut,
    decl = Decl {
        name: "plugin.start",
        group: "plugin",
        verb: "start",
        title: "Start a plugin",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        // Admin, not Operator. 13 §4 is explicit that Operator cannot
        // install plugins — a plugin outlives the afternoon somebody was
        // handed a credential for, and it runs with omt's own reach.
        role: Role::Admin,
        effects: Effects::SPAWNS_PROCESS,
        intent: Some(Intent::Append {
            dedup: DedupKey::IntentIdAndTarget,
            ttl_secs: 600,
        }),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Run a plugin, handing it a socket and a token whose role comes from what it was granted. A plugin reaches omt through the same capability surface as any other client — there is no second API.",
    },
}

pub(crate) struct PluginStartHandler(pub(crate) State);

impl CapabilityHandler<PluginStart> for PluginStartHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: PluginStartIn,
    ) -> Result<PluginStartOut, CapabilityError> {
        let (launch, role) = {
            let plugins = self.0.plugins()?;
            let plugin = plugins
                .iter()
                .find(|p| p.manifest.id == input.id)
                .ok_or_else(|| CapabilityError::not_found(format!("no plugin {}", input.id)))?;
            let role = plugin.role();
            // The token is minted here and never stored: it is handed to the
            // process and nowhere else, so a state directory that leaks does
            // not leak a plugin's authority.
            let token = self.0.mint_plugin_token(role, &input.id)?;
            let socket = input
                .socket
                .clone()
                .unwrap_or_else(|| crate::serve::default_socket_path().display().to_string());
            (
                plugin
                    .launch(&socket, &token)
                    .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?,
                role,
            )
        };

        let child = std::process::Command::new(&launch.program)
            .args(&launch.args)
            .envs(launch.env.iter().map(|(k, v)| (k.clone(), v.clone())))
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| {
                CapabilityError::internal(format!("could not start {}: {e}", launch.program))
            })?;

        Ok(PluginStartOut {
            pid: child.id(),
            role: format!("{role:?}").to_lowercase(),
        })
    }
}

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

pub(crate) struct PluginListHandler(pub(crate) State);

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

pub(crate) fn summarize_plugin(p: &omt_plugin_host::Installed) -> PluginSummary {
    PluginSummary {
        id: p.manifest.id.clone(),
        name: p.manifest.name.clone(),
        version: p.manifest.version.clone(),
        enabled: p.enabled,
        granted: p
            .granted
            .iter()
            .map(|g| permission_name(*g).to_owned())
            .collect(),
        // From what it was *granted*, not from what it asked for. Read from the
        // manifest, this listed permissions that were requested and refused —
        // so a settings screen warned about authority the plugin does not have,
        // which teaches people to ignore the warning.
        high_consequence: p
            .manifest
            .high_consequence()
            .iter()
            .filter(|g| p.granted.contains(g))
            .map(|g| permission_name(*g).to_owned())
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
        // Admin, not Operator. 13 §4 is explicit that Operator cannot
        // install plugins — a plugin outlives the afternoon somebody was
        // handed a credential for, and it runs with omt's own reach.
        role: Role::Admin,
        effects: Effects::empty(),
        // Compare-and-swap on the plugin's state: a repeat sets the same value
        // and reports it, which is what makes a toggle safe to retry.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Switch a plugin on or off. Idempotent: setting the state it already has is not an error.",
    },
}

pub(crate) struct PluginEnableHandler(pub(crate) State);

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
    /// The workspace it runs in, by id.
    ///
    /// An id, like every other capability that names a workspace. It took a
    /// directory path, which meant the generated client — whose parameter is
    /// called `workspace` everywhere and carries an id everywhere else — got
    /// `is not a directory` for the one obvious call.
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

pub(crate) fn yes() -> bool {
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

pub(crate) struct JobCreateHandler(pub(crate) State);

impl CapabilityHandler<JobCreate> for JobCreateHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: JobCreateIn,
    ) -> Result<JobCreateOut, CapabilityError> {
        if input.name.trim().is_empty() {
            return Err(CapabilityError::invalid_input("a job needs a name"));
        }
        // Resolved through the instance, so a job cannot be scheduled in a
        // directory omt does not have open — which would fire every interval
        // and fail every time, silently.
        let directory = {
            let instance = self.0.lock()?;
            let workspace = instance
                .workspaces()
                .into_iter()
                .find(|w| w.id.to_wire() == input.workspace)
                .ok_or_else(|| {
                    CapabilityError::invalid_input(format!(
                        "`{}` is not a workspace this instance has open",
                        input.workspace
                    ))
                })?;
            workspace.root.clone()
        };
        let trigger = match (input.every_seconds, input.daily_at_secs) {
            (_, Some(at_secs)) => omt_recall::Trigger::Daily { at_secs },
            (0, None) => omt_recall::Trigger::Manual,
            (seconds, None) => omt_recall::Trigger::Every { seconds },
        };
        let job = omt_recall::Job {
            name: input.name.clone(),
            workspace: directory,
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

pub(crate) struct JobListHandler(pub(crate) State);

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

pub(crate) fn describe_trigger(trigger: &omt_recall::Trigger) -> String {
    match trigger {
        omt_recall::Trigger::Every { seconds } => format!("every {seconds}s"),
        omt_recall::Trigger::Daily { at_secs } => format!("daily at {at_secs}s"),
        omt_recall::Trigger::Manual => "manually".to_owned(),
    }
}
