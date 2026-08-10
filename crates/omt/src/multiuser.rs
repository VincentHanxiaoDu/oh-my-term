//! Sharing a machine without sharing anything else.
//!
//! `docs/architecture/22-operations.md` §2. The rule is short:
//!
//! > **omt is per-uid, entirely.** Two uids on one machine run two independent
//! > daemons that share no socket, no state directory, no port, no blob store
//! > and no log.
//!
//! There is no system-wide daemon, and `omt service install --system` is
//! refused with a reason. A system daemon serving several users would have to
//! authenticate them and switch uid to run their PTYs — that is a privilege
//! boundary, and 13 §1.3 says omt does not implement one. Building half of one
//! would be worse than not building it.
//!
//! What is here is the three hazards that are not already covered by the socket
//! mode and the peer-credential check in `omt-transport`.
//!
//! **The fallback runtime directory.** Where `$XDG_RUNTIME_DIR` is unset —
//! macOS, some containers — a predictable `/tmp/omt/` would let a hostile local
//! user pre-create it and read every screenshot pasted into an agent prompt, or
//! plant a symlink and have omt write through it. So the path carries the uid,
//! it is created 0700, its owner and mode are checked, and omt refuses to start
//! rather than use a directory it does not own.
//!
//! **`umask`.** A daemon started from a shell with `umask 000` creates 0666
//! files. omt sets `umask(0o077)` itself, before opening anything, rather than
//! inheriting whatever it was given.
//!
//! **Secrets in `argv`.** This is the VS Code connection-token lesson: their
//! remote server passed its token on the command line, where `ps aux` made it
//! readable by every other user on the host. omt passes tokens by file or on
//! stdin, and checks its own command line and environment for anything
//! credential-shaped.

use std::path::{Path, PathBuf};

/// The mode every per-uid directory is created with.
pub const DIR_MODE: u32 = 0o700;

/// The umask omt sets for itself.
pub const UMASK: u32 = 0o077;

/// Why omt will not use a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirRefusal {
    /// It exists and belongs to somebody else.
    ///
    /// The pre-created-directory attack, and the reason this is a refusal to
    /// start rather than a warning.
    NotOurs {
        /// Who owns it.
        owner: u32,
        /// Who we are.
        expected: u32,
    },
    /// It exists and others can reach it.
    TooOpen {
        /// The mode found.
        mode: u32,
    },
    /// It is a symlink.
    ///
    /// Following one would have omt write wherever it points, which is the
    /// second half of the same attack.
    Symlink,
}

impl std::fmt::Display for DirRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotOurs { owner, expected } => write!(
                f,
                "it is owned by uid {owner} and this is uid {expected}; omt will not use a directory it does not own"
            ),
            Self::TooOpen { mode } => write!(
                f,
                "its mode is {mode:04o} and must be {DIR_MODE:04o}; another user on this machine can read it"
            ),
            Self::Symlink => write!(f, "it is a symlink, and following it would write elsewhere"),
        }
    }
}

/// Where per-uid runtime state goes.
///
/// The uid is in the fallback path, not only the parent, so two users on a
/// machine without `$XDG_RUNTIME_DIR` cannot land on the same directory.
#[must_use]
pub fn runtime_root(xdg_runtime_dir: Option<&Path>, tmp: &Path, uid: u32) -> PathBuf {
    xdg_runtime_dir.map_or_else(|| tmp.join(format!("omt-{uid}")), |dir| dir.join("omt"))
}

/// What to do with a directory that already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirVerdict {
    /// Use it as it is.
    Use,
    /// Use it, after narrowing its mode to `0700`, and log that.
    ///
    /// A mode omt owns and can fix is not a reason to refuse to start —
    /// 22 §2.2 says modes are checked *and corrected*. The logging is the point:
    /// a mode that a careless `chmod` widened becomes an event rather than a
    /// silent exposure.
    Correct,
}

/// Whether a directory that already exists may be used.
///
/// # Errors
/// Refused if it is a symlink or owned by another uid — neither is something
/// omt can fix, and using it anyway is the attack.
pub fn check_dir(
    is_symlink: bool,
    owner: u32,
    mode: u32,
    uid: u32,
) -> Result<DirVerdict, DirRefusal> {
    if is_symlink {
        return Err(DirRefusal::Symlink);
    }
    if owner != uid {
        return Err(DirRefusal::NotOurs {
            owner,
            expected: uid,
        });
    }
    if mode & 0o077 == 0 {
        Ok(DirVerdict::Use)
    } else {
        Ok(DirVerdict::Correct)
    }
}

/// Things that look like a credential when they appear in `argv`.
const CREDENTIAL_SHAPED: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "api_key",
    "api-key",
    "bearer",
    "credential",
    "private_key",
    "private-key",
];

/// Anything credential-shaped in a command line or environment.
///
/// Reads the daemon's own `argv` and `environ`. The check exists because
/// `ps aux` is readable by every user on the host, so a secret there is a
/// secret shared with all of them — the bug this would have caught in somebody
/// else's remote server.
///
/// Matches on the *name*, and reports the name without its value: a check that
/// printed what it found would put the secret in the log it was protecting it
/// from.
#[must_use]
pub fn credential_shaped(entries: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for entry in entries {
        let name = entry.split(['=', ' ']).next().unwrap_or(entry);
        let lowered = name.to_lowercase();
        if CREDENTIAL_SHAPED.iter().any(|c| lowered.contains(c)) {
            found.push(name.to_owned());
        }
    }
    found
}

/// Why a system-level service is refused.
///
/// Stated rather than silently ignored, because somebody asking for it has a
/// real problem — "be there when I ssh in" — and the answer is a user service,
/// which they should be told about in the same breath.
#[must_use]
pub const fn system_service_refusal() -> &'static str {
    "omt installs user services only. A system daemon serving several users would have to authenticate them and switch uid to run their PTYs, which is a privilege boundary omt does not implement; half of one would be worse than none. Run `omt service install` without --system: it starts at login and survives logout."
}

/// Everything that must never be shared between uids.
///
/// A list, because it is the review checklist. A future feature that needs
/// cross-uid anything needs a privilege boundary design first, and that is a
/// decision record rather than a patch.
pub const NEVER_SHARED: &[&str] = &[
    "control socket",
    "state directory",
    "blob store",
    "ssh ControlPath",
    "log",
    "credentials",
    "instance key",
    "TCP listener",
];

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_path_carries_the_uid() {
        // A predictable /tmp/omt/ lets a hostile local user pre-create it and
        // read every screenshot pasted into an agent prompt.
        let a = runtime_root(None, Path::new("/tmp"), 501);
        let b = runtime_root(None, Path::new("/tmp"), 502);
        assert_ne!(a, b);
        assert!(a.display().to_string().contains("501"));
    }

    #[test]
    fn the_runtime_dir_is_used_when_it_is_set() {
        let root = runtime_root(Some(Path::new("/run/user/501")), Path::new("/tmp"), 501);
        assert_eq!(root, Path::new("/run/user/501/omt"));
    }

    #[test]
    fn a_directory_owned_by_someone_else_is_refused() {
        // The pre-created-directory attack, and why this refuses to start
        // rather than warning.
        let refusal = check_dir(false, 502, 0o700, 501).expect_err("refused");
        assert!(matches!(refusal, DirRefusal::NotOurs { .. }));
    }

    #[test]
    fn a_world_readable_directory_of_ours_is_corrected_rather_than_fatal() {
        // Refusing to start over a mode omt can fix turns a careless `chmod`
        // into an outage. Correcting it and saying so is what 22 §2.2 asks for,
        // and the saying-so is what makes it an event rather than a silent
        // exposure.
        assert_eq!(
            check_dir(false, 501, 0o755, 501).expect("ours"),
            DirVerdict::Correct
        );
    }

    #[test]
    fn a_symlink_is_refused_before_anything_is_written_through_it() {
        assert_eq!(
            check_dir(true, 501, 0o700, 501).expect_err("refused"),
            DirRefusal::Symlink
        );
    }

    #[test]
    fn our_own_private_directory_is_accepted_untouched() {
        assert_eq!(
            check_dir(false, 501, DIR_MODE, 501).expect("ours"),
            DirVerdict::Use
        );
    }

    #[test]
    fn every_refusal_says_what_is_wrong_in_words() {
        // "omt refused to start" with no reason is a support ticket.
        for refusal in [
            DirRefusal::NotOurs {
                owner: 502,
                expected: 501,
            },
            DirRefusal::TooOpen { mode: 0o755 },
            DirRefusal::Symlink,
        ] {
            assert!(refusal.to_string().len() > 30, "{refusal:?}");
        }
    }

    #[test]
    fn a_token_on_the_command_line_is_caught() {
        // The lesson from somebody else's remote server: `ps aux` is readable
        // by every user on the host.
        let found = credential_shaped(&[
            "omt".to_owned(),
            "--connection-token=abc123".to_owned(),
            "--foreground".to_owned(),
        ]);
        assert_eq!(found, vec!["--connection-token"]);
    }

    #[test]
    fn what_it_found_is_named_but_never_printed() {
        // A check that printed the value would put the secret into the log it
        // was protecting it from.
        let found = credential_shaped(&["OMT_TOKEN=hunter2".to_owned()]);
        assert_eq!(found, vec!["OMT_TOKEN"]);
        assert!(!found.iter().any(|f| f.contains("hunter2")));
    }

    #[test]
    fn an_ordinary_command_line_is_clean() {
        // A check that fires on everything is a check people switch off.
        let found = credential_shaped(&[
            "omt".to_owned(),
            "daemon".to_owned(),
            "--foreground".to_owned(),
            "PATH=/usr/bin".to_owned(),
            "HOME=/home/ada".to_owned(),
        ]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn the_umask_omt_sets_denies_group_and_other() {
        // A daemon started from a shell with umask 000 would create 0666 files.
        assert_eq!(0o666 & !UMASK, 0o600);
        assert_eq!(0o777 & !UMASK, 0o700);
    }

    #[test]
    fn a_system_service_is_refused_with_a_reason_and_an_alternative() {
        // Somebody asking for it has a real problem, and the answer is a user
        // service.
        let refusal = system_service_refusal();
        assert!(refusal.contains("privilege boundary"), "{refusal}");
        assert!(refusal.contains("omt service install"), "{refusal}");
    }

    #[test]
    fn the_never_shared_list_covers_every_resource_in_the_table() {
        for resource in ["control socket", "blob store", "TCP listener", "log"] {
            assert!(NEVER_SHARED.contains(&resource), "{resource}");
        }
    }
}
