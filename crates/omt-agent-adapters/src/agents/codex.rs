//! Codex.
//!
//! Codex ships two channels and omt currently drives one. Its `app-server` is a
//! JSON-RPC dialect of its own rather than ACP, and until there is a client for
//! it this adapter reports [`Tier::Hook`] — the notify hook, which is what is
//! implemented and what works when somebody started `codex` themselves in a
//! shell omt merely happens to be running.
//!
//! Stating the lower tier is the point. A tier is a promise about what a
//! surface will receive, and claiming Protocol here would turn on views that
//! nothing populates.

use std::ffi::OsString;
use std::path::Path;

use omt_events::{AgentPayload, FileChange, MessageOrigin, StartReason, TurnOutcome, TurnTrigger};
use omt_types::{AgentKind, Tier};

use crate::adapter::{
    AdapterError, AgentAdapter, AttachmentClass, AttachmentReference, Fingerprint, Interrupt,
    SessionModeSet, SpawnCtx,
};

/// The Codex adapter.
#[derive(Debug, Clone, Copy)]
pub struct Codex;

impl AgentAdapter for Codex {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            // Set by Codex itself, so its presence is a statement rather than a
            // resemblance — which matters because the executable is a Node
            // bundle whose name several other agents also use.
            env_markers: vec!["CODEX_SANDBOX", "CODEX_HOME"],
            exe_names: vec!["codex"],
            argv_patterns: vec!["@openai/codex", "codex/cli.js", "codex-cli"],
            session_id_var: Some("CODEX_SESSION_ID"),
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
            // Codex calls a single program for every notification rather than
            // registering per-event handlers, so one variable is the whole
            // wiring.
            env.push((OsString::from("OMT_HOOK"), OsString::from(hook)));
            env.push((OsString::from("CODEX_NOTIFY"), OsString::from(hook)));
        }
        env
    }

    fn supported_modes(&self) -> SessionModeSet {
        // PTY only, for now, and the reason is worth stating: Codex's
        // `app-server` is its own JSON-RPC dialect rather than ACP, and omt has
        // no client for it yet. Declaring native mode without one would make
        // this adapter claim a channel nothing can open — which the tier-ladder
        // test refuses, correctly.
        SessionModeSet::PTY_ONLY
    }

    fn best_tier(&self) -> Tier {
        // Hook, not Protocol. The notify hook is what is implemented, and a
        // tier is a promise about what a surface will actually receive.
        Tier::Hook
    }

    fn interrupt(&self) -> Interrupt {
        Interrupt::ControlC
    }

    fn path_mention(&self, rel: &str) -> Option<String> {
        Some(format!("@{rel}"))
    }

    fn attachment_reference(
        &self,
        path: &Path,
        class: AttachmentClass,
    ) -> Option<AttachmentReference> {
        let _ = class;
        Some(AttachmentReference::BarePath(path.display().to_string()))
    }

    fn normalize(
        &self,
        event: &str,
        payload: &serde_json::Value,
    ) -> Result<Vec<AgentPayload>, AdapterError> {
        let s = |k: &str| payload.get(k).and_then(|v| v.as_str()).map(str::to_owned);
        match event {
            // Codex names its notifications by type rather than by lifecycle
            // point, so the mapping is by `type` and the event name is the
            // outer envelope.
            "session-start" | "session_configured" => Ok(vec![AgentPayload::SessionStart {
                reason: match s("reason").as_deref() {
                    Some("resume") => StartReason::Resume,
                    Some("fork") => StartReason::Fork,
                    _ => StartReason::Startup,
                },
                model: s("model"),
            }]),
            "turn-start" | "task_started" => Ok(vec![AgentPayload::TurnStart {
                turn: s("turn_id"),
                trigger: TurnTrigger::Human,
            }]),
            "turn-complete" | "task_complete" => Ok(vec![AgentPayload::TurnEnd {
                turn: s("turn_id"),
                outcome: match s("status").as_deref() {
                    Some("error") | Some("failed") => TurnOutcome::Error,
                    Some("interrupted") | Some("cancelled") => TurnOutcome::Aborted,
                    _ => TurnOutcome::Completed,
                },
            }]),
            "user-message" | "user_message" => Ok(vec![AgentPayload::UserMessage {
                text: s("message").or_else(|| s("text")).unwrap_or_default(),
                origin: MessageOrigin::Human,
            }]),
            "agent-message" | "agent_message" => Ok(vec![AgentPayload::AssistantText {
                text: s("message").or_else(|| s("text")).unwrap_or_default(),
                // Never a fragment: a notify hook fires once per message, so
                // claiming more is coming would leave a surface waiting.
                partial: false,
            }]),
            "exec-command-begin" | "tool-call" => {
                let call = s("call_id").or_else(|| s("id")).unwrap_or_default();
                Ok(vec![AgentPayload::ToolCall {
                    call,
                    name: s("tool").unwrap_or_else(|| "exec".to_owned()),
                    // Verbatim, because a human approving a command has to be
                    // approving the bytes that will run.
                    input: payload
                        .get("input")
                        .or_else(|| payload.get("command"))
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                }])
            }
            "exec-command-end" | "tool-result" => {
                let call = s("call_id").or_else(|| s("id")).unwrap_or_default();
                let mut out = vec![AgentPayload::ToolResult {
                    call,
                    output: payload.get("output").cloned(),
                    error: s("error"),
                    duration_ms: payload
                        .get("duration_ms")
                        .and_then(serde_json::Value::as_u64),
                }];
                if let Some(path) = payload
                    .get("changed_file")
                    .and_then(|v| v.as_str())
                    .or_else(|| payload.get("path").and_then(|v| v.as_str()))
                {
                    out.push(AgentPayload::FileChanged {
                        path: path.to_owned(),
                        change: FileChange::Modified,
                        tool: s("tool"),
                    });
                }
                Ok(out)
            }
            "patch-apply-end" => {
                let path = s("path").unwrap_or_default();
                Ok(vec![AgentPayload::FileChanged {
                    path,
                    tool: Some("apply_patch".to_owned()),
                    change: if payload
                        .get("created")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                    {
                        FileChange::Created
                    } else {
                        FileChange::Modified
                    },
                }])
            }
            "token-count" | "token_count" => {
                let n = |k: &str| {
                    payload
                        .get(k)
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0)
                };
                Ok(vec![AgentPayload::Usage {
                    input: n("input_tokens"),
                    output: n("output_tokens"),
                    cache_read: n("cached_input_tokens"),
                    cache_write: n("cache_creation_tokens"),
                    cost_usd: None,
                }])
            }
            "agent-turn-complete" => Ok(vec![AgentPayload::TurnEnd {
                turn: s("turn_id"),
                outcome: TurnOutcome::Completed,
            }]),
            // Not silently dropped: an event omt does not know about is a gap
            // in this table, and a gap that logs is one somebody can close.
            other => Err(AdapterError::UnknownEvent {
                agent: AgentKind::Codex,
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
        Codex.normalize(event, &payload).expect("normalize")
    }

    #[test]
    fn codex_is_recognised_by_what_it_sets_rather_than_what_it_is_called() {
        // Four agents ship as Node bundles called something generic. An env
        // marker is the agent saying who it is; an exe name is a resemblance.
        let fp = Codex.fingerprint();
        assert!(fp.env_markers.contains(&"CODEX_SANDBOX"));
        assert!(fp.session_id_var.is_some());
    }

    #[test]
    fn the_notify_hook_is_pointed_at_omt_when_omt_starts_it() {
        // Codex calls one program for every notification, so this variable is
        // the entire wiring — without it the session is heuristic-tier.
        let env = Codex.spawn_env(&SpawnCtx {
            instance: "i".to_owned(),
            session: "s".to_owned(),
            socket: "sock".to_owned(),
            hook_path: Some("/usr/local/bin/omt-hook".to_owned()),
            cwd: None,
        });
        let notify = env
            .iter()
            .find(|(k, _)| k == "CODEX_NOTIFY")
            .expect("CODEX_NOTIFY");
        assert_eq!(notify.1, OsString::from("/usr/local/bin/omt-hook"));
    }

    #[test]
    fn a_turn_that_failed_is_not_reported_as_completed() {
        // The difference a phone shows as a red dot rather than a green one.
        let out = norm("turn-complete", serde_json::json!({ "status": "error" }));
        assert!(matches!(
            out[0],
            AgentPayload::TurnEnd {
                outcome: TurnOutcome::Error,
                ..
            }
        ));
    }

    #[test]
    fn an_interrupted_turn_is_its_own_outcome() {
        let out = norm("turn-complete", serde_json::json!({ "status": "interrupted" }));
        assert!(matches!(
            out[0],
            AgentPayload::TurnEnd {
                outcome: TurnOutcome::Aborted,
                ..
            }
        ));
    }

    #[test]
    fn a_command_carries_its_arguments_verbatim() {
        // A human approving a command must be approving the bytes that run.
        let out = norm(
            "exec-command-begin",
            serde_json::json!({ "call_id": "c1", "command": ["rm", "-rf", "/srv"] }),
        );
        let AgentPayload::ToolCall { input, call, .. } = &out[0] else {
            panic!("expected a tool call");
        };
        assert_eq!(call, "c1");
        assert_eq!(input, &serde_json::json!(["rm", "-rf", "/srv"]));
    }

    #[test]
    fn a_patch_that_created_a_file_says_created() {
        let out = norm(
            "patch-apply-end",
            serde_json::json!({ "path": "src/new.rs", "created": true }),
        );
        assert!(matches!(
            &out[0],
            AgentPayload::FileChanged {
                change: FileChange::Created,
                ..
            }
        ));
    }

    #[test]
    fn usage_is_carried_so_a_budget_can_be_shown() {
        let out = norm(
            "token-count",
            serde_json::json!({ "input_tokens": 10, "output_tokens": 20 }),
        );
        let AgentPayload::Usage { input, output, .. } = &out[0] else {
            panic!("expected usage");
        };
        assert_eq!(*input, 10);
        assert_eq!(*output, 20);
    }

    #[test]
    fn an_unknown_event_is_an_error_not_a_silent_drop() {
        // A gap in the table that logs is one somebody can close.
        assert!(Codex.normalize("nonsense", &serde_json::json!({})).is_err());
    }

    #[test]
    fn codex_claims_only_the_tier_it_can_actually_deliver() {
        // Its app-server exists, but omt has no client for it. Claiming
        // Protocol would turn on a surface nothing can populate, which is the
        // exact failure the tier ladder exists to prevent.
        assert_eq!(Codex.best_tier(), Tier::Hook);
        assert!(!Codex.supported_modes().native);
    }

    #[test]
    fn a_path_is_mentioned_the_way_codex_spells_it() {
        assert_eq!(Codex.path_mention("src/main.rs").as_deref(), Some("@src/main.rs"));
    }
}
