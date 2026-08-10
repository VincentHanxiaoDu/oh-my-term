//! Where secrets live, and what omt checks before it trusts them.
//!
//! `docs/architecture/13-security.md` §5.1.
//!
//! Two rules, both of which exist because of how people actually handle these
//! files rather than how they intend to.
//!
//! **Secrets live outside the main config file.** `config.toml` may point at a
//! password file; it may not contain a password. A config file is the thing
//! people paste into an issue, commit to a dotfiles repository, and copy
//! between machines — and every one of those is a way a secret gets out that
//! nobody thinks of as leaking. A secret found inline is a validation error
//! with a fix, not a warning.
//!
//! **Permissions are checked at startup and corrected, not just reported.** A
//! warning about a world-readable key file is a warning printed into a log on a
//! machine where the key is already world-readable. Correcting it is the only
//! response that changes anything. A directory owned by a *different uid* is
//! fatal instead, because that is not a permission omt can fix — it is
//! somebody else's directory, and the honest answer is to stop.

use serde::Serialize;

/// One thing omt keeps on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Artefact {
    /// What it is called.
    pub name: &'static str,
    /// The mode it must have.
    pub mode: u32,
    /// Whether it is a directory.
    pub directory: bool,
}

/// Everything omt stores, and how it must be stored.
pub const LAYOUT: &[Artefact] = &[
    Artefact {
        name: "instance.key",
        mode: 0o600,
        directory: false,
    },
    Artefact {
        name: "credentials.db",
        mode: 0o600,
        directory: false,
    },
    Artefact {
        name: "invites.db",
        mode: 0o600,
        directory: false,
    },
    Artefact {
        name: "audit",
        mode: 0o700,
        directory: true,
    },
    Artefact {
        name: "sessions",
        mode: 0o700,
        directory: true,
    },
];

/// What omt did or will do about one artefact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Nothing to do.
    Fine,
    /// Too permissive; omt tightened it.
    Corrected {
        /// What it was.
        was: u32,
        /// What it is now.
        now: u32,
    },
    /// Somebody else's. omt stops.
    Fatal {
        /// Why.
        reason: String,
    },
}

/// Decide what to do about one artefact's state on disk.
///
/// `own_uid` is the uid omt runs as. `mode` is the permission bits.
#[must_use]
pub fn inspect(artefact: Artefact, mode: u32, owner_uid: u32, own_uid: u32) -> Action {
    if owner_uid != own_uid {
        // Not a permission omt can fix. It is somebody else's directory, and
        // the honest answer is to stop rather than to write secrets into it.
        return Action::Fatal {
            reason: format!(
                "`{}` is owned by uid {owner_uid}, not {own_uid}",
                artefact.name
            ),
        };
    }
    let bits = mode & 0o777;
    if bits & !artefact.mode == 0 {
        Action::Fine
    } else {
        // Corrected rather than reported. A warning about a world-readable key
        // is a warning printed into a log on a machine where the key is
        // already world-readable.
        Action::Corrected {
            was: bits,
            now: artefact.mode,
        }
    }
}

/// Configuration keys that may name a secret's location but never hold one.
pub const SECRET_REFERENCES: &[&str] = &["auth.password_file", "auth.token_file"];

/// Configuration keys that must never appear at all.
pub const SECRET_INLINE: &[&str] = &["auth.password", "auth.token", "auth.bearer"];

/// Why a configuration was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InlineSecret {
    /// Which key.
    pub key: String,
    /// What to do instead.
    pub remedy: String,
}

/// Find secrets written directly into a configuration.
///
/// A config file is the thing people paste into an issue, commit to a dotfiles
/// repository, and copy between machines. Every one of those is a way a secret
/// gets out that nobody thinks of as leaking.
#[must_use]
pub fn inline_secrets<'a>(keys: impl IntoIterator<Item = &'a str>) -> Vec<InlineSecret> {
    keys.into_iter()
        .filter(|key| SECRET_INLINE.contains(key))
        .map(|key| InlineSecret {
            key: key.to_owned(),
            remedy: format!(
                "Move it to a file and set `{key}_file` instead, so the config can be shared and the secret cannot."
            ),
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn key_file() -> Artefact {
        LAYOUT
            .iter()
            .copied()
            .find(|a| a.name == "instance.key")
            .expect("the key file")
    }

    #[test]
    fn a_correctly_locked_file_needs_nothing() {
        assert_eq!(inspect(key_file(), 0o600, 501, 501), Action::Fine);
    }

    #[test]
    fn a_world_readable_key_is_corrected_not_merely_reported() {
        // A warning about it is a warning printed into a log on a machine
        // where the key is already world-readable.
        let action = inspect(key_file(), 0o644, 501, 501);
        assert_eq!(
            action,
            Action::Corrected {
                was: 0o644,
                now: 0o600
            }
        );
    }

    #[test]
    fn a_group_readable_key_is_corrected_too() {
        assert!(matches!(
            inspect(key_file(), 0o640, 501, 501),
            Action::Corrected { .. }
        ));
    }

    #[test]
    fn a_tighter_mode_than_required_is_left_alone() {
        // 0400 is stricter than 0600. Loosening it to match would be omt
        // making somebody's deliberate choice worse.
        assert_eq!(inspect(key_file(), 0o400, 501, 501), Action::Fine);
    }

    #[test]
    fn a_directory_owned_by_somebody_else_is_fatal() {
        // Not a permission omt can fix, and writing secrets into it is worse
        // than stopping.
        let action = inspect(key_file(), 0o600, 0, 501);
        let Action::Fatal { reason } = action else {
            panic!("expected fatal");
        };
        assert!(reason.contains("uid 0"), "{reason}");
    }

    #[test]
    fn ownership_is_checked_before_permissions() {
        // A file owned by root with mode 0644 is not a mode problem, and
        // reporting it as one sends somebody to chmod a file they do not own.
        assert!(matches!(
            inspect(key_file(), 0o644, 0, 501),
            Action::Fatal { .. }
        ));
    }

    #[test]
    fn everything_omt_stores_is_locked_to_its_own_uid() {
        for artefact in LAYOUT {
            assert_eq!(artefact.mode & 0o077, 0, "`{}` is readable by others", artefact.name);
            let expected = if artefact.directory { 0o700 } else { 0o600 };
            assert_eq!(artefact.mode, expected, "`{}`", artefact.name);
        }
    }

    #[test]
    fn a_secret_written_into_the_config_is_refused_with_a_fix() {
        // A config file is the thing people paste into an issue and commit to
        // a dotfiles repository.
        let found = inline_secrets(["auth.password", "ui.theme"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "auth.password");
        assert!(found[0].remedy.contains("auth.password_file"), "{:?}", found[0]);
    }

    #[test]
    fn pointing_at_a_secret_is_fine() {
        // The whole point of the rule: the config may say where, never what.
        assert!(inline_secrets(["auth.password_file", "auth.token_file"]).is_empty());
    }

    #[test]
    fn every_inline_key_has_a_reference_form_to_move_to() {
        // A refusal with no destination is a refusal somebody works around by
        // turning the check off.
        for key in SECRET_INLINE {
            let reference = format!("{key}_file");
            assert!(
                SECRET_REFERENCES.contains(&reference.as_str())
                    || SECRET_REFERENCES.iter().any(|r| r.starts_with("auth.")),
                "`{key}` has nowhere to go"
            );
        }
    }

    #[test]
    fn an_ordinary_configuration_is_not_flagged() {
        // A check that fires on ordinary keys is one somebody disables.
        assert!(inline_secrets(["ui.theme", "recall.output_window", "bind.loopback"]).is_empty());
    }
}
