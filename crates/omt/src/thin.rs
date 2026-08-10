//! `omt ssh <target>` — the thin client.
//!
//! `docs/architecture/09-ssh-and-media.md` §6. The easy case, and the one omt
//! should push people toward, because everything in §5 collapses when omt is on
//! both ends: no OSC, no synthetic input, no echo suppression, no base64
//! expansion, no tmux wrapping, no terminal detection.
//!
//! What is decided here is the command line and the bootstrap policy. The
//! transport itself is `omt-transport`'s `SshStdio`, no different in kind from
//! the WebSocket one — a thin client is a renderer over the remote catalog,
//! exactly as the web client is, and it holds no session state.
//!
//! The command is built rather than interpolated into a shell string. `ssh
//! box "omt serve --stdio"` puts the remote command through the remote login
//! shell, and a hostname from a config file with a quote in it then becomes
//! remote execution. Passing an argument vector means there is no shell to
//! confuse.

use serde::{Deserialize, Serialize};

/// What to do when the far side has no omt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Bootstrap {
    /// Offer to install it, and ask first.
    Ask,
    /// Never install; fall back to running ssh inside a local pty.
    Never,
    /// Trust that it is there and fail loudly if it is not.
    ///
    /// For hosts where the probe itself is the problem — a locked-down box
    /// where an unexpected exec shows up in an audit log somebody has to
    /// explain.
    AssumeInstalled,
}

/// How a thin-client session was reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// omt on both ends, over ssh's stdio.
    Native,
    /// ssh running inside a local omt-managed pty, because the far side has no
    /// omt and will not be given one.
    ManagedPty,
}

/// The plan for one `omt ssh` invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Plan {
    /// How this session will be run.
    pub mode: Mode,
    /// The argument vector, with no shell involved.
    pub argv: Vec<String>,
    /// Why this mode, in words somebody can act on.
    pub reason: String,
}

/// Why a target was refused.
///
/// `Display` written out rather than adding a derive-macro dependency to this
/// crate for one variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThinError {
    /// The target is not something that can be handed to ssh.
    BadTarget(String),
}

impl std::fmt::Display for ThinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadTarget(target) => write!(
                f,
                "`{target}` is not a usable ssh target: it would be read as an option or passed through a shell"
            ),
        }
    }
}

impl std::error::Error for ThinError {}

/// Whether a target is something omt is willing to hand to ssh.
///
/// Rejected rather than escaped. An `ssh` target beginning with `-` is read as
/// an option — `-oProxyCommand=...` is the well-known way that turns a
/// hostname into arbitrary local execution — and a target carrying whitespace
/// or a shell metacharacter has no legitimate form omt needs to support.
fn usable(target: &str) -> bool {
    !target.is_empty()
        && !target.starts_with('-')
        && !target.contains(|c: char| c.is_whitespace() || c.is_control())
        && !target.contains(['"', '\'', '`', '$', ';', '&', '|', '\\'])
}

/// Build the plan for `omt ssh <target>`.
///
/// # Errors
/// Refuses a target that cannot be safely handed to ssh.
pub fn plan(target: &str, remote_has_omt: bool, bootstrap: Bootstrap) -> Result<Plan, ThinError> {
    if !usable(target) {
        return Err(ThinError::BadTarget(target.to_owned()));
    }

    let native = |argv: Vec<&str>, reason: &str| Plan {
        mode: Mode::Native,
        argv: argv.into_iter().map(str::to_owned).collect(),
        reason: reason.to_owned(),
    };

    match (remote_has_omt, bootstrap) {
        (true, _) => Ok(native(
            vec!["ssh", target, "omt", "serve", "--stdio"],
            "omt is on both ends, so the wire protocol runs over ssh's stdio directly.",
        )),
        // Told to assume, so omt does not probe and does not fall back. On a
        // locked-down host the probe is itself the problem: an unexpected exec
        // is a line in an audit log somebody has to explain.
        (false, Bootstrap::AssumeInstalled) => Ok(native(
            vec!["ssh", target, "omt", "serve", "--stdio"],
            "Configured to assume omt is installed, so no probe was run. This will fail loudly if it is not there.",
        )),
        (false, Bootstrap::Ask) => Ok(native(
            vec!["ssh", target, "omt", "serve", "--stdio", "--exec"],
            "omt was not found on the far side. It will be started, or offered for install.",
        )),
        (false, Bootstrap::Never) => Ok(Plan {
            mode: Mode::ManagedPty,
            argv: vec!["ssh".to_owned(), target.to_owned()],
            // The same command the user typed still works; what changed is
            // what they get, and that is reported rather than chosen by which
            // command they happened to remember.
            reason: "No omt on the far side and installing was declined, so ssh runs inside a local omt pty. Paste falls back to the OSC bridge.".to_owned(),
        }),
    }
}

impl Plan {
    /// Whether this session gets the collapsed media path of §6.1.
    ///
    /// Only the native mode does. Saying otherwise would promise a paste that
    /// crosses directly when it would in fact go through the escape-sequence
    /// bridge, and the user would find out by pasting.
    #[must_use]
    pub fn direct_media(&self) -> bool {
        self.mode == Mode::Native
    }
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
    fn omt_on_both_ends_speaks_the_wire_protocol_over_stdio() {
        let p = plan("box", true, Bootstrap::Ask).expect("plan");
        assert_eq!(p.mode, Mode::Native);
        assert_eq!(p.argv, ["ssh", "box", "omt", "serve", "--stdio"]);
    }

    #[test]
    fn the_command_is_an_argument_vector_with_no_shell_in_it() {
        // `ssh box "omt serve --stdio"` puts the remote command through the
        // remote login shell. An argument vector has no shell to confuse.
        let p = plan("box", true, Bootstrap::Ask).expect("plan");
        assert!(
            p.argv.iter().all(|a| !a.contains(' ')),
            "{:?} would need quoting",
            p.argv
        );
    }

    #[test]
    fn a_target_that_would_be_read_as_an_option_is_refused() {
        // `-oProxyCommand=...` is the well-known way a hostname becomes local
        // execution.
        for target in ["-oProxyCommand=touch /tmp/pwned", "-J evil", "-"] {
            assert!(
                matches!(plan(target, true, Bootstrap::Ask), Err(ThinError::BadTarget(_))),
                "{target} was accepted"
            );
        }
    }

    #[test]
    fn a_target_carrying_shell_metacharacters_is_refused() {
        // Refused rather than escaped: there is no legitimate ssh target of
        // this shape, so there is nothing to preserve by being clever.
        for target in [
            "box; rm -rf /",
            "box$(whoami)",
            "box`id`",
            "box|nc evil 1234",
            "box\\'",
            "a box",
            "",
        ] {
            assert!(
                plan(target, true, Bootstrap::Ask).is_err(),
                "{target:?} was accepted"
            );
        }
    }

    #[test]
    fn an_ordinary_target_is_accepted_in_every_usual_form() {
        for target in ["box", "user@box", "user@box.example.com", "10.0.0.4", "my-host"] {
            assert!(plan(target, true, Bootstrap::Ask).is_ok(), "{target}");
        }
    }

    #[test]
    fn a_host_without_omt_is_offered_a_start_when_asking_is_allowed() {
        let p = plan("box", false, Bootstrap::Ask).expect("plan");
        assert!(p.argv.contains(&"--exec".to_owned()));
    }

    #[test]
    fn declining_the_install_falls_back_rather_than_failing() {
        // The user types one thing. What they get is reported, not chosen by
        // which command they remembered.
        let p = plan("box", false, Bootstrap::Never).expect("plan");
        assert_eq!(p.mode, Mode::ManagedPty);
        assert_eq!(p.argv, ["ssh", "box"]);
        assert!(p.reason.contains("OSC bridge"), "{}", p.reason);
    }

    #[test]
    fn assuming_it_is_installed_does_not_probe_and_says_it_will_fail_loudly() {
        // On a locked-down host the probe is itself the problem: an unexpected
        // exec is a line in an audit log somebody has to explain.
        let p = plan("box", false, Bootstrap::AssumeInstalled).expect("plan");
        assert_eq!(p.mode, Mode::Native);
        assert!(!p.argv.contains(&"--exec".to_owned()));
        assert!(p.reason.contains("fail loudly"), "{}", p.reason);
    }

    #[test]
    fn only_the_native_mode_promises_the_collapsed_media_path() {
        // Saying otherwise promises a paste that crosses directly when it
        // would in fact go through the escape-sequence bridge, and the user
        // finds out by pasting.
        assert!(plan("box", true, Bootstrap::Ask).expect("a").direct_media());
        assert!(!plan("box", false, Bootstrap::Never)
            .expect("b")
            .direct_media());
    }

    #[test]
    fn every_plan_explains_itself() {
        for (has, boot) in [
            (true, Bootstrap::Ask),
            (false, Bootstrap::Ask),
            (false, Bootstrap::Never),
            (false, Bootstrap::AssumeInstalled),
        ] {
            let p = plan("box", has, boot).expect("plan");
            assert!(!p.reason.is_empty(), "{boot:?}");
            assert!(p.reason.ends_with('.'), "{}", p.reason);
        }
    }
}
