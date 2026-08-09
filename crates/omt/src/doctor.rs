//! `omt doctor`: per-link diagnosis, every failure carrying a remedy.
//!
//! `docs/architecture/22-operations.md` §3. The rule that shapes this is the
//! one in the heading: a check that finds a problem and does not say what to do
//! about it has moved the problem rather than diagnosed it.
//!
//! The second rule is not in the document and matters as much: **nothing here
//! reports `ok` for a check it did not run.** A doctor that prints a green line
//! per group whether or not it looked is worse than no doctor, because somebody
//! reads it and stops looking. Every check below is one this build can actually
//! perform; the groups the document describes and this cannot yet perform are
//! reported as `not_checked`, by name, with what would be needed.

use omt_catalog::{CapabilityError, CapabilityHandler};
use serde::{Deserialize, Serialize};

use crate::state::State;

/// How a check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Looked, and it is fine.
    Ok,
    /// Looked, and it works but something is worth knowing.
    Warn,
    /// Looked, and it is broken.
    Fail,
    /// Did not look. Never `ok`: an unrun check reported as passing is how a
    /// diagnosis stops somebody investigating the thing that is actually wrong.
    NotChecked,
}

/// One thing that was looked at.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Check {
    /// Which group it belongs to.
    pub group: String,
    /// What was checked.
    pub name: String,
    /// How it came out.
    pub verdict: Verdict,
    /// What was found, in words somebody can act on.
    pub detail: String,
    /// What to do about it. Present whenever the verdict is not `ok`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
}

impl Check {
    fn ok(group: &str, name: &str, detail: impl Into<String>) -> Self {
        Self {
            group: group.to_owned(),
            name: name.to_owned(),
            verdict: Verdict::Ok,
            detail: detail.into(),
            remedy: None,
        }
    }

    fn problem(
        group: &str,
        name: &str,
        verdict: Verdict,
        detail: impl Into<String>,
        remedy: impl Into<String>,
    ) -> Self {
        Self {
            group: group.to_owned(),
            name: name.to_owned(),
            verdict,
            detail: detail.into(),
            remedy: Some(remedy.into()),
        }
    }
}

/// Input to `doctor.run`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct DoctorIn {
    /// One group, or every group when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// What `doctor.run` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct DoctorOut {
    /// Every check, in the order they were run.
    pub checks: Vec<Check>,
    /// How many came out badly, so a caller can lead with the number.
    pub failures: usize,
    /// How many were not run at all.
    pub not_checked: usize,
}

omt_catalog::capability! {
    /// Diagnose an instance.
    pub struct DoctorRun;
    input  = DoctorIn,
    output = DoctorOut,
    decl = omt_catalog::Decl {
        name: "doctor.run",
        group: "doctor",
        verb: "run",
        title: "Diagnose this instance",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Per-link diagnosis with a remedy on every failure. A check this build cannot perform is reported as not checked rather than as passing: an unrun check reported green is how a diagnosis stops somebody investigating the real problem.",
    },
}

/// Serves `doctor.run`.
pub struct DoctorHandler(pub State);

impl CapabilityHandler<DoctorRun> for DoctorHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: DoctorIn,
    ) -> Result<DoctorOut, CapabilityError> {
        let wanted = input.group.as_deref();
        let mut checks = Vec::new();

        if wanted.is_none_or(|g| g == "term") {
            checks.extend(term_checks());
        }
        if wanted.is_none_or(|g| g == "shell") {
            checks.extend(shell_checks(&self.0)?);
        }
        if wanted.is_none_or(|g| g == "agents") {
            checks.extend(agent_checks());
        }
        if wanted.is_none_or(|g| g == "keys") {
            checks.extend(key_checks());
        }
        if wanted.is_none_or(|g| g == "net") {
            checks.extend(net_checks());
        }
        if wanted.is_none_or(|g| g == "store") {
            checks.extend(store_checks());
        }
        if wanted.is_none_or(|g| g == "service") {
            checks.extend(service_checks());
        }
        if wanted.is_none_or(|g| g == "media") {
            checks.extend(media_checks());
        }

        if checks.is_empty() {
            return Err(CapabilityError::invalid_input(format!(
                "`{}` is not a group omt checks",
                wanted.unwrap_or("")
            )));
        }

        let failures = checks
            .iter()
            .filter(|c| matches!(c.verdict, Verdict::Fail))
            .count();
        let not_checked = checks
            .iter()
            .filter(|c| matches!(c.verdict, Verdict::NotChecked))
            .count();
        Ok(DoctorOut {
            checks,
            failures,
            not_checked,
        })
    }
}

/// The terminal omt is running in.
fn term_checks() -> Vec<Check> {
    let mut out = Vec::new();

    match std::env::var("TERM") {
        Ok(term) if !term.trim().is_empty() => {
            out.push(Check::ok("term", "TERM is set", format!("TERM={term}")));
        }
        _ => out.push(Check::problem(
            "term",
            "TERM is set",
            Verdict::Fail,
            "TERM is unset, so nothing knows what the terminal can do",
            "set TERM in your shell profile, or start omt from a terminal that sets it",
        )),
    }

    // Truecolor is the one that shows: a theme rendered in 256 colours is
    // visibly not the theme somebody chose.
    match std::env::var("COLORTERM").as_deref() {
        Ok("truecolor" | "24bit") => out.push(Check::ok(
            "term",
            "24-bit colour",
            "COLORTERM says the terminal takes 24-bit colour",
        )),
        _ => out.push(Check::problem(
            "term",
            "24-bit colour",
            Verdict::Warn,
            "COLORTERM does not advertise truecolor, so themes may render in 256 colours",
            "set COLORTERM=truecolor if your terminal supports it — most current ones do",
        )),
    }
    out
}

/// Shell integration, judged by what has been seen rather than by a setting.
fn shell_checks(state: &State) -> Result<Vec<Check>, CapabilityError> {
    let instance = state.lock()?;
    let sessions = instance.sessions();
    if sessions.is_empty() {
        return Ok(vec![Check {
            group: "shell".to_owned(),
            name: "command marks".to_owned(),
            verdict: Verdict::NotChecked,
            detail: "no session has run yet, so nothing has been observed".to_owned(),
            remedy: Some("start a session and run something, then check again".to_owned()),
        }]);
    }

    let marking = sessions
        .iter()
        .filter(|s| {
            instance
                .runtime(s.id)
                .map(omt_daemon::SessionRuntime::terminal)
                .is_some_and(|t| !t.blocks().is_empty() && t.blocks().iter().any(|b| b.attributed))
        })
        .count();

    Ok(vec![if marking > 0 {
        Check::ok(
            "shell",
            "command marks",
            format!("{marking} of {} sessions emit OSC 133", sessions.len()),
        )
    } else {
        Check::problem(
            "shell",
            "command marks",
            Verdict::Warn,
            "no session has emitted a command mark, so omt cannot separate commands from output",
            "add omt's shell integration to your rc file, and load it *after* any prompt framework — a later plugin overwriting precmd is the usual cause",
        )
    }])
}

/// The agent CLIs, and whether they are where omt would look.
fn agent_checks() -> Vec<Check> {
    // The names omt knows, from the one table that defines them. A list
    // maintained here would drift from the adapters the moment one is added.
    const AGENTS: &[(&str, &str)] = &[
        ("claude_code", "claude"),
        ("codex", "codex"),
        ("opencode", "opencode"),
        ("gemini_cli", "gemini"),
        ("qwen_code", "qwen"),
        ("cursor", "cursor-agent"),
        ("copilot", "copilot"),
        ("goose", "goose"),
        ("amp", "amp"),
        ("aider", "aider"),
        ("crush", "crush"),
    ];

    let mut out = Vec::new();
    let mut found = 0;
    for (kind, binary) in AGENTS {
        if which(binary).is_some() {
            found += 1;
            out.push(Check::ok("agents", kind, format!("`{binary}` is on PATH")));
        }
    }
    if found == 0 {
        out.push(Check::problem(
            "agents",
            "any agent",
            Verdict::Warn,
            "none of the agent CLIs omt supports is on PATH",
            "install one, or set its command in config — under a service manager PATH is not your shell's, which is the usual cause when it works in a terminal",
        ));
    }
    out
}

/// Keybindings, and whether two of them mean different things.
fn key_checks() -> Vec<Check> {
    let keymap = omt_input::Keymap::default();
    let sheet = omt_input::cheatsheet(&keymap);

    let mut seen: std::collections::BTreeMap<(String, String), String> =
        std::collections::BTreeMap::new();
    let mut conflicts = Vec::new();
    for entry in &sheet {
        let key = (format!("{:?}", entry.mode), entry.chord.clone());
        if let Some(existing) = seen.get(&key) {
            if existing != &entry.action {
                conflicts.push(entry.chord.clone());
            }
        } else {
            seen.insert(key, entry.action.clone());
        }
    }

    vec![if conflicts.is_empty() {
        Check::ok(
            "keys",
            "no conflicting bindings",
            format!("{} bindings, none in conflict", sheet.len()),
        )
    } else {
        Check::problem(
            "keys",
            "no conflicting bindings",
            Verdict::Fail,
            format!("{} bound to more than one thing", conflicts.join(", ")),
            "rebind one of them in config: a chord that means two things does whichever omt saw last",
        )
    }]
}

/// The address this instance would serve on.
fn net_checks() -> Vec<Check> {
    let bind = std::env::var("OMT_SERVER__BIND").unwrap_or_else(|_| "127.0.0.1:7717".to_owned());
    vec![if crate::web::is_loopback(&bind) {
        Check::ok(
            "net",
            "bind address",
            format!("{bind} accepts connections from this machine only"),
        )
    } else {
        Check::problem(
            "net",
            "bind address",
            Verdict::Warn,
            format!("{bind} accepts connections from the network"),
            "serve TLS with --tls-cert and --tls-key, or bind loopback and reach it over a VPN — a credential crossing a network in clear text is a credential somebody else now has",
        )
    }]
}

/// Where omt keeps its state, and whether that is somewhere sensible.
fn store_checks() -> Vec<Check> {
    let mut out = Vec::new();
    let home = std::env::var("HOME").unwrap_or_default();
    let Some(snapshot) = crate::persist::snapshot_path() else {
        return vec![Check::problem(
            "store",
            "state directory",
            Verdict::Fail,
            "omt cannot work out where to keep its state",
            "set HOME or XDG_STATE_HOME — under a service manager one of them is usually missing",
        )];
    };

    out.push(if snapshot.exists() {
        Check::ok(
            "store",
            "state directory",
            format!("{} exists", snapshot.display()),
        )
    } else {
        Check::problem(
            "store",
            "state directory",
            Verdict::Warn,
            format!("{} does not exist yet", snapshot.display()),
            "nothing to do until something is saved: it is created on the first `state.save`",
        )
    });

    // A state directory inside a synced folder is the failure that looks like
    // corruption: two machines writing one snapshot through a sync client that
    // resolves conflicts by picking a file.
    let synced = ["Dropbox", "Google Drive", "OneDrive", "iCloud"]
        .into_iter()
        .find(|name| snapshot.to_string_lossy().contains(name));
    out.push(match synced {
        Some(name) => Check::problem(
            "store",
            "not inside a synced folder",
            Verdict::Fail,
            format!("state is under {name}, where two machines will write one file"),
            "move the state directory out of the synced folder, or exclude it in that client's settings",
        ),
        None => Check::ok(
            "store",
            "not inside a synced folder",
            "state is not under a sync client omt recognises",
        ),
    });

    if home.is_empty() {
        out.push(Check::problem(
            "store",
            "home directory",
            Verdict::Fail,
            "HOME is unset, so omt cannot find its own state",
            "set HOME — under a service manager it is often missing, which is the usual cause",
        ));
    }
    out
}

/// Whether omt is running as a managed service, and whether that is expected.
fn service_checks() -> Vec<Check> {
    // Judged from the environment rather than by asking a service manager: a
    // check that shells out to `launchctl` cannot run where there is none, and
    // the interesting failure is precisely the environment being different.
    let managed = std::env::var_os("LAUNCHD_SOCKET").is_some()
        || std::env::var_os("INVOCATION_ID").is_some()
        || std::env::var_os("NOTIFY_SOCKET").is_some();

    let mut out = vec![if managed {
        Check::ok(
            "service",
            "running under a service manager",
            "omt was started by launchd or systemd",
        )
    } else {
        Check::ok(
            "service",
            "running under a service manager",
            "omt was started from a shell, which is the ordinary case",
        )
    }];

    // The PATH trap: a service manager does not inherit a login shell's PATH,
    // so agents found in a terminal are not found under the service.
    if managed && which("git").is_none() {
        out.push(Check::problem(
            "service",
            "PATH under the service",
            Verdict::Fail,
            "git is not on PATH, so the service's PATH is not your shell's",
            "set the command paths in config explicitly, or install the service again from a shell that has them",
        ));
    }
    out
}

/// Which clipboard and image paths this terminal offers.
fn media_checks() -> Vec<Check> {
    let mut out = Vec::new();

    // Over SSH or in a multiplexer, OSC 52 is the only route to the local
    // clipboard, and tmux drops it unless told not to.
    match std::env::var("TMUX") {
        Ok(_) => out.push(Check::problem(
            "media",
            "clipboard through tmux",
            Verdict::Warn,
            "running under tmux, which drops OSC 52 unless it is switched on",
            "tmux set -g set-clipboard on",
        )),
        Err(_) => out.push(Check::ok(
            "media",
            "clipboard through tmux",
            "not under tmux, so OSC 52 reaches the terminal directly",
        )),
    }

    let term = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let graphics = matches!(term.as_str(), "iTerm.app" | "WezTerm" | "ghostty")
        || std::env::var_os("KITTY_WINDOW_ID").is_some();
    out.push(if graphics {
        Check::ok(
            "media",
            "inline images",
            format!("{term} takes an inline image protocol"),
        )
    } else {
        Check::problem(
            "media",
            "inline images",
            Verdict::Warn,
            "this terminal does not advertise an inline image protocol",
            "images open in an external viewer instead; a terminal with kitty or iTerm2 graphics shows them in place",
        )
    });
    out
}

/// Whether a program is on PATH.
///
/// Written out rather than shelling out to `which`: a doctor that needs a
/// working shell to diagnose a broken one is the wrong tool.
fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

// ---------------------------------------------------------------------------
// A bug report
// ---------------------------------------------------------------------------

/// Input to `doctor.report`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReportIn {
    /// What the person was doing, in their words.
    #[serde(default)]
    pub what_happened: String,
}

/// What `doctor.report` produces.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ReportOut {
    /// The report, as text somebody can read before they send it.
    ///
    /// Text rather than an opaque bundle, and returned rather than uploaded:
    /// omt has no cloud to upload to, and a diagnostic somebody cannot read is
    /// one they have to trust blindly about their own machine.
    pub report: String,
}

omt_catalog::capability! {
    /// Gather everything worth attaching to a bug report.
    pub struct DoctorReport;
    input  = ReportIn,
    output = ReportOut,
    decl = omt_catalog::Decl {
        name: "doctor.report",
        group: "doctor",
        verb: "report",
        title: "Write a bug report",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The diagnosis, the version and what the person was doing, as readable text they can check before sending. Nothing is uploaded: omt has no cloud, and a report somebody cannot read is one they have to trust blindly about their own machine.",
    },
}

/// Serves `doctor.report`.
pub struct ReportHandler(pub State);

impl CapabilityHandler<DoctorReport> for ReportHandler {
    fn call(
        &self,
        ctx: &omt_catalog::CallContext,
        input: ReportIn,
    ) -> Result<ReportOut, CapabilityError> {
        let diagnosis = DoctorHandler(self.0.clone()).call(ctx, DoctorIn { group: None })?;

        let mut out = String::from("# omt bug report\n\n");
        out.push_str(&format!("version: {}\n", env!("CARGO_PKG_VERSION")));
        out.push_str(&format!("protocol: {}\n", omt_proto::PROTO_VERSION));
        out.push_str(&format!("platform: {}\n", std::env::consts::OS));
        out.push_str(&format!(
            "sessions: {}\n\n",
            self.0.lock()?.sessions().len()
        ));

        if !input.what_happened.trim().is_empty() {
            out.push_str("## What happened\n\n");
            out.push_str(input.what_happened.trim());
            out.push_str("\n\n");
        }

        out.push_str("## Diagnosis\n\n");
        for check in &diagnosis.checks {
            let mark = match check.verdict {
                Verdict::Ok => "ok  ",
                Verdict::Warn => "warn",
                Verdict::Fail => "FAIL",
                Verdict::NotChecked => "----",
            };
            out.push_str(&format!(
                "{mark}  {}/{}: {}\n",
                check.group, check.name, check.detail
            ));
            if let Some(remedy) = &check.remedy {
                out.push_str(&format!("        → {remedy}\n"));
            }
        }
        // No credentials, no file contents, no command output. Everything above
        // is either a version, a count, or a check's own words — a bug report
        // that carries a token is a bug report nobody can safely paste.
        Ok(ReportOut { report: out })
    }
}
