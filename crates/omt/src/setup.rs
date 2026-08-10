//! What is installed, and whether this terminal can be driven.
//!
//! `docs/architecture/19-onboarding.md` §3.3 and §7.
//!
//! Two questions a first run has to answer before anything else is worth
//! saying. Which agent CLIs are on this machine — because omt's whole pitch is
//! about agents, and a setup that lists none should say so rather than present
//! an empty roster as if it were a working one. And whether `TERM` describes
//! the terminal actually in use — because the failure mode there is not an
//! error but a subtly wrong screen, which people blame on omt for weeks.
//!
//! Both report what they *checked*, not merely what they found. "No agents"
//! and "I did not look" are different answers, and collapsing them is how a
//! diagnostic becomes worse than nothing.

use std::path::PathBuf;

use omt_catalog::{CapabilityError, CapabilityHandler};
use omt_types::AgentKind;
use serde::{Deserialize, Serialize};

/// The command each agent is invoked as.
///
/// A table rather than derived from the kind's name: `GeminiCli` is `gemini`
/// and `ClaudeCode` is `claude`, and a rule that guessed would be wrong for
/// most of them in a way nobody would notice until it silently found nothing.
pub const COMMANDS: &[(AgentKind, &str)] = &[
    (AgentKind::ClaudeCode, "claude"),
    (AgentKind::Codex, "codex"),
    (AgentKind::Opencode, "opencode"),
    (AgentKind::GeminiCli, "gemini"),
    (AgentKind::QwenCode, "qwen"),
    (AgentKind::Cursor, "cursor-agent"),
    (AgentKind::Copilot, "copilot"),
    (AgentKind::Goose, "goose"),
    (AgentKind::Amp, "amp"),
    (AgentKind::Aider, "aider"),
    (AgentKind::Crush, "crush"),
];

/// Find an executable on `PATH`.
///
/// Written rather than shelled out to `which`: spawning a process per agent on
/// every setup call is eleven processes, and `which` is a shell builtin on some
/// systems and absent on others.
#[must_use]
pub fn on_path(command: &str, path: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Input to `setup.detect_agents`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct DetectIn {}

/// One agent, and whether it is here.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FoundAgent {
    /// Which agent.
    pub kind: String,
    /// What it is invoked as.
    pub command: String,
    /// Where it is, if it is anywhere.
    pub path: Option<String>,
}

/// What `setup.detect_agents` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DetectOut {
    /// Every agent omt knows how to drive, found or not.
    ///
    /// All of them, not only the ones present: somebody whose agent is missing
    /// needs to see that omt knows about it and did not find it, which is a
    /// different problem from omt never having heard of it.
    pub agents: Vec<FoundAgent>,
    /// How many were found.
    pub found: usize,
    /// Whether `PATH` was readable at all.
    ///
    /// Distinguished from finding nothing, because an unreadable `PATH` makes
    /// every answer below meaningless and saying "no agents" would be a
    /// confident wrong answer.
    pub searched: bool,
}

omt_catalog::capability! {
    /// Which agent CLIs are installed on this machine.
    pub struct DetectAgents;
    input  = DetectIn,
    output = DetectOut,
    decl = omt_catalog::Decl {
        name: "setup.detect_agents",
        group: "setup",
        verb: "detect_agents",
        title: "Find installed agents",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Every agent omt can drive, found or not. Listing only what is present hides the difference between an agent omt did not find and one it has never heard of.",
    },
}

/// Serves the setup capabilities.
pub struct SetupHandler;

impl CapabilityHandler<DetectAgents> for SetupHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: DetectIn,
    ) -> Result<DetectOut, CapabilityError> {
        let path = std::env::var("PATH").ok();
        let agents: Vec<FoundAgent> = COMMANDS
            .iter()
            .map(|(kind, command)| FoundAgent {
                kind: format!("{kind:?}"),
                command: (*command).to_owned(),
                path: path
                    .as_deref()
                    .and_then(|p| on_path(command, p))
                    .map(|p| p.display().to_string()),
            })
            .collect();
        Ok(DetectOut {
            found: agents.iter().filter(|a| a.path.is_some()).count(),
            searched: path.is_some(),
            agents,
        })
    }
}

// ---------------------------------------------------------------------------
// TERM and terminfo — §7
// ---------------------------------------------------------------------------

/// Input to `term.profile`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TermIn {}

/// Something the advertised `TERM` and the terminal in use disagree about.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Disagreement {
    /// What disagrees.
    pub about: String,
    /// What `TERM` implies.
    pub advertised: String,
    /// What the terminal appears to actually be.
    pub observed: String,
    /// What to do about it.
    pub remedy: String,
}

/// What `term.profile` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TermOut {
    /// What `TERM` says.
    pub term: Option<String>,
    /// Whether a terminfo entry for it could be resolved.
    pub entry_resolved: bool,
    /// What the outer terminal calls itself, where it says so.
    pub program: Option<String>,
    /// Whether the terminal claims 24-bit colour.
    pub truecolor: bool,
    /// Whether omt is running inside another multiplexer.
    ///
    /// Worth saying plainly: nested multiplexers are the single most common
    /// reason a prefix key appears not to work, and somebody in that situation
    /// has no way to tell from the screen.
    pub nested: Option<String>,
    /// Where the two accounts disagree.
    pub disagreements: Vec<Disagreement>,
}

omt_catalog::capability! {
    /// What this terminal advertises, and whether it is true.
    pub struct TermProfile;
    input  = TermIn,
    output = TermOut,
    decl = omt_catalog::Decl {
        name: "term.profile",
        group: "term",
        verb: "profile",
        title: "Check this terminal",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A wrong TERM does not fail; it draws a subtly wrong screen that people blame on omt for weeks. This says what is advertised, what was observed, and where they disagree.",
    },
}

/// The environment `term.profile` reads, named so it can be tested.
///
/// Taken as a parameter rather than read from the process, because the
/// interesting cases are a `TERM` nobody has set and a nested multiplexer, and
/// neither can be arranged inside a test that reads the real environment.
#[derive(Debug, Default, Clone)]
pub struct TermEnv {
    /// `TERM`.
    pub term: Option<String>,
    /// `COLORTERM`.
    pub colorterm: Option<String>,
    /// `TERM_PROGRAM`.
    pub program: Option<String>,
    /// `TMUX`, if omt is inside tmux.
    pub tmux: Option<String>,
    /// `ZELLIJ`, likewise.
    pub zellij: Option<String>,
    /// `STY`, for screen.
    pub screen: Option<String>,
}

impl TermEnv {
    /// Read it from the process.
    #[must_use]
    pub fn from_process() -> Self {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Self {
            term: get("TERM"),
            colorterm: get("COLORTERM"),
            program: get("TERM_PROGRAM"),
            tmux: get("TMUX"),
            zellij: get("ZELLIJ"),
            screen: get("STY"),
        }
    }
}

/// Whether a terminfo entry exists for a `TERM`.
///
/// Checked against the terminfo directories rather than by loading the entry:
/// the question here is "would a program find this", and a description omt
/// could parse but ncurses could not is not the failure people hit.
#[must_use]
pub fn terminfo_exists(term: &str, dirs: &[PathBuf]) -> bool {
    let Some(first) = term.chars().next() else {
        return false;
    };
    dirs.iter().any(|dir| {
        // Both layouts: a letter directory on most systems, and the hex of the
        // first byte on the ones that use that instead.
        dir.join(first.to_string()).join(term).exists()
            || dir.join(format!("{:x}", first as u32)).join(term).exists()
    })
}

/// Where terminfo entries live.
#[must_use]
pub fn terminfo_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".terminfo"));
    }
    if let Ok(set) = std::env::var("TERMINFO") {
        dirs.push(PathBuf::from(set));
    }
    dirs.extend(
        [
            "/usr/share/terminfo",
            "/lib/terminfo",
            "/usr/lib/terminfo",
            "/usr/share/lib/terminfo",
            "/opt/homebrew/opt/ncurses/share/terminfo",
        ]
        .iter()
        .map(PathBuf::from),
    );
    dirs
}

/// Build the profile from an environment.
#[must_use]
pub fn profile(env: &TermEnv, dirs: &[PathBuf]) -> TermOut {
    let truecolor = env
        .colorterm
        .as_deref()
        .is_some_and(|c| c.contains("truecolor") || c.contains("24bit"));
    let entry_resolved = env
        .term
        .as_deref()
        .is_some_and(|t| terminfo_exists(t, dirs));

    let nested = if env.tmux.is_some() {
        Some("tmux".to_owned())
    } else if env.zellij.is_some() {
        Some("zellij".to_owned())
    } else if env.screen.is_some() {
        Some("screen".to_owned())
    } else {
        None
    };

    let mut disagreements = Vec::new();
    match env.term.as_deref() {
        None => disagreements.push(Disagreement {
            about: "TERM".to_owned(),
            advertised: "unset".to_owned(),
            observed: "a terminal is attached".to_owned(),
            remedy: "Set TERM. Without it programs fall back to the dumbest terminal they support."
                .to_owned(),
        }),
        Some(term) => {
            if !entry_resolved {
                disagreements.push(Disagreement {
                    about: "terminfo".to_owned(),
                    advertised: term.to_owned(),
                    observed: "no entry found".to_owned(),
                    remedy: format!(
                        "No terminfo entry for `{term}`. `omt term install` writes one, or set TERM to something this machine knows."
                    ),
                });
            }
            // The classic: 24-bit colour advertised by the terminal while TERM
            // promises 8. Nothing errors — the colours are just wrong, and it
            // is blamed on whatever drew them.
            if truecolor && !term.contains("256") && !term.contains("direct") {
                disagreements.push(Disagreement {
                    about: "colour".to_owned(),
                    advertised: term.to_owned(),
                    observed: "COLORTERM claims 24-bit".to_owned(),
                    remedy: "TERM promises fewer colours than the terminal supports. `xterm-256color` is usually right.".to_owned(),
                });
            }
        }
    }
    if let Some(outer) = &nested {
        disagreements.push(Disagreement {
            about: "nesting".to_owned(),
            advertised: "omt is the multiplexer".to_owned(),
            observed: format!("running inside {outer}"),
            remedy: format!(
                "omt is inside {outer}, so {outer} sees the prefix key first. Detach it, or change one of the two prefixes."
            ),
        });
    }

    TermOut {
        term: env.term.clone(),
        program: env.program.clone(),
        entry_resolved,
        truecolor,
        nested,
        disagreements,
    }
}

impl CapabilityHandler<TermProfile> for SetupHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: TermIn,
    ) -> Result<TermOut, CapabilityError> {
        Ok(profile(&TermEnv::from_process(), &terminfo_dirs()))
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
    fn every_agent_omt_can_drive_has_a_command() {
        // A kind with no command is one detection silently never finds.
        for (_, command) in COMMANDS {
            assert!(!command.is_empty());
        }
        assert!(COMMANDS.len() >= 10, "all the mainstream CLIs");
    }

    #[test]
    fn no_two_agents_share_a_command() {
        let mut seen = std::collections::BTreeSet::new();
        for (_, command) in COMMANDS {
            assert!(seen.insert(command), "`{command}` is listed twice");
        }
    }

    #[test]
    fn an_executable_is_found_on_a_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bin = dir.path().join("claude");
        std::fs::write(&bin, "#!/bin/sh\n").expect("write");
        let path = dir.path().display().to_string();
        assert_eq!(on_path("claude", &path), Some(bin));
        assert_eq!(on_path("codex", &path), None);
    }

    #[test]
    fn a_directory_is_not_an_executable() {
        // `PATH` entries that happen to contain a directory of the right name
        // would otherwise report an agent that cannot be run.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("claude")).expect("mkdir");
        assert_eq!(on_path("claude", &dir.path().display().to_string()), None);
    }

    #[test]
    fn detection_lists_every_agent_not_only_the_ones_present() {
        // Somebody whose agent is missing needs to see omt knows about it.
        let out = CapabilityHandler::<DetectAgents>::call(&SetupHandler, &ctx(), DetectIn {})
            .expect("detect");
        assert_eq!(out.agents.len(), COMMANDS.len());
        assert!(out.searched, "PATH is set in a test process");
    }

    fn ctx() -> omt_catalog::CallContext {
        omt_catalog::CallContext {
            actor: omt_types::Actor::Local,
            role: omt_types::Role::Viewer,
            request: omt_catalog::RequestId {
                device: omt_types::DeviceId::new(),
                n: 1,
            },
            intent: None,
        }
    }

    fn env(term: Option<&str>) -> TermEnv {
        TermEnv {
            term: term.map(str::to_owned),
            ..TermEnv::default()
        }
    }

    #[test]
    fn an_unset_term_is_a_disagreement_with_a_remedy() {
        let out = profile(&env(None), &[]);
        assert!(out.disagreements.iter().any(|d| d.about == "TERM"));
        assert!(out.disagreements.iter().all(|d| !d.remedy.is_empty()));
    }

    #[test]
    fn a_term_with_no_terminfo_entry_says_so() {
        let out = profile(&env(Some("nonsense-terminal")), &[]);
        assert!(!out.entry_resolved);
        assert!(out.disagreements.iter().any(|d| d.about == "terminfo"));
    }

    #[test]
    fn a_resolvable_term_reports_no_terminfo_complaint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let letter = dir.path().join("x");
        std::fs::create_dir_all(&letter).expect("mkdir");
        std::fs::write(letter.join("xterm-256color"), "").expect("write");
        let out = profile(&env(Some("xterm-256color")), &[dir.path().to_path_buf()]);
        assert!(out.entry_resolved);
        assert!(out.disagreements.iter().all(|d| d.about != "terminfo"));
    }

    #[test]
    fn the_hex_terminfo_layout_is_found_too() {
        // Some systems name the directory by the hex of the first byte. Missing
        // it reports "no terminfo entry" on a machine that has one.
        let dir = tempfile::tempdir().expect("tempdir");
        let hex = dir.path().join(format!("{:x}", 'x' as u32));
        std::fs::create_dir_all(&hex).expect("mkdir");
        std::fs::write(hex.join("xterm"), "").expect("write");
        assert!(terminfo_exists("xterm", &[dir.path().to_path_buf()]));
    }

    #[test]
    fn truecolor_under_a_term_that_promises_less_is_reported() {
        // Nothing errors. The colours are just wrong, and it is blamed on
        // whatever drew them.
        let mut e = env(Some("xterm"));
        e.colorterm = Some("truecolor".to_owned());
        let out = profile(&e, &[]);
        assert!(out.truecolor);
        assert!(out.disagreements.iter().any(|d| d.about == "colour"));
    }

    #[test]
    fn truecolor_under_a_256_term_is_not_complained_about() {
        let mut e = env(Some("xterm-256color"));
        e.colorterm = Some("truecolor".to_owned());
        let out = profile(&e, &[]);
        assert!(out.disagreements.iter().all(|d| d.about != "colour"));
    }

    #[test]
    fn running_inside_tmux_is_said_plainly() {
        // The single most common reason a prefix key appears not to work, and
        // somebody in that situation cannot tell from the screen.
        let mut e = env(Some("screen-256color"));
        e.tmux = Some("/tmp/tmux-501/default,123,0".to_owned());
        let out = profile(&e, &[]);
        assert_eq!(out.nested.as_deref(), Some("tmux"));
        let nesting = out
            .disagreements
            .iter()
            .find(|d| d.about == "nesting")
            .expect("said");
        assert!(nesting.remedy.contains("prefix"));
    }

    #[test]
    fn zellij_and_screen_are_recognised_as_well() {
        let mut e = env(Some("xterm-256color"));
        e.zellij = Some("0".to_owned());
        assert_eq!(profile(&e, &[]).nested.as_deref(), Some("zellij"));
        let mut e = env(Some("screen"));
        e.screen = Some("1234.pts-0".to_owned());
        assert_eq!(profile(&e, &[]).nested.as_deref(), Some("screen"));
    }

    #[test]
    fn a_healthy_terminal_has_nothing_to_say() {
        // The common case has to be silent, or the ones that matter are noise.
        let dir = tempfile::tempdir().expect("tempdir");
        let letter = dir.path().join("x");
        std::fs::create_dir_all(&letter).expect("mkdir");
        std::fs::write(letter.join("xterm-256color"), "").expect("write");
        let mut e = env(Some("xterm-256color"));
        e.colorterm = Some("truecolor".to_owned());
        let out = profile(&e, &[dir.path().to_path_buf()]);
        assert_eq!(out.disagreements.len(), 0, "{:?}", out.disagreements);
    }

    #[test]
    fn every_disagreement_carries_a_remedy() {
        // A diagnostic that names a problem and not its fix is one people learn
        // to scroll past.
        let mut e = env(None);
        e.tmux = Some("x".to_owned());
        e.colorterm = Some("truecolor".to_owned());
        let out = profile(&e, &[]);
        assert!(!out.disagreements.is_empty());
        for d in &out.disagreements {
            assert!(!d.remedy.is_empty(), "{d:?}");
        }
    }
}
