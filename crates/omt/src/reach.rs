//! Capabilities for machinery that existed and nothing could reach.
//!
//! Three crates were written, tested and unreachable: `omt-identity` knew about
//! devices and could revoke them, `omt-media` could identify and name a blob,
//! and `omt-config`'s importer could read settings out of the tools people
//! already use. None of them had a capability, so none of them had a surface —
//! and a feature no client can call is a feature nobody has.
//!
//! The device one matters most: pairing now mints a credential per device, and
//! without this there was no way to see what had been paired or to revoke a
//! phone that was lost. A revocable credential nobody can revoke is not one.

use omt_catalog::{CapabilityError, CapabilityHandler};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/// Input to `device.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeviceListIn {}

/// One device that has been let in.
#[derive(Serialize, schemars::JsonSchema)]
pub struct DeviceSummary {
    /// The credential's id, which is what revoking names.
    pub id: String,
    /// What the device called itself when it joined.
    pub label: String,
    /// When it joined.
    pub created_at: String,
    /// Whether it has been revoked.
    pub revoked: bool,
    /// Whether the credential is bound to a key the device holds.
    ///
    /// False for a browser, and honestly so: a key in local storage is
    /// exfiltrated with the token it would protect, so binding there is theatre.
    pub bound: bool,
    /// What it may do.
    pub role: String,
}

/// What `device.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct DeviceListOut {
    /// Newest first, because the one somebody is about to revoke is usually the
    /// one they just noticed.
    pub devices: Vec<DeviceSummary>,
}

omt_catalog::capability! {
    /// Every device that holds a credential.
    pub struct DeviceList;
    input  = DeviceListIn,
    output = DeviceListOut,
    decl = omt_catalog::Decl {
        name: "device.list",
        group: "device",
        verb: "list",
        title: "List paired devices",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Every device that has been let in, by the name it gave. Pairing mints a credential per device, and a credential nobody can see is one nobody revokes.",
    },
}

/// Serves `device.list`.
pub struct DeviceListHandler(pub std::sync::Arc<std::sync::Mutex<omt_auth::CredentialStore>>);

impl CapabilityHandler<DeviceList> for DeviceListHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: DeviceListIn,
    ) -> Result<DeviceListOut, CapabilityError> {
        let store = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the credential lock was poisoned"))?;
        let mut devices: Vec<DeviceSummary> = store
            .list()
            .into_iter()
            .map(|c| DeviceSummary {
                id: c.id.clone(),
                label: c.label.clone(),
                created_at: c.created_at.to_string(),
                revoked: c.revoked,
                bound: c.device_key.is_some(),
                role: format!("{:?}", c.role).to_lowercase(),
            })
            .collect();
        devices.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(DeviceListOut { devices })
    }
}

/// Input to `device.revoke`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeviceRevokeIn {
    /// Which credential.
    pub device: String,
}

/// What `device.revoke` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct DeviceRevokeOut {
    /// Whether this call is what revoked it.
    ///
    /// False for one already revoked, which is not an error: a second tap on a
    /// row somebody already dealt with should not look like a failure.
    pub revoked: bool,
}

omt_catalog::capability! {
    /// Take a device's access away.
    pub struct DeviceRevoke;
    input  = DeviceRevokeIn,
    output = DeviceRevokeOut,
    decl = omt_catalog::Decl {
        name: "device.revoke",
        group: "device",
        verb: "revoke",
        title: "Revoke a device",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        // Admin: revoking a device is administering the instance, not driving
        // a terminal, and an operator credential should not be able to remove
        // the credential of the person who issued it.
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::DESTRUCTIVE,
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Stop one device's credential working, leaving every other device alone. This is what makes a lost phone something somebody can act on.",
    },
}

/// Serves `device.revoke`.
pub struct DeviceRevokeHandler(pub std::sync::Arc<std::sync::Mutex<omt_auth::CredentialStore>>);

impl CapabilityHandler<DeviceRevoke> for DeviceRevokeHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: DeviceRevokeIn,
    ) -> Result<DeviceRevokeOut, CapabilityError> {
        let mut store = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the credential lock was poisoned"))?;
        // `revoke` answers "did I find it", which is not the question this
        // reports. Whether *this call* changed anything is read before, so a
        // second tap on a row somebody already dealt with says so rather than
        // claiming to have revoked it again.
        let already = match store.list().iter().find(|c| c.id == input.device) {
            Some(found) => found.revoked,
            None => {
                return Err(CapabilityError::not_found(format!(
                    "no device {}",
                    input.device
                )));
            }
        };
        store.revoke(&input.device);
        Ok(DeviceRevokeOut { revoked: !already })
    }
}

// ---------------------------------------------------------------------------
// Media
// ---------------------------------------------------------------------------

/// Input to `media.describe`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct MediaDescribeIn {
    /// The bytes, base64 encoded.
    pub data: String,
}

/// What `media.describe` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct MediaDescribeOut {
    /// What it is, judged from the bytes.
    pub kind: String,
    /// Its media type, where the bytes identify one.
    pub media_type: Option<String>,
    /// A filename safe to write it as.
    pub filename: String,
    /// Its content digest, which is its identity in the blob store.
    pub digest: String,
    /// How many bytes.
    pub bytes: u64,
}

omt_catalog::capability! {
    /// Identify a blob from its bytes.
    pub struct MediaDescribe;
    input  = MediaDescribeIn,
    output = MediaDescribeOut,
    decl = omt_catalog::Decl {
        name: "media.describe",
        group: "media",
        verb: "describe",
        title: "Identify a blob",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "What a blob is, from its bytes rather than from a filename somebody supplied. A name is a claim; the bytes are the evidence, and writing an executable as a `.png` is what trusting the claim allows.",
    },
}

/// Serves `media.describe`.
pub struct MediaDescribeHandler;

impl CapabilityHandler<MediaDescribe> for MediaDescribeHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: MediaDescribeIn,
    ) -> Result<MediaDescribeOut, CapabilityError> {
        let bytes = crate::capabilities::b64_decode(&input.data)
            .ok_or_else(|| CapabilityError::invalid_input("the blob is not valid base64"))?;
        let meta = omt_media::describe(&bytes)
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(MediaDescribeOut {
            kind: format!("{:?}", meta.kind).to_lowercase(),
            media_type: meta.media_type.clone(),
            filename: omt_media::safe_filename(&meta),
            digest: meta.digest.clone(),
            bytes: meta.size as u64,
        })
    }
}

// ---------------------------------------------------------------------------
// Importing settings from what somebody already uses
// ---------------------------------------------------------------------------

/// Input to `config.import`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConfigImportIn {
    /// Which tool's settings.
    pub source: String,
    /// The settings themselves, as JSON.
    pub settings: serde_json::Value,
}

/// What `config.import` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ConfigImportOut {
    /// What omt understood, as dotted keys.
    pub values: serde_json::Value,
    /// What it did not, by name, with why.
    ///
    /// Reported rather than dropped: an import that silently ignores half a
    /// config leaves somebody wondering why their font is wrong, and looking in
    /// the wrong place for the answer.
    pub unmapped: Vec<UnmappedSetting>,
    /// Where this tool usually keeps its settings, so a client can offer to
    /// read one rather than asking somebody to find it.
    pub usual_paths: Vec<String>,
}

/// One setting omt could not map.
#[derive(Serialize, schemars::JsonSchema)]
pub struct UnmappedSetting {
    /// Its key in the source.
    pub key: String,
    /// Why it was not imported.
    pub why: String,
}

omt_catalog::capability! {
    /// Read settings out of another tool.
    pub struct ConfigImport;
    input  = ConfigImportIn,
    output = ConfigImportOut,
    decl = omt_catalog::Decl {
        name: "config.import",
        group: "config",
        verb: "import",
        title: "Import settings",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Translate another terminal's settings into omt's, reporting what did not map rather than dropping it. A query: it says what the import would be, and writing it is a separate decision somebody makes.",
    },
}

/// Serves `config.import`.
pub struct ConfigImportHandler;

impl CapabilityHandler<ConfigImport> for ConfigImportHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ConfigImportIn,
    ) -> Result<ConfigImportOut, CapabilityError> {
        let source = omt_config::Source::from_wire(&input.source).ok_or_else(|| {
            CapabilityError::invalid_input(format!(
                "`{}` is not a settings source omt can read",
                input.source
            ))
        })?;
        let imported = omt_config::import(source, &input.settings);
        Ok(ConfigImportOut {
            values: serde_json::to_value(&imported.settings).unwrap_or(serde_json::Value::Null),
            unmapped: imported
                .unmapped
                .into_iter()
                .map(|u| UnmappedSetting {
                    key: u.key,
                    why: u.reason.to_owned(),
                })
                .collect(),
            usual_paths: omt_config::usual_paths(source)
                .into_iter()
                .map(str::to_owned)
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Inviting another device
// ---------------------------------------------------------------------------

/// Input to `invite.create`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct InviteCreateIn {}

/// What `invite.create` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InviteCreateOut {
    /// The invite, shown once. Exchanged for a credential by whoever scans it.
    pub invite: String,
    /// How many seconds it remains exchangeable.
    pub expires_in_secs: u64,
}

omt_catalog::capability! {
    /// Mint an invite for another device.
    pub struct InviteCreate;
    input  = InviteCreateIn,
    output = InviteCreateOut,
    decl = omt_catalog::Decl {
        name: "invite.create",
        group: "invite",
        verb: "create",
        title: "Invite another device",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::empty(),
        intent: Some(omt_catalog::Intent::Append {
            dedup: omt_catalog::DedupKey::IntentId,
            ttl_secs: 60,
        }),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A fresh invite, so a second device can be paired from a device already paired. Without it the only invite is the one printed at startup, and adding a phone means restarting the instance everybody else is connected to.",
    },
}

/// Serves `invite.create`.
pub struct InviteCreateHandler(pub std::sync::Arc<std::sync::Mutex<omt_auth::CredentialStore>>);

impl CapabilityHandler<InviteCreate> for InviteCreateHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: InviteCreateIn,
    ) -> Result<InviteCreateOut, CapabilityError> {
        let mut store = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the credential lock was poisoned"))?;
        // Housekeeping first, so a long-running instance does not accumulate
        // invites nobody can use.
        store.prune_invites(omt_types::Timestamp::now());
        let minted = store.mint_invite(omt_types::Role::Operator, omt_types::Timestamp::now());
        Ok(InviteCreateOut {
            invite: minted.token,
            expires_in_secs: omt_auth::INVITE_TTL_SECS,
        })
    }
}

// ---------------------------------------------------------------------------
// Presence, the audit log, and how the process is bearing up
// ---------------------------------------------------------------------------

/// Input to `presence.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PresenceIn {}

/// What `presence.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PresenceOut {
    /// Everybody connected right now.
    pub clients: Vec<omt_server::Connected>,
}

omt_catalog::capability! {
    /// Who else is connected.
    pub struct PresenceList;
    input  = PresenceIn,
    output = PresenceOut,
    decl = omt_catalog::Decl {
        name: "presence.list",
        group: "presence",
        verb: "list",
        title: "Who is connected",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Every client connected right now. \"Is somebody else looking at this\" is a question people ask before they type, and a terminal that cannot answer it is one where two people interleave keystrokes into one shell.",
    },
}

/// Serves `presence.list`.
pub struct PresenceListHandler(pub omt_server::watch::Shared);

impl CapabilityHandler<PresenceList> for PresenceListHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: PresenceIn,
    ) -> Result<PresenceOut, CapabilityError> {
        let watch = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the watch lock was poisoned"))?;
        Ok(PresenceOut {
            clients: watch.present(),
        })
    }
}

/// Input to `audit.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AuditIn {
    /// Most entries to return, newest last.
    #[serde(default = "default_audit")]
    pub limit: u32,
}

const fn default_audit() -> u32 {
    200
}

/// What `audit.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AuditOut {
    /// Oldest first, so a reader follows what happened in order.
    pub entries: Vec<omt_server::Audited>,
    /// How many fell off the front of a bounded log, so a partial one is never
    /// presented as whole.
    pub dropped: u64,
}

omt_catalog::capability! {
    /// What has been done here.
    pub struct AuditList;
    input  = AuditIn,
    output = AuditOut,
    decl = omt_catalog::Decl {
        name: "audit.list",
        group: "audit",
        verb: "list",
        title: "What has been done",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Every call, with who made it and whether it worked. Refusals included: a log of only what succeeded answers the wrong question, because who *tried* to do what is most of what an audit is for.",
    },
}

/// Serves `audit.list`.
pub struct AuditListHandler(pub omt_server::watch::Shared);

impl CapabilityHandler<AuditList> for AuditListHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: AuditIn,
    ) -> Result<AuditOut, CapabilityError> {
        let watch = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the watch lock was poisoned"))?;
        let all = watch.audited();
        let from = all.len().saturating_sub(input.limit as usize);
        Ok(AuditOut {
            entries: all[from..].to_vec(),
            dropped: watch.dropped(),
        })
    }
}

/// Input to `instance.metrics`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct MetricsIn {}

/// What `instance.metrics` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct MetricsOut {
    /// How many clients are connected.
    pub clients: usize,
    /// How many sessions exist.
    pub sessions: usize,
    /// How many capability calls have been made.
    pub calls: u64,
    /// How many of those were refused.
    pub failures: u64,
    /// When the instance started.
    pub started_at: Option<String>,
}

omt_catalog::capability! {
    /// How this instance is bearing up.
    pub struct InstanceMetrics;
    input  = MetricsIn,
    output = MetricsOut,
    decl = omt_catalog::Decl {
        name: "instance.metrics",
        group: "instance",
        verb: "metrics",
        title: "How the instance is doing",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Clients, sessions, calls and refusals. The health route says the process is up and says nothing else, which is not the same as knowing it is doing anything.",
    },
}

/// Serves `instance.metrics`.
pub struct InstanceMetricsHandler(pub crate::state::State, pub omt_server::watch::Shared);

impl CapabilityHandler<InstanceMetrics> for InstanceMetricsHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: MetricsIn,
    ) -> Result<MetricsOut, CapabilityError> {
        let watch = self
            .1
            .lock()
            .map_err(|_| CapabilityError::internal("the watch lock was poisoned"))?;
        let (calls, failures) = watch.counts();
        Ok(MetricsOut {
            clients: watch.present().len(),
            sessions: self.0.lock()?.sessions().len(),
            calls,
            failures,
            started_at: watch.started_at().map(|t| t.to_string()),
        })
    }
}

// ---------------------------------------------------------------------------
// Who owns what
// ---------------------------------------------------------------------------

/// Input to `identity.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct IdentityListIn {}

/// One person, and the devices they hold.
#[derive(Serialize, schemars::JsonSchema)]
pub struct IdentitySummary {
    /// Their id.
    pub id: String,
    /// What they are called.
    pub name: String,
    /// When they were added.
    pub created_at: String,
    /// Their devices.
    pub devices: Vec<IdentityDevice>,
}

/// One device, under the person who holds it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct IdentityDevice {
    /// Its id.
    pub id: String,
    /// What it is called.
    pub name: String,
    /// Whether a key it holds backs the credential.
    ///
    /// False for a browser, and recorded as false rather than papered over: a
    /// key in local storage is exfiltrated with the token it would protect, so
    /// binding there would be a claim omt cannot keep.
    pub bound: bool,
    /// When it registered.
    pub registered_at: String,
    /// When it was last seen.
    pub last_seen: Option<String>,
    /// Whether it has been revoked.
    pub revoked: bool,
}

/// What `identity.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct IdentityListOut {
    /// Everybody this instance knows, with their devices.
    pub identities: Vec<IdentitySummary>,
}

omt_catalog::capability! {
    /// Who this instance knows, and what they hold.
    pub struct IdentityList;
    input  = IdentityListIn,
    output = IdentityListOut,
    decl = omt_catalog::Decl {
        name: "identity.list",
        group: "identity",
        verb: "list",
        title: "Who holds what",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Every person this instance knows and the devices each holds. A credential answers whether a token may do something; this answers whose phone it is, which is what makes revoking somebody's access — rather than one token — expressible.",
    },
}

/// Serves `identity.list`.
pub struct IdentityListHandler(pub std::sync::Arc<std::sync::Mutex<omt_identity::Directory>>);

impl CapabilityHandler<IdentityList> for IdentityListHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: IdentityListIn,
    ) -> Result<IdentityListOut, CapabilityError> {
        let directory = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the identity lock was poisoned"))?;
        Ok(IdentityListOut {
            identities: directory
                .identities()
                .into_iter()
                .map(|identity| IdentitySummary {
                    id: identity.id.to_wire(),
                    name: identity.name.clone(),
                    created_at: identity.created_at.to_string(),
                    devices: directory
                        .devices_of(identity.id)
                        .into_iter()
                        .map(|d| IdentityDevice {
                            id: d.id.to_wire(),
                            name: d.name.clone(),
                            bound: !d.public_key.starts_with("unbound:"),
                            registered_at: d.registered_at.to_string(),
                            last_seen: d.last_seen.map(|t| t.to_string()),
                            revoked: d.revoked,
                        })
                        .collect(),
                })
                .collect(),
        })
    }
}

/// Input to `identity.revoke`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct IdentityRevokeIn {
    /// Whose access to take away.
    pub identity: String,
}

/// What `identity.revoke` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct IdentityRevokeOut {
    /// How many devices this took away.
    pub devices: usize,
}

omt_catalog::capability! {
    /// Take away everything one person holds.
    pub struct IdentityRevoke;
    input  = IdentityRevokeIn,
    output = IdentityRevokeOut,
    decl = omt_catalog::Decl {
        name: "identity.revoke",
        group: "identity",
        verb: "revoke",
        title: "Revoke a person's access",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::DESTRUCTIVE,
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Revoke every device one person holds, in one action. Doing it device by device means somebody who has left keeps whichever one was missed, and there is no moment at which their access is known to be gone.",
    },
}

/// Serves `identity.revoke`.
pub struct IdentityRevokeHandler(
    pub std::sync::Arc<std::sync::Mutex<omt_identity::Directory>>,
    pub std::sync::Arc<std::sync::Mutex<omt_auth::CredentialStore>>,
);

impl CapabilityHandler<IdentityRevoke> for IdentityRevokeHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: IdentityRevokeIn,
    ) -> Result<IdentityRevokeOut, CapabilityError> {
        let id = omt_types::IdentityId::from_wire(&input.identity).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not an identity", input.identity))
        })?;
        let mut directory = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the identity lock was poisoned"))?;
        if directory.identity(id).is_none() {
            return Err(CapabilityError::not_found(format!(
                "no identity {}",
                input.identity
            )));
        }

        // The credential and the device record are two halves of the same fact.
        // Revoking one and not the other leaves either a token that works with
        // no device behind it, or a device listed as gone that can still call.
        let devices: Vec<(omt_types::DeviceId, String)> = directory
            .devices_of(id)
            .into_iter()
            .filter(|d| !d.revoked)
            .map(|d| (d.id, d.public_key.clone()))
            .collect();

        let mut credentials = self
            .1
            .lock()
            .map_err(|_| CapabilityError::internal("the credential lock was poisoned"))?;
        for (device, key) in &devices {
            directory.revoke_device(*device);
            // An unbound device records the credential it was issued alongside,
            // which is how the two are put back together.
            if let Some(credential) = key.strip_prefix("unbound:") {
                credentials.revoke(credential);
            } else if let Some(found) = credentials
                .list()
                .iter()
                .find(|c| c.device_key.as_deref() == Some(key.as_str()))
                .map(|c| c.id.clone())
            {
                credentials.revoke(&found);
            }
        }

        Ok(IdentityRevokeOut {
            devices: devices.len(),
        })
    }
}

// ---------------------------------------------------------------------------
// Renaming and mass revocation — 23 §12
// ---------------------------------------------------------------------------

/// Input to `device.rename`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeviceRenameIn {
    /// Which credential.
    pub device: String,
    /// What to call it.
    pub label: String,
}

/// What `device.rename` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DeviceRenameOut {
    /// What it is called now.
    pub label: String,
}

omt_catalog::capability! {
    /// Rename a paired device.
    pub struct DeviceRename;
    input  = DeviceRenameIn,
    output = DeviceRenameOut,
    decl = omt_catalog::Decl {
        name: "device.rename",
        group: "device",
        verb: "rename",
        title: "Rename a device",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::WRITES_FS,
        // Cas: renaming to the same thing twice is the same rename, so a retry
        // after a lost acknowledgement lands on the same answer.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The label is the only thing distinguishing two devices in a list, and a list of three `iPhone`s is one nobody can revoke from — so renaming is part of revocation working.",
    },
}

/// Serves renaming.
pub struct DeviceRenameHandler(pub std::sync::Arc<std::sync::Mutex<omt_auth::CredentialStore>>);

impl CapabilityHandler<DeviceRename> for DeviceRenameHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: DeviceRenameIn,
    ) -> Result<DeviceRenameOut, CapabilityError> {
        let mut store = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the credential lock was poisoned"))?;
        if !store.rename(&input.device, &input.label) {
            return Err(CapabilityError::invalid_input(format!(
                "no device `{}`, or the name was empty",
                input.device
            )));
        }
        Ok(DeviceRenameOut {
            label: input.label.trim().to_owned(),
        })
    }
}

/// Input to `device.revoke_all`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RevokeAllIn {
    /// The device to keep, which is the one asking.
    ///
    /// Required rather than optional. Revoking everything including the device
    /// in your hand locks you out of the machine you were securing, and the
    /// recovery for that is physical access — so there is no spelling of this
    /// call that does it by accident.
    pub keep: String,
}

/// What `device.revoke_all` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RevokeAllOut {
    /// How many were revoked.
    pub revoked: usize,
    /// Which one was kept.
    pub kept: String,
}

omt_catalog::capability! {
    /// Revoke every device except the one asking.
    pub struct DeviceRevokeAll;
    input  = RevokeAllIn,
    output = RevokeAllOut,
    decl = omt_catalog::Decl {
        name: "device.revoke_all",
        group: "device",
        verb: "revoke_all",
        title: "Revoke every other device",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::WRITES_FS.union(omt_catalog::Effects::DESTRUCTIVE),
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The `I lost a device and do not know which` button. Keeps the one asking, because revoking that too locks somebody out of the machine they were securing.",
    },
}

impl CapabilityHandler<DeviceRevokeAll> for DeviceRenameHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RevokeAllIn,
    ) -> Result<RevokeAllOut, CapabilityError> {
        let mut store = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the credential lock was poisoned"))?;
        if !store.list().iter().any(|c| c.id == input.keep) {
            // Refused rather than treated as "keep nothing". A typo in the id
            // would otherwise revoke every device including the caller's, which
            // is the one outcome this call is shaped to prevent.
            return Err(CapabilityError::invalid_input(format!(
                "no device `{}` to keep; refusing rather than revoking everything",
                input.keep
            )));
        }
        Ok(RevokeAllOut {
            revoked: store.revoke_all_except(&input.keep),
            kept: input.keep,
        })
    }
}
