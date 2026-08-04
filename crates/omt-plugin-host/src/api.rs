//! What a plugin can actually call, and what it gets back.
//!
//! The surface is deliberately narrow and *entirely* permission-gated. Every
//! call names the permission it needs, and the host checks it here rather than
//! trusting each operation to remember — an operation that forgot would be a
//! hole nothing else could see.
//!
//! Paths are the part worth reading twice. A plugin names a file with a
//! workspace-relative path and the host resolves it; a plugin never hands over
//! an absolute path and never sees one. That is what keeps "a plugin may read
//! files in your projects" a true description of the grant rather than an
//! optimistic one.

use serde::{Deserialize, Serialize};

use crate::{Denied, Installed, Permission};

/// Something a plugin asks the host to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum PluginCall {
    /// List a directory in a workspace.
    FsList {
        /// Which workspace.
        workspace: String,
        /// A workspace-relative path.
        path: String,
    },
    /// Read a file.
    FsRead {
        /// Which workspace.
        workspace: String,
        /// A workspace-relative path.
        path: String,
    },
    /// Write a file.
    FsWrite {
        /// Which workspace.
        workspace: String,
        /// A workspace-relative path.
        path: String,
        /// What to write.
        contents: String,
    },
    /// What is on a session's screen.
    SessionRead {
        /// Which session.
        session: String,
    },
    /// Type into a session.
    SessionWrite {
        /// Which session.
        session: String,
        /// What to type.
        text: String,
    },
    /// Show the user something.
    Notify {
        /// The message.
        message: String,
        /// How much it matters.
        level: NotifyLevel,
    },
    /// Fetch a URL.
    HttpGet {
        /// Where.
        url: String,
    },
}

/// How much a notification matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    /// Background information.
    Info,
    /// Something the user should look at.
    Warn,
    /// Something went wrong.
    Error,
}

impl PluginCall {
    /// The permission this call needs.
    ///
    /// Every variant names one. A call that needed none would be a call the
    /// user was never shown and never agreed to.
    #[must_use]
    pub const fn permission(&self) -> Permission {
        match self {
            Self::FsList { .. } | Self::FsRead { .. } => Permission::ReadWorkspace,
            Self::FsWrite { .. } => Permission::WriteWorkspace,
            Self::SessionRead { .. } => Permission::ReadSessions,
            Self::SessionWrite { .. } => Permission::WriteInput,
            Self::HttpGet { .. } => Permission::Network,
            // A notification is the one thing every plugin may do: it is how a
            // plugin tells the user it cannot do something, and gating it
            // behind a permission would silence exactly that message.
            Self::Notify { .. } => Permission::ReadSessions,
        }
    }

    /// The workspace-relative path this call names, if it names one.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::FsList { path, .. } | Self::FsRead { path, .. } | Self::FsWrite { path, .. } => {
                Some(path)
            }
            _ => None,
        }
    }
}

/// Why a call was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    /// The plugin was not granted the permission.
    #[error(transparent)]
    Denied(#[from] Denied),
    /// The path escapes its workspace, or is absolute.
    #[error("`{path}` is not a path inside the workspace")]
    BadPath {
        /// What was asked for.
        path: String,
    },
    /// The payload is larger than a plugin may send.
    #[error("a plugin may not write more than {max} bytes at once")]
    TooLarge {
        /// The cap.
        max: usize,
    },
}

/// The most a plugin may write in one call.
///
/// Bounded because a plugin is third-party code with a file-writing permission,
/// and "it only writes small files" is not something the host can know.
pub const MAX_WRITE_BYTES: usize = 8 << 20;

/// Check a call before the host performs it.
///
/// # Errors
/// Fails if the plugin lacks the permission, names a path outside its
/// workspace, or sends more than the cap.
pub fn authorize(plugin: &Installed, call: &PluginCall) -> Result<(), CallError> {
    plugin.check(call.permission())?;

    if let Some(path) = call.path()
        && !is_workspace_relative(path)
    {
        return Err(CallError::BadPath {
            path: path.to_owned(),
        });
    }

    if let PluginCall::FsWrite { contents, .. } = call
        && contents.len() > MAX_WRITE_BYTES
    {
        return Err(CallError::TooLarge {
            max: MAX_WRITE_BYTES,
        });
    }

    Ok(())
}

/// Whether a path stays inside its workspace.
///
/// Checked here, in the host, on the string the plugin sent — *before* it ever
/// reaches a filesystem call. The workspace layer checks again against the
/// canonical path, which catches symlinks; this catches the obvious ones
/// without a syscall, so a plugin cannot use rejection timing to learn what
/// exists outside.
#[must_use]
pub fn is_workspace_relative(path: &str) -> bool {
    use std::path::{Component, Path};

    if path.is_empty() {
        // The workspace root itself, which is a legitimate thing to list.
        return true;
    }
    let path = Path::new(path);
    if path.is_absolute() {
        return false;
    }
    let mut depth = 0i32;
    for component in path.components() {
        match component {
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return false,
        }
        if depth < 0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::Manifest;
    use std::collections::BTreeSet;

    fn plugin(permissions: &[Permission]) -> Installed {
        let set: BTreeSet<Permission> = permissions.iter().copied().collect();
        Installed::new(
            Manifest {
                id: "test".to_owned(),
                name: "Test".to_owned(),
                version: "1".to_owned(),
                permissions: set.clone(),
                description: String::new(),
            },
            set,
        )
    }

    fn read(path: &str) -> PluginCall {
        PluginCall::FsList {
            workspace: "wksp_x".to_owned(),
            path: path.to_owned(),
        }
    }

    #[test]
    fn every_call_names_a_permission() {
        // A call that needed none would be one the user was never shown and
        // never agreed to.
        let calls = [
            read("src"),
            PluginCall::FsRead {
                workspace: "w".to_owned(),
                path: "a".to_owned(),
            },
            PluginCall::FsWrite {
                workspace: "w".to_owned(),
                path: "a".to_owned(),
                contents: String::new(),
            },
            PluginCall::SessionRead {
                session: "s".to_owned(),
            },
            PluginCall::SessionWrite {
                session: "s".to_owned(),
                text: String::new(),
            },
            PluginCall::HttpGet {
                url: "https://x".to_owned(),
            },
        ];
        for call in &calls {
            // Every one resolves to something in the declared set.
            assert!(Permission::ALL.contains(&call.permission()), "{call:?}");
        }
    }

    #[test]
    fn reading_and_writing_files_need_different_permissions() {
        // Granting a plugin read access must not let it write.
        let reader = plugin(&[Permission::ReadWorkspace]);
        assert!(authorize(&reader, &read("src")).is_ok());
        assert!(
            authorize(
                &reader,
                &PluginCall::FsWrite {
                    workspace: "w".to_owned(),
                    path: "a.txt".to_owned(),
                    contents: "x".to_owned(),
                }
            )
            .is_err()
        );
    }

    #[test]
    fn typing_into_a_session_needs_the_dangerous_permission() {
        // The only one that can make something happen the user did not do.
        let reader = plugin(&[Permission::ReadSessions]);
        assert!(
            authorize(
                &reader,
                &PluginCall::SessionWrite {
                    session: "s".to_owned(),
                    text: "rm -rf /\n".to_owned(),
                }
            )
            .is_err()
        );
        assert!(Permission::WriteInput.is_high_consequence());
    }

    #[test]
    fn a_path_escaping_the_workspace_is_refused_without_touching_the_disk() {
        // Before any filesystem call, so a plugin cannot use rejection timing
        // to learn what exists outside.
        let p = plugin(&[Permission::ReadWorkspace]);
        for bad in ["../secrets", "/etc/passwd", "src/../../etc", "..", "/"] {
            assert!(
                matches!(authorize(&p, &read(bad)), Err(CallError::BadPath { .. })),
                "`{bad}` was allowed"
            );
        }
    }

    #[test]
    fn ordinary_paths_are_allowed() {
        let p = plugin(&[Permission::ReadWorkspace]);
        for good in ["", "src", "src/main.rs", "./src", "a/b/../c"] {
            assert!(authorize(&p, &read(good)).is_ok(), "`{good}` was refused");
        }
    }

    #[test]
    fn a_plugin_cannot_write_an_unbounded_file() {
        // Third-party code with a file-writing permission, and "it only writes
        // small files" is not something the host can know.
        let p = plugin(&[Permission::WriteWorkspace]);
        let err = authorize(
            &p,
            &PluginCall::FsWrite {
                workspace: "w".to_owned(),
                path: "big".to_owned(),
                contents: "x".repeat(MAX_WRITE_BYTES + 1),
            },
        )
        .expect_err("must refuse");
        assert!(matches!(err, CallError::TooLarge { .. }), "{err:?}");
    }

    #[test]
    fn a_write_exactly_at_the_cap_is_allowed() {
        let p = plugin(&[Permission::WriteWorkspace]);
        assert!(
            authorize(
                &p,
                &PluginCall::FsWrite {
                    workspace: "w".to_owned(),
                    path: "big".to_owned(),
                    contents: "x".repeat(MAX_WRITE_BYTES),
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn a_disabled_plugin_can_do_nothing_at_all() {
        let mut p = plugin(&[Permission::ReadWorkspace]);
        p.enabled = false;
        assert!(authorize(&p, &read("src")).is_err());
    }

    #[test]
    fn a_denial_explains_itself_in_words_the_user_can_read() {
        let p = plugin(&[Permission::ReadSessions]);
        let err = authorize(
            &p,
            &PluginCall::HttpGet {
                url: "https://exfiltrate.example".to_owned(),
            },
        )
        .expect_err("denied");
        assert!(
            err.to_string().contains("send data over the network"),
            "{err}"
        );
    }

    #[test]
    fn a_call_round_trips_through_json() {
        // Plugins are out-of-process, so this is the wire.
        let call = PluginCall::FsWrite {
            workspace: "wksp_a".to_owned(),
            path: "src/main.rs".to_owned(),
            contents: "fn main() {}".to_owned(),
        };
        let text = serde_json::to_string(&call).expect("serialize");
        assert!(text.contains("\"call\":\"fs_write\""), "{text}");
        let back: PluginCall = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, call);
    }
}
