//! Claude Code — the deepest adapter, and the one the others are measured
//! against.

use std::ffi::OsString;
use std::path::Path;

use omt_events::{AgentPayload, FileChange, MessageOrigin, StartReason, TurnOutcome, TurnTrigger};
use omt_types::{AgentKind, Tier};

use crate::adapter::{
    AdapterError, AgentAdapter, AttachmentClass, AttachmentReference, Fingerprint, Interrupt,
    SpawnCtx,
};

/// Claude Code.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeCode;

impl AgentAdapter for ClaudeCode {
    fn kind(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            // Claude Code sets this itself, so its presence is a statement by
            // the agent rather than a resemblance to it.
            env_markers: vec!["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"],
            exe_names: vec!["claude"],
            argv_patterns: vec!["@anthropic-ai/claude-code", "claude-code/cli.js"],
            session_id_var: Some("CLAUDE_CODE_SESSION_ID"),
        }
    }

    fn spawn_env(&self, ctx: &SpawnCtx) -> Vec<(OsString, OsString)> {
        // These three are what remove the entire class of "match the
        // transcript to the pane" heuristics: a hook that starts with them
        // already knows which pane it belongs to.
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

    fn best_tier(&self) -> Tier {
        Tier::Hook
    }

    fn interrupt(&self) -> Interrupt {
        // Claude Code's own interrupt key. Ctrl-C would work too, but Esc is
        // what the agent itself documents, and mirroring the agent's own
        // semantics beats substituting ours.
        Interrupt::Escape
    }

    fn path_mention(&self, rel: &str) -> Option<String> {
        Some(format!("@{rel}"))
    }

    fn attachment_reference(
        &self,
        path: &Path,
        class: AttachmentClass,
    ) -> Option<AttachmentReference> {
        match class {
            // Claude Code reads an image from a path given as a mention; there
            // is no need to inline anything.
            AttachmentClass::Image | AttachmentClass::Text => {
                Some(AttachmentReference::Mention(format!("@{}", path.display())))
            }
            AttachmentClass::Binary => {
                Some(AttachmentReference::BarePath(path.display().to_string()))
            }
        }
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
                    Some("compact") => StartReason::Compact,
                    _ => StartReason::Startup,
                },
                model: s("model"),
            }]),
            "SessionEnd" => Ok(vec![AgentPayload::SessionEnd {
                reason: s("reason").unwrap_or_else(|| "ended".to_owned()),
            }]),
            "UserPromptSubmit" => Ok(vec![AgentPayload::UserMessage {
                text: s("prompt").unwrap_or_default(),
                // A hook cannot tell a typed prompt from one omt injected, so
                // it must not claim either. The session layer, which knows
                // whether it just wrote one, corrects this.
                origin: MessageOrigin::Human,
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
            "PostToolUse" => {
                let call = s("tool_use_id").unwrap_or_default();
                let mut out = vec![AgentPayload::ToolResult {
                    call,
                    output: payload.get("tool_response").cloned(),
                    error: s("error"),
                    duration_ms: payload
                        .get("duration_ms")
                        .and_then(serde_json::Value::as_u64),
                }];
                // A write tool that succeeded changed a file, and saying so
                // here means the workspace view does not have to re-derive it
                // from a diff nobody asked for.
                if let Some(change) = file_change(&s("tool_name").unwrap_or_default())
                    && let Some(path) = payload
                        .get("tool_input")
                        .and_then(|i| i.get("file_path"))
                        .and_then(|v| v.as_str())
                {
                    out.push(AgentPayload::FileChanged {
                        path: path.to_owned(),
                        change,
                        tool: s("tool_name"),
                    });
                }
                Ok(out)
            }
            "Stop" => Ok(vec![AgentPayload::TurnEnd {
                turn: s("turn_id"),
                outcome: TurnOutcome::Completed,
            }]),
            "SubagentStop" => Ok(vec![AgentPayload::TurnEnd {
                turn: s("turn_id"),
                outcome: TurnOutcome::Completed,
            }]),
            "Notification" => Ok(vec![AgentPayload::Notification {
                kind: s("type").unwrap_or_else(|| "notification".to_owned()),
                message: s("message").unwrap_or_default(),
            }]),
            "PreCompact" => Ok(vec![AgentPayload::Compaction {
                phase: omt_events::CompactionPhase::Before,
                trigger: s("trigger"),
            }]),
            "PostCompact" => Ok(vec![AgentPayload::Compaction {
                phase: omt_events::CompactionPhase::After,
                trigger: s("trigger"),
            }]),
            "TurnStart" => Ok(vec![AgentPayload::TurnStart {
                turn: s("turn_id"),
                trigger: TurnTrigger::Human,
            }]),
            // Deliberately an error rather than a silent drop: an event this
            // build has never seen is the first sign of a version that moved,
            // and swallowing it makes that invisible.
            other => Err(AdapterError::UnknownEvent {
                agent: AgentKind::ClaudeCode,
                event: other.to_owned(),
            }),
        }
    }
}

fn file_change(tool: &str) -> Option<FileChange> {
    match tool {
        "Write" => Some(FileChange::Created),
        "Edit" | "MultiEdit" | "NotebookEdit" => Some(FileChange::Modified),
        _ => None,
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

    fn norm(event: &str, payload: serde_json::Value) -> Vec<AgentPayload> {
        ClaudeCode
            .normalize(event, &payload)
            .expect("a known event normalizes")
    }

    #[test]
    fn the_correlation_variables_are_injected() {
        let env = ClaudeCode.spawn_env(&SpawnCtx {
            instance: "i-1".into(),
            session: "s-1".into(),
            socket: "/tmp/omt.sock".into(),
            ..SpawnCtx::default()
        });
        let names: Vec<String> = env
            .iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for expected in ["OMT_INSTANCE", "OMT_SESSION", "OMT_SOCK"] {
            assert!(names.contains(&expected.to_owned()), "{names:?}");
        }
    }

    #[test]
    fn a_tool_call_carries_its_input_verbatim() {
        // A human approves what will actually run, so the bytes must be the
        // agent's own rather than anything reformatted on the way through.
        let input = json!({"command": "rm -rf /tmp/x", "nested": [1, 2]});
        let out = norm(
            "PreToolUse",
            json!({"tool_use_id": "t1", "tool_name": "Bash", "tool_input": input}),
        );
        let AgentPayload::ToolCall {
            call,
            name,
            input: got,
        } = &out[0]
        else {
            panic!("expected a tool call: {out:?}");
        };
        assert_eq!(call, "t1");
        assert_eq!(name, "Bash");
        assert_eq!(*got, input);
    }

    #[test]
    fn a_result_correlates_with_its_call_by_id() {
        // The id is what makes "was this answered?" checkable rather than
        // guessable.
        let out = norm(
            "PostToolUse",
            json!({"tool_use_id": "t1", "tool_name": "Bash", "tool_response": "ok"}),
        );
        let AgentPayload::ToolResult { call, output, .. } = &out[0] else {
            panic!("{out:?}");
        };
        assert_eq!(call, "t1");
        assert_eq!(output.as_ref().and_then(|v| v.as_str()), Some("ok"));
    }

    #[test]
    fn a_write_also_reports_the_file_it_changed() {
        // So the workspace view does not have to re-derive it from a diff.
        let out = norm(
            "PostToolUse",
            json!({
                "tool_use_id": "t2",
                "tool_name": "Write",
                "tool_input": {"file_path": "/w/src/main.rs"},
                "tool_response": "written"
            }),
        );
        assert!(
            out.iter().any(|p| matches!(
                p,
                AgentPayload::FileChanged { path, change: FileChange::Created, .. }
                    if path == "/w/src/main.rs"
            )),
            "{out:?}"
        );
    }

    #[test]
    fn a_read_does_not_claim_to_have_changed_anything() {
        let out = norm(
            "PostToolUse",
            json!({"tool_use_id": "t3", "tool_name": "Read",
                   "tool_input": {"file_path": "/w/x"}, "tool_response": "..."}),
        );
        assert!(
            !out.iter()
                .any(|p| matches!(p, AgentPayload::FileChanged { .. })),
            "{out:?}"
        );
    }

    #[test]
    fn the_session_start_reason_is_carried_rather_than_flattened() {
        // A resume and a fresh start look identical on screen and mean
        // different things to every surface above.
        let out = norm("SessionStart", json!({"source": "resume"}));
        assert!(matches!(
            out[0],
            AgentPayload::SessionStart {
                reason: StartReason::Resume,
                ..
            }
        ));
        let out = norm("SessionStart", json!({"source": "startup"}));
        assert!(matches!(
            out[0],
            AgentPayload::SessionStart {
                reason: StartReason::Startup,
                ..
            }
        ));
    }

    #[test]
    fn compaction_is_reported_with_its_phase() {
        // The one place a transcript legitimately loses history, so both sides
        // of it have to be visible.
        let before = norm("PreCompact", json!({"trigger": "auto"}));
        let after = norm("PostCompact", json!({"trigger": "auto"}));
        assert!(matches!(
            before[0],
            AgentPayload::Compaction {
                phase: omt_events::CompactionPhase::Before,
                ..
            }
        ));
        assert!(matches!(
            after[0],
            AgentPayload::Compaction {
                phase: omt_events::CompactionPhase::After,
                ..
            }
        ));
    }

    #[test]
    fn an_unknown_event_is_an_error_not_a_silent_drop() {
        // An event this build has never seen is the first sign of a version
        // that moved; swallowing it makes that invisible until something else
        // breaks.
        let err = ClaudeCode
            .normalize("SomeFutureEvent", &json!({}))
            .expect_err("must not swallow it");
        assert!(
            matches!(err, AdapterError::UnknownEvent { ref event, .. } if event == "SomeFutureEvent"),
            "{err:?}"
        );
    }

    #[test]
    fn a_missing_field_degrades_rather_than_failing_the_whole_event() {
        // A payload that lost a field still says a tool ran, and that is worth
        // more than nothing.
        let out = norm("PreToolUse", json!({"tool_name": "Bash"}));
        let AgentPayload::ToolCall { call, .. } = &out[0] else {
            panic!("{out:?}");
        };
        assert!(call.is_empty(), "unknown, but the call is still reported");
    }

    #[test]
    fn the_interrupt_is_the_agents_own_key() {
        // Mirroring the agent's own semantics beats substituting ours.
        assert_eq!(ClaudeCode.interrupt(), Interrupt::Escape);
        assert_eq!(ClaudeCode.interrupt().bytes(), Some(&b"\x1b"[..]));
    }

    #[test]
    fn a_path_is_offered_in_the_agents_own_mention_syntax() {
        assert_eq!(
            ClaudeCode.path_mention("crates/omt-term/src/lib.rs"),
            Some("@crates/omt-term/src/lib.rs".to_owned())
        );
    }

    #[test]
    fn an_image_is_handed_over_as_a_mention_not_inlined() {
        let r = ClaudeCode.attachment_reference(Path::new("/tmp/shot.png"), AttachmentClass::Image);
        assert_eq!(
            r,
            Some(AttachmentReference::Mention("@/tmp/shot.png".to_owned()))
        );
    }

    #[test]
    fn the_best_tier_is_hook_because_that_is_what_it_actually_reaches() {
        assert_eq!(ClaudeCode.best_tier(), Tier::Hook);
        assert!(
            ClaudeCode.best_tier().may_emit_structured_content(),
            "a hook was told; it did not guess"
        );
    }
}
