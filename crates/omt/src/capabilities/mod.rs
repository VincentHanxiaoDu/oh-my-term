//! The capabilities this binary declares, and the dump codegen reads.

use anyhow::Result;
use omt_catalog::{
    CallContext, CapabilityError, CapabilityHandler, CapabilityRegistry, Decl, DedupKey, Effects,
    Intent, Kind, Parity, capability,
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
pub struct CatalogIn {
    /// The hash the caller already has.
    ///
    /// A matching hash means "nothing to send", which is the point of the
    /// catalog being cacheable at all: a phone reconnecting on a train should
    /// not pull two hundred schemas to learn that none of them changed
    /// ([03 §3.5.1](../../../docs/architecture/03-capability-catalog.md)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_hash: Option<omt_catalog::CatalogHash>,
}

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
    /// Which tier it belongs to.
    ///
    /// Declared rather than hidden, per 03 §3.6: a core capability was checked
    /// by the macro and appears in every generated artifact; a plugin's
    /// registers at run time and can appear in none of them. A client that
    /// could not tell them apart would have no way to explain why one of them
    /// is missing from its generated union.
    pub origin: omt_catalog::Origin,
}

/// What `instance.catalog` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct CatalogOut {
    /// What identifies this catalog.
    pub hash: omt_catalog::CatalogHash,
    /// Whether the caller's cached copy is still current.
    ///
    /// When true, `entries` is empty because there was nothing to say — not
    /// because there are no capabilities. Two different facts, and a client
    /// that conflated them would render an empty instance.
    pub unchanged: bool,
    /// Every capability, in name order. Omitted when `unchanged`.
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
    fn call(&self, _ctx: &CallContext, input: CatalogIn) -> Result<CatalogOut, CapabilityError> {
        let entries: Vec<CatalogEntry> = omt_catalog::linked_decls()
            .into_iter()
            .map(|d| CatalogEntry {
                name: d.name.to_owned(),
                route: d.route(),
                role: d.role,
                effects: d.effects,
                doc: d.doc.to_owned(),
                // Everything in `DECLS` is core by construction: a plugin's
                // capability cannot be there, because it registers after this
                // binary was linked.
                origin: omt_catalog::Origin::Core,
            })
            .collect();
        // From what a client would render differently, not from the binary's
        // version: two builds at the same version with a different plugin
        // enabled are different catalogs.
        let hash = omt_catalog::CatalogHash::of(
            entries
                .iter()
                .map(|e| format!("{} {:?} {:?}", e.name, e.role, e.effects))
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str),
        );
        let unchanged = input.known_hash.as_ref() == Some(&hash);
        Ok(CatalogOut {
            unchanged,
            entries: if unchanged { Vec::new() } else { entries },
            hash,
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

mod dictation;
mod file_transfer;
mod history;
mod keys;
mod layouts;
mod session_input_and_geometry;
mod workspaces;
mod worktrees;

pub use dictation::*;
pub use file_transfer::*;
pub use history::*;
pub use keys::*;
pub use layouts::*;
pub use session_input_and_geometry::*;
pub use workspaces::*;
pub use worktrees::*;

/// Build the registry this binary serves.
///
/// # Errors
/// Fails if a declaration is invalid, duplicated, or has no handler — all at
/// startup, by name, rather than when a caller trips over it.
pub fn registry(state: State) -> Result<CapabilityRegistry> {
    registry_with(state, std::sync::Arc::default())
}

/// Build the registry around a particular credential store.
///
/// Always registered, whether or not anything has been issued: a catalog that
/// changes shape depending on how the process was started is one the parity
/// gate cannot check and the generated client cannot cover. A store with no
/// credentials in it lists no devices, which is a true answer.
///
/// # Errors
/// Fails if a declaration is invalid, duplicated, or has no handler.
pub fn registry_with(
    state: State,
    credentials: std::sync::Arc<std::sync::Mutex<omt_auth::CredentialStore>>,
) -> Result<CapabilityRegistry> {
    registry_watching(state, credentials, std::sync::Arc::default())
}

/// Build the registry around a credential store and a watch.
///
/// # Errors
/// Fails if a declaration is invalid, duplicated, or has no handler.
pub fn registry_watching(
    state: State,
    credentials: std::sync::Arc<std::sync::Mutex<omt_auth::CredentialStore>>,
    watch: omt_server::watch::Shared,
) -> Result<CapabilityRegistry> {
    registry_full(state, credentials, watch, std::sync::Arc::default())
}

/// Build the registry around everything a serving instance holds.
///
/// # Errors
/// Fails if a declaration is invalid, duplicated, or has no handler.
pub fn registry_full(
    state: State,
    credentials: std::sync::Arc<std::sync::Mutex<omt_auth::CredentialStore>>,
    watch: omt_server::watch::Shared,
    identities: std::sync::Arc<std::sync::Mutex<omt_identity::Directory>>,
) -> Result<CapabilityRegistry> {
    let mut r = CapabilityRegistry::new();
    // One shared hint state, because the rule is "one hint", not "one hint per
    // client" — a phone and a terminal each offering their own first tip is the
    // failure this exists to prevent.
    let hints = std::sync::Arc::new(std::sync::Mutex::new(crate::onboarding::Hints::default()));
    r.register::<crate::onboarding::HintState, _>(crate::onboarding::HintsHandler(hints.clone()))?;
    r.register::<crate::onboarding::DismissHints, _>(crate::onboarding::HintsHandler(hints))?;
    let blobs: crate::media::Shared = std::sync::Arc::new(std::sync::Mutex::new(
        omt_media::store::BlobStore::new(omt_media::store::Quota::default()),
    ));
    r.register::<crate::media::MediaPut, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::MediaGet, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::MediaList, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::MediaClipboard, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::MediaAttach, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::MediaMaterialize, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::PasteTier, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::SshPlan, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::OpenResolve, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::OpenClick, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::OpenRulesList, _>(crate::media::MediaHandler(blobs.clone()))?;
    r.register::<crate::media::OpenRulesTest, _>(crate::media::MediaHandler(blobs))?;
    r.register::<crate::surfaces::AgentWait, _>(crate::surfaces::AgentWaitHandler(state.clone()))?;
    r.register::<crate::surfaces::SessionCapture, _>(crate::surfaces::SessionCaptureHandler(
        state.clone(),
    ))?;
    r.register::<crate::surfaces::HistoryQuery, _>(crate::surfaces::HistoryHandler(state.clone()))?;
    r.register::<crate::surfaces::HistoryForget, _>(crate::surfaces::HistoryHandler(
        state.clone(),
    ))?;
    let tasks: crate::ops::Tasks =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
    r.register::<crate::ops::TaskList, _>(crate::ops::TaskHandler(tasks.clone()))?;
    r.register::<crate::ops::TaskGet, _>(crate::ops::TaskHandler(tasks.clone()))?;
    r.register::<crate::ops::TaskCancel, _>(crate::ops::TaskHandler(tasks))?;
    r.register::<crate::ops::UpgradeCheck, _>(crate::ops::UpgradeHandler(state.clone()))?;
    r.register::<crate::ops::InstanceShutdown, _>(crate::ops::LifecycleHandler(state.clone()))?;
    r.register::<crate::ops::RemoteProbe, _>(crate::ops::RemoteHandler)?;
    let queues: crate::queue::Queues =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
    r.register::<crate::queue::QueueEnqueue, _>(crate::queue::QueueHandler(queues.clone()))?;
    r.register::<crate::queue::QueueList, _>(crate::queue::QueueHandler(queues.clone()))?;
    r.register::<crate::queue::QueueRemove, _>(crate::queue::QueueHandler(queues.clone()))?;
    r.register::<crate::queue::QueueEdit, _>(crate::queue::QueueHandler(queues.clone()))?;
    r.register::<crate::queue::QueueReorder, _>(crate::queue::QueueHandler(queues))?;
    let mirrors: crate::media::Mirrors =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
    r.register::<crate::media::RemoteList, _>(crate::media::MirrorHandler(mirrors.clone()))?;
    r.register::<crate::media::RemoteDiscard, _>(crate::media::MirrorHandler(mirrors))?;
    r.register::<crate::media::KeysConflicts, _>(crate::media::KeysHandler)?;
    r.register::<crate::media::KeysAudit, _>(crate::media::KeysHandler)?;
    r.register::<crate::media::KeysFlavour, _>(crate::media::KeysHandler)?;
    r.register::<crate::media::KeysList, _>(crate::media::KeysHandler)?;
    r.register::<crate::media::KeysExplain, _>(crate::media::KeysHandler)?;
    r.register::<crate::media::KeysRegistry, _>(crate::media::KeysHandler)?;
    r.register::<crate::media::PaneNavigate, _>(crate::media::KeysHandler)?;
    let layouts: crate::media::Layouts =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
    r.register::<crate::media::LayoutSave, _>(crate::media::LayoutHandler(layouts.clone()))?;
    r.register::<crate::media::LayoutRestore, _>(crate::media::LayoutHandler(layouts))?;
    r.register::<crate::media::SearchExplain, _>(crate::media::SearchHandler)?;
    r.register::<crate::media::AgentAttention, _>(crate::media::AttentionHandler)?;
    r.register::<crate::media::AgentCompare, _>(crate::media::AttentionHandler)?;
    r.register::<crate::media::SessionTimeline, _>(crate::media::AttentionHandler)?;
    r.register::<crate::media::RecallPolicy, _>(crate::media::AttentionHandler)?;
    r.register::<crate::media::BindCheck, _>(crate::media::SecurityHandler)?;
    r.register::<crate::media::PublishCheck, _>(crate::media::SecurityHandler)?;
    r.register::<crate::media::ReloadClass, _>(crate::media::SecurityHandler)?;
    r.register::<crate::media::LaunchPreview, _>(crate::media::SecurityHandler)?;
    r.register::<crate::media::ReauthCheck, _>(crate::media::SecurityHandler)?;
    r.register::<crate::media::StoreRetention, _>(crate::media::SecurityHandler)?;
    r.register::<crate::media::StorePaths, _>(crate::media::SecurityHandler)?;
    r.register::<crate::media::MigrateStatus, _>(crate::media::SecurityHandler)?;
    r.register::<crate::media::RedactionTest, _>(crate::media::SecurityHandler)?;
    // Filled after every registration, below. A binding file is checked
    // against what this instance actually offers, which is the only list that
    // answers "will this key do anything here".
    let names: std::sync::Arc<std::sync::OnceLock<Vec<String>>> = std::sync::Arc::default();
    r.register::<crate::media::CheckBindingFile, _>(crate::media::BindingFileHandler(
        names.clone(),
    ))?;
    r.register::<crate::migrate::TmuxPreview, _>(crate::migrate::MigrateHandler)?;
    let roots = std::sync::Arc::new(crate::uninstall::Roots::from_environment());
    r.register::<crate::uninstall::UninstallPlan, _>(crate::uninstall::UninstallHandler(
        roots.clone(),
    ))?;
    r.register::<crate::uninstall::UninstallApply, _>(crate::uninstall::UninstallHandler(roots))?;
    r.register::<crate::setup::DetectAgents, _>(crate::setup::SetupHandler)?;
    r.register::<crate::setup::TermProfile, _>(crate::setup::SetupHandler)?;
    r.register::<crate::onboarding::DisclosureState, _>(crate::onboarding::DisclosureHandler(
        std::sync::Arc::new(std::sync::Mutex::new(
            crate::onboarding::Disclosures::default(),
        )),
    ))?;
    r.register::<crate::onboarding::HelpTopics, _>(crate::onboarding::HelpHandler)?;
    r.register::<crate::onboarding::HelpRender, _>(crate::onboarding::HelpHandler)?;
    r.register::<crate::doctor::DoctorRun, _>(crate::doctor::DoctorHandler(state.clone()))?;
    r.register::<crate::doctor::DoctorReport, _>(crate::doctor::ReportHandler(state.clone()))?;
    r.register::<crate::reach::PresenceList, _>(crate::reach::PresenceListHandler(watch.clone()))?;
    r.register::<crate::reach::IdentityList, _>(crate::reach::IdentityListHandler(
        identities.clone(),
    ))?;
    r.register::<crate::reach::IdentityRevoke, _>(crate::reach::IdentityRevokeHandler(
        identities,
        credentials.clone(),
    ))?;
    r.register::<crate::reach::AuditList, _>(crate::reach::AuditListHandler(watch.clone()))?;
    r.register::<crate::reach::InstanceMetrics, _>(crate::reach::InstanceMetricsHandler(
        state.clone(),
        watch,
    ))?;
    r.register::<crate::reach::MediaDescribe, _>(crate::reach::MediaDescribeHandler)?;
    r.register::<crate::reach::ConfigImport, _>(crate::reach::ConfigImportHandler)?;
    r.register::<crate::reach::DeviceList, _>(crate::reach::DeviceListHandler(
        credentials.clone(),
    ))?;
    r.register::<crate::reach::DeviceRevoke, _>(crate::reach::DeviceRevokeHandler(
        credentials.clone(),
    ))?;
    r.register::<crate::reach::DeviceRename, _>(crate::reach::DeviceRenameHandler(
        credentials.clone(),
    ))?;
    r.register::<crate::reach::DeviceRevokeAll, _>(crate::reach::DeviceRenameHandler(
        credentials.clone(),
    ))?;
    r.register::<crate::reach::InviteCreate, _>(crate::reach::InviteCreateHandler(credentials))?;
    r.register::<crate::surfaces::BlockBody, _>(crate::surfaces::BlockBodyHandler(state.clone()))?;
    r.register::<crate::surfaces::SessionScrollback, _>(
        crate::surfaces::SessionScrollbackHandler(state.clone()),
    )?;
    r.register::<crate::surfaces::SearchSession, _>(crate::surfaces::SearchSessionHandler(
        state.clone(),
    ))?;
    r.register::<crate::surfaces::SessionTranscript, _>(
        crate::surfaces::SessionTranscriptHandler(state.clone()),
    )?;
    r.register::<crate::surfaces::AgentOffers, _>(crate::surfaces::AgentOffersHandler(
        state.clone(),
    ))?;
    r.register::<crate::surfaces::BlockRerun, _>(crate::surfaces::BlockRerunHandler(
        state.clone(),
    ))?;
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
    r.register::<AgentConnect, _>(AgentConnectHandler(state.clone()))?;
    r.register::<LayoutOpen, _>(LayoutOpenHandler(state.clone()))?;
    r.register::<PaneList, _>(PaneListHandler(state.clone()))?;
    r.register::<PaneOpen, _>(PaneOpenHandler(state.clone()))?;
    r.register::<PaneClose, _>(PaneCloseHandler(state.clone()))?;
    r.register::<PaneFocus, _>(PaneFocusHandler(state.clone()))?;
    r.register::<StateSave, _>(StateSaveHandler(state.clone()))?;
    r.register::<StateRestore, _>(StateRestoreHandler(state.clone()))?;
    r.register::<RecallSuggest, _>(RecallSuggestHandler(state.clone()))?;
    r.register::<RecallRecord, _>(RecallRecordHandler(state.clone()))?;
    r.register::<PluginInstall, _>(PluginInstallHandler(state.clone()))?;
    r.register::<PluginStart, _>(PluginStartHandler(state.clone()))?;
    r.register::<PluginList, _>(PluginListHandler(state.clone()))?;
    r.register::<PluginEnable, _>(PluginEnableHandler(state.clone()))?;
    r.register::<JobCreate, _>(JobCreateHandler(state.clone()))?;
    r.register::<JobList, _>(JobListHandler(state.clone()))?;
    r.register::<VoiceProviders, _>(VoiceProvidersHandler(state.clone()))?;
    r.register::<VoiceAppend, _>(VoiceAppendHandler(state.clone()))?;
    r.register::<VoiceClear, _>(VoiceClearHandler(state.clone()))?;
    r.register::<FanoutStart, _>(FanoutStartHandler(state.clone()))?;
    r.register::<FanoutStatus, _>(FanoutStatusHandler(state.clone()))?;
    r.register::<FanoutChoose, _>(FanoutChooseHandler(state.clone()))?;
    r.register::<WorktreeList, _>(WorktreeListHandler(state.clone()))?;
    r.register::<WorktreeAdd, _>(WorktreeAddHandler(state.clone()))?;
    r.register::<WorktreeRemove, _>(WorktreeRemoveHandler(state.clone()))?;
    r.register::<GitHunks, _>(GitHunksHandler(state.clone()))?;
    r.register::<UsageReport, _>(UsageReportHandler(state.clone()))?;
    r.register::<ConfigSchema, _>(ConfigSchemaHandler(state.clone()))?;
    r.register::<ShellIntegration, _>(ShellIntegrationHandler(state.clone()))?;
    r.register::<ThemeGet, _>(ThemeGetHandler(state.clone()))?;
    r.register::<OpenRecognize, _>(OpenRecognizeHandler(state.clone()))?;
    r.register::<FsRead, _>(FsReadHandler(state.clone()))?;
    r.register::<FsWrite, _>(FsWriteHandler(state.clone()))?;
    r.register::<InteractionList, _>(InteractionListHandler(state.clone()))?;
    r.register::<InteractionRespond, _>(InteractionRespondHandler(state.clone()))?;
    r.register::<SessionBlocks, _>(SessionBlocksHandler(state.clone()))?;
    r.register::<SessionRestart, _>(SessionRestartHandler(state.clone()))?;
    r.register::<SessionCreate, _>(SessionCreateHandler(state.clone()))?;
    r.register::<SessionAcquire, _>(SessionAcquireHandler(state.clone()))?;
    r.register::<SessionRelease, _>(SessionReleaseHandler(state.clone()))?;
    r.register::<SessionRead, _>(SessionReadHandler(state.clone()))?;
    r.register::<SessionSnapshot, _>(SessionSnapshotHandler(state.clone()))?;
    r.register::<ConfigGet, _>(ConfigGetHandler(state))?;
    r.register::<KeysCheatsheet, _>(KeysHandler)?;
    // Now that everything is registered. Doing this above would capture a
    // subset that depends on the order of the lines, and a binding naming a
    // real capability would be reported as naming none.
    let _ = names.set(r.decls().map(|d| d.name.to_owned()).collect());
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

    // Walked, not written out. This hand-listed six schemas, which meant the
    // committed artifact described three capabilities out of a hundred and
    // forty — and every client that generates types from it generated types
    // for three.
    let per_capability = registry.schemas(&mut generator);
    let schemas: serde_json::Map<String, serde_json::Value> = per_capability
        .into_iter()
        .flat_map(|(name, (input, output))| {
            [
                (format!("{name}.input"), input),
                (format!("{name}.output"), output),
            ]
        })
        .collect();

    // The definitions those schemas point at. Without them every `$ref`
    // resolves to nothing, which is worse than an absent schema: it parses,
    // and a generator built on it emits empty types rather than failing.
    let definitions: serde_json::Map<String, serde_json::Value> =
        generator.take_definitions(true).into_iter().collect();

    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "proto": omt_proto::PROTO_VERSION,
        "capabilities": entries,
        "schemas": schemas,
        "$defs": definitions,
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
    fn two_repositories_sharing_a_parent_do_not_collide_on_a_worktree() {
        // Every `~/src` is two repositories in one parent. Resolving a
        // worktree straight into that parent means the second one to ask for
        // `feature` gets git complaining about a path already in use — for a
        // reason that has nothing to do with what they asked.
        let a = resolve_worktree_path("/home/me/src/alpha", "feature");
        let b = resolve_worktree_path("/home/me/src/beta", "feature");
        assert_ne!(a, b, "two repositories resolved to one worktree path");
    }

    #[test]
    fn a_worktree_is_not_placed_inside_the_checkout() {
        // A second checkout inside the tree git already tracks confuses every
        // tool that walks it.
        let path = resolve_worktree_path("/home/me/src/alpha", "feature");
        assert!(
            !path.starts_with("/home/me/src/alpha/"),
            "the worktree is inside the repository: {path}"
        );
    }

    #[test]
    fn a_worktree_directory_says_which_repository_it_belongs_to() {
        // A stray `feature/` in a parent directory says nothing. This one does.
        let path = resolve_worktree_path("/home/me/src/alpha", "feature");
        assert!(path.contains("alpha"), "{path}");
    }

    #[test]
    fn an_absolute_worktree_path_is_taken_as_given() {
        // Somebody who spelled out where they want it means it.
        assert_eq!(
            resolve_worktree_path("/home/me/src/alpha", "/tmp/elsewhere"),
            "/tmp/elsewhere"
        );
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
