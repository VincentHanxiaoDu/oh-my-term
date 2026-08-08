//! Plugins: what they declare, and what the host will let them do.
//!
//! A plugin is third-party code running beside the user's terminal, so
//! permissions are **declared up front, shown before install, and enforced at
//! call time**. Every one of those three is load-bearing: a permission granted
//! silently is not a grant, and one that is checked only at install is not an
//! enforcement.

pub mod api;

pub use api::{
    CallError, MAX_WRITE_BYTES, NotifyLevel, PluginCall, authorize, is_workspace_relative,
};

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Something a plugin may ask to do.
///
/// Coarse on purpose. A permission a user cannot picture is one they click
/// through, and a list of forty is worse than a list of six.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// See terminal output and session state.
    ReadSessions,
    /// Write to a session's input channel.
    ///
    /// The dangerous one, and the only one that can make something happen the
    /// user did not do.
    WriteInput,
    /// Read files inside a workspace.
    ReadWorkspace,
    /// Write files inside a workspace.
    WriteWorkspace,
    /// Reach the network.
    Network,
    /// Run a process.
    SpawnProcess,
}

impl Permission {
    /// Every permission, so a UI can enumerate them without a second list.
    pub const ALL: &'static [Self] = &[
        Self::ReadSessions,
        Self::WriteInput,
        Self::ReadWorkspace,
        Self::WriteWorkspace,
        Self::Network,
        Self::SpawnProcess,
    ];

    /// Whether granting this deserves a deliberate confirmation.
    ///
    /// These are the ones that can make something happen the user did not do,
    /// or move their data off the machine.
    #[must_use]
    pub const fn is_high_consequence(self) -> bool {
        matches!(
            self,
            Self::WriteInput | Self::WriteWorkspace | Self::Network | Self::SpawnProcess
        )
    }

    /// What to show a person deciding.
    ///
    /// Phrased as what it lets the plugin *do*, not as the name of a flag: a
    /// user granting `WriteInput` needs to read "type into your terminals".
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::ReadSessions => "see what is on your terminals",
            Self::WriteInput => "type into your terminals",
            Self::ReadWorkspace => "read files in your projects",
            Self::WriteWorkspace => "change files in your projects",
            Self::Network => "send data over the network",
            Self::SpawnProcess => "run programs on this machine",
        }
    }
}

/// What a plugin declares about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Its stable id.
    pub id: String,
    /// Its display name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// What it needs.
    pub permissions: BTreeSet<Permission>,
    /// What it says it does.
    pub description: String,
    /// The program omt runs, with its arguments.
    ///
    /// Empty means a plugin that declares behaviour but has nothing to run —
    /// a theme, a keymap. Those are real plugins and must not be forced to
    /// invent an executable, which is why this is a list rather than a
    /// required string.
    #[serde(default)]
    pub entry: Vec<String>,
}

/// How a plugin should be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment omt sets, including where to reach it.
    pub env: Vec<(String, String)>,
}

impl Installed {
    /// How to start this plugin, or why it cannot be started.
    ///
    /// The token omt hands over is the plugin's whole authority: it carries a
    /// role, and the role is derived from what the plugin was *granted* rather
    /// than from what it asked for. A plugin that requested `WriteInput` and
    /// was not granted it must not receive a token that can type.
    ///
    /// # Errors
    /// Fails if the plugin is disabled or declares nothing to run.
    pub fn launch(&self, socket: &str, token: &str) -> Result<Launch, LaunchError> {
        if !self.enabled {
            return Err(LaunchError::Disabled {
                plugin: self.manifest.id.clone(),
            });
        }
        let (program, args) =
            self.manifest
                .entry
                .split_first()
                .ok_or_else(|| LaunchError::NothingToRun {
                    plugin: self.manifest.id.clone(),
                })?;
        Ok(Launch {
            program: program.clone(),
            args: args.to_vec(),
            env: vec![
                ("OMT_SOCK".to_owned(), socket.to_owned()),
                ("OMT_TOKEN".to_owned(), token.to_owned()),
                ("OMT_PLUGIN".to_owned(), self.manifest.id.clone()),
            ],
        })
    }

    /// The role a token for this plugin should carry.
    ///
    /// Derived from the grants, not the request. Anything that can change the
    /// world needs operator; everything else is a viewer, and a plugin that
    /// only reads should not hold a credential that could write.
    #[must_use]
    pub fn role(&self) -> omt_types::Role {
        let writes = self.granted.iter().any(|p| {
            matches!(
                p,
                Permission::WriteInput | Permission::WriteWorkspace | Permission::SpawnProcess
            )
        });
        if writes {
            omt_types::Role::Operator
        } else {
            omt_types::Role::Viewer
        }
    }
}

/// Why a plugin could not be started.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchError {
    /// It is switched off.
    #[error("`{plugin}` is disabled")]
    Disabled {
        /// Which plugin.
        plugin: String,
    },
    /// It declares no entry point.
    #[error("`{plugin}` declares nothing to run")]
    NothingToRun {
        /// Which plugin.
        plugin: String,
    },
}

/// Why a manifest was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// The id is not usable as a namespace.
    #[error("`{id}` is not a valid plugin id: {reason}")]
    BadId {
        /// What was given.
        id: String,
        /// Why not.
        reason: &'static str,
    },
    /// It declared nothing it needs.
    #[error("a plugin that declares no permissions cannot do anything")]
    NoPermissions,
}

/// Why a call was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{plugin}` did not declare permission to {}", .permission.describe())]
pub struct Denied {
    /// Which plugin.
    pub plugin: String,
    /// What it tried to do.
    pub permission: Permission,
}

impl Manifest {
    /// Check a manifest.
    ///
    /// # Errors
    /// Fails if the id cannot serve as a config namespace, or if the plugin
    /// declares nothing — which is either a mistake or an attempt to be granted
    /// things later without having been shown.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.id.is_empty() {
            return Err(ManifestError::BadId {
                id: self.id.clone(),
                reason: "it is empty",
            });
        }
        // The id becomes a config namespace and a directory name, so anything
        // that could escape either is refused here rather than surprising
        // somebody later.
        if !self
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ManifestError::BadId {
                id: self.id.clone(),
                reason: "it must be lowercase letters, digits and hyphens",
            });
        }
        if self.permissions.is_empty() {
            return Err(ManifestError::NoPermissions);
        }
        Ok(())
    }

    /// The permissions worth stopping a user over.
    #[must_use]
    pub fn high_consequence(&self) -> Vec<Permission> {
        self.permissions
            .iter()
            .copied()
            .filter(|p| p.is_high_consequence())
            .collect()
    }
}

/// An installed plugin, and what it was actually granted.
#[derive(Debug, Clone)]
pub struct Installed {
    /// What it declared.
    pub manifest: Manifest,
    /// What the user agreed to.
    ///
    /// Stored separately from the manifest, and it is the *granted* set that is
    /// enforced. An update that quietly widened its declaration would otherwise
    /// widen what it may do without anyone agreeing to it.
    pub granted: BTreeSet<Permission>,
    /// Whether it is currently on.
    pub enabled: bool,
}

impl Installed {
    /// Install a plugin with exactly the permissions the user granted.
    #[must_use]
    pub fn new(manifest: Manifest, granted: BTreeSet<Permission>) -> Self {
        // The intersection, not the union: a plugin cannot be granted something
        // it never declared, however the grant was produced.
        let declared = manifest.permissions.clone();
        Self {
            manifest,
            granted: granted.intersection(&declared).copied().collect(),
            enabled: true,
        }
    }

    /// Check one call.
    ///
    /// # Errors
    /// Fails if the plugin is disabled or was not granted the permission.
    pub fn check(&self, permission: Permission) -> Result<(), Denied> {
        if !self.enabled || !self.granted.contains(&permission) {
            return Err(Denied {
                plugin: self.manifest.id.clone(),
                permission,
            });
        }
        Ok(())
    }

    /// What an update would newly ask for.
    ///
    /// Non-empty means the user has to be asked again. An update that silently
    /// gained `WriteInput` is the supply-chain shape this exists to catch.
    #[must_use]
    pub fn newly_requested(&self, update: &Manifest) -> Vec<Permission> {
        update
            .permissions
            .difference(&self.granted)
            .copied()
            .collect()
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {

    #[test]
    fn a_plugin_is_handed_the_socket_and_a_token_and_its_own_id() {
        // Its whole authority. Without the id it cannot namespace what it
        // registers; without the token it cannot call anything at all.
        let p = Installed::new(
            Manifest {
                id: "demo".into(),
                name: "Demo".into(),
                version: "1.0".into(),
                permissions: [Permission::ReadSessions].into_iter().collect(),
                description: "d".into(),
                entry: vec!["/usr/bin/demo".into(), "--serve".into()],
            },
            [Permission::ReadSessions].into_iter().collect(),
        );
        let launch = p.launch("/tmp/omt.sock", "tok").expect("launch");
        assert_eq!(launch.program, "/usr/bin/demo");
        assert_eq!(launch.args, vec!["--serve".to_owned()]);
        assert!(
            launch
                .env
                .iter()
                .any(|(k, v)| k == "OMT_SOCK" && v == "/tmp/omt.sock")
        );
        assert!(
            launch
                .env
                .iter()
                .any(|(k, v)| k == "OMT_TOKEN" && v == "tok")
        );
        assert!(
            launch
                .env
                .iter()
                .any(|(k, v)| k == "OMT_PLUGIN" && v == "demo")
        );
    }

    #[test]
    fn a_disabled_plugin_is_not_started() {
        let mut p = Installed::new(
            Manifest {
                id: "demo".into(),
                name: "Demo".into(),
                version: "1.0".into(),
                permissions: [Permission::ReadSessions].into_iter().collect(),
                description: "d".into(),
                entry: vec!["/usr/bin/demo".into()],
            },
            [Permission::ReadSessions].into_iter().collect(),
        );
        p.enabled = false;
        assert!(matches!(
            p.launch("s", "t"),
            Err(LaunchError::Disabled { .. })
        ));
    }

    #[test]
    fn a_plugin_with_nothing_to_run_says_so_rather_than_failing_obscurely() {
        // A theme or a keymap is a real plugin with no executable, and it must
        // not be forced to invent one.
        let p = Installed::new(
            Manifest {
                id: "theme".into(),
                name: "Theme".into(),
                version: "1.0".into(),
                permissions: [Permission::ReadSessions].into_iter().collect(),
                description: "d".into(),
                entry: Vec::new(),
            },
            [Permission::ReadSessions].into_iter().collect(),
        );
        assert!(matches!(
            p.launch("s", "t"),
            Err(LaunchError::NothingToRun { .. })
        ));
    }

    #[test]
    fn the_token_role_comes_from_what_was_granted_not_what_was_asked_for() {
        // The bug this prevents is the one that matters: a plugin that asked
        // for WriteInput and was refused must not be handed a token that can
        // type into somebody's shell.
        let p = Installed::new(
            Manifest {
                id: "greedy".into(),
                name: "Greedy".into(),
                version: "1.0".into(),
                permissions: [Permission::WriteInput].into_iter().collect(),
                description: "d".into(),
                entry: vec!["/bin/true".into()],
            },
            [Permission::ReadSessions].into_iter().collect(),
        );
        assert_eq!(p.role(), omt_types::Role::Viewer);
    }

    #[test]
    fn a_plugin_granted_a_writing_permission_gets_an_operator_token() {
        let p = Installed::new(
            Manifest {
                id: "typer".into(),
                name: "Typer".into(),
                version: "1.0".into(),
                permissions: [Permission::WriteInput].into_iter().collect(),
                description: "d".into(),
                entry: vec!["/bin/true".into()],
            },
            [Permission::WriteInput].into_iter().collect(),
        );
        assert_eq!(p.role(), omt_types::Role::Operator);
    }
    use super::*;

    fn manifest(id: &str, permissions: &[Permission]) -> Manifest {
        Manifest {
            id: id.to_owned(),
            name: "Test".to_owned(),
            version: "1.0.0".to_owned(),
            permissions: permissions.iter().copied().collect(),
            description: "does a thing".to_owned(),
            entry: Vec::new(),
        }
    }

    #[test]
    fn a_plugin_may_do_only_what_it_was_granted() {
        let m = manifest("notify", &[Permission::ReadSessions, Permission::Network]);
        let p = Installed::new(m, [Permission::ReadSessions].into_iter().collect());
        assert!(p.check(Permission::ReadSessions).is_ok());
        assert!(
            p.check(Permission::Network).is_err(),
            "declared is not granted"
        );
    }

    #[test]
    fn a_grant_cannot_exceed_what_was_declared() {
        // However the grant was produced — a hand-edited config, a bug — a
        // plugin must not end up able to do something it never asked for and
        // the user was never shown.
        let m = manifest("notify", &[Permission::ReadSessions]);
        let p = Installed::new(
            m,
            [Permission::ReadSessions, Permission::WriteInput]
                .into_iter()
                .collect(),
        );
        assert!(p.check(Permission::WriteInput).is_err());
        assert_eq!(p.granted.len(), 1);
    }

    #[test]
    fn a_disabled_plugin_may_do_nothing() {
        let m = manifest("notify", &[Permission::ReadSessions]);
        let mut p = Installed::new(m, [Permission::ReadSessions].into_iter().collect());
        p.enabled = false;
        assert!(p.check(Permission::ReadSessions).is_err());
    }

    #[test]
    fn a_denial_says_what_was_attempted_in_words() {
        // A user reading a log needs "type into your terminals", not a flag
        // name they have to look up.
        let m = manifest("notify", &[Permission::ReadSessions]);
        let p = Installed::new(m, [Permission::ReadSessions].into_iter().collect());
        let err = p.check(Permission::WriteInput).expect_err("denied");
        assert!(
            err.to_string().contains("type into your terminals"),
            "{err}"
        );
    }

    #[test]
    fn an_update_that_wants_more_is_reported_rather_than_applied() {
        // The supply-chain shape: version 1.1 quietly gains WriteInput.
        let m = manifest("notify", &[Permission::ReadSessions]);
        let p = Installed::new(m, [Permission::ReadSessions].into_iter().collect());
        let update = manifest(
            "notify",
            &[Permission::ReadSessions, Permission::WriteInput],
        );
        assert_eq!(p.newly_requested(&update), [Permission::WriteInput]);
    }

    #[test]
    fn an_update_that_wants_the_same_needs_no_new_consent() {
        let m = manifest("notify", &[Permission::ReadSessions]);
        let p = Installed::new(m.clone(), [Permission::ReadSessions].into_iter().collect());
        assert!(p.newly_requested(&m).is_empty());
    }

    #[test]
    fn an_update_that_wants_less_needs_no_consent_either() {
        let m = manifest("notify", &[Permission::ReadSessions, Permission::Network]);
        let p = Installed::new(
            m,
            [Permission::ReadSessions, Permission::Network]
                .into_iter()
                .collect(),
        );
        let update = manifest("notify", &[Permission::ReadSessions]);
        assert!(p.newly_requested(&update).is_empty());
    }

    #[test]
    fn an_id_that_could_escape_a_directory_is_refused() {
        // The id becomes a config namespace and a directory name.
        for bad in ["../evil", "Has Spaces", "UPPER", "sla/sh", ""] {
            assert!(
                manifest(bad, &[Permission::ReadSessions])
                    .validate()
                    .is_err(),
                "`{bad}` was accepted"
            );
        }
        assert!(
            manifest("notify-ntfy", &[Permission::ReadSessions])
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn a_plugin_declaring_nothing_is_refused() {
        // Either a mistake, or an attempt to be granted things later without
        // ever having been shown.
        assert_eq!(
            manifest("empty", &[]).validate(),
            Err(ManifestError::NoPermissions)
        );
    }

    #[test]
    fn the_high_consequence_permissions_are_the_ones_that_act() {
        assert!(Permission::WriteInput.is_high_consequence());
        assert!(Permission::Network.is_high_consequence());
        assert!(Permission::SpawnProcess.is_high_consequence());
        assert!(!Permission::ReadSessions.is_high_consequence());
    }

    #[test]
    fn a_manifest_can_report_what_deserves_a_second_look() {
        let m = manifest("risky", &[Permission::ReadSessions, Permission::WriteInput]);
        assert_eq!(m.high_consequence(), [Permission::WriteInput]);
    }

    #[test]
    fn every_permission_has_a_description_a_person_can_read() {
        for p in Permission::ALL {
            let d = p.describe();
            assert!(!d.is_empty());
            assert!(
                d.chars().next().is_some_and(char::is_lowercase),
                "{p:?} reads like a flag name rather than a sentence: {d}"
            );
        }
    }
}
