//! Writing omt's hook into an agent's own configuration.
//!
//! This edits a file the user owns and other tools also write to. Three rules
//! follow, and each is a test:
//!
//! **Merge, never overwrite.** People already have hooks. A config that arrived
//! with omt's and lost theirs is a bug they discover when something they rely
//! on stops running, with nothing pointing at omt.
//!
//! **Every artifact is stamped**, so a stale install is detectable and an
//! uninstall can remove exactly what was added rather than everything matching
//! a guess.
//!
//! **Every install is reversible and previewable.** A tool that edits your
//! configuration and cannot show you what it will do first is a tool people are
//! right not to run.
//!
//! **Known limitation: JSON formatting is normalized.** The document is parsed
//! and re-rendered, so indentation and key order become omt's rather than the
//! user's, and any comments a JSONC file carried are lost. Content is
//! preserved exactly — that is what the tests hold — but the diff is larger
//! than the change. Doing better needs a concrete-syntax-tree editor for JSONC,
//! the way the TOML path already has one; this is recorded here rather than
//! discovered by somebody whose carefully commented settings came back
//! reformatted.

use omt_types::AgentKind;
use serde_json::{Map, Value};

/// The key every artifact omt writes carries.
///
/// Its presence is what makes an entry omt's. Without it, uninstall has to
/// guess by matching a command string — and would remove a hook the user wrote
/// that happened to look similar.
pub const STAMP_KEY: &str = "omtInstalledVersion";

/// The version stamped into artifacts by this build.
pub const INSTALL_VERSION: &str = "1";

/// What an install would do, before it does it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The file that would change.
    pub path: String,
    /// What it looks like now.
    pub before: String,
    /// What it would look like after.
    pub after: String,
    /// Whether anything would actually change.
    pub changes: bool,
}

impl Plan {
    /// A unified-ish diff, for `--dry-run`.
    #[must_use]
    pub fn diff(&self) -> String {
        let before: Vec<&str> = self.before.lines().collect();
        let after: Vec<&str> = self.after.lines().collect();
        let mut out = String::new();
        for line in &before {
            if !after.contains(line) {
                out.push_str(&format!("- {line}\n"));
            }
        }
        for line in &after {
            if !before.contains(line) {
                out.push_str(&format!("+ {line}\n"));
            }
        }
        out
    }
}

/// Why an install could not be planned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
    /// The existing configuration could not be parsed.
    ///
    /// Refused rather than replaced: rewriting a file somebody is in the middle
    /// of fixing destroys the thing they were fixing.
    #[error("`{path}` is not valid JSON, so omt will not rewrite it: {detail}")]
    Unparseable {
        /// Which file.
        path: String,
        /// What the parser said.
        detail: String,
    },
    /// The file's shape is not what an agent config looks like.
    #[error("`{path}` is not shaped like a {agent:?} configuration")]
    UnexpectedShape {
        /// Which file.
        path: String,
        /// Which agent's config it should have been.
        agent: AgentKind,
    },
}

/// Plan an install into an existing configuration.
///
/// # Errors
/// Fails if the existing file cannot be parsed or is not the expected shape.
pub fn plan_install(
    path: &str,
    existing: &str,
    agent: AgentKind,
    hook_command: &str,
) -> Result<Plan, InstallError> {
    let mut root = parse_root(path, existing)?;

    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err(InstallError::UnexpectedShape {
            path: path.to_owned(),
            agent,
        });
    };

    for event in events_for(agent) {
        let entry = hooks
            .entry((*event).to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(list) = entry.as_array_mut() else {
            return Err(InstallError::UnexpectedShape {
                path: path.to_owned(),
                agent,
            });
        };
        // Replace omt's own entry rather than appending another. Installing
        // twice must not leave two hooks that both fire.
        list.retain(|v| !is_ours(v));
        list.push(our_entry(hook_command, event));
    }

    let after = render(&root);
    Ok(Plan {
        path: path.to_owned(),
        before: existing.to_owned(),
        changes: after.trim() != existing.trim(),
        after,
    })
}

/// Plan an uninstall.
///
/// # Errors
/// Fails if the existing file cannot be parsed.
pub fn plan_uninstall(path: &str, existing: &str) -> Result<Plan, InstallError> {
    let mut root = parse_root(path, existing)?;

    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for entry in hooks.values_mut() {
            if let Some(list) = entry.as_array_mut() {
                // Only what omt stamped. Matching on the command string would
                // remove a hook the user wrote that happened to look similar.
                list.retain(|v| !is_ours(v));
            }
        }
        // An event left with no hooks is removed, so uninstalling returns the
        // file to what it looked like rather than leaving empty scaffolding.
        hooks.retain(|_, v| !v.as_array().is_some_and(std::vec::Vec::is_empty));
        let empty = hooks.is_empty();
        if empty {
            root.remove("hooks");
        }
    }

    let after = render(&root);
    Ok(Plan {
        path: path.to_owned(),
        before: existing.to_owned(),
        changes: after.trim() != existing.trim(),
        after,
    })
}

/// Whether an installed artifact was written by an older build.
#[must_use]
pub fn is_stale(existing: &str) -> bool {
    let Ok(root) = serde_json::from_str::<Value>(existing) else {
        return false;
    };
    root.get("hooks")
        .and_then(|h| h.as_object())
        .into_iter()
        .flat_map(serde_json::Map::values)
        .filter_map(|v| v.as_array())
        .flatten()
        .filter(|v| is_ours(v))
        .any(|v| v.get(STAMP_KEY).and_then(|s| s.as_str()) != Some(INSTALL_VERSION))
}

fn parse_root(path: &str, existing: &str) -> Result<Map<String, Value>, InstallError> {
    if existing.trim().is_empty() {
        return Ok(Map::new());
    }
    let value: Value = serde_json::from_str(existing).map_err(|e| InstallError::Unparseable {
        path: path.to_owned(),
        detail: e.to_string(),
    })?;
    value.as_object().cloned().ok_or(InstallError::Unparseable {
        path: path.to_owned(),
        detail: "the top level is not an object".to_owned(),
    })
}

fn is_ours(value: &Value) -> bool {
    value.get(STAMP_KEY).is_some()
}

fn our_entry(command: &str, event: &str) -> Value {
    serde_json::json!({
        STAMP_KEY: INSTALL_VERSION,
        "matcher": "*",
        "hooks": [{
            "type": "command",
            // The event travels as an argument so the hook does not have to
            // infer which one fired from its payload.
            "command": format!("{command} --agent claude-code --event {event}"),
        }],
    })
}

/// The events omt asks an agent to notify it about.
#[must_use]
pub fn events_for(agent: AgentKind) -> &'static [&'static str] {
    match agent {
        AgentKind::ClaudeCode => &[
            "SessionStart",
            "SessionEnd",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "Stop",
            "Notification",
        ],
        // Everything else either has no hook system or is reached over a
        // protocol. Installing nothing is the honest outcome.
        _ => &[],
    }
}

fn render(root: &Map<String, Value>) -> String {
    let mut out = serde_json::to_string_pretty(&Value::Object(root.clone()))
        .unwrap_or_else(|_| "{}".to_owned());
    out.push('\n');
    out
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    const CMD: &str = "/usr/local/bin/omt-hook";

    fn install(existing: &str) -> Plan {
        plan_install(
            "~/.claude/settings.json",
            existing,
            AgentKind::ClaudeCode,
            CMD,
        )
        .expect("plan")
    }

    #[test]
    fn an_existing_hook_survives_the_install() {
        // The bug people discover when something they rely on stops running,
        // with nothing pointing at omt.
        let before = r#"{
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-own-audit" }] }
    ]
  }
}"#;
        let plan = install(before);
        assert!(
            plan.after.contains("my-own-audit"),
            "the user's hook was destroyed:\n{}",
            plan.after
        );
        assert!(plan.after.contains("omt-hook"));
    }

    #[test]
    fn unrelated_settings_survive_the_install() {
        let before = r#"{ "theme": "dark", "model": "opus" }"#;
        let plan = install(before);
        assert!(plan.after.contains("\"theme\""));
        assert!(plan.after.contains("\"model\""));
    }

    #[test]
    fn installing_twice_does_not_leave_two_hooks() {
        // Both would fire, and the agent would pay the cost twice per event.
        let once = install("{}");
        let twice = install(&once.after);
        assert_eq!(
            twice.after.matches("omt-hook").count(),
            once.after.matches("omt-hook").count()
        );
        assert!(!twice.changes, "a second install changed something");
    }

    #[test]
    fn every_artifact_is_stamped() {
        // Without it, uninstall has to guess by matching a command string, and
        // would remove a hook the user wrote that looked similar.
        let plan = install("{}");
        assert!(plan.after.contains(STAMP_KEY), "{}", plan.after);
    }

    #[test]
    fn uninstall_removes_exactly_what_omt_added() {
        let before = r#"{
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-own-audit" }] }
    ]
  }
}"#;
        let installed = install(before);
        let removed = plan_uninstall("~/.claude/settings.json", &installed.after).expect("plan");
        assert!(removed.after.contains("my-own-audit"), "{}", removed.after);
        assert!(!removed.after.contains("omt-hook"));
        assert!(!removed.after.contains(STAMP_KEY));
    }

    #[test]
    fn uninstalling_leaves_no_empty_scaffolding() {
        // The file should come back looking like it did, not like something
        // was removed from it.
        let installed = install("{}");
        let removed = plan_uninstall("~/.claude/settings.json", &installed.after).expect("plan");
        assert!(
            !removed.after.contains("hooks"),
            "an empty hooks block was left behind:\n{}",
            removed.after
        );
    }

    #[test]
    fn uninstalling_something_never_installed_changes_nothing() {
        let before = "{\n  \"theme\": \"dark\"\n}\n";
        let removed = plan_uninstall("~/.claude/settings.json", before).expect("plan");
        assert!(!removed.changes);
    }

    #[test]
    fn a_broken_config_is_refused_rather_than_rewritten() {
        // Rewriting a file somebody is in the middle of fixing destroys the
        // thing they were fixing.
        let err = plan_install(
            "~/.claude/settings.json",
            "{ not json",
            AgentKind::ClaudeCode,
            CMD,
        )
        .expect_err("must refuse");
        assert!(matches!(err, InstallError::Unparseable { .. }), "{err:?}");
    }

    #[test]
    fn an_empty_file_is_a_fresh_install_not_an_error() {
        // A first install is the normal case.
        let plan = install("");
        assert!(plan.changes);
        assert!(plan.after.contains("omt-hook"));
    }

    #[test]
    fn a_plan_can_be_previewed_before_anything_is_written() {
        // A tool that edits your configuration and cannot show you what it
        // will do first is one people are right not to run.
        let plan = install(r#"{ "theme": "dark" }"#);
        let diff = plan.diff();
        assert!(diff.contains('+'), "{diff}");
        assert!(diff.contains("omt-hook"), "{diff}");
    }

    #[test]
    fn reformatting_changes_the_layout_but_never_the_content() {
        // The known limitation, pinned so it stays a limitation rather than
        // quietly becoming data loss: the diff is larger than the change, but
        // every value the user had is still there.
        let before = r#"{"theme":"dark","model":"opus","permissions":{"allow":["Bash"]}}"#;
        let plan = install(before);
        let after: serde_json::Value = serde_json::from_str(&plan.after).expect("still valid json");
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["model"], "opus");
        assert_eq!(after["permissions"]["allow"][0], "Bash");
    }

    #[test]
    fn an_artifact_from_an_older_build_is_detectable() {
        let old = format!(
            r#"{{ "hooks": {{ "PreToolUse": [ {{ "{STAMP_KEY}": "0", "hooks": [] }} ] }} }}"#
        );
        assert!(is_stale(&old));
        assert!(!is_stale(&install("{}").after));
    }

    #[test]
    fn an_agent_with_no_hook_system_gets_nothing_installed() {
        // Installing nothing is the honest outcome, rather than writing a file
        // the agent will never read.
        assert!(events_for(AgentKind::Goose).is_empty());
        let plan = plan_install("~/.config/goose.json", "{}", AgentKind::Goose, CMD).expect("plan");
        assert!(!plan.after.contains("omt-hook"));
    }

    #[test]
    fn the_hook_is_told_which_event_fired() {
        // So it does not have to infer it from the payload, which is the one
        // thing a hook should never guess.
        let plan = install("{}");
        assert!(plan.after.contains("--event PreToolUse"), "{}", plan.after);
    }
}
