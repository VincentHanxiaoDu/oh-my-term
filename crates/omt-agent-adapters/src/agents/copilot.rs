//! GitHub Copilot CLI.
//!
//! Two real channels, and both are verified against the installed binary rather
//! than assumed. `copilot --acp` starts an Agent Client Protocol server, which
//! is the better source; and it fires Claude-Code-shaped hooks out of
//! `~/.copilot/hooks/*.json`, which is what works when somebody started
//! `copilot` themselves in a shell omt merely happens to be running.
//!
//! The hook vocabulary is Claude Code's with additions — `ErrorOccurred`,
//! `PermissionRequest`, `subagentStart` with a lowercase s. That last one is
//! not a typo here: it is how the file on disk spells it, and an adapter that
//! "corrected" it would silently stop seeing subagents start.

use std::ffi::OsString;

use omt_events::{
    AgentPayload, ChoiceOption, ChoiceQuestion, FileChange, MessageOrigin, StartReason,
    TurnOutcome, TurnTrigger,
};
use omt_types::{AgentKind, Tier};

use crate::adapter::{
    AcpSpawn, AdapterError, AgentAdapter, Fingerprint, Interrupt, SessionModeSet, SpawnCtx,
};

/// The Copilot adapter.
#[derive(Debug, Clone, Copy)]
pub struct Copilot;

impl AgentAdapter for Copilot {
    fn kind(&self) -> AgentKind {
        AgentKind::Copilot
    }

    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            env_markers: vec!["COPILOT_AGENT", "GH_COPILOT_TOKEN", "COPILOT_MODEL"],
            exe_names: vec!["copilot"],
            argv_patterns: vec!["@github/copilot", "copilot/index.js"],
            session_id_var: Some("COPILOT_SESSION_ID"),
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
            program: "copilot".to_owned(),
            args: vec!["--acp".to_owned()],
            // Converted rather than shared: the ACP spawn carries strings
            // because it crosses a serialisation boundary, and an OsString
            // that is not valid UTF-8 has no place on a wire.
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
            "SessionStart" => Ok(vec![AgentPayload::SessionStart {
                reason: match s("source").as_deref() {
                    Some("resume") => StartReason::Resume,
                    Some("clear") => StartReason::Clear,
                    _ => StartReason::Startup,
                },
                model: s("model"),
            }]),
            "SessionEnd" => Ok(vec![AgentPayload::SessionEnd {
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
            // Copilot has an explicit failure hook, which Claude Code does not.
            // Reporting it as a completed turn would show green for a turn that
            // ended badly.
            "ErrorOccurred" | "StopFailure" => Ok(vec![AgentPayload::TurnEnd {
                turn: s("session_id"),
                outcome: TurnOutcome::Error,
            }]),
            "PreToolUse" => {
                let call = s("tool_use_id").unwrap_or_default();
                let name = s("tool_name").unwrap_or_default();
                Ok(vec![AgentPayload::ToolCall {
                    call,
                    name,
                    input: payload
                        .get("tool_input")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                }])
            }
            "PostToolUse" | "PostToolUseFailure" => {
                let call = s("tool_use_id").unwrap_or_default();
                let mut out = vec![AgentPayload::ToolResult {
                    call,
                    output: payload.get("tool_response").cloned(),
                    // The failure hook is a distinct event, so an error is
                    // known rather than inferred from an empty response.
                    error: if event.ends_with("Failure") {
                        Some(s("error").unwrap_or_else(|| "the tool failed".to_owned()))
                    } else {
                        s("error")
                    },
                    duration_ms: payload
                        .get("duration_ms")
                        .and_then(serde_json::Value::as_u64),
                }];
                if let Some(path) = payload
                    .get("tool_input")
                    .and_then(|i| i.get("path").or_else(|| i.get("file_path")))
                    .and_then(|v| v.as_str())
                    && writes(&s("tool_name").unwrap_or_default())
                {
                    out.push(AgentPayload::FileChanged {
                        path: path.to_owned(),
                        change: FileChange::Modified,
                        tool: s("tool_name"),
                    });
                }
                Ok(out)
            }
            // The whole reason a phone is worth carrying: Copilot asks, and the
            // options arrive verbatim rather than being read off a screen.
            "PermissionRequest" => {
                let options = permission_options(payload);
                if options.is_empty() {
                    return Ok(vec![AgentPayload::Activity {
                        state: omt_events::ActivityGuess::NeedsAttention,
                    }]);
                }
                Ok(vec![AgentPayload::Question {
                    call: s("tool_use_id").unwrap_or_default(),
                    questions: vec![ChoiceQuestion {
                        question: s("tool_name")
                            .map(|t| format!("Allow {t}?"))
                            .unwrap_or_else(|| "Allow this?".to_owned()),
                        header: "Permission".to_owned(),
                        multi_select: false,
                        options,
                        allow_free_text: false,
                    }],
                }])
            }
            // Lowercase `s`, because that is how the file on disk spells it.
            // "Correcting" it would silently stop seeing subagents start.
            "subagentStart" | "SubagentStart" => Ok(vec![AgentPayload::Activity {
                state: omt_events::ActivityGuess::Busy,
            }]),
            "SubagentStop" => Ok(vec![AgentPayload::Activity {
                state: omt_events::ActivityGuess::Idle,
            }]),
            "Notification" => Ok(vec![AgentPayload::Notification {
                kind: s("type").unwrap_or_else(|| "notification".to_owned()),
                message: s("message").unwrap_or_default(),
            }]),
            "PreCompact" => Ok(vec![AgentPayload::Compaction {
                phase: omt_events::CompactionPhase::Before,
                trigger: s("trigger"),
            }]),
            other => Err(AdapterError::UnknownEvent {
                agent: AgentKind::Copilot,
                event: other.to_owned(),
            }),
        }
    }
}

/// Whether a tool changed a file, as opposed to reading one.
///
/// The distinction a diff view is built on: a read that claimed to be a write
/// would put an unchanged file in somebody's review.
fn writes(tool: &str) -> bool {
    matches!(
        tool,
        "write" | "edit" | "str_replace" | "create" | "Write" | "Edit" | "MultiEdit"
    )
}

/// The options on a permission request, verbatim and in the agent's order.
///
/// Returns nothing rather than a guess when the shape is not understood. A
/// half-parsed permission card renders with the wrong options on it, and
/// somebody presses one.
fn permission_options(payload: &serde_json::Value) -> Vec<ChoiceOption> {
    let Some(raw) = payload.get("options").and_then(|o| o.as_array()) else {
        return Vec::new();
    };
    let parsed: Vec<ChoiceOption> = raw
        .iter()
        .filter_map(|o| {
            Some(ChoiceOption {
                label: o
                    .get("label")
                    .or_else(|| o.get("name"))
                    .and_then(|l| l.as_str())?
                    .to_owned(),
                description: o
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(str::to_owned),
            })
        })
        .collect();
    // All of them or none. Presenting three of four means somebody chooses
    // without ever seeing the one they would have picked.
    if parsed.len() == raw.len() {
        parsed
    } else {
        Vec::new()
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

    fn norm(event: &str, payload: serde_json::Value) -> Vec<AgentPayload> {
        Copilot.normalize(event, &payload).expect("normalize")
    }

    #[test]
    fn copilot_is_driven_over_acp_because_it_ships_one() {
        // `copilot --acp` is in its own `--help`. Declaring native mode without
        // the spawn is what the tier ladder refuses, and rightly.
        assert_eq!(Copilot.best_tier(), Tier::Protocol);
        assert!(Copilot.supported_modes().native);
        let spawn = Copilot.acp_spawn(&SpawnCtx::default()).expect("a spawn");
        assert_eq!(spawn.args, vec!["--acp".to_owned()]);
    }

    #[test]
    fn a_prompt_starts_a_turn_as_well_as_recording_it() {
        // Without this a session reads as idle for the whole time it works, and
        // a phone shows green while an agent burns tokens.
        let out = norm(
            "UserPromptSubmit",
            serde_json::json!({ "prompt": "fix the build", "session_id": "s1" }),
        );
        assert!(
            out.iter()
                .any(|p| matches!(p, AgentPayload::TurnStart { .. }))
        );
    }

    #[test]
    fn an_error_is_not_reported_as_a_completed_turn() {
        // Copilot has an explicit failure hook that Claude Code does not, so
        // this is known rather than inferred.
        let out = norm("ErrorOccurred", serde_json::json!({ "session_id": "s1" }));
        assert!(matches!(
            out[0],
            AgentPayload::TurnEnd {
                outcome: TurnOutcome::Error,
                ..
            }
        ));
    }

    #[test]
    fn a_failed_tool_says_so_rather_than_returning_an_empty_result() {
        let out = norm(
            "PostToolUseFailure",
            serde_json::json!({ "tool_use_id": "t1", "tool_name": "bash" }),
        );
        let AgentPayload::ToolResult { error, .. } = &out[0] else {
            panic!("expected a tool result");
        };
        assert!(
            error.is_some(),
            "a failure came back looking like a success"
        );
    }

    #[test]
    fn a_permission_request_becomes_a_card_with_copilots_own_options() {
        // The whole reason a phone is worth carrying. The options are the
        // agent's, verbatim — not read off a screen.
        let out = norm(
            "PermissionRequest",
            serde_json::json!({
                "tool_use_id": "t1",
                "tool_name": "bash",
                "options": [
                    { "label": "Allow once" },
                    { "label": "Allow always", "description": "for this session" },
                    { "label": "Deny" }
                ]
            }),
        );
        let AgentPayload::Question { questions, .. } = &out[0] else {
            panic!("expected a question, got {out:?}");
        };
        assert_eq!(
            questions[0]
                .options
                .iter()
                .map(|o| o.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Allow once", "Allow always", "Deny"]
        );
    }

    #[test]
    fn an_option_that_cannot_be_read_drops_the_whole_card() {
        // Three of four options means somebody chooses without ever seeing the
        // one they would have picked.
        let out = norm(
            "PermissionRequest",
            serde_json::json!({
                "tool_use_id": "t1",
                "tool_name": "bash",
                "options": [{ "label": "Allow" }, { "note": "no label" }]
            }),
        );
        assert!(
            !out.iter()
                .any(|p| matches!(p, AgentPayload::Question { .. })),
            "a card was shown with an option missing"
        );
    }

    #[test]
    fn the_lowercase_subagent_start_is_understood() {
        // That is how `~/.copilot/hooks/*.json` spells it. An adapter that
        // "corrected" it would silently stop seeing subagents start.
        assert!(
            Copilot
                .normalize("subagentStart", &serde_json::json!({}))
                .is_ok()
        );
    }

    #[test]
    fn a_read_does_not_claim_to_have_changed_a_file() {
        let out = norm(
            "PostToolUse",
            serde_json::json!({
                "tool_use_id": "t1",
                "tool_name": "read",
                "tool_input": { "path": "src/a.rs" }
            }),
        );
        assert!(
            !out.iter()
                .any(|p| matches!(p, AgentPayload::FileChanged { .. }))
        );
    }

    #[test]
    fn an_unknown_event_is_an_error_not_a_silent_drop() {
        assert!(
            Copilot
                .normalize("nonsense", &serde_json::json!({}))
                .is_err()
        );
    }
}
