//! Cursor's CLI agent.
//!
//! Hook tier and no further. Cursor fires lifecycle hooks and reports its tool
//! calls, which is enough for blocks, transcript and answerable cards — but it
//! exposes no protocol omt can drive, so [`best_tier`](AgentAdapter::best_tier)
//! stops at [`Tier::Hook`]. Saying so is the point: a surface that shows the
//! tier is better than one that leaves somebody to infer it from a feature that
//! quietly is not there.

use std::ffi::OsString;

use omt_events::{AgentPayload, FileChange, MessageOrigin, StartReason, TurnOutcome, TurnTrigger};
use omt_types::{AgentKind, Tier};

use crate::adapter::{
    AdapterError, AgentAdapter, Fingerprint, Interrupt, SessionModeSet, SpawnCtx,
};

/// The Cursor adapter.
#[derive(Debug, Clone, Copy)]
pub struct Cursor;

impl AgentAdapter for Cursor {
    fn kind(&self) -> AgentKind {
        AgentKind::Cursor
    }

    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            env_markers: vec!["CURSOR_AGENT", "CURSOR_TRACE_ID"],
            exe_names: vec!["cursor-agent"],
            argv_patterns: vec!["cursor-agent", "@cursor/agent"],
            session_id_var: Some("CURSOR_SESSION_ID"),
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
        SessionModeSet::PTY_ONLY
    }

    fn best_tier(&self) -> Tier {
        Tier::Hook
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
            "beforeSubmitPrompt" => Ok(vec![
                AgentPayload::UserMessage {
                    text: s("prompt").unwrap_or_default(),
                    origin: MessageOrigin::Human,
                },
                AgentPayload::TurnStart {
                    turn: s("conversation_id"),
                    trigger: TurnTrigger::Human,
                },
            ]),
            "afterFileEdit" => Ok(vec![AgentPayload::FileChanged {
                path: s("file_path").unwrap_or_default(),
                change: FileChange::Modified,
                tool: Some("edit".to_owned()),
            }]),
            "beforeShellExecution" => Ok(vec![AgentPayload::ToolCall {
                call: s("call_id").unwrap_or_default(),
                name: "shell".to_owned(),
                // Verbatim: this is the hook Cursor uses to gate a command, so
                // what omt shows has to be what will run.
                input: payload
                    .get("command")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            }]),
            "beforeReadFile" => Ok(vec![AgentPayload::ToolCall {
                call: s("call_id").unwrap_or_default(),
                name: "read".to_owned(),
                input: payload
                    .get("file_path")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            }]),
            "beforeMCPExecution" => Ok(vec![AgentPayload::ToolCall {
                call: s("call_id").unwrap_or_default(),
                name: s("tool_name").unwrap_or_else(|| "mcp".to_owned()),
                input: payload
                    .get("tool_input")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            }]),
            "stop" => Ok(vec![AgentPayload::TurnEnd {
                turn: s("conversation_id"),
                outcome: match s("status").as_deref() {
                    Some("error") => TurnOutcome::Error,
                    Some("aborted") | Some("cancelled") => TurnOutcome::Aborted,
                    _ => TurnOutcome::Completed,
                },
            }]),
            "start" => Ok(vec![AgentPayload::SessionStart {
                reason: StartReason::Startup,
                model: s("model"),
            }]),
            other => Err(AdapterError::UnknownEvent {
                agent: AgentKind::Cursor,
                event: other.to_owned(),
            }),
        }
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
        Cursor.normalize(event, &payload).expect("normalize")
    }

    #[test]
    fn cursor_stops_at_hook_tier_and_says_so() {
        // The honest answer. A surface that shows the tier beats one that
        // leaves somebody to infer it from a feature that is quietly absent.
        assert_eq!(Cursor.best_tier(), Tier::Hook);
        assert!(!Cursor.supported_modes().native);
    }

    #[test]
    fn a_prompt_starts_a_turn_as_well_as_recording_the_message() {
        // Cursor has no separate turn-start hook, so the prompt is the only
        // moment omt can mark one — without this a session shows as idle for
        // the whole time it is working.
        let out = norm(
            "beforeSubmitPrompt",
            serde_json::json!({ "prompt": "fix the build", "conversation_id": "c1" }),
        );
        assert!(
            out.iter()
                .any(|p| matches!(p, AgentPayload::TurnStart { .. })),
            "a turn never started, so the agent looks idle while it works"
        );
    }

    #[test]
    fn a_shell_command_is_carried_verbatim() {
        // This is the hook Cursor uses to gate a command, so what omt shows
        // has to be what will run.
        let out = norm(
            "beforeShellExecution",
            serde_json::json!({ "call_id": "c1", "command": "rm -rf /srv" }),
        );
        let AgentPayload::ToolCall { input, .. } = &out[0] else {
            panic!("expected a tool call");
        };
        assert_eq!(input, &serde_json::json!("rm -rf /srv"));
    }

    #[test]
    fn an_edit_reports_the_file_it_touched() {
        let out = norm("afterFileEdit", serde_json::json!({ "file_path": "src/a.rs" }));
        let AgentPayload::FileChanged { path, .. } = &out[0] else {
            panic!("expected a file change");
        };
        assert_eq!(path, "src/a.rs");
    }

    #[test]
    fn a_read_does_not_claim_to_have_changed_anything() {
        // The distinction a diff view is built on.
        let out = norm("beforeReadFile", serde_json::json!({ "file_path": "src/a.rs" }));
        assert!(
            !out.iter()
                .any(|p| matches!(p, AgentPayload::FileChanged { .. }))
        );
    }

    #[test]
    fn a_failed_turn_is_not_reported_as_completed() {
        let out = norm("stop", serde_json::json!({ "status": "error" }));
        assert!(matches!(
            out[0],
            AgentPayload::TurnEnd {
                outcome: TurnOutcome::Error,
                ..
            }
        ));
    }

    #[test]
    fn an_unknown_event_is_an_error_not_a_silent_drop() {
        assert!(Cursor.normalize("nonsense", &serde_json::json!({})).is_err());
    }
}
