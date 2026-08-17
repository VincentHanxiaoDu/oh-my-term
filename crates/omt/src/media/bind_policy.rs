//! Bind policy.
//!
//! Split out of one 3478-line module, over P1's 2000-line limit.

use super::*;

// ---------------------------------------------------------------------------
// Bind policy — 13 §2
// ---------------------------------------------------------------------------

/// Input to `security.bind_check`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct BindCheckIn {
    /// Where omt would listen: `unix`, `loopback`, `tailnet`, or an address.
    pub binds: Vec<String>,
    /// Whether a password or bearer token is configured.
    #[serde(default)]
    pub auth_backend: bool,
    /// Whether Tailscale's LocalAPI supplies identity.
    #[serde(default)]
    pub tailnet_identity: bool,
    /// Whether omt terminates TLS.
    #[serde(default)]
    pub tls: bool,
    /// Whether a reverse proxy is declared to terminate it.
    #[serde(default)]
    pub trust_proxy: bool,
    /// What an unauthenticated caller gets: `none`, `viewer` or `operator`.
    #[serde(default)]
    pub default_role: Option<String>,
}

/// Something that must be fixed first.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RefusalOut {
    /// What is wrong.
    pub problem: String,
    /// What to do about it.
    pub remedy: String,
}

/// What `security.bind_check` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BindCheckOut {
    /// Whether omt would start.
    pub may_start: bool,
    /// Everything that must be fixed, not just the first.
    pub refusals: Vec<RefusalOut>,
    /// Things worth knowing that do not stop it.
    pub warnings: Vec<String>,
}

omt_catalog::capability! {
    /// Whether a listen configuration is one omt will start on.
    pub struct BindCheck;
    input  = BindCheckIn,
    output = BindCheckOut,
    decl = omt_catalog::Decl {
        name: "security.bind_check",
        group: "security",
        verb: "bind_check",
        title: "Check a listen configuration",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A misconfiguration fails the daemon rather than warning. A warning about an open port is printed once, into a log nobody reads, on a machine that is now reachable from the network.",
    },
}

/// Serves the bind check.
pub struct SecurityHandler;

impl CapabilityHandler<BindCheck> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: BindCheckIn,
    ) -> Result<BindCheckOut, CapabilityError> {
        use omt_auth::bind::{Auth, Bind, DefaultRole, Listen, Transport, check};
        let mut binds = Vec::new();
        for spec in &input.binds {
            binds.push(match spec.as_str() {
                "unix" => Bind::UnixSocket {
                    path: "/run/omt.sock".to_owned(),
                },
                "loopback" => Bind::Loopback { port: 7878 },
                "tailnet" => Bind::Tailnet { port: 7878 },
                other => {
                    let addr = other.parse().map_err(|_| {
                        CapabilityError::invalid_input(format!(
                            "`{other}` is not `unix`, `loopback`, `tailnet` or an IP address"
                        ))
                    })?;
                    Bind::Interface { addr, port: 7878 }
                }
            });
        }
        let verdict = check(&Listen {
            binds,
            auth: Auth {
                backend: input.auth_backend,
                tailnet_identity: input.tailnet_identity,
            },
            transport: Transport {
                tls: input.tls,
                trust_proxy: input.trust_proxy,
            },
            default_role: match input.default_role.as_deref() {
                Some("viewer") => DefaultRole::Viewer,
                Some("operator") => DefaultRole::Operator,
                _ => DefaultRole::None,
            },
        });
        Ok(BindCheckOut {
            may_start: verdict.may_start(),
            refusals: verdict
                .refusals
                .into_iter()
                .map(|r| RefusalOut {
                    problem: r.problem,
                    remedy: r.remedy,
                })
                .collect(),
            warnings: verdict.warnings.into_iter().map(|w| w.note).collect(),
        })
    }
}

/// Input to `security.publish_check`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PublishCheckIn {
    /// A password or bearer backend is configured.
    #[serde(default)]
    pub auth_backend: bool,
    /// The only identity available is the tailnet's.
    #[serde(default)]
    pub tailnet_identity_only: bool,
    /// How long the configured password is.
    #[serde(default)]
    pub password_length: Option<usize>,
    /// Bearer tokens are bound to a device key.
    #[serde(default)]
    pub bearer_device_bound: bool,
    /// Rate limiting and lockout are on.
    #[serde(default)]
    pub rate_limited: bool,
    /// The origins configured, verbatim.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// What an unauthenticated caller gets.
    #[serde(default)]
    pub default_role: Option<String>,
}

omt_catalog::capability! {
    /// Whether this instance may be put on the public internet.
    pub struct PublishCheck;
    input  = PublishCheckIn,
    output = BindCheckOut,
    decl = omt_catalog::Decl {
        name: "security.publish_check",
        group: "security",
        verb: "publish_check",
        title: "Check before publishing",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Funnel puts the instance on the open internet, and the difference between that and a tailnet is not visible in any configuration omt reads. Somebody who enabled it for a demo and forgot is who this is for.",
    },
}

impl CapabilityHandler<PublishCheck> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: PublishCheckIn,
    ) -> Result<BindCheckOut, CapabilityError> {
        use omt_auth::bind::{DefaultRole, Publication, check_publication};
        let verdict = check_publication(&Publication {
            auth_backend: input.auth_backend,
            tailnet_identity_only: input.tailnet_identity_only,
            password_length: input.password_length,
            bearer_device_bound: input.bearer_device_bound,
            rate_limited: input.rate_limited,
            allowed_origins: input.allowed_origins,
            default_role: match input.default_role.as_deref() {
                Some("viewer") => DefaultRole::Viewer,
                Some("operator") => DefaultRole::Operator,
                _ => DefaultRole::None,
            },
        });
        Ok(BindCheckOut {
            may_start: verdict.may_start(),
            refusals: verdict
                .refusals
                .into_iter()
                .map(|r| RefusalOut {
                    problem: r.problem,
                    remedy: r.remedy,
                })
                .collect(),
            warnings: verdict.warnings.into_iter().map(|w| w.note).collect(),
        })
    }
}

/// Input to `config.reload_class`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReloadClassIn {
    /// The keys to ask about.
    pub keys: Vec<String>,
}

/// One key and when a change to it takes effect.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReloadEntry {
    /// The key.
    pub key: String,
    /// `hot`, `restart_server` or `restart`.
    pub class: String,
}

/// What `config.reload_class` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReloadClassOut {
    /// One entry per key asked about.
    pub keys: Vec<ReloadEntry>,
    /// Which of them will not take effect until something restarts.
    ///
    /// Listed separately because that is the answer somebody needs. A setting
    /// that quietly waits for a restart is the worst of the three: it is
    /// changed, nothing happens, and the conclusion is that the setting does
    /// not work.
    pub pending: Vec<String>,
}

omt_catalog::capability! {
    /// When a change to a setting takes effect.
    pub struct ReloadClass;
    input  = ReloadClassIn,
    output = ReloadClassOut,
    decl = omt_catalog::Decl {
        name: "config.reload_class",
        group: "config",
        verb: "reload_class",
        title: "When a setting takes effect",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A setting that quietly waits for a restart is changed, nothing happens, and the conclusion is that the setting does not work.",
    },
}

impl CapabilityHandler<ReloadClass> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ReloadClassIn,
    ) -> Result<ReloadClassOut, CapabilityError> {
        let entries: Vec<ReloadEntry> = input
            .keys
            .into_iter()
            .map(|key| {
                let class = omt_config::schema::reload_class(&key);
                ReloadEntry {
                    key,
                    class: format!("{class:?}")
                        .chars()
                        .flat_map(|c| {
                            if c.is_uppercase() {
                                vec!['_', c.to_ascii_lowercase()]
                            } else {
                                vec![c]
                            }
                        })
                        .collect::<String>()
                        .trim_start_matches('_')
                        .to_owned(),
                }
            })
            .collect();
        Ok(ReloadClassOut {
            pending: entries
                .iter()
                .filter(|e| e.class != "hot")
                .map(|e| e.key.clone())
                .collect(),
            keys: entries,
        })
    }
}

/// Input to `launch.preview`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct LaunchPreviewIn {
    /// The launch configuration, as JSON.
    pub launch: serde_json::Value,
}

/// One pane omt would open.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LaunchPane {
    /// Where it starts.
    pub cwd: Option<String>,
    /// What runs in it: `shell`, `commands` or `agent`.
    pub start: String,
    /// The argument vector, for an agent pane.
    ///
    /// A vector, never a string. There is no shell in this path, so a prompt
    /// containing a backtick is a prompt rather than an execution.
    pub argv: Vec<String>,
}

/// What `launch.preview` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LaunchPreviewOut {
    /// What omt would open.
    pub panes: Vec<LaunchPane>,
    /// Which pane focus lands in.
    pub focused: Option<usize>,
    /// Everything wrong with it, all at once.
    pub problems: Vec<RefusalOut>,
}

omt_catalog::capability! {
    /// What a launch configuration would start.
    pub struct LaunchPreview;
    input  = LaunchPreviewIn,
    output = LaunchPreviewOut,
    decl = omt_catalog::Decl {
        name: "launch.preview",
        group: "launch",
        verb: "preview",
        title: "Preview a launch configuration",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "An agent is spawned with an argument vector, never by typing into a shell — a launch file is copied from a colleague and the prompt is the part people edit.",
    },
}

impl CapabilityHandler<LaunchPreview> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: LaunchPreviewIn,
    ) -> Result<LaunchPreviewOut, CapabilityError> {
        let launch: omt_config::launch::Launch =
            serde_json::from_value(input.launch).map_err(|e| {
                CapabilityError::invalid_input(format!("that is not a launch configuration: {e}"))
            })?;
        let problems = omt_config::launch::check(&launch);
        Ok(LaunchPreviewOut {
            focused: omt_config::launch::focused(&launch),
            panes: launch
                .panes
                .iter()
                .map(|p| LaunchPane {
                    cwd: p.cwd.clone(),
                    start: match &p.start {
                        omt_config::launch::Start::Shell => "shell".to_owned(),
                        omt_config::launch::Start::Commands { .. } => "commands".to_owned(),
                        omt_config::launch::Start::Agent { .. } => "agent".to_owned(),
                    },
                    argv: match &p.start {
                        omt_config::launch::Start::Agent { id, args, prompt } => {
                            omt_config::launch::agent_argv(id, args, prompt.as_deref())
                        }
                        _ => Vec::new(),
                    },
                })
                .collect(),
            problems: problems
                .into_iter()
                .map(|r| RefusalOut {
                    problem: r.problem,
                    remedy: r.help,
                })
                .collect(),
        })
    }
}

/// Input to `keys.check_file`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CheckFileIn {
    /// The leader chord.
    #[serde(default)]
    pub leader: String,
    /// Which flavour.
    #[serde(default)]
    pub keymap: Option<String>,
    /// Trigger to action.
    #[serde(default)]
    pub bindings: Vec<(String, String)>,
    /// The file itself.
    ///
    /// The form a user actually has: everything 10 §8.2 specifies beyond a
    /// trigger and a name — `when`, `args`, `force`, `platform`, `requires`,
    /// `modes` — can only be checked from the text. The pair form remains for
    /// a client that is building a file rather than reading one.
    #[serde(default)]
    pub text: Option<String>,
}

/// Something wrong with a keybinding file.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BindingProblemOut {
    /// The stable code to look it up by.
    #[serde(default)]
    pub code: String,
    /// Which trigger.
    pub trigger: String,
    /// What is wrong.
    pub message: String,
    /// What to do.
    pub help: String,
}

/// What `keys.check_file` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CheckFileOut {
    /// Everything wrong, in file order.
    pub problems: Vec<BindingProblemOut>,
}

omt_catalog::capability! {
    /// Check a keybinding file against the capabilities that exist.
    pub struct CheckBindingFile;
    input  = CheckFileIn,
    output = CheckFileOut,
    decl = omt_catalog::Decl {
        name: "keys.check_file",
        group: "keys",
        verb: "check_file",
        title: "Check a keybinding file",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A binding naming a capability that is not there parses, loads and does nothing — and the person who wrote it concludes their key is broken rather than their spelling.",
    },
}

/// Serves the keybinding file check, which needs the catalog to check against.
///
/// A cell filled *after* registration finishes rather than a list captured
/// during it. Capturing mid-registration gives whatever happened to be
/// registered so far, which is a subset that changes with the order of the
/// lines above — and the failure is a valid binding reported as naming a
/// capability that does not exist. Found exactly that way.
pub struct BindingFileHandler(pub std::sync::Arc<std::sync::OnceLock<Vec<String>>>);

impl CapabilityHandler<CheckBindingFile> for BindingFileHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: CheckFileIn,
    ) -> Result<CheckFileOut, CapabilityError> {
        let capabilities = self.0.get().map_or(&[][..], Vec::as_slice);

        // The text form when there is one, because it is the only form that
        // can carry what the document specifies.
        let (file, mut problems) = match &input.text {
            Some(text) => match omt_input::bindings::parse_text(text) {
                Ok(read) => read,
                Err(err) => {
                    return Ok(CheckFileOut {
                        problems: vec![BindingProblemOut {
                            code: omt_input::bindings::codes::BAD_ACTION.to_owned(),
                            trigger: "the file".to_owned(),
                            message: format!("this is not valid TOML: {err}"),
                            help: "Fix the syntax; nothing in it can be checked until then."
                                .to_owned(),
                        }],
                    });
                }
            },
            None => (
                omt_input::bindings::BindingFile {
                    leader: if input.leader.is_empty() {
                        omt_input::bindings::DEFAULT_LEADER.to_owned()
                    } else {
                        input.leader.clone()
                    },
                    keymap: input.keymap.clone().unwrap_or_else(|| "default".to_owned()),
                    bindings: input
                        .bindings
                        .iter()
                        .map(|(trigger, action)| {
                            omt_input::bindings::Binding::simple(trigger, action)
                        })
                        .collect(),
                },
                Vec::new(),
            ),
        };

        // Compiling is the check: a trigger that is not a chord, a `when` over
        // a context omt does not have, a reserved chord taken without meaning
        // it, a gate that cannot be met here — each is a binding that would
        // not be installed, and every one of them used to be silence.
        let modal = matches!(file.keymap.as_str(), "vim" | "emacs");
        let (_, refused) = omt_input::resolve::compile(
            &[(omt_input::bindings::Source::User, file)],
            &omt_input::resolve::Gates {
                platform: std::env::consts::OS.to_owned(),
                terminal: Vec::new(),
                modal,
            },
            capabilities,
        );
        problems.extend(refused);

        Ok(CheckFileOut {
            problems: problems
                .into_iter()
                .map(|p| BindingProblemOut {
                    code: p.code.to_owned(),
                    trigger: p.trigger,
                    message: p.message,
                    help: p.help,
                })
                .collect(),
        })
    }
}

/// Input to `identity.reauth_check`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReauthIn {
    /// Which capability is being called.
    pub capability: String,
    /// Its effects.
    #[serde(default)]
    pub effects: Vec<String>,
    /// How long since this device verified, in seconds. Absent means never.
    #[serde(default)]
    pub verified_secs_ago: Option<u64>,
    /// Whether step-up is on.
    #[serde(default)]
    pub enabled: bool,
    /// Capability names that always ask.
    #[serde(default)]
    pub require: Vec<String>,
    /// Effects that ask.
    #[serde(default)]
    pub require_effects: Vec<String>,
    /// How long a verification counts for.
    #[serde(default)]
    pub freshness_secs: Option<u64>,
}

/// What `identity.reauth_check` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReauthOut {
    /// Whether the call proceeds without a prompt.
    pub proceeds: bool,
    /// Why it is asking, when it is.
    pub because: Option<String>,
    /// Whether failing narrows what the device may do.
    ///
    /// Always false, and reported so a client cannot implement it as if it
    /// might be true. A failed step-up is "not now", never "not you".
    pub failing_narrows_authority: bool,
}

omt_catalog::capability! {
    /// Whether this call needs a fresh proof that a human is here.
    pub struct ReauthCheck;
    input  = ReauthIn,
    output = ReauthOut,
    decl = omt_catalog::Decl {
        name: "identity.reauth_check",
        group: "identity",
        verb: "reauth_check",
        title: "Does this need a fresh check",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A proof-of-presence timer, not an authorisation narrowing. Failing it means the device must show a human is holding it, and then it may do everything again.",
    },
}

impl CapabilityHandler<ReauthCheck> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ReauthIn,
    ) -> Result<ReauthOut, CapabilityError> {
        let policy = omt_auth::reauth::Policy {
            mode: if input.enabled {
                omt_auth::reauth::Mode::On
            } else {
                omt_auth::reauth::Mode::Off
            },
            require: input.require.into_iter().collect(),
            require_effects: input.require_effects,
            freshness_secs: input.freshness_secs.unwrap_or(300),
            fallback: omt_auth::reauth::Fallback::Passphrase,
        };
        let decision = omt_auth::reauth::decide(
            &policy,
            &input.capability,
            &input.effects,
            input.verified_secs_ago,
        );
        Ok(ReauthOut {
            proceeds: decision.proceeds(),
            because: match decision {
                omt_auth::reauth::Decision::Challenge { because } => Some(because),
                omt_auth::reauth::Decision::Proceed => None,
            },
            failing_narrows_authority: omt_auth::reauth::failing_narrows_authority(),
        })
    }
}

/// Input to `store.retention`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RetentionIn {
    /// How old, in days.
    pub age_days: u32,
    /// `output`, `block_metadata`, `history`, `agent_events`, `interactions`
    /// or `transcript`.
    pub kind: String,
    /// Whether the block exited non-zero.
    #[serde(default)]
    pub failed: bool,
    /// Whether an agent was attributed to it.
    #[serde(default)]
    pub agent_touched: bool,
    /// How large its output is.
    #[serde(default)]
    pub output_bytes: u64,
}

/// What `store.retention` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RetentionOut {
    /// `keep`, `compact` or `delete`.
    pub fate: String,
    /// When it would be compacted, in days. Absent means never.
    pub compact_after_days: Option<u32>,
    /// When it would be deleted.
    pub delete_after_days: u32,
}

omt_catalog::capability! {
    /// What happens to a piece of data of this age.
    pub struct StoreRetention;
    input  = RetentionIn,
    output = RetentionOut,
    decl = omt_catalog::Decl {
        name: "store.retention",
        group: "store",
        verb: "retention",
        title: "What happens to this data",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Compact first, delete second. The output of a build from three months ago is worthless; the fact that you ran it, where, and that it failed, is still useful and costs two hundred bytes.",
    },
}

impl CapabilityHandler<StoreRetention> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RetentionIn,
    ) -> Result<RetentionOut, CapabilityError> {
        use omt_store::retention::{Block, Kind, compact, fate, ladder};
        let kind = match input.kind.as_str() {
            "block_metadata" => Kind::BlockMetadata,
            "history" => Kind::History,
            "agent_events" => Kind::AgentEvents,
            "interactions" => Kind::Interactions,
            "transcript" => Kind::Transcript,
            "output" => Kind::Output,
            other => {
                return Err(CapabilityError::invalid_input(format!(
                    "no data kind `{other}`"
                )));
            }
        };
        let decided = if kind == Kind::Output {
            compact(&Block {
                age_days: input.age_days,
                failed: input.failed,
                agent_touched: input.agent_touched,
                output_bytes: input.output_bytes,
            })
        } else {
            fate(kind, input.age_days)
        };
        let l = ladder(kind);
        Ok(RetentionOut {
            fate: format!("{decided:?}").to_lowercase(),
            compact_after_days: l.compact_after_days,
            delete_after_days: l.delete_after_days,
        })
    }
}

/// Input to `store.paths`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct StorePathsIn {}

/// One thing omt writes down.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ArtefactOut {
    /// What it is.
    pub what: String,
    /// Where it lives, relative to the state directory.
    pub location: String,
    /// The permission bits it must have.
    pub mode: String,
    /// Whether it can hold something somebody would not want copied.
    pub sensitive: bool,
    /// When it is deleted.
    pub delete_after_days: u32,
}

/// What `store.paths` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct StorePathsOut {
    /// Everything omt persists.
    pub artefacts: Vec<ArtefactOut>,
    /// One line somebody deciding whether to run this can read.
    pub summary: String,
    /// What omt does and does not encrypt.
    pub at_rest: String,
}

omt_catalog::capability! {
    /// Everything omt writes to disk, and what happens to it.
    pub struct StorePaths;
    input  = StorePathsIn,
    output = StorePathsOut,
    decl = omt_catalog::Decl {
        name: "store.paths",
        group: "store",
        verb: "paths",
        title: "What omt writes down",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The alternative to this list is finding out what omt persists by reading twelve crates, which nobody does — least of all somebody deciding whether to run it on a work machine.",
    },
}

impl CapabilityHandler<StorePaths> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: StorePathsIn,
    ) -> Result<StorePathsOut, CapabilityError> {
        Ok(StorePathsOut {
            artefacts: omt_store::inventory::INVENTORY
                .iter()
                .map(|a| ArtefactOut {
                    what: a.what.to_owned(),
                    location: a.where_.to_owned(),
                    mode: format!("{:o}", a.mode),
                    sensitive: a.sensitive,
                    delete_after_days: omt_store::retention::ladder(a.retention).delete_after_days,
                })
                .collect(),
            summary: omt_store::inventory::summary(),
            at_rest: omt_store::inventory::at_rest_summary(),
        })
    }
}

/// Input to `store.migrate_status`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct MigrateStatusIn {
    /// The contents of the store's version marker.
    pub marker: String,
}

/// What `store.migrate_status` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MigrateStatusOut {
    /// Whether this build can open it.
    pub openable: bool,
    /// The migrations that would run, in order.
    pub steps: Vec<u32>,
    /// Why not, when it cannot.
    pub problem: Option<String>,
    /// What to do about it.
    pub remedy: Option<String>,
}

omt_catalog::capability! {
    /// Whether this build can open a store, and what it would migrate.
    pub struct MigrateStatus;
    input  = MigrateStatusIn,
    output = MigrateStatusOut,
    decl = omt_catalog::Decl {
        name: "store.migrate_status",
        group: "store",
        verb: "migrate_status",
        title: "Can this store be opened",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "There is no best-effort partial parse. A partial read of a newer format silently drops whatever that version added, and writes the store back without it.",
    },
}

impl CapabilityHandler<MigrateStatus> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: MigrateStatusIn,
    ) -> Result<MigrateStatusOut, CapabilityError> {
        match omt_store::version::plan(&input.marker) {
            Ok(steps) => Ok(MigrateStatusOut {
                openable: true,
                steps,
                problem: None,
                remedy: None,
            }),
            Err(refusal) => {
                let json = serde_json::to_value(&refusal).unwrap_or_default();
                Ok(MigrateStatusOut {
                    openable: false,
                    steps: Vec::new(),
                    problem: Some(
                        json.get("code")
                            .and_then(|c| c.as_str())
                            .unwrap_or("unreadable")
                            .to_owned(),
                    ),
                    remedy: json
                        .get("remedy")
                        .and_then(|r| r.as_str())
                        .map(str::to_owned),
                })
            }
        }
    }
}

/// Input to `store.redaction_test`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RedactionTestIn {
    /// The text to try.
    pub text: String,
}

/// What `store.redaction_test` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RedactionTestOut {
    /// What would be written.
    pub redacted: String,
    /// How many spans were replaced.
    pub removed: usize,
}

omt_catalog::capability! {
    /// What the redactor would do to this text.
    pub struct RedactionTest;
    input  = RedactionTestIn,
    output = RedactionTestOut,
    decl = omt_catalog::Decl {
        name: "store.redaction_test",
        group: "store",
        verb: "redaction_test",
        title: "Try the redactor",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "So somebody can find out what the detector catches before trusting it with something, rather than after.",
    },
}

impl CapabilityHandler<RedactionTest> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RedactionTestIn,
    ) -> Result<RedactionTestOut, CapabilityError> {
        let out = omt_recall::redact::redact(&omt_recall::redact::Raw::new(input.text));
        Ok(RedactionTestOut {
            redacted: out.text().to_owned(),
            removed: out.removed,
        })
    }
}
