//! The generic ACP adapter — one implementation, five agents.
//!
//! Built early rather than last, and deliberately: an adapter trait shaped only
//! by Claude Code would fit exactly one agent, and nobody would find out until
//! the second one was written. This is the second shape, and it is what the
//! trait is validated against before it is frozen.

use std::ffi::OsString;

use omt_events::{AgentPayload, SlashCommand, TurnOutcome, TurnTrigger};
use omt_types::{AgentKind, Tier};

use crate::adapter::{
    AcpSpawn, AdapterError, AgentAdapter, Fingerprint, Interrupt, SessionModeSet, SpawnCtx,
};

/// Any agent that speaks the Agent Client Protocol.
#[derive(Debug, Clone)]
pub struct GenericAcp {
    kind: AgentKind,
}

impl GenericAcp {
    /// An ACP adapter for one agent.
    #[must_use]
    pub const fn new(kind: AgentKind) -> Self {
        Self { kind }
    }

    /// Every agent this one adapter covers.
    ///
    /// The best coverage-per-unit-work available: one implementation, and
    /// opencode, Gemini CLI, Qwen Code and Goose all work.
    pub const COVERS: &'static [AgentKind] = &[
        AgentKind::Opencode,
        AgentKind::GeminiCli,
        AgentKind::QwenCode,
        AgentKind::Goose,
    ];
}

impl AgentAdapter for GenericAcp {
    fn kind(&self) -> AgentKind {
        self.kind
    }

    fn fingerprint(&self) -> Fingerprint {
        match self.kind {
            AgentKind::Opencode => Fingerprint {
                env_markers: vec!["OPENCODE"],
                exe_names: vec!["opencode"],
                argv_patterns: vec!["opencode"],
                session_id_var: Some("OPENCODE_SESSION_ID"),
            },
            AgentKind::GeminiCli => Fingerprint {
                env_markers: vec!["GEMINI_CLI"],
                exe_names: vec!["gemini"],
                argv_patterns: vec!["@google/gemini-cli"],
                session_id_var: None,
            },
            AgentKind::QwenCode => Fingerprint {
                env_markers: vec![],
                exe_names: vec!["qwen"],
                argv_patterns: vec!["qwen-code"],
                session_id_var: None,
            },
            AgentKind::Goose => Fingerprint {
                env_markers: vec![],
                exe_names: vec!["goose"],
                argv_patterns: vec!["goose"],
                session_id_var: None,
            },
            _ => Fingerprint::default(),
        }
    }

    fn spawn_env(&self, ctx: &SpawnCtx) -> Vec<(OsString, OsString)> {
        vec![
            (
                OsString::from("OMT_INSTANCE"),
                OsString::from(&ctx.instance),
            ),
            (OsString::from("OMT_SESSION"), OsString::from(&ctx.session)),
            (OsString::from("OMT_SOCK"), OsString::from(&ctx.socket)),
        ]
    }

    fn supported_modes(&self) -> SessionModeSet {
        SessionModeSet::BOTH
    }

    fn acp_spawn(&self, _ctx: &SpawnCtx) -> Option<AcpSpawn> {
        let (program, args) = match self.kind {
            AgentKind::Opencode => ("opencode", vec!["acp"]),
            AgentKind::GeminiCli => ("gemini", vec!["--experimental-acp"]),
            AgentKind::QwenCode => ("qwen", vec!["--experimental-acp"]),
            AgentKind::Goose => ("goose", vec!["acp"]),
            _ => return None,
        };
        Some(AcpSpawn {
            program: program.to_owned(),
            args: args.into_iter().map(str::to_owned).collect(),
            env: Vec::new(),
        })
    }

    fn best_tier(&self) -> Tier {
        Tier::Protocol
    }

    fn interrupt(&self) -> Interrupt {
        // ACP has `session/cancel`, so the pane is left untouched — no
        // keystroke goes anywhere near a screen nothing inspected.
        Interrupt::Native
    }

    fn path_mention(&self, rel: &str) -> Option<String> {
        match self.kind {
            AgentKind::Opencode => Some(format!("@{rel}")),
            // The rest have no mention syntax of their own, and inventing one
            // would put literal `@` characters into a prompt as text.
            _ => None,
        }
    }

    fn normalize(
        &self,
        event: &str,
        payload: &serde_json::Value,
    ) -> Result<Vec<AgentPayload>, AdapterError> {
        // ACP method names, as they appear on the wire.
        match event {
            "session/update" => Ok(session_update(payload)),
            "session/request_permission" => {
                // The interaction itself travels as its own event; what belongs
                // in the payload stream is only that one was raised, and the
                // id it can be found by.
                Ok(Vec::new())
            }
            "available_commands_update" => {
                let commands: Vec<SlashCommand> = payload
                    .get("availableCommands")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|c| {
                                Some(SlashCommand {
                                    name: c.get("name")?.as_str()?.to_owned(),
                                    description: c
                                        .get("description")
                                        .and_then(|d| d.as_str())
                                        .map(str::to_owned),
                                    argument_hint: c
                                        .get("input")
                                        .and_then(|i| i.get("hint"))
                                        .and_then(|h| h.as_str())
                                        .map(str::to_owned),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(vec![AgentPayload::Capabilities {
                    tools: Vec::new(),
                    slash_commands: commands,
                    models: Vec::new(),
                }])
            }
            other => Err(AdapterError::UnknownEvent {
                agent: self.kind,
                event: other.to_owned(),
            }),
        }
    }
}

/// ACP's one notification carrying everything, discriminated by `sessionUpdate`.
fn session_update(payload: &serde_json::Value) -> Vec<AgentPayload> {
    let update = payload.get("update").unwrap_or(payload);
    let kind = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let text = || {
        update
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    match kind {
        "agent_message_chunk" => vec![AgentPayload::AssistantText {
            text: text(),
            // A protocol streams chunks; that is a true observation of the
            // same thing a transcript reports whole.
            partial: true,
        }],
        "agent_thought_chunk" => vec![AgentPayload::Reasoning {
            text: text(),
            partial: true,
        }],
        "user_message_chunk" => vec![AgentPayload::UserMessage {
            text: text(),
            origin: omt_events::MessageOrigin::Human,
        }],
        "tool_call" => vec![AgentPayload::ToolCall {
            call: update
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned(),
            name: update
                .get("title")
                .or_else(|| update.get("kind"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned(),
            input: update
                .get("rawInput")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        }],
        "tool_call_update" => {
            let call = update
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            let status = update.get("status").and_then(|v| v.as_str()).unwrap_or("");
            // Only a finished call is a result. Reporting an in-progress
            // update as one would close a tool call that is still running.
            match status {
                "completed" => vec![AgentPayload::ToolResult {
                    call,
                    output: update.get("rawOutput").cloned(),
                    error: None,
                    duration_ms: None,
                }],
                "failed" => vec![AgentPayload::ToolResult {
                    call,
                    output: None,
                    error: Some(
                        update
                            .get("rawOutput")
                            .map(std::string::ToString::to_string)
                            .unwrap_or_else(|| "the tool call failed".to_owned()),
                    ),
                    duration_ms: None,
                }],
                _ => Vec::new(),
            }
        }
        "plan" => {
            let steps = update
                .get("entries")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|e| {
                            Some(omt_events::PlanStep {
                                title: e.get("content")?.as_str()?.to_owned(),
                                status: match e.get("status").and_then(|s| s.as_str()) {
                                    Some("completed") => omt_events::PlanStepStatus::Completed,
                                    Some("in_progress") => omt_events::PlanStepStatus::InProgress,
                                    _ => omt_events::PlanStepStatus::Pending,
                                },
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            vec![AgentPayload::Plan { steps }]
        }
        "current_mode_update" => vec![AgentPayload::Notification {
            kind: "mode".to_owned(),
            message: update
                .get("currentModeId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned(),
        }],
        // An update shape this build does not model is still worth recording;
        // dropping it would make a protocol change invisible.
        other => vec![AgentPayload::Notification {
            kind: "acp".to_owned(),
            message: format!("unmodelled session update `{other}`"),
        }],
    }
}

/// A turn's boundaries, which ACP reports as the result of `session/prompt`
/// rather than as an update.
#[must_use]
pub fn turn_from_stop_reason(reason: &str) -> AgentPayload {
    AgentPayload::TurnEnd {
        turn: None,
        outcome: match reason {
            "cancelled" => TurnOutcome::Aborted,
            "refusal" | "max_tokens" | "max_turn_requests" => TurnOutcome::Error,
            _ => TurnOutcome::Completed,
        },
    }
}

/// The start of a turn, for symmetry with the stop reason above.
#[must_use]
pub const fn turn_start(trigger: TurnTrigger) -> AgentPayload {
    AgentPayload::TurnStart {
        turn: None,
        trigger,
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
    use serde_json::json;

    fn opencode() -> GenericAcp {
        GenericAcp::new(AgentKind::Opencode)
    }

    fn update(v: serde_json::Value) -> Vec<AgentPayload> {
        opencode()
            .normalize("session/update", &v)
            .expect("a known method")
    }

    #[test]
    fn one_adapter_covers_four_agents() {
        // The reason this was built second rather than last.
        assert_eq!(GenericAcp::COVERS.len(), 4);
        for kind in GenericAcp::COVERS {
            let a = GenericAcp::new(*kind);
            assert!(
                a.acp_spawn(&SpawnCtx::default()).is_some(),
                "{kind:?} must be startable over ACP"
            );
        }
    }

    #[test]
    fn an_acp_agent_supports_both_modes() {
        // Supporting a protocol must not cost the user the agent's own CLI.
        let modes = opencode().supported_modes();
        assert!(modes.pty && modes.native);
    }

    #[test]
    fn a_message_chunk_is_marked_partial() {
        // A protocol streams; a transcript does not. Both are true readings of
        // the same message, and a surface has to be able to tell.
        let out = update(json!({
            "update": {"sessionUpdate": "agent_message_chunk",
                       "content": {"type": "text", "text": "hel"}}
        }));
        assert!(matches!(
            &out[0],
            AgentPayload::AssistantText { text, partial: true } if text == "hel"
        ));
    }

    #[test]
    fn reasoning_is_distinct_from_what_the_agent_said() {
        // Rendering a thought as speech would put the agent's private
        // reasoning in front of the user as an answer.
        let out = update(json!({
            "update": {"sessionUpdate": "agent_thought_chunk",
                       "content": {"type": "text", "text": "hmm"}}
        }));
        assert!(matches!(&out[0], AgentPayload::Reasoning { .. }), "{out:?}");
    }

    #[test]
    fn a_tool_call_keeps_its_id_so_the_result_can_find_it() {
        let out = update(json!({
            "update": {"sessionUpdate": "tool_call", "toolCallId": "c1",
                       "title": "Read", "rawInput": {"path": "/x"}}
        }));
        let AgentPayload::ToolCall { call, name, input } = &out[0] else {
            panic!("{out:?}");
        };
        assert_eq!(call, "c1");
        assert_eq!(name, "Read");
        assert_eq!(input["path"], "/x");
    }

    #[test]
    fn an_in_progress_update_is_not_a_result() {
        // Reporting one would close a tool call that is still running, and the
        // UI would show it finished while it was not.
        let out = update(json!({
            "update": {"sessionUpdate": "tool_call_update", "toolCallId": "c1",
                       "status": "in_progress"}
        }));
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn a_completed_update_is_a_result() {
        let out = update(json!({
            "update": {"sessionUpdate": "tool_call_update", "toolCallId": "c1",
                       "status": "completed", "rawOutput": {"ok": true}}
        }));
        assert!(matches!(
            &out[0],
            AgentPayload::ToolResult { call, error: None, .. } if call == "c1"
        ));
    }

    #[test]
    fn a_failed_update_carries_an_error_rather_than_an_empty_success() {
        let out = update(json!({
            "update": {"sessionUpdate": "tool_call_update", "toolCallId": "c1",
                       "status": "failed", "rawOutput": "boom"}
        }));
        let AgentPayload::ToolResult { error, .. } = &out[0] else {
            panic!("{out:?}");
        };
        assert!(error.is_some(), "a failure must not read as a success");
    }

    #[test]
    fn a_plan_carries_each_step_with_its_status() {
        let out = update(json!({
            "update": {"sessionUpdate": "plan", "entries": [
                {"content": "read the file", "status": "completed"},
                {"content": "edit it", "status": "in_progress"},
                {"content": "test", "status": "pending"}
            ]}
        }));
        let AgentPayload::Plan { steps } = &out[0] else {
            panic!("{out:?}");
        };
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].status, omt_events::PlanStepStatus::Completed);
        assert_eq!(steps[1].status, omt_events::PlanStepStatus::InProgress);
        assert_eq!(steps[2].status, omt_events::PlanStepStatus::Pending);
    }

    #[test]
    fn commands_come_from_the_agent_rather_than_a_static_list() {
        // A list omt maintained would go stale the first time the agent
        // shipped a release.
        let out = opencode()
            .normalize(
                "available_commands_update",
                &json!({"availableCommands": [
                    {"name": "init", "description": "set up", "input": {"hint": "[path]"}}
                ]}),
            )
            .expect("known method");
        let AgentPayload::Capabilities { slash_commands, .. } = &out[0] else {
            panic!("{out:?}");
        };
        assert_eq!(slash_commands[0].name, "init");
        assert_eq!(slash_commands[0].argument_hint.as_deref(), Some("[path]"));
        assert!(opencode().static_commands().is_empty(), "nothing hardcoded");
    }

    #[test]
    fn an_unmodelled_update_is_recorded_rather_than_dropped() {
        // Dropping it would make a protocol change invisible until something
        // else broke.
        let out = update(json!({"update": {"sessionUpdate": "something_new"}}));
        assert!(matches!(
            &out[0],
            AgentPayload::Notification { message, .. } if message.contains("something_new")
        ));
    }

    #[test]
    fn an_unknown_method_is_an_error() {
        let err = opencode()
            .normalize("session/teleport", &json!({}))
            .expect_err("must not invent a meaning");
        assert!(matches!(err, AdapterError::UnknownEvent { .. }));
    }

    #[test]
    fn a_cancelled_turn_is_aborted_not_completed() {
        // A user who interrupted must not see "done".
        assert!(matches!(
            turn_from_stop_reason("cancelled"),
            AgentPayload::TurnEnd {
                outcome: TurnOutcome::Aborted,
                ..
            }
        ));
        assert!(matches!(
            turn_from_stop_reason("end_turn"),
            AgentPayload::TurnEnd {
                outcome: TurnOutcome::Completed,
                ..
            }
        ));
        assert!(matches!(
            turn_from_stop_reason("refusal"),
            AgentPayload::TurnEnd {
                outcome: TurnOutcome::Error,
                ..
            }
        ));
    }

    #[test]
    fn interrupting_over_the_protocol_touches_no_keys() {
        assert_eq!(opencode().interrupt(), Interrupt::Native);
        assert_eq!(opencode().interrupt().bytes(), None);
    }

    #[test]
    fn an_agent_without_a_mention_syntax_is_not_given_one() {
        // Inventing one puts a literal `@` into the prompt as text.
        assert_eq!(
            GenericAcp::new(AgentKind::Goose).path_mention("src/x.rs"),
            None
        );
        assert_eq!(
            opencode().path_mention("src/x.rs"),
            Some("@src/x.rs".to_owned())
        );
    }
}
