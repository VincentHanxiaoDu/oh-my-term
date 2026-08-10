//! Upgrading without killing anything that is working.
//!
//! `docs/architecture/22-operations.md` §6, and the refusal is the point:
//! killing an agent mid-`terraform apply` because a version was available is
//! the single worst thing an upgrade can do.
//!
//! So the default is to install the binary — which is safe, the running daemon
//! keeps its inode — and *not* restart. What restarts is chosen by the user:
//! wait for every agent to go idle, force it and lose them, or install the
//! binary and restart by hand later.
//!
//! **omt never checks for updates on its own.** `omt upgrade` reaches the
//! network because the user typed it. A terminal that phones home on a timer is
//! one that leaks when you are working, and 13 §9.1 says egress is a thing the
//! user initiates.
//!
//! The multi-host path (§6.3) is a loop over hosts the client already knows. It
//! is deliberately not fleet management: no inventory, no groups, no rollout
//! policy, no canary. Past about ten hosts the right tool is the configuration
//! management the user already has, and "we are a single static binary, `ansible
//! copy` works fine" is a better answer than a half-built fleet tool.

use serde::Serialize;

/// What a session is doing, as an upgrade cares about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Busy {
    /// Which session.
    pub session: String,
    /// Which agent.
    pub agent: String,
    /// Whether it is blocked on a human.
    pub blocked: bool,
}

/// What the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// The default: decide from what is running.
    Auto,
    /// Install now, restart at the first idle moment.
    Wait,
    /// Restart now, and lose what is running.
    Force,
    /// Install the binary; the user restarts.
    BinaryOnly,
}

/// What an upgrade will do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "plan")]
pub enum Plan {
    /// Install and restart straight away.
    Restart,
    /// Install now; restart when every agent is idle.
    Deferred {
        /// What it is waiting for.
        working: Vec<Busy>,
    },
    /// Install and leave the restart to the user.
    BinaryOnly,
    /// Refuse, and say what is running.
    Refuse {
        /// What is running.
        working: Vec<Busy>,
    },
}

/// Decide what to do.
///
/// With nothing running, an upgrade is uneventful and restarts. With something
/// running the default refuses rather than choosing for the user, because the
/// cost of being wrong is asymmetric: a deferred restart costs a few minutes
/// and a forced one costs whatever the agent was in the middle of.
#[must_use]
pub fn plan(busy: Vec<Busy>, choice: Choice) -> Plan {
    match choice {
        Choice::Force => Plan::Restart,
        Choice::BinaryOnly => Plan::BinaryOnly,
        Choice::Wait => Plan::Deferred { working: busy },
        Choice::Auto if busy.is_empty() => Plan::Restart,
        Choice::Auto => Plan::Refuse { working: busy },
    }
}

/// Whether a deferred restart may now go ahead.
///
/// Every agent idle *and* no open interaction. An interaction nobody has
/// answered is a question on somebody's screen, and restarting under it loses
/// the question as surely as killing a working agent loses the work.
#[must_use]
pub fn may_restart_now(busy: &[Busy], open_interactions: usize) -> bool {
    busy.is_empty() && open_interactions == 0
}

/// How omt was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    /// A package manager owns the file.
    Brew,
    /// Likewise.
    Cargo,
    /// Likewise.
    Apt,
    /// A binary the user put there.
    Plain,
}

impl Installed {
    /// The command to run instead, when a package manager owns the file.
    ///
    /// Overwriting a package manager's file leaves it certain it has a version
    /// it does not have, and the next `brew upgrade` puts the old one back.
    #[must_use]
    pub const fn delegate_to(self) -> Option<&'static str> {
        match self {
            Self::Brew => Some("brew upgrade oh-my-term"),
            Self::Cargo => Some("cargo install omt --force"),
            Self::Apt => Some("apt install --only-upgrade omt"),
            Self::Plain => None,
        }
    }
}

/// Guess how omt was installed, from where its binary is.
#[must_use]
pub fn installed_from(path: &str) -> Installed {
    if path.contains("/Cellar/") || path.contains("/homebrew/") {
        Installed::Brew
    } else if path.contains("/.cargo/bin/") {
        Installed::Cargo
    } else if path.starts_with("/usr/bin/") {
        Installed::Apt
    } else {
        Installed::Plain
    }
}

/// What a version difference between a CLI and its daemon means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skew {
    /// The same version.
    Same,
    /// Different, but the catalogs overlap: proceed, and say so once.
    Tolerated {
        /// What to tell the user.
        notice: String,
    },
    /// No common protocol. The only hard failure there is.
    Incompatible {
        /// The one action offered.
        remedy: String,
    },
}

/// Compare a CLI against the daemon it is talking to.
///
/// The handshake carries the version, so this is detected rather than guessed.
#[must_use]
pub fn skew(cli: &str, daemon: &str, catalogs_intersect: bool) -> Skew {
    if cli == daemon {
        return Skew::Same;
    }
    if catalogs_intersect {
        Skew::Tolerated {
            notice: format!(
                "your omt binary was upgraded but the running daemon is {daemon}; `omt instance restart` when convenient"
            ),
        }
    } else {
        Skew::Incompatible {
            remedy: format!("upgrade the remote: it is {daemon} and this is {cli}"),
        }
    }
}

/// One host in a multi-host upgrade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    /// Its name in `instances.toml`.
    pub name: String,
    /// Its version, or `None` if it could not be reached.
    pub version: Option<String>,
    /// What is working there.
    pub busy: Vec<Busy>,
    /// Whether anyone other than the caller is attached.
    pub other_users_attached: bool,
}

/// What will happen to one host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum HostPlan {
    /// Upload, install, restart.
    Upgrade,
    /// Install, restart later.
    Deferred,
    /// Skip it, with a reason.
    Skip {
        /// Why.
        reason: String,
    },
}

/// Plan one host.
///
/// The rule that is easy to leave out: **omt will not upgrade a host serving
/// another user's sessions.** Replacing a shared dev box's binary is not one
/// laptop's decision.
#[must_use]
pub fn plan_host(host: &Host, choice: Choice) -> HostPlan {
    let Some(_) = &host.version else {
        return HostPlan::Skip {
            reason: "unreachable".to_owned(),
        };
    };
    if host.other_users_attached {
        return HostPlan::Skip {
            reason: "another user is attached; replacing a shared host's binary is not this laptop's decision".to_owned(),
        };
    }
    match plan(host.busy.clone(), choice) {
        Plan::Restart => HostPlan::Upgrade,
        Plan::Deferred { .. } | Plan::BinaryOnly => HostPlan::Deferred,
        Plan::Refuse { working } => HostPlan::Skip {
            reason: format!(
                "{} session{} working; retry with --wait",
                working.len(),
                if working.len() == 1 { " is" } else { "s are" }
            ),
        },
    }
}

/// The warning shown before a downgrade, rather than after.
///
/// An older binary refuses to open a store a newer one migrated, which is
/// correct — and is a thing to learn before the downgrade, not during it.
#[must_use]
pub fn downgrade_warning(from: &str, to: &str) -> String {
    format!(
        "{to} is older than {from}. If this host has already migrated its store, the older binary will refuse to open it and you will need {from} back."
    )
}

/// The scale past which this is the wrong tool.
pub const HOST_LIMIT: usize = 10;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn working(session: &str) -> Busy {
        Busy {
            session: session.to_owned(),
            agent: "claude".to_owned(),
            blocked: false,
        }
    }

    #[test]
    fn an_idle_instance_upgrades_without_ceremony() {
        assert_eq!(plan(Vec::new(), Choice::Auto), Plan::Restart);
    }

    #[test]
    fn a_working_agent_stops_the_upgrade_by_default() {
        // Killing an agent mid-`terraform apply` because a version was
        // available is the worst thing an upgrade can do.
        let plan = plan(vec![working("s_4b2f")], Choice::Auto);
        assert!(matches!(plan, Plan::Refuse { .. }));
    }

    #[test]
    fn the_refusal_names_what_is_running() {
        // "refusing to restart" without saying what would be lost gives the
        // user nothing to decide with.
        let Plan::Refuse { working: busy } = plan(vec![working("s_4b2f")], Choice::Auto) else {
            panic!("refused");
        };
        assert_eq!(busy[0].session, "s_4b2f");
    }

    #[test]
    fn force_is_available_and_is_the_users_choice_to_make() {
        // The refusal is a default, not a lock. Somebody who knows their
        // sessions are disposable should not have to fight omt.
        assert_eq!(plan(vec![working("s")], Choice::Force), Plan::Restart);
    }

    #[test]
    fn wait_installs_now_and_restarts_later() {
        // Installing is safe: the running daemon keeps its inode.
        let plan = plan(vec![working("s")], Choice::Wait);
        assert!(matches!(plan, Plan::Deferred { .. }));
    }

    #[test]
    fn a_deferred_restart_waits_for_an_unanswered_question_too() {
        // An open interaction is a question on somebody's screen, and
        // restarting under it loses the question as surely as killing a
        // working agent loses the work.
        assert!(may_restart_now(&[], 0));
        assert!(!may_restart_now(&[], 1));
        assert!(!may_restart_now(&[working("s")], 0));
    }

    #[test]
    fn a_package_managers_file_is_not_overwritten() {
        // It would leave the package manager certain it has a version it does
        // not have, and the next upgrade puts the old one back.
        assert_eq!(
            installed_from("/opt/homebrew/bin/omt").delegate_to(),
            Some("brew upgrade oh-my-term")
        );
        assert_eq!(installed_from("/home/ada/.cargo/bin/omt").delegate_to(), Some("cargo install omt --force"));
        assert_eq!(installed_from("/usr/local/bin/omt").delegate_to(), None);
    }

    #[test]
    fn a_newer_cli_against_an_older_daemon_proceeds_and_says_so() {
        // The handshake carries the version, so this is detected rather than
        // guessed — and it is a notice, not a failure.
        let Skew::Tolerated { notice } = skew("0.5.0", "0.4.1", true) else {
            panic!("tolerated");
        };
        assert!(notice.contains("0.4.1"), "{notice}");
        assert!(notice.contains("instance restart"), "{notice}");
    }

    #[test]
    fn matching_versions_say_nothing_at_all() {
        assert_eq!(skew("0.5.0", "0.5.0", true), Skew::Same);
    }

    #[test]
    fn no_common_protocol_is_the_only_hard_failure() {
        let Skew::Incompatible { remedy } = skew("0.5.0", "0.3.2", false) else {
            panic!("incompatible");
        };
        assert!(remedy.contains("upgrade the remote"), "{remedy}");
    }

    #[test]
    fn a_host_serving_another_user_is_never_upgraded() {
        // Replacing a shared dev box's binary is not one laptop's decision.
        let host = Host {
            name: "box".to_owned(),
            version: Some("0.3.2".to_owned()),
            busy: Vec::new(),
            other_users_attached: true,
        };
        let HostPlan::Skip { reason } = plan_host(&host, Choice::Force) else {
            panic!("skipped");
        };
        assert!(reason.contains("another user"), "{reason}");
    }

    #[test]
    fn an_unreachable_host_is_skipped_rather_than_failing_the_run() {
        // Hosts are independent; there is no transaction across them.
        let host = Host {
            name: "vpn-box".to_owned(),
            version: None,
            busy: Vec::new(),
            other_users_attached: false,
        };
        assert_eq!(
            plan_host(&host, Choice::Auto),
            HostPlan::Skip {
                reason: "unreachable".to_owned()
            }
        );
    }

    #[test]
    fn a_busy_remote_is_skipped_with_the_command_that_would_work() {
        // A refusal that does not say what to do next is a dead end.
        let host = Host {
            name: "ci-runner".to_owned(),
            version: Some("0.4.1".to_owned()),
            busy: vec![working("s_1")],
            other_users_attached: false,
        };
        let HostPlan::Skip { reason } = plan_host(&host, Choice::Auto) else {
            panic!("skipped");
        };
        assert!(reason.contains("--wait"), "{reason}");
        assert!(reason.contains("1 session is"), "{reason}");
    }

    #[test]
    fn an_idle_remote_is_upgraded() {
        let host = Host {
            name: "pi".to_owned(),
            version: Some("0.4.1".to_owned()),
            busy: Vec::new(),
            other_users_attached: false,
        };
        assert_eq!(plan_host(&host, Choice::Auto), HostPlan::Upgrade);
    }

    #[test]
    fn a_downgrade_is_warned_about_before_it_happens_not_after() {
        // The older binary refusing to open a migrated store is correct, and is
        // a thing to learn beforehand.
        let warning = downgrade_warning("0.5.0", "0.4.1");
        assert!(warning.contains("refuse to open"), "{warning}");
    }
}
