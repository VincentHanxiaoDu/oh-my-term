//! Installing omt's hooks must not cost somebody theirs.
//!
//! omt edits a file the agent owns and that people put their own hooks into.
//! The plan is rendered as a whole-file diff, so a reformat shows as deletions
//! — which looks alarming and proves nothing either way. What matters is not
//! what the diff *looks* like but whether every command that was in the file
//! is still in it, and that is what this asserts.
//!
//! Run against the real settings file on this machine when there is one,
//! because the shapes people actually have are the ones a hand-written fixture
//! never covers: nested matchers, timeouts, commands with quotes and shell
//! conditionals in them.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "in a test, expect() is the assertion"
)]

use omt_agent_adapters::install::plan_install;
use omt_types::AgentKind;

/// Every `"command": "..."` value in a settings document.
fn commands(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    walk(&value, &mut found);
    found.sort();
    found
}

fn walk(value: &serde_json::Value, found: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "command"
                    && let Some(text) = child.as_str()
                {
                    found.push(text.to_owned());
                }
                walk(child, found);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                walk(item, found);
            }
        }
        _ => {}
    }
}

#[test]
fn every_hook_that_was_there_is_still_there() {
    let existing = r#"{
      "agentPushNotifEnabled": true,
      "hooks": {
        "SessionStart": [
          {
            "hooks": [
              {
                "command": "if [ -f '/somewhere/statusline.sh' ]; then bash '/somewhere/statusline.sh'; fi",
                "type": "command",
                "timeout": 10
              }
            ]
          }
        ],
        "PreToolUse": [
          { "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-own-audit" }] }
        ]
      }
    }"#;

    let plan = plan_install(
        "settings.json",
        existing,
        AgentKind::ClaudeCode,
        "/usr/local/bin/omt-hook",
    )
    .expect("a plan");

    let before = commands(existing);
    let after = commands(&plan.after);
    for command in &before {
        assert!(
            after.contains(command),
            "installing omt's hooks removed `{command}`"
        );
    }
    // And it added its own.
    assert!(
        after.iter().any(|c| c.contains("omt-hook")),
        "omt's own hook is not there: {after:?}"
    );
    // Other settings survive too: a file that lost `agentPushNotifEnabled`
    // would be a file that lost whatever else somebody had configured.
    assert!(plan.after.contains("agentPushNotifEnabled"));
}

#[test]
fn installing_twice_leaves_one_hook_rather_than_two() {
    // Two hooks that both fire is two of every event, which is a card shown
    // twice and an interaction answered against the wrong id.
    let once = plan_install("s.json", "{}", AgentKind::ClaudeCode, "omt-hook").expect("first");
    let twice =
        plan_install("s.json", &once.after, AgentKind::ClaudeCode, "omt-hook").expect("second");
    assert_eq!(
        commands(&once.after)
            .iter()
            .filter(|c| c.contains("omt-hook"))
            .count(),
        commands(&twice.after)
            .iter()
            .filter(|c| c.contains("omt-hook"))
            .count(),
    );
    assert!(!twice.changes, "a second install rewrote the file");
}

#[test]
fn the_real_settings_file_on_this_machine_keeps_its_hooks() {
    // The shapes people actually have are the ones a fixture never covers.
    // Skipped where there is no such file, which is every machine that has
    // never run the agent.
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let path = std::path::Path::new(&home).join(".claude/settings.json");
    let Ok(existing) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no settings file at {}", path.display());
        return;
    };

    let plan = plan_install(
        &path.display().to_string(),
        &existing,
        AgentKind::ClaudeCode,
        "/usr/local/bin/omt-hook",
    )
    .expect("a plan for the real file");

    let before = commands(&existing);
    let after = commands(&plan.after);
    assert!(!before.is_empty(), "the fixture has no hooks to lose");
    for command in &before {
        assert!(
            after.contains(command),
            "installing omt's hooks would remove `{command}` from {}",
            path.display()
        );
    }
}
