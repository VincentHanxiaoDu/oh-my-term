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
        // Protocol, now that there is a client for the app-server and it is
        // checked against the real binary. A tier is a promise about what a
        // surface will receive, and this one can now be kept.
        Tier::Protocol
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
        // The method names are Codex's own, taken from the bindings its binary
        // generates (`codex app-server generate-ts`) rather than guessed. An
        // earlier version of this table guessed snake_case and would have
        // matched nothing at all — the real protocol is `slash/separated`.
        match event {
            "thread/started" | "thread/resumed" => Ok(vec![AgentPayload::SessionStart {
                reason: if event.ends_with("resumed") {
                    StartReason::Resume
                } else {
                    StartReason::Startup
                },
                model: s("model"),
            }]),
            "thread/closed" | "thread/archived" => Ok(vec![AgentPayload::SessionEnd {
                reason: event.rsplit('/').next().unwrap_or("ended").to_owned(),
            }]),
            "turn/started" => Ok(vec![AgentPayload::TurnStart {
                turn: payload
                    .pointer("/turn/id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                trigger: TurnTrigger::Human,
            }]),
            "turn/completed" => Ok(vec![AgentPayload::TurnEnd {
                turn: payload
                    .pointer("/turn/id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                // Codex's own vocabulary, and the distinction a phone shows as
                // a red dot rather than a green one.
                outcome: match payload.pointer("/turn/status").and_then(|v| v.as_str()) {
                    Some("failed") => TurnOutcome::Error,
                    Some("interrupted") => TurnOutcome::Aborted,
                    _ => TurnOutcome::Completed,
                },
            }]),
            "item/started" | "item/completed" => Ok(item_payloads(payload, event)),
            "item/agentMessage/delta" => Ok(vec![AgentPayload::AssistantText {
                text: s("delta").unwrap_or_default(),
                // A delta, so more is coming — which is the whole reason a
                // protocol source beats a transcript one.
                partial: true,
            }]),
            "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
                Ok(vec![AgentPayload::Reasoning {
                    text: s("delta").unwrap_or_default(),
                    partial: true,
                }])
            }
            "thread/compacted" | "thread/compact/start" => Ok(vec![AgentPayload::Compaction {
                phase: if event.ends_with("start") {
                    omt_events::CompactionPhase::Before
                } else {
                    omt_events::CompactionPhase::After
                },
                trigger: None,
            }]),
            // A rate-limit update means the agent is working, and nothing more
            // specific. Reported as activity rather than invented into a turn.
            "account/rateLimits/updated" => Ok(vec![AgentPayload::Activity {
                state: omt_events::ActivityGuess::Busy,
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

/// Turn a thread item into omt's vocabulary.
///
/// One notification carries the whole item, tagged by `type`. An item type omt
/// has no use for produces nothing rather than an error: unlike an unknown
/// *method*, an unmodelled item type is a normal thing for a protocol to carry.
fn item_payloads(payload: &serde_json::Value, event: &str) -> Vec<AgentPayload> {
    let Some(item) = payload.get("item") else {
        return Vec::new();
    };
    let text = |k: &str| {
        item.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let id = text("id");
    let completed = event.ends_with("completed");

    match item.get("type").and_then(|t| t.as_str()) {
        Some("agentMessage") if completed => vec![AgentPayload::AssistantText {
            text: text("text"),
            partial: false,
        }],
        Some("userMessage") if completed => vec![AgentPayload::UserMessage {
            text: item
                .get("content")
                .and_then(|c| c.as_array())
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default(),
            origin: MessageOrigin::Human,
        }],
        Some("commandExecution") => {
            if completed {
                vec![AgentPayload::ToolResult {
                    call: id,
                    output: item.get("aggregatedOutput").cloned(),
                    error: None,
                    duration_ms: None,
                }]
            } else {
                vec![AgentPayload::ToolCall {
                    call: id,
                    name: "command".to_owned(),
                    // Verbatim: a human approving a command has to be approving
                    // the bytes that will run.
                    input: item
                        .get("command")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                }]
            }
        }
        Some("fileChange") if completed => item
            .get("changes")
            .and_then(|c| c.as_array())
            .map(|changes| {
                changes
                    .iter()
                    .filter_map(|c| {
                        Some(AgentPayload::FileChanged {
                            path: c.get("path")?.as_str()?.to_owned(),
                            change: FileChange::Modified,
                            tool: Some("fileChange".to_owned()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Some("mcpToolCall" | "dynamicToolCall") if !completed => vec![AgentPayload::ToolCall {
            call: id,
            name: text("name"),
            input: item
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        }],
        _ => Vec::new(),
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
        // Codex's own status vocabulary, from the bindings its binary
        // generates. The distinction a phone shows as a red dot, not green.
        let out = norm(
            "turn/completed",
            serde_json::json!({ "turn": { "id": "t1", "status": "failed" } }),
        );
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
        let out = norm(
            "turn/completed",
            serde_json::json!({ "turn": { "id": "t1", "status": "interrupted" } }),
        );
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
            "item/started",
            serde_json::json!({
                "item": { "type": "commandExecution", "id": "c1", "command": "rm -rf /srv" }
            }),
        );
        let AgentPayload::ToolCall { input, call, .. } = &out[0] else {
            panic!("expected a tool call, got {out:?}");
        };
        assert_eq!(call, "c1");
        assert_eq!(input, &serde_json::json!("rm -rf /srv"));
    }

    #[test]
    fn a_finished_command_reports_its_output_rather_than_a_second_call() {
        // The same item arrives twice, started and completed. Treating both as
        // calls would show every command running twice.
        let out = norm(
            "item/completed",
            serde_json::json!({
                "item": { "type": "commandExecution", "id": "c1", "aggregatedOutput": "ok" }
            }),
        );
        assert!(matches!(out[0], AgentPayload::ToolResult { .. }), "{out:?}");
    }

    #[test]
    fn a_file_change_reports_every_path_it_touched() {
        let out = norm(
            "item/completed",
            serde_json::json!({
                "item": {
                    "type": "fileChange",
                    "id": "f1",
                    "changes": [{ "path": "src/a.rs" }, { "path": "src/b.rs" }]
                }
            }),
        );
        assert_eq!(out.len(), 2, "{out:?}");
    }

    #[test]
    fn a_streamed_message_says_more_is_coming() {
        // The reason a protocol source beats a transcript one: the text arrives
        // while it is being written rather than after.
        let out = norm(
            "item/agentMessage/delta",
            serde_json::json!({ "delta": "think" }),
        );
        assert!(matches!(
            out[0],
            AgentPayload::AssistantText { partial: true, .. }
        ));
    }

    #[test]
    fn a_completed_message_does_not_claim_more_is_coming() {
        // A client waiting for the rest of a finished message shows a reply
        // that never settles.
        let out = norm(
            "item/completed",
            serde_json::json!({
                "item": { "type": "agentMessage", "id": "m1", "text": "done" }
            }),
        );
        assert!(matches!(
            out[0],
            AgentPayload::AssistantText { partial: false, .. }
        ));
    }

    #[test]
    fn an_item_type_omt_does_not_model_produces_nothing_rather_than_an_error() {
        // Unlike an unknown method, an unmodelled item is a normal thing for a
        // protocol to carry — erroring on it would fill the log with noise.
        let out = norm(
            "item/completed",
            serde_json::json!({ "item": { "type": "imageGeneration", "id": "i1" } }),
        );
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn an_unknown_method_is_an_error_not_a_silent_drop() {
        // A gap in the table that logs is one somebody can close.
        assert!(
            Codex
                .normalize("brand/new/thing", &serde_json::json!({}))
                .is_err()
        );
    }

    #[test]
    fn codex_claims_the_tier_its_client_can_deliver() {
        // Protocol, because the app-server client exists and is checked
        // against the real binary. Native session mode stays off: omt drives
        // the protocol as a source, and declaring a channel it cannot open is
        // what the tier ladder refuses.
        assert_eq!(Codex.best_tier(), Tier::Protocol);
        assert!(!Codex.supported_modes().native);
    }

    #[test]
    fn a_path_is_mentioned_the_way_codex_spells_it() {
        assert_eq!(Codex.path_mention("src/main.rs").as_deref(), Some("@src/main.rs"));
    }
}
