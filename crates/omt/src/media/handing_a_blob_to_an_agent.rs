//! Handing a blob to an agent, Resolving a recognised target, Remote mirrors.
//!
//! Split out of one 3478-line module, over P1's 2000-line limit.

use super::*;

// ---------------------------------------------------------------------------
// Handing a blob to an agent — §7.1
// ---------------------------------------------------------------------------

/// How one agent wants to be told about a file.
///
/// Per-adapter data, not a global format. Every one of these was observed from
/// the agent's own interface rather than guessed: `@"path"` is what Claude
/// Code's own TUI writes for a pasted image, `/add` is Aider's documented
/// command. A single format applied to all of them would be silently ignored
/// by most, which looks exactly like an agent choosing not to read the file.
#[must_use]
pub fn attachment_reference(agent: &str, path: &str) -> (String, &'static str) {
    match agent {
        // Quoted, because a path with a space in it is otherwise two arguments
        // and the reference resolves to nothing.
        "claudecode" | "claude" => (format!("@\"{path}\""), "high"),
        "aider" => (format!("/add {path}"), "high"),
        "geminicli" | "gemini" | "qwencode" | "qwen" => (format!("@{path}"), "medium"),
        "codex" => (path.to_string(), "medium"),
        // The only ones with a real structured attachment channel. A path in
        // the prompt would work less well than the channel they already have.
        "opencode" => (path.to_owned(), "high"),
        // The fallback is a bare absolute path with a human sentence in front,
        // because an agent that does not recognise any syntax still reads
        // English and can be asked to open a path.
        _ => (format!("Please look at this file: {path}"), "fallback"),
    }
}

/// Input to `media.attach`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AttachIn {
    /// Which agent is being told.
    pub agent: String,
    /// The absolute path the blob was materialised at.
    pub path: String,
}

/// What `media.attach` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AttachOut {
    /// The text to inject, in the agent's own syntax.
    pub reference: String,
    /// How well established this form is, rather than a claim it will work.
    pub confidence: String,
}

omt_catalog::capability! {
    /// How to reference a file to a particular agent.
    pub struct MediaAttach;
    input  = AttachIn,
    output = AttachOut,
    decl = omt_catalog::Decl {
        name: "media.attach",
        group: "media",
        verb: "attach",
        title: "Reference a file to an agent",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Per-adapter, because one format applied to all of them is silently ignored by most — which looks exactly like an agent choosing not to read the file.",
    },
}

impl CapabilityHandler<MediaAttach> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: AttachIn,
    ) -> Result<AttachOut, CapabilityError> {
        if !input.path.starts_with('/') {
            return Err(CapabilityError::invalid_input(
                "an attachment reference needs an absolute path; a relative one resolves against whatever directory the agent happens to be in",
            ));
        }
        let (reference, confidence) = attachment_reference(
            &input.agent.to_lowercase().replace(['-', '_'], ""),
            &input.path,
        );
        Ok(AttachOut {
            reference,
            confidence: confidence.to_owned(),
        })
    }
}

/// Input to `media.paste_tier`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TierIn {
    /// omt answered on the far side of the connection.
    #[serde(default)]
    pub omt_on_both_ends: bool,
    /// A forwarded socket exists *and* an omt instance answered on it.
    #[serde(default)]
    pub reverse_socket_answered: bool,
    /// A companion on the near side acknowledged the OSC handshake.
    #[serde(default)]
    pub osc_companion_answered: bool,
    /// The terminal replied about its own upload protocol.
    #[serde(default)]
    pub terminal_upload_confirmed: bool,
}

/// What `media.paste_tier` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TierOut {
    /// Which mechanism is actually available.
    pub tier: String,
    /// Why paste behaves the way it does, in words for a person.
    pub explanation: String,
}

omt_catalog::capability! {
    /// Which paste mechanism this connection actually achieved.
    pub struct PasteTier;
    input  = TierIn,
    output = TierOut,
    decl = omt_catalog::Decl {
        name: "media.paste_tier",
        group: "media",
        verb: "paste_tier",
        title: "How paste reaches this machine",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A tier is never claimed without a positive handshake. `Probably` means a paste that silently does nothing, and no way for the user to tell whether they mis-copied or omt mis-guessed.",
    },
}

impl CapabilityHandler<PasteTier> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: TierIn,
    ) -> Result<TierOut, CapabilityError> {
        let probe = omt_media::tier::Probe {
            omt_on_both_ends: input.omt_on_both_ends,
            reverse_socket_answered: input.reverse_socket_answered,
            osc_companion_answered: input.osc_companion_answered,
            terminal_upload_confirmed: input.terminal_upload_confirmed,
        };
        let tier = omt_media::tier::achieved(&probe);
        Ok(TierOut {
            tier: format!("{tier:?}").to_lowercase(),
            explanation: tier.explain().to_owned(),
        })
    }
}

/// Input to `ssh.plan`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SshPlanIn {
    /// The ssh target.
    pub target: String,
    /// Whether a probe found omt on the far side.
    #[serde(default)]
    pub remote_has_omt: bool,
    /// What to do if it did not.
    #[serde(default)]
    pub bootstrap: Option<String>,
}

/// What `ssh.plan` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SshPlanOut {
    /// How the session would be run.
    pub mode: String,
    /// The argument vector, with no shell involved.
    pub argv: Vec<String>,
    /// Why, in words somebody can act on.
    pub reason: String,
    /// Whether paste crosses directly, or goes through the escape bridge.
    pub direct_media: bool,
}

omt_catalog::capability! {
    /// What `omt ssh <target>` would run, and why.
    pub struct SshPlan;
    input  = SshPlanIn,
    output = SshPlanOut,
    decl = omt_catalog::Decl {
        name: "ssh.plan",
        group: "ssh",
        verb: "plan",
        title: "Preview an omt ssh session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "An argument vector, never a shell string: a hostname from a config file with a quote in it would otherwise become remote execution.",
    },
}

impl CapabilityHandler<SshPlan> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: SshPlanIn,
    ) -> Result<SshPlanOut, CapabilityError> {
        let bootstrap = match input.bootstrap.as_deref() {
            Some("never") => crate::thin::Bootstrap::Never,
            Some("assume_installed") => crate::thin::Bootstrap::AssumeInstalled,
            _ => crate::thin::Bootstrap::Ask,
        };
        let plan = crate::thin::plan(&input.target, input.remote_has_omt, bootstrap)
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(SshPlanOut {
            mode: format!("{:?}", plan.mode).to_lowercase(),
            direct_media: plan.direct_media(),
            argv: plan.argv,
            reason: plan.reason,
        })
    }
}

// ---------------------------------------------------------------------------
// Resolving a recognised target — 18 §3, §4
// ---------------------------------------------------------------------------

/// Input to `open.resolve`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveIn {
    /// The path as it was recognised.
    pub path: String,
    /// A line, where the text carried one.
    #[serde(default)]
    pub line: Option<u32>,
    /// The roots it may resolve against.
    ///
    /// Passed in rather than taken from the daemon's open workspaces, because
    /// the caller knows which pane the text came from and the daemon does not.
    pub roots: Vec<String>,
}

/// What `open.resolve` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ResolveOut {
    /// The absolute path.
    pub path: String,
    /// Which root it resolved against, so a wrong answer is visible.
    pub root: Option<String>,
    /// Whether it is there — or was not looked for.
    pub existence: String,
    /// A line, where there was one.
    pub line: Option<u32>,
    /// What can be done, best first.
    pub actions: Vec<Offer>,
}

/// One thing that could be done.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Offer {
    /// Which handler.
    pub handler: String,
    /// What to call it in a menu.
    pub label: String,
    /// Higher is a better default.
    pub rank: i16,
    /// Whether it changes anything, so a confirm sheet is decidable without
    /// running it.
    pub mutates: bool,
}

omt_catalog::capability! {
    /// Turn a recognised path into a concrete target, or an honest failure.
    pub struct OpenResolve;
    input  = ResolveIn,
    output = ResolveOut,
    decl = omt_catalog::Decl {
        name: "open.resolve",
        group: "open",
        verb: "resolve",
        title: "Resolve a recognised path",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A path resolved against the wrong root opens somebody else's file, so the root that was used is part of the answer and anything escaping every root is refused rather than clamped.",
    },
}

impl CapabilityHandler<OpenResolve> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ResolveIn,
    ) -> Result<ResolveOut, CapabilityError> {
        let roots: Vec<std::path::PathBuf> =
            input.roots.iter().map(std::path::PathBuf::from).collect();
        let resolved = omt_open::resolve::resolve(&input.path, input.line, &roots, true)
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(ResolveOut {
            path: resolved.path,
            root: resolved.root,
            existence: format!("{:?}", resolved.existence).to_lowercase(),
            line: resolved.line,
            actions: resolved
                .actions
                .into_iter()
                .map(|a| Offer {
                    handler: a.handler,
                    label: a.label,
                    rank: a.rank,
                    mutates: a.mutates,
                })
                .collect(),
        })
    }
}

/// Input to `open.click`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClickIn {
    /// Whether the program in the pane has enabled mouse reporting.
    #[serde(default)]
    pub mouse_reporting: bool,
    /// Whether Shift was held.
    #[serde(default)]
    pub shift: bool,
    /// Cmd on macOS, Super elsewhere.
    #[serde(default)]
    pub super_key: bool,
    /// Alt / Option.
    #[serde(default)]
    pub alt: bool,
}

/// What `open.click` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ClickOut {
    /// Who owns this click.
    pub owner: String,
    /// Whether omt took it, or the program under it did.
    pub consumed: bool,
}

omt_catalog::capability! {
    /// Who owns a click at these coordinates.
    pub struct OpenClick;
    input  = ClickIn,
    output = ClickOut,
    decl = omt_catalog::Decl {
        name: "open.click",
        group: "open",
        verb: "click",
        title: "Who owns this click",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "When the program in the pane has mouse reporting on, every press is its input. A multiplexer that swallows clicks so its own link handler can run has broken vim, and no link feature is worth that.",
    },
}

impl CapabilityHandler<OpenClick> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ClickIn,
    ) -> Result<ClickOut, CapabilityError> {
        let owner = omt_open::click::owner(
            input.mouse_reporting,
            omt_open::click::Modifiers {
                shift: input.shift,
                super_key: input.super_key,
                alt: input.alt,
            },
        );
        Ok(ClickOut {
            owner: format!("{owner:?}").to_lowercase(),
            consumed: omt_open::click::consumed(owner),
        })
    }
}

/// Input to `open.rules_list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RulesListIn {}

/// One recognition rule.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RuleInfo {
    /// Its id.
    pub id: String,
    /// Higher wins.
    pub precedence: u16,
    /// What it recognises, in words.
    pub about: String,
}

/// What `open.rules_list` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RulesListOut {
    /// Every rule, in the order they are tried.
    pub rules: Vec<RuleInfo>,
}

omt_catalog::capability! {
    /// Every recognition rule omt runs.
    pub struct OpenRulesList;
    input  = RulesListIn,
    output = RulesListOut,
    decl = omt_catalog::Decl {
        name: "open.rules_list",
        group: "open",
        verb: "rules_list",
        title: "List recognition rules",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A recognition system nobody can enumerate is one where `why did that not become a link` has no answer short of reading the source.",
    },
}

impl CapabilityHandler<OpenRulesList> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: RulesListIn,
    ) -> Result<RulesListOut, CapabilityError> {
        Ok(RulesListOut {
            rules: omt_open::rules::catalog()
                .into_iter()
                .map(|r| RuleInfo {
                    id: r.id.to_owned(),
                    precedence: r.precedence,
                    about: r.about.to_owned(),
                })
                .collect(),
        })
    }
}

/// Input to `open.rules_test`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RulesTestIn {
    /// The line to try.
    pub text: String,
}

/// What `open.rules_test` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RulesTestOut {
    /// Which rule fired, if any.
    ///
    /// Named rather than a bare boolean: "it matched" is not an answer
    /// somebody can use to fix a rule that did not.
    pub rule: Option<String>,
    /// What was recognised, if anything.
    pub matches: Vec<OpenMatchOut>,
}

/// One match, as the preview shows it.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct OpenMatchOut {
    /// What kind of thing.
    pub kind: String,
    /// The text of it.
    pub target: String,
    /// Byte offset of the first character.
    pub start: usize,
    /// One past the last.
    pub end: usize,
    /// A line number, where the text carried one.
    pub line_number: Option<u32>,
}

omt_catalog::capability! {
    /// Try the recognition rules against a line and say what fired.
    pub struct OpenRulesTest;
    input  = RulesTestIn,
    output = RulesTestOut,
    decl = omt_catalog::Decl {
        name: "open.rules_test",
        group: "open",
        verb: "rules_test",
        title: "Test a line against the rules",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Powers the config editor's live preview. Names the rule that fired, because a preview that only says whether something matched cannot help fix one that did not.",
    },
}

impl CapabilityHandler<OpenRulesTest> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RulesTestIn,
    ) -> Result<RulesTestOut, CapabilityError> {
        Ok(RulesTestOut {
            rule: omt_open::rules::which(&input.text).map(str::to_owned),
            matches: omt_open::recognize(&input.text)
                .into_iter()
                .map(|m| {
                    let (kind, target, line_number) = match &m.target {
                        omt_open::Target::Path { raw, line, .. } => {
                            ("path".to_owned(), raw.clone(), *line)
                        }
                        omt_open::Target::Url { raw } => ("url".to_owned(), raw.clone(), None),
                    };
                    OpenMatchOut {
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

// ---------------------------------------------------------------------------
// Remote mirrors — 18 §6.3
// ---------------------------------------------------------------------------

/// Every mirror this instance holds, keyed by local path.
pub type Mirrors =
    std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, omt_media::mirror::Mirror>>>;

/// Input to `open.remote_list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoteListIn {}

/// One mirrored file.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MirrorOut {
    /// Where it is locally.
    pub local_path: String,
    /// Where it came from.
    pub remote_path: String,
    /// Which host.
    pub host: String,
    /// When it was fetched.
    pub fetched_at: u64,
    /// A hash of what was fetched, so staleness is checkable.
    pub digest: String,
    /// Whether omt will refuse to write it back.
    pub read_only: bool,
}

/// What `open.remote_list` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RemoteListOut {
    /// Every mirror, newest first.
    pub files: Vec<MirrorOut>,
}

omt_catalog::capability! {
    /// Every remote file mirrored locally.
    pub struct RemoteList;
    input  = RemoteListIn,
    output = RemoteListOut,
    decl = omt_catalog::Decl {
        name: "open.remote_list",
        group: "open",
        verb: "remote_list",
        title: "List mirrored remote files",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A mirror nobody can see is a copy of somebody's file sitting on disk with no way to tell where it came from or to get rid of it.",
    },
}

/// Serves the mirror registry.
pub struct MirrorHandler(pub Mirrors);

impl CapabilityHandler<RemoteList> for MirrorHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: RemoteListIn,
    ) -> Result<RemoteListOut, CapabilityError> {
        let held = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the mirror lock was poisoned"))?;
        let mut files: Vec<MirrorOut> = held
            .iter()
            .map(|(local, m)| MirrorOut {
                local_path: local.clone(),
                remote_path: m.remote_path.clone(),
                host: m.host.clone(),
                fetched_at: m.fetched_at,
                digest: m.digest.clone(),
                read_only: m.read_only,
            })
            .collect();
        files.sort_by(|a, b| {
            b.fetched_at
                .cmp(&a.fetched_at)
                .then(a.local_path.cmp(&b.local_path))
        });
        Ok(RemoteListOut { files })
    }
}

/// Input to `open.remote_discard`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoteDiscardIn {
    /// The local path of the mirror to drop.
    pub path: String,
}

/// What `open.remote_discard` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RemoteDiscardOut {
    /// Whether anything was there to drop.
    pub discarded: bool,
    /// How many mirrors remain.
    pub remaining: usize,
}

omt_catalog::capability! {
    /// Drop a mirrored file and stop watching it.
    pub struct RemoteDiscard;
    input  = RemoteDiscardIn,
    output = RemoteDiscardOut,
    decl = omt_catalog::Decl {
        name: "open.remote_discard",
        group: "open",
        verb: "remote_discard",
        title: "Discard a mirrored file",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_FS,
        // Cas: discarding twice is the same as discarding once, so a retry
        // after a dropped acknowledgement is safe and returns the same answer.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Unpins the blob so the sweeper can take it. Discarding something already gone is reported, not an error — a retry after a lost acknowledgement must not look like a failure.",
    },
}

impl CapabilityHandler<RemoteDiscard> for MirrorHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RemoteDiscardIn,
    ) -> Result<RemoteDiscardOut, CapabilityError> {
        let mut held = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the mirror lock was poisoned"))?;
        let discarded = held.remove(&input.path).is_some();
        Ok(RemoteDiscardOut {
            discarded,
            remaining: held.len(),
        })
    }
}

// ---------------------------------------------------------------------------
// Materialising a blob — 09 §2
// ---------------------------------------------------------------------------

/// Where a materialised blob is written.
///
/// omt's own managed root, never the workspace. 09 §2 is explicit about this
/// and the reason is not tidiness: writing an attachment into the repository
/// puts an unreviewed file from a phone into somebody's working tree, where it
/// shows up in `git status`, in the agent's own file listing, and eventually in
/// a commit. Every agent in the covered set expands an absolute path outside
/// the workspace (09 §4.3.2 records the verification), so there is nothing to
/// buy by putting it inside one.
fn managed_root() -> Result<std::path::PathBuf, CapabilityError> {
    if let Ok(explicit) = std::env::var("OMT_MEDIA_DIR") {
        return Ok(std::path::PathBuf::from(explicit));
    }
    let base = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })
        .map_err(|_| {
            CapabilityError::precondition_failed(
                "neither HOME nor XDG_STATE_HOME is set, so there is nowhere to put an attachment",
            )
        })?;
    Ok(base.join("omt").join("media"))
}

/// The extension a materialised blob gets.
///
/// From the sniffed class, never from the name the sender supplied. A sender
/// that names its own file type decides how the receiver treats it — the same
/// rule `media.put` already applies to the content type, applied to the one
/// other place the type is visible.
fn extension_for(kind: &str) -> &'static str {
    match kind {
        "image" => "png",
        "text" => "txt",
        "pdf" => "pdf",
        "archive" => "zip",
        "audio" => "audio",
        "video" => "video",
        _ => "bin",
    }
}

/// Input to `media.materialize`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct MaterializeIn {
    /// Which blob, by digest.
    pub digest: String,
    /// A name to prefer for the file, where the caller has one.
    ///
    /// Advisory. The stem is taken from it and everything else is discarded:
    /// the extension comes from what the bytes are, and a name is otherwise a
    /// path traversal waiting to be written somewhere it was not meant to go.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// What `media.materialize` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MaterializeOut {
    /// The absolute path the bytes are now at.
    pub path: String,
    /// How many bytes were written.
    pub size: usize,
}

omt_catalog::capability! {
    /// Write a blob out where an agent can read it.
    pub struct MediaMaterialize;
    input  = MaterializeIn,
    output = MaterializeOut,
    decl = omt_catalog::Decl {
        name: "media.materialize",
        group: "media",
        verb: "materialize",
        title: "Write a blob to a path",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_FS,
        // Cas: the path is derived from the digest, so materialising twice
        // writes the same bytes to the same place and answers the same.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Under omt's managed root, never inside the workspace: an attachment written into the repository is an unreviewed file from a phone sitting in somebody's working tree.",
    },
}

/// Write the bytes out, given a root.
///
/// Separate from the handler because the root is the only thing here that
/// depends on the environment, and a function that reads the environment can
/// only be tested by writing to it — which this workspace forbids, for the
/// good reason that a process-wide variable is not a test fixture.
fn materialize_into(
    root: &std::path::Path,
    bytes: &[u8],
    kind: &str,
    digest: &str,
    name: Option<&str>,
) -> Result<MaterializeOut, CapabilityError> {
    // The stem, and nothing else. `../../.ssh/authorized_keys` arrives as a
    // name like any other, and joining it to a root is how a blob store
    // becomes a write-anywhere primitive.
    let stem = name
        .and_then(|n| std::path::Path::new(n).file_stem())
        .and_then(|s| s.to_str())
        .map(|s| {
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .take(48)
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "attachment".to_owned());

    std::fs::create_dir_all(root)
        .map_err(|e| CapabilityError::internal(format!("creating the media root: {e}")))?;
    // The digest is in the filename, so two files with the same name are two
    // files, and the same file twice is one.
    let short: String = digest
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect();
    let path = root.join(format!("{stem}-{short}.{}", extension_for(kind)));
    std::fs::write(&path, bytes)
        .map_err(|e| CapabilityError::internal(format!("writing the attachment: {e}")))?;

    Ok(MaterializeOut {
        path: path.display().to_string(),
        size: bytes.len(),
    })
}

impl CapabilityHandler<MediaMaterialize> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: MaterializeIn,
    ) -> Result<MaterializeOut, CapabilityError> {
        let (bytes, kind) = {
            let store = self
                .0
                .lock()
                .map_err(|_| CapabilityError::internal("the blob lock was poisoned"))?;
            let bytes = store
                .get(&input.digest)
                .map_err(|e| CapabilityError::not_found(e.to_string()))?
                .to_vec();
            let kind = store.meta(&input.digest).map_or_else(
                || "binary".to_owned(),
                |r| format!("{:?}", r.meta.kind).to_lowercase(),
            );
            (bytes, kind)
        };
        materialize_into(
            &managed_root()?,
            &bytes,
            &kind,
            &input.digest,
            input.name.as_deref(),
        )
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod materialize_tests {
    use super::*;

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temp dir")
    }

    #[test]
    fn writes_outside_any_workspace() {
        // The point of the whole capability: an attachment from a phone must
        // not appear in somebody's `git status`.
        let dir = root();
        let out = materialize_into(dir.path(), b"hello", "text", "abc123", Some("notes.md"))
            .expect("it materialises");
        assert!(out.path.starts_with('/'), "an agent needs an absolute path");
        assert_eq!(std::fs::read(&out.path).expect("readable"), b"hello");
        assert_eq!(out.size, 5);
    }

    #[test]
    fn a_name_cannot_climb_out_of_the_root() {
        // The name comes from a phone. Joined to a root it is a write-anywhere
        // primitive, so only the stem survives.
        let dir = root();
        let out = materialize_into(
            dir.path(),
            b"x",
            "binary",
            "abc123",
            Some("../../../../etc/passwd"),
        )
        .expect("it materialises");
        assert!(!out.path.contains(".."), "{}", out.path);
        assert!(
            out.path.starts_with(&dir.path().display().to_string()),
            "{}",
            out.path
        );
        assert!(out.path.contains("passwd"), "the stem is still useful");
    }

    #[test]
    fn the_extension_comes_from_the_bytes() {
        // Not from the name. A sender that names its own content type decides
        // how the receiver treats it.
        let dir = root();
        let out = materialize_into(dir.path(), b"text", "text", "abc123", Some("totally.exe"))
            .expect("it materialises");
        assert!(out.path.ends_with(".txt"), "{}", out.path);
    }

    #[test]
    fn the_same_blob_twice_is_one_file() {
        // Cas, as the declaration claims. A retried upload after a dropped
        // acknowledgement must not leave two copies.
        let dir = root();
        let first = materialize_into(dir.path(), b"once", "text", "deadbeef", None).expect("first");
        let second =
            materialize_into(dir.path(), b"once", "text", "deadbeef", None).expect("second");
        assert_eq!(first.path, second.path);
        assert_eq!(std::fs::read_dir(dir.path()).expect("readable").count(), 1);
    }

    #[test]
    fn a_nameless_blob_still_gets_a_filename() {
        // The clipboard has no name, and a file called `.txt` is one nothing
        // will open.
        let dir = root();
        let out =
            materialize_into(dir.path(), b"pasted", "text", "abc123", None).expect("it works");
        assert!(out.path.contains("attachment-"), "{}", out.path);
    }

    #[test]
    fn a_digest_is_never_taken_into_a_filename_verbatim() {
        // A digest arrives as a string like `sha256:...`, and a colon is a
        // path separator on one of the platforms this runs on.
        let dir = root();
        let out =
            materialize_into(dir.path(), b"x", "text", "sha256:ff/00", None).expect("it works");
        let name = std::path::Path::new(&out.path)
            .file_name()
            .expect("a filename");
        assert!(!name.to_string_lossy().contains(':'));
        assert!(!name.to_string_lossy().contains('/'));
    }
}
