//! Gemini CLI.
//!
//! ACP over `gemini --acp`, and its own hooks. The hook names are the finding
//! that made this adapter worth writing separately: the research said Gemini's
//! hooks were "Claude-Code-compatible by design", and the file on this machine
//! says otherwise. Gemini's native vocabulary is `BeforeAgent`, `AfterAgent`,
//! `BeforeTool`, `AfterTool` — four events, none of them spelled the way Claude
//! Code spells them.
//!
//! `gemini hooks migrate` translates a Claude configuration across, which is
//! presumably where "compatible" came from. Both spellings are accepted here,
//! because a user who migrated has one and a user who did not has the other,
//! and an adapter that only knew one would work for exactly half of them.

use std::ffi::OsString;

use omt_events::{AgentPayload, FileChange, MessageOrigin, StartReason, TurnOutcome, TurnTrigger};
use omt_types::{AgentKind, Tier};

use crate::adapter::{
    AcpSpawn, AdapterError, AgentAdapter, Fingerprint, Interrupt, SessionModeSet, SpawnCtx,
};

/// The Gemini CLI adapter.
#[derive(Debug, Clone, Copy)]
pub struct Gemini;

impl AgentAdapter for Gemini {
    fn screen_rules(&self) -> Vec<crate::rules::Rule> {
        use crate::regions::Region;
        use crate::rules::{Rule, ScreenState, Test};
        vec![
            // Its first-run trust dialog, drawn as a numbered list inside a
            // box. Captured from the shipped binary rather than written from
            // memory — the wording is theirs and this is the screen somebody
            // meets before any other.
            Rule {
                id: "the trust dialog",
                region: Region::BottomNonEmpty(12),
                test: Test::All(vec![
                    Test::Contains(vec!["trust folder"]),
                    Test::Any(vec![
                        Test::Contains(vec!["don't trust"]),
                        Test::Contains(vec!["trust parent folder"]),
                    ]),
                ]),
                state: ScreenState::Blocked,
                priority: 900,
            },
        ]
    }

    fn kind(&self) -> AgentKind {
        AgentKind::GeminiCli
    }

    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            env_markers: vec!["GEMINI_CLI", "GEMINI_API_KEY", "GEMINI_SANDBOX"],
            exe_names: vec!["gemini"],
            argv_patterns: vec!["@google/gemini-cli", "gemini-cli/index.js"],
            session_id_var: Some("GEMINI_SESSION_ID"),
            title: Some(crate::adapter::TitleMarks {
                // Its title carries a state and not a name: `◇ Ready (dir)`
                // says how it is and which directory, and nothing about which
                // agent. Identity comes from the process; this is only asked
                // once omt knows the answer, which is why this list is empty.
                names: vec![],
                // Observed from the shipped binary.
                working_prefix: vec!['\u{2801}'..='\u{28FF}'],
                idle_prefix: vec!['\u{25c7}'],
            }),
        }
    }

    fn spawn_env(&self, ctx: &SpawnCtx) -> Vec<(OsString, OsString)> {
        let mut env = vec![
            (
                OsString::from("OMT_INSTANCE"),
                OsString::from(&ctx.instance),
            ),
            (OsString::from("OMT_SESSION"), OsString::from(&ctx.session)),
            (OsString::from("OMT_SOCK"), OsString::from(&ctx.socket)),
        ];
        if let Some(hook) = &ctx.hook_path {
            env.push((OsString::from("OMT_HOOK"), OsString::from(hook)));
        }
        env
    }

    fn supported_modes(&self) -> SessionModeSet {
        SessionModeSet::BOTH
    }

    fn acp_spawn(&self, ctx: &SpawnCtx) -> Option<AcpSpawn> {
        Some(AcpSpawn {
            program: "gemini".to_owned(),
            // `--acp`, not `--experimental-acp`: the latter is deprecated in
            // v0.46 and the binary says so in its own help.
            args: vec!["--acp".to_owned()],
            env: self
                .spawn_env(ctx)
                .into_iter()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
                .collect(),
        })
    }

    fn best_tier(&self) -> Tier {
        Tier::Protocol
    }

    fn interrupt(&self) -> Interrupt {
        Interrupt::ControlC
    }

    fn path_mention(&self, rel: &str) -> Option<String> {
        Some(format!("@{rel}"))
    }

    fn normalize(
        &self,
        event: &str,
        payload: &serde_json::Value,
    ) -> Result<Vec<AgentPayload>, AdapterError> {
        let s = |k: &str| payload.get(k).and_then(|v| v.as_str()).map(str::to_owned);
        match event {
            // Gemini's own four, and Claude's spellings for anybody who ran
            // `gemini hooks migrate`.
            "BeforeAgent" | "SessionStart" => Ok(vec![AgentPayload::SessionStart {
                reason: match s("source").as_deref() {
                    Some("resume") => StartReason::Resume,
                    _ => StartReason::Startup,
                },
                model: s("model"),
            }]),
            "AfterAgent" | "SessionEnd" => Ok(vec![AgentPayload::SessionEnd {
                reason: s("reason").unwrap_or_else(|| "ended".to_owned()),
            }]),
            "UserPromptSubmit" => Ok(vec![
                AgentPayload::UserMessage {
                    text: s("prompt").unwrap_or_default(),
                    origin: MessageOrigin::Human,
                },
                AgentPayload::TurnStart {
                    turn: s("session_id"),
                    trigger: TurnTrigger::Human,
                },
            ]),
            "Stop" => Ok(vec![AgentPayload::TurnEnd {
                turn: s("session_id"),
                outcome: TurnOutcome::Completed,
            }]),
            "BeforeTool" | "PreToolUse" => Ok(vec![AgentPayload::ToolCall {
                call: call_id(payload),
                name: tool_name(payload),
                // Gemini names it `args`; a migrated Claude config says
                // `tool_input`. Both are the same thing and both are verbatim,
                // because a human approving a command approves what will run.
                input: payload
                    .get("args")
                    .or_else(|| payload.get("tool_input"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            }]),
            "AfterTool" | "PostToolUse" => {
                let mut out = vec![AgentPayload::ToolResult {
                    call: call_id(payload),
                    output: payload
                        .get("result")
                        .or_else(|| payload.get("tool_response"))
                        .cloned(),
                    error: s("error"),
                    duration_ms: payload
                        .get("duration_ms")
                        .and_then(serde_json::Value::as_u64),
                }];
                if let Some(path) = payload
                    .get("args")
                    .or_else(|| payload.get("tool_input"))
                    .and_then(|a| a.get("file_path").or_else(|| a.get("path")))
                    .and_then(|v| v.as_str())
                    && writes(&tool_name(payload))
                {
                    out.push(AgentPayload::FileChanged {
                        path: path.to_owned(),
                        change: FileChange::Modified,
                        tool: Some(tool_name(payload)),
                    });
                }
                Ok(out)
            }
            other => Err(AdapterError::UnknownEvent {
                agent: AgentKind::GeminiCli,
                event: other.to_owned(),
            }),
        }
    }
}

/// The call id, under either spelling.
fn call_id(payload: &serde_json::Value) -> String {
    payload
        .get("call_id")
        .or_else(|| payload.get("tool_use_id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned()
}

/// The tool's name, under either spelling.
fn tool_name(payload: &serde_json::Value) -> String {
    payload
        .get("tool_name")
        .or_else(|| payload.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned()
}

/// Whether a tool changed a file rather than reading one.
fn writes(tool: &str) -> bool {
    matches!(
        tool,
        "write_file" | "replace" | "edit" | "Write" | "Edit" | "WriteFile"
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn norm(event: &str, payload: serde_json::Value) -> Vec<AgentPayload> {
        Gemini.normalize(event, &payload).expect("normalize")
    }

    #[test]
    fn geminis_own_hook_names_are_understood() {
        // The finding this adapter exists for. `~/.gemini/settings.json` on a
        // real machine registers BeforeAgent, AfterAgent, BeforeTool and
        // AfterTool — not one of Claude Code's spellings.
        for event in ["BeforeAgent", "AfterAgent", "BeforeTool", "AfterTool"] {
            assert!(
                Gemini.normalize(event, &serde_json::json!({})).is_ok(),
                "{event} was not understood"
            );
        }
    }

    #[test]
    fn a_migrated_claude_configuration_also_works() {
        // `gemini hooks migrate` produces Claude's names. A user who ran it has
        // one vocabulary and a user who did not has the other, and an adapter
        // that knew only one would work for exactly half of them.
        for event in ["SessionStart", "PreToolUse", "PostToolUse", "Stop"] {
            assert!(
                Gemini.normalize(event, &serde_json::json!({})).is_ok(),
                "{event} was not understood"
            );
        }
    }

    #[test]
    fn a_tool_call_is_verbatim_under_either_spelling() {
        // Gemini says `args`; a migrated config says `tool_input`.
        let native = norm(
            "BeforeTool",
            serde_json::json!({ "call_id": "c1", "name": "run_shell_command", "args": { "command": "rm -rf /srv" } }),
        );
        let migrated = norm(
            "PreToolUse",
            serde_json::json!({ "tool_use_id": "c1", "tool_name": "run_shell_command", "tool_input": { "command": "rm -rf /srv" } }),
        );
        let input_of = |p: &Vec<AgentPayload>| match &p[0] {
            AgentPayload::ToolCall { input, .. } => input.clone(),
            other => panic!("expected a tool call, got {other:?}"),
        };
        assert_eq!(input_of(&native), input_of(&migrated));
    }

    #[test]
    fn a_write_reports_the_file_and_a_read_does_not() {
        let wrote = norm(
            "AfterTool",
            serde_json::json!({ "call_id": "c1", "name": "write_file", "args": { "file_path": "src/a.rs" } }),
        );
        assert!(
            wrote
                .iter()
                .any(|p| matches!(p, AgentPayload::FileChanged { .. }))
        );

        let read = norm(
            "AfterTool",
            serde_json::json!({ "call_id": "c2", "name": "read_file", "args": { "file_path": "src/a.rs" } }),
        );
        assert!(
            !read
                .iter()
                .any(|p| matches!(p, AgentPayload::FileChanged { .. })),
            "a read claimed to have changed a file"
        );
    }

    #[test]
    fn gemini_is_driven_over_acp_with_the_flag_that_is_not_deprecated() {
        // v0.46 deprecates `--experimental-acp` in its own help.
        let spawn = Gemini.acp_spawn(&SpawnCtx::default()).expect("a spawn");
        assert_eq!(spawn.args, vec!["--acp".to_owned()]);
        assert!(!spawn.args.iter().any(|a| a.contains("experimental")));
    }

    #[test]
    fn an_unknown_event_is_an_error_not_a_silent_drop() {
        assert!(
            Gemini
                .normalize("nonsense", &serde_json::json!({}))
                .is_err()
        );
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod screen_rule_tests {
    use super::*;
    use crate::adapter::AgentAdapter;
    use crate::rules::{ScreenState, read};

    /// Its first-run trust dialog, captured from the shipped binary.
    ///
    /// Written from what it actually drew rather than from memory: the rule
    /// for Claude Code's equivalent screen was wrong on exactly this, and a
    /// captured screen is what found it.
    const TRUST: &str = "\
 \u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}
 \u{2502} Do you trust this folder?                \u{2502}
 \u{2502}                                          \u{2502}
 \u{2502} \u{25cf} 1. Trust folder (tmp)                  \u{2502}
 \u{2502}   2. Trust parent folder (private)        \u{2502}
 \u{2502}   3. Don't trust                          \u{2502}
 \u{2502}                                          \u{2502}
 \u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}
";

    #[test]
    fn its_trust_dialog_is_somebody_being_waited_for() {
        let verdict = read(&Gemini.screen_rules(), TRUST).expect("a verdict");
        assert_eq!(
            verdict.state,
            ScreenState::Blocked,
            "rule: {}",
            verdict.rule
        );
    }

    #[test]
    fn ordinary_output_says_nothing() {
        assert!(read(&Gemini.screen_rules(), "reading files\ndone\n").is_none());
    }

    #[test]
    fn its_title_carries_a_state_and_no_name() {
        // `◇ Ready (tmp)` — how it is and which directory, and nothing about
        // which agent. Identity comes from the process; this is only asked
        // once omt knows the answer, which is why the name list is empty.
        let marks = Gemini.fingerprint().title.expect("title marks");
        assert!(marks.names.is_empty(), "a name would never match its title");
        assert_eq!(
            crate::detect::activity_from_title(&marks, "\u{25c7}  Ready (tmp)"),
            Some(crate::detect::TitleActivity::Idle)
        );
    }
}
