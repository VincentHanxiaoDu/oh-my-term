//! Getting omt onto a host that does not have it.
//!
//! `docs/architecture/22-operations.md` §7. The transport is 07 §2.4; this is
//! the policy around it.
//!
//! **musl is the default remote artifact regardless of the host's libc.** Fully
//! static, no glibc-version cliff, and it works on Alpine and on a 2018 CentOS
//! box alike — precisely the class of host where the obvious approach, matching
//! the remote's libc, fails outright.
//!
//! **The binary goes over the ssh channel that is already open**, not fetched
//! by the remote. It is about twenty megabytes, the channel is already
//! authenticated, and this works on an air-gapped host and behind a hostile
//! proxy on the first try. It also means nothing is downloaded on somebody
//! else's machine on omt's say-so.
//!
//! **A host that refuses the install still gets a terminal.** Somebody who
//! typed `omt ssh box` wants to work on box. Falling back to the pty-wrapper
//! rung and showing the diagnostic as a notice is the right trade; refusing to
//! give them a terminal because a media tier is unavailable is not.

use serde::Serialize;

/// What the probe said about a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// `uname -s`.
    pub system: String,
    /// `uname -m`.
    pub machine: String,
    /// Whether `zstd` is there.
    pub zstd: bool,
}

impl Probe {
    /// Parse the probe's one line of output.
    ///
    /// `None` when it is not a shape omt recognises — which is reported with
    /// the raw text rather than guessed at.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let rest = line.strip_prefix("OMT-MISSING")?.trim();
        let mut parts = rest.split_whitespace();
        let system = parts.next()?.to_owned();
        let machine = parts.next()?.to_owned();
        Some(Self {
            system,
            machine,
            zstd: rest.contains("zstd") && !rest.contains("nozstd"),
        })
    }
}

/// Which artifact to send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Target {
    /// Send this one.
    Artifact(&'static str),
    /// omt does not build for this host.
    ///
    /// Refused with the `uname` and the release URL, rather than sending
    /// something that will not run and failing later with a worse message.
    Unsupported {
        /// What the host said it is.
        uname: String,
    },
}

/// Pick the artifact for a probed host.
#[must_use]
pub fn target(probe: &Probe) -> Target {
    match (probe.system.as_str(), probe.machine.as_str()) {
        // musl regardless of the host's libc: static, and no glibc cliff.
        ("Linux", "x86_64") => Target::Artifact("x86_64-unknown-linux-musl"),
        ("Linux", "aarch64" | "arm64") => Target::Artifact("aarch64-unknown-linux-musl"),
        ("Linux", "armv7l") => Target::Artifact("armv7-unknown-linux-musleabihf"),
        ("Darwin", "arm64") => Target::Artifact("aarch64-apple-darwin"),
        ("Darwin", "x86_64") => Target::Artifact("x86_64-apple-darwin"),
        _ => Target::Unsupported {
            uname: format!("{} {}", probe.system, probe.machine),
        },
    }
}

/// Where the binary goes.
///
/// One file, no sudo, no versioned tree — and therefore no garbage to collect.
/// A single file replaced in place is the whole lifecycle.
pub const INSTALL_PATH: &str = "~/.local/bin/omt";

/// The install, as a shell command.
///
/// `mv` is a `rename(2)`, which safely replaces a running binary on Unix: the
/// running process keeps its inode. There is no half-written state, which is
/// the failure this shape exists to rule out.
#[must_use]
pub fn install_command(prefix: &str) -> String {
    format!(
        "cat > {prefix}/omt.tmp && chmod 755 {prefix}/omt.tmp && mv -f {prefix}/omt.tmp {prefix}/omt"
    )
}

/// What the user has said about installing on a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// Never asked.
    Unknown,
    /// Yes, and remembered.
    Given,
    /// No.
    Refused,
    /// The host is locked down; do not even ask.
    AssumeInstalled,
}

/// What to do about a host with no omt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum Decision {
    /// Ask, once, and remember the answer.
    ///
    /// Writing an executable into somebody's `~/.local/bin` is a side effect.
    Ask,
    /// Upload it.
    Install,
    /// Give them a terminal anyway, and say what is missing.
    Degrade {
        /// Shown as a notice rather than raised as a failure.
        notice: String,
    },
}

/// Decide, given consent and what the probe found.
#[must_use]
pub fn decide(consent: Consent, target: &Target) -> Decision {
    if let Target::Unsupported { uname } = target {
        return Decision::Degrade {
            notice: format!(
                "no omt build for {uname}; this session uses the pty wrapper. Builds are listed at the release page."
            ),
        };
    }
    match consent {
        Consent::Given => Decision::Install,
        Consent::Unknown => Decision::Ask,
        Consent::Refused | Consent::AssumeInstalled => Decision::Degrade {
            notice: "omt is not installed on this host and was not installed; this session uses the pty wrapper".to_owned(),
        },
    }
}

/// Whether an uploaded artifact is the one that was sent.
///
/// Checked on the remote by the freshly installed binary itself, against the
/// local value. A truncated upload is caught here rather than becoming a
/// confusing runtime failure an hour later.
#[must_use]
pub fn verified(local_hash: &str, remote_hash: &str) -> bool {
    !local_hash.is_empty() && local_hash == remote_hash
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn probe(system: &str, machine: &str) -> Probe {
        Probe {
            system: system.to_owned(),
            machine: machine.to_owned(),
            zstd: false,
        }
    }

    #[test]
    fn the_probe_line_parses() {
        let p = Probe::parse("OMT-MISSING Linux aarch64 musl libc nozstd").expect("parsed");
        assert_eq!(p.system, "Linux");
        assert_eq!(p.machine, "aarch64");
        assert!(!p.zstd);
    }

    #[test]
    fn nozstd_is_not_read_as_zstd() {
        // The words overlap, and getting this backwards means compressing to a
        // host that cannot decompress.
        assert!(
            !Probe::parse("OMT-MISSING Linux x86_64 nozstd")
                .expect("parsed")
                .zstd
        );
        assert!(
            Probe::parse("OMT-MISSING Linux x86_64 zstd")
                .expect("parsed")
                .zstd
        );
    }

    #[test]
    fn a_line_that_is_not_the_probe_is_not_parsed_as_one() {
        // The same exec carries a real protocol frame when omt is present, and
        // mistaking one for the other would reinstall over a working host.
        assert!(Probe::parse("{\"t\":\"welcome\"}").is_none());
    }

    #[test]
    fn every_linux_host_gets_a_musl_build_whatever_its_libc() {
        // Static, no glibc-version cliff, works on Alpine and on a 2018 CentOS
        // box alike — the class of host where matching the remote's libc fails
        // outright.
        for machine in ["x86_64", "aarch64", "armv7l"] {
            let Target::Artifact(name) = target(&probe("Linux", machine)) else {
                panic!("{machine} unsupported");
            };
            assert!(name.contains("musl"), "{name}");
        }
    }

    #[test]
    fn macos_gets_its_own_targets() {
        assert_eq!(
            target(&probe("Darwin", "arm64")),
            Target::Artifact("aarch64-apple-darwin")
        );
    }

    #[test]
    fn an_architecture_omt_does_not_build_for_is_refused_with_the_uname() {
        // Rather than sending something that will not run and failing later
        // with a worse message.
        let Target::Unsupported { uname } = target(&probe("Linux", "sparc64")) else {
            panic!("unsupported");
        };
        assert!(uname.contains("sparc64"), "{uname}");
    }

    #[test]
    fn installing_is_asked_once_and_remembered() {
        // Writing an executable into somebody's ~/.local/bin is a side effect.
        let t = Target::Artifact("x86_64-unknown-linux-musl");
        assert_eq!(decide(Consent::Unknown, &t), Decision::Ask);
        assert_eq!(decide(Consent::Given, &t), Decision::Install);
    }

    #[test]
    fn a_host_that_refused_still_gets_a_terminal() {
        // Somebody who typed `omt ssh box` wants to work on box. Refusing to
        // give them a terminal because a media tier is unavailable would be the
        // wrong trade.
        let t = Target::Artifact("x86_64-unknown-linux-musl");
        let Decision::Degrade { notice } = decide(Consent::Refused, &t) else {
            panic!("degraded");
        };
        assert!(notice.contains("pty wrapper"), "{notice}");
    }

    #[test]
    fn an_unsupported_architecture_degrades_rather_than_asking() {
        // There is nothing to consent to: no artifact exists.
        let decision = decide(Consent::Unknown, &target(&probe("Linux", "sparc64")));
        assert!(matches!(decision, Decision::Degrade { .. }));
    }

    #[test]
    fn a_locked_down_host_is_never_prompted() {
        let t = Target::Artifact("x86_64-unknown-linux-musl");
        assert!(matches!(
            decide(Consent::AssumeInstalled, &t),
            Decision::Degrade { .. }
        ));
    }

    #[test]
    fn the_install_replaces_the_file_atomically() {
        // `mv` is a rename(2), which safely replaces a running binary: the
        // running process keeps its inode, and there is no half-written state.
        let command = install_command("$HOME/.local/bin");
        assert!(command.contains("omt.tmp"), "{command}");
        assert!(command.contains("mv -f"), "{command}");
        assert!(
            command.find("chmod").expect("chmod") < command.find("mv -f").expect("mv"),
            "it is made executable before it is put in place, not after"
        );
    }

    #[test]
    fn a_truncated_upload_is_caught_before_it_is_used() {
        assert!(verified("blake3:abc", "blake3:abc"));
        assert!(!verified("blake3:abc", "blake3:abd"));
    }

    #[test]
    fn a_missing_hash_never_counts_as_verified() {
        // The failure mode where verification silently passes because nothing
        // was computed.
        assert!(!verified("", ""));
    }
}
