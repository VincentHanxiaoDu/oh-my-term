//! Worktrees, Fan-out.
//!
//! Split out of one 6454-line module, which P1's own file-length rule
//! calls a design smell and which nothing measured until the check that
//! enforces it was written.

use super::*;

// ---------------------------------------------------------------------------
// Worktrees
// ---------------------------------------------------------------------------

/// Input to `git.worktree.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeListIn {
    /// Which workspace's repository.
    pub workspace: String,
}

/// One worktree.
#[derive(Serialize, schemars::JsonSchema)]
pub struct WorktreeSummary {
    /// Its absolute path, which is also what its workspace id derives from.
    pub path: String,
    /// The branch checked out in it, absent for a detached HEAD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The commit it is at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// Whether it is the repository's original checkout.
    ///
    /// Reported because it cannot be removed, and a UI offering to is a UI
    /// whose button fails.
    pub is_main: bool,
    /// The workspace id this path would open as.
    ///
    /// Derived, not allocated: a worktree *is* a workspace the moment it
    /// exists, and this saves a client computing the same hash.
    pub workspace: String,
}

/// What `git.worktree.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct WorktreeListOut {
    /// Every worktree, the main checkout first.
    pub worktrees: Vec<WorktreeSummary>,
}

capability! {
    /// Every worktree of a repository.
    pub struct WorktreeList;
    input  = WorktreeListIn,
    output = WorktreeListOut,
    decl = Decl {
        name: "worktree.list",
        group: "worktree",
        verb: "list",
        title: "List worktrees",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every worktree of a workspace's repository, with the workspace id each would open as. Read from git rather than from omt's own records, so it cannot disagree with reality.",
    },
}

pub(crate) struct WorktreeListHandler(pub(crate) State);

impl CapabilityHandler<WorktreeList> for WorktreeListHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: WorktreeListIn,
    ) -> Result<WorktreeListOut, CapabilityError> {
        let root = workspace_root(&self.0, &input.workspace)?;
        let worktrees = omt_workspace_fs::worktree::list(std::path::Path::new(&root))
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(WorktreeListOut {
            worktrees: worktrees.into_iter().map(summarize_worktree).collect(),
        })
    }
}

pub(crate) fn summarize_worktree(w: omt_workspace_fs::Worktree) -> WorktreeSummary {
    WorktreeSummary {
        workspace: WorkspaceId::from_canonical_path(&w.path).to_wire(),
        path: w.path,
        branch: w.branch,
        head: w.head,
        is_main: w.is_main,
    }
}

/// A workspace's canonical root, or why not.
pub(crate) fn workspace_root(state: &State, workspace: &str) -> Result<String, CapabilityError> {
    let id = workspace_id(workspace)?;
    let instance = state.lock()?;
    instance
        .workspace_root(id)
        .ok_or_else(|| CapabilityError::not_found("no such workspace"))
}

/// Input to `git.worktree.add`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeAddIn {
    /// Which workspace's repository.
    pub workspace: String,
    /// Where to put it. Relative paths are resolved against the repository's
    /// parent, so `../omt-feature` is the shape people actually use.
    pub path: String,
    /// The branch to check out, created if it does not exist.
    pub branch: String,
}

/// What `git.worktree.add` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct WorktreeAddOut {
    /// The worktree that now exists.
    pub worktree: WorktreeSummary,
}

capability! {
    /// Add a worktree.
    pub struct WorktreeAdd;
    input  = WorktreeAddIn,
    output = WorktreeAddOut,
    decl = Decl {
        name: "worktree.add",
        group: "worktree",
        verb: "add",
        title: "Add a worktree",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_FS,
        // Keyed by the path: a retry after a dropped acknowledgement finds the
        // worktree already there rather than creating a second one beside it.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Create a git worktree, which becomes a workspace by the id its path derives. Additive: it touches nothing that already exists, and a path already in use is refused rather than overwritten.",
    },
}

pub(crate) struct WorktreeAddHandler(pub(crate) State);

impl CapabilityHandler<WorktreeAdd> for WorktreeAddHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: WorktreeAddIn,
    ) -> Result<WorktreeAddOut, CapabilityError> {
        let root = workspace_root(&self.0, &input.workspace)?;
        let path = resolve_worktree_path(&root, &input.path);
        let worktree =
            omt_workspace_fs::worktree::add(std::path::Path::new(&root), &path, &input.branch)
                .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(WorktreeAddOut {
            worktree: summarize_worktree(worktree),
        })
    }
}

/// Where a worktree path means.
///
/// Relative paths resolve against a directory *named for the repository*, beside
/// it: `<parent>/<repo>.worktrees/<name>`. Three properties, each of which the
/// obvious alternatives lose:
///
/// - Not inside the checkout: a second checkout in the tree git already tracks
///   confuses every tool that walks it.
/// - Not directly in the parent: two repositories that share a parent — which
///   every `~/src` has — would collide on a worktree of the same name, and the
///   second one silently fails with git complaining about a path in use.
/// - Named for the repository, so a stray directory says what it belongs to.
pub(crate) fn resolve_worktree_path(root: &str, path: &str) -> String {
    let candidate = std::path::Path::new(path);
    if candidate.is_absolute() {
        return path.to_owned();
    }
    let root = std::path::Path::new(root);
    let name = root
        .file_name()
        .map_or_else(|| "repo".to_owned(), |n| n.to_string_lossy().into_owned());
    root.parent()
        .unwrap_or(root)
        .join(format!("{name}.worktrees"))
        .join(candidate)
        .display()
        .to_string()
}

/// Input to `git.worktree.remove`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeRemoveIn {
    /// Which workspace's repository.
    pub workspace: String,
    /// The worktree's path.
    pub path: String,
    /// Remove it even with uncommitted changes.
    #[serde(default)]
    pub force: bool,
}

/// What `git.worktree.remove` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct WorktreeRemoveOut {
    /// How many worktrees are left.
    pub remaining: u32,
}

capability! {
    /// Remove a worktree.
    pub struct WorktreeRemove;
    input  = WorktreeRemoveIn,
    output = WorktreeRemoveOut,
    decl = Decl {
        name: "worktree.remove",
        group: "worktree",
        verb: "remove",
        title: "Remove a worktree",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::DESTRUCTIVE,
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Remove a worktree. Refused when it has uncommitted changes unless force is given — an agent's output is usually why the worktree was wanted, and deleting it on a tap is unrecoverable.",
    },
}

pub(crate) struct WorktreeRemoveHandler(pub(crate) State);

impl CapabilityHandler<WorktreeRemove> for WorktreeRemoveHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: WorktreeRemoveIn,
    ) -> Result<WorktreeRemoveOut, CapabilityError> {
        let root = workspace_root(&self.0, &input.workspace)?;
        let dir = std::path::Path::new(&root);
        omt_workspace_fs::worktree::remove(dir, &input.path, input.force)
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
        Ok(WorktreeRemoveOut {
            remaining: omt_workspace_fs::worktree::list(dir)
                .map(|w| w.len() as u32)
                .unwrap_or(0),
        })
    }
}

/// Input to `git.hunks`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct GitHunksIn {
    /// Which workspace.
    pub workspace: String,
    /// The file, workspace-relative.
    pub path: String,
    /// Staged changes rather than unstaged.
    #[serde(default)]
    pub staged: bool,
    /// Compare against this revision instead of the index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

/// One run of changed lines.
#[derive(Serialize, schemars::JsonSchema)]
pub struct HunkSummary {
    /// Where it starts in the old file.
    pub old_start: u32,
    /// How many lines it covers there.
    pub old_lines: u32,
    /// Where it starts in the new file.
    pub new_start: u32,
    /// How many lines it covers.
    pub new_lines: u32,
    /// The lines themselves, each prefixed as git prefixes them.
    ///
    /// Prefixed rather than split into added and removed: the order is what
    /// makes a diff readable, and two lists lose it.
    pub lines: Vec<String>,
}

/// What `git.hunks` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct GitHunksOut {
    /// The hunks, in file order.
    pub hunks: Vec<HunkSummary>,
}

capability! {
    /// What changed inside a file.
    pub struct GitHunks;
    input  = GitHunksIn,
    output = GitHunksOut,
    decl = Decl {
        name: "git.hunks",
        group: "git",
        verb: "hunks",
        title: "Diff a file",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "The changed lines of one file. `git.diff` reports which files changed; this is what changed in them. Returned as lines rather than rendered, because a terminal, a phone and a browser each want to draw it differently.",
    },
}

pub(crate) struct GitHunksHandler(pub(crate) State);

impl CapabilityHandler<GitHunks> for GitHunksHandler {
    fn call(&self, _ctx: &CallContext, input: GitHunksIn) -> Result<GitHunksOut, CapabilityError> {
        let root = workspace_root(&self.0, &input.workspace)?;
        let target = if input.staged {
            omt_workspace_fs::DiffTarget::Staged
        } else {
            omt_workspace_fs::DiffTarget::Unstaged
        };
        let hunks = omt_workspace_fs::hunks(
            std::path::Path::new(&root),
            &input.path,
            target,
            input.base.as_deref(),
        )
        .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;

        Ok(GitHunksOut {
            hunks: hunks
                .into_iter()
                .map(|h| HunkSummary {
                    old_start: h.old_start,
                    old_lines: h.old_lines,
                    new_start: h.new_start,
                    new_lines: h.new_lines,
                    lines: h.lines,
                })
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Fan-out
// ---------------------------------------------------------------------------

/// Input to `fanout.start`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FanoutStartIn {
    /// Which workspace's repository the worktrees come from.
    pub workspace: String,
    /// What to call this fan-out, so it can be asked about later — from another
    /// client, on another device.
    pub name: String,
    /// The prompt every arm is given.
    pub prompt: String,
    /// The agents to run, by the names the catalog uses.
    pub agents: Vec<String>,
    /// The revision every worktree branches from. Absent means the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

/// One arm of a fan-out.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ArmSummary {
    /// Which agent.
    pub agent: String,
    /// Its worktree.
    pub worktree: String,
    /// Its branch.
    pub branch: String,
    /// The workspace its worktree opens as.
    pub workspace: String,
    /// Where it is: `prepared`, `running`, `finished`, `failed` or `chosen`.
    pub state: String,
    /// How many files it changed, once it has finished.
    ///
    /// The first thing anybody comparing two attempts looks at, and the reason
    /// it is here rather than left to a client to compute from a diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_changed: Option<u32>,
    /// Why it failed, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `fanout.start` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FanoutStartOut {
    /// Its name.
    pub name: String,
    /// Every arm, including the ones that failed to start.
    ///
    /// Every one, not only the failures: a fan-out that half-started and said
    /// "error" leaves somebody to work out which of four agents is missing.
    pub arms: Vec<ArmSummary>,
    /// How many worktrees exist.
    pub prepared: u32,
}

capability! {
    /// Run one prompt across several agents, each in its own worktree.
    pub struct FanoutStart;
    input  = FanoutStartIn,
    output = FanoutStartOut,
    decl = Decl {
        name: "fanout.start",
        group: "fanout",
        verb: "start",
        title: "Fan a prompt across agents",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_FS,
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Create a worktree per agent and record them as one fan-out. Every arm is reported, including any that could not be prepared.",
    },
}

pub(crate) struct FanoutStartHandler(pub(crate) State);

impl CapabilityHandler<FanoutStart> for FanoutStartHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: FanoutStartIn,
    ) -> Result<FanoutStartOut, CapabilityError> {
        if input.name.trim().is_empty() {
            return Err(CapabilityError::invalid_input("a fan-out needs a name"));
        }
        let root = workspace_root(&self.0, &input.workspace)?;
        let dir = std::path::Path::new(&root);

        let mut arms = Vec::new();
        let mut failures = Vec::new();
        for name in &input.agents {
            let Some(agent) = agent_kind(name) else {
                failures.push(ArmSummary {
                    agent: name.clone(),
                    worktree: String::new(),
                    branch: String::new(),
                    workspace: String::new(),
                    state: "failed".to_owned(),
                    files_changed: None,
                    error: Some(format!("`{name}` is not an agent omt knows")),
                });
                continue;
            };
            // Named for the fan-out and the agent, so two fan-outs in one
            // repository do not collide and a stray worktree says what it was
            // for.
            let branch = format!("omt/{}/{name}", input.name);
            let path = resolve_worktree_path(&root, &format!("{}-{name}", input.name));

            match omt_workspace_fs::worktree::add(dir, &path, &branch) {
                Ok(_) => arms.push((agent, path, branch)),
                Err(e) => failures.push(ArmSummary {
                    agent: name.clone(),
                    worktree: path,
                    branch,
                    workspace: String::new(),
                    state: "failed".to_owned(),
                    files_changed: None,
                    error: Some(e.to_string()),
                }),
            }
        }

        if arms.len() < 2 {
            return Err(CapabilityError::invalid_input(
                "a fan-out needs at least two arms that could be prepared",
            ));
        }

        let fanout =
            omt_session::Fanout::new(&input.prompt, input.base.as_deref().unwrap_or("HEAD"), arms)
                .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;

        let mut summaries: Vec<ArmSummary> = fanout.arms().into_iter().map(summarize_arm).collect();
        let prepared = summaries.len() as u32;
        summaries.extend(failures);

        self.0.fanouts()?.insert(input.name.clone(), fanout);
        Ok(FanoutStartOut {
            name: input.name,
            arms: summaries,
            prepared,
        })
    }
}

pub(crate) fn summarize_arm(arm: &omt_session::Arm) -> ArmSummary {
    ArmSummary {
        agent: arm.agent.as_str().to_owned(),
        workspace: WorkspaceId::from_canonical_path(&arm.worktree).to_wire(),
        worktree: arm.worktree.clone(),
        branch: arm.branch.clone(),
        state: match &arm.state {
            omt_session::ArmState::Prepared => "prepared",
            omt_session::ArmState::Running => "running",
            omt_session::ArmState::Finished { .. } => "finished",
            omt_session::ArmState::Failed { .. } => "failed",
        }
        .to_owned(),
        // How much this arm changed, which is the first thing anybody
        // comparing two attempts looks at.
        files_changed: match &arm.state {
            omt_session::ArmState::Finished { files_changed } => Some(*files_changed),
            _ => None,
        },
        error: match &arm.state {
            omt_session::ArmState::Failed { reason } => Some(reason.clone()),
            _ => None,
        },
    }
}

/// An agent by the name the catalog uses.
pub(crate) fn agent_kind(name: &str) -> Option<omt_types::AgentKind> {
    Some(match name {
        "claude" | "claude_code" => omt_types::AgentKind::ClaudeCode,
        "codex" => omt_types::AgentKind::Codex,
        "copilot" => omt_types::AgentKind::Copilot,
        "cursor" => omt_types::AgentKind::Cursor,
        "gemini" | "gemini_cli" => omt_types::AgentKind::GeminiCli,
        "opencode" => omt_types::AgentKind::Opencode,
        "qwen" | "qwen_code" => omt_types::AgentKind::QwenCode,
        "goose" => omt_types::AgentKind::Goose,
        "aider" => omt_types::AgentKind::Aider,
        "amp" => omt_types::AgentKind::Amp,
        "crush" => omt_types::AgentKind::Crush,
        _ => return None,
    })
}

/// Input to `fanout.status`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FanoutStatusIn {
    /// Which fan-out. Absent lists them all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One fan-out.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FanoutSummary {
    /// Its name.
    pub name: String,
    /// Its arms.
    pub arms: Vec<ArmSummary>,
    /// Whether every arm has finished or failed.
    pub complete: bool,
    /// The agent whose arm was chosen, when one has been.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen: Option<String>,
}

/// What `fanout.status` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FanoutStatusOut {
    /// The fan-outs.
    pub fanouts: Vec<FanoutSummary>,
}

capability! {
    /// How the fan-outs are going.
    pub struct FanoutStatus;
    input  = FanoutStatusIn,
    output = FanoutStatusOut,
    decl = Decl {
        name: "fanout.status",
        group: "fanout",
        verb: "status",
        title: "Fan-out status",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every arm of every fan-out, with its worktree, branch and state.",
    },
}

pub(crate) struct FanoutStatusHandler(pub(crate) State);

impl CapabilityHandler<FanoutStatus> for FanoutStatusHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: FanoutStatusIn,
    ) -> Result<FanoutStatusOut, CapabilityError> {
        let fanouts = self.0.fanouts()?;
        Ok(FanoutStatusOut {
            fanouts: fanouts
                .iter()
                .filter(|(name, _)| input.name.as_deref().is_none_or(|want| *name == want))
                .map(|(name, f)| FanoutSummary {
                    name: name.clone(),
                    arms: f.arms().into_iter().map(summarize_arm).collect(),
                    complete: f.is_complete(),
                    chosen: f.chosen().map(|a| a.agent.as_str().to_owned()),
                })
                .collect(),
        })
    }
}

/// Input to `fanout.choose`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FanoutChooseIn {
    /// Which fan-out.
    pub name: String,
    /// Whose arm won.
    pub agent: String,
}

/// What `fanout.choose` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FanoutChooseOut {
    /// The branch that won, which is what to merge.
    pub branch: String,
    /// Its worktree, which is kept.
    pub worktree: String,
    /// What omt did *not* do, said plainly.
    ///
    /// Choosing records the choice. Merging is a decision with conflicts in it,
    /// and a tool that merges when you tap "choose" is one you stop trusting
    /// the first time it picks wrong.
    pub merged: bool,
}

capability! {
    /// Say which arm won.
    pub struct FanoutChoose;
    input  = FanoutChooseIn,
    output = FanoutChooseOut,
    decl = Decl {
        name: "fanout.choose",
        group: "fanout",
        verb: "choose",
        title: "Choose an arm",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Record which arm won, naming its branch. Merges nothing and deletes nothing: the losing worktrees are what you would look at to check the choice was right.",
    },
}

pub(crate) struct FanoutChooseHandler(pub(crate) State);

impl CapabilityHandler<FanoutChoose> for FanoutChooseHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: FanoutChooseIn,
    ) -> Result<FanoutChooseOut, CapabilityError> {
        let agent = agent_kind(&input.agent).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not an agent omt knows", input.agent))
        })?;
        let mut fanouts = self.0.fanouts()?;
        let fanout = fanouts
            .get_mut(&input.name)
            .ok_or_else(|| CapabilityError::not_found(format!("no fan-out {}", input.name)))?;
        let arm = fanout
            .choose(agent)
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
        Ok(FanoutChooseOut {
            branch: arm.branch.clone(),
            worktree: arm.worktree.clone(),
            merged: false,
        })
    }
}

/// Input to `config.schema`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConfigSchemaIn {}

/// One thing a user can set.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SettingSchema {
    /// Its dotted key.
    pub key: String,
    /// What it holds.
    pub kind: String,
    /// What it is when nobody has set it.
    pub default: serde_json::Value,
    /// One line about what it does.
    pub doc: String,
    /// The permitted values, where there is a fixed set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// What it currently resolves to, and where that came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<serde_json::Value>,
    /// Which layer supplied the current value, absent when it is the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
}

/// What `config.schema` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ConfigSchemaOut {
    /// Every setting omt understands.
    pub settings: Vec<SettingSchema>,
}

capability! {
    /// Every setting, whether or not anyone has set it.
    pub struct ConfigSchema;
    input  = ConfigSchemaIn,
    output = ConfigSchemaOut,
    decl = Decl {
        name: "config.schema",
        group: "config",
        verb: "schema",
        title: "List every setting",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every setting with its default, type and description — so a settings screen can be built without knowing the key names in advance. `config.get` reports what is set; this reports what could be.",
    },
}

pub(crate) struct ConfigSchemaHandler(pub(crate) State);

impl CapabilityHandler<ConfigSchema> for ConfigSchemaHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        _input: ConfigSchemaIn,
    ) -> Result<ConfigSchemaOut, CapabilityError> {
        let resolved = self.0.config()?;
        Ok(ConfigSchemaOut {
            settings: omt_config::SETTINGS
                .iter()
                .map(|s| {
                    let provenance = resolved.source(s.key);
                    SettingSchema {
                        key: s.key.to_owned(),
                        kind: s.kind.as_str().to_owned(),
                        // Parsed rather than passed through as text: a client
                        // comparing the default against the current value would
                        // otherwise be comparing a string to a number.
                        default: serde_json::from_str(s.default).unwrap_or(serde_json::Value::Null),
                        doc: s.doc.to_owned(),
                        choices: s.choices.iter().map(|c| (*c).to_owned()).collect(),
                        current: resolved.get(s.key).cloned(),
                        layer: provenance.map(|p| format!("{:?}", p.layer)),
                    }
                })
                .collect(),
        })
    }
}

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

pub(crate) struct ConfigGetHandler(pub(crate) State);

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
