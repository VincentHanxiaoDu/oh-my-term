//! `OpencodeHttp` — opencode's own `serve` REST + SSE API.
//!
//! The last of [06 §3](../../../docs/architecture/06-agent-layer.md)'s protocol
//! sources. Until now opencode was driven over generic ACP, which works and
//! reaches the same tier, but throws away everything opencode reports that ACP
//! has no word for: which model a step ran on, what a tool cost, and — since
//! 1.18 — structured questions with their own options.
//!
//! Two halves, deliberately separated. [`decode`] is the interesting one and is
//! a pure function of a line: what an event means never depends on a socket.
//! [`EventStream`] is the pipe, and does nothing but hand lines over.
//!
//! ## The event names are not the ones in the database
//!
//! opencode's `event` table stores versioned names (`session.updated.1`). The
//! SSE stream sends the *unversioned* spelling (`session.updated`), and the
//! `.1` suffix appears only inside a `sync` envelope. Both are accepted here,
//! because reading the database and reading the stream are two ways to observe
//! one agent and an adapter that knew only one spelling would work for exactly
//! one of them.

use omt_events::{
    AgentPayload, ChoiceOption, ChoiceQuestion, MessageOrigin, StartReason, TurnOutcome,
    TurnTrigger,
};

use crate::sse;

/// Where opencode's event stream lives on a `serve` instance.
pub const EVENT_PATH: &str = "/api/event";

/// Where its session list lives, for the REST half.
pub const SESSION_PATH: &str = "/api/session";

/// What one line off the stream turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    /// It said something omt has a word for.
    Payloads(Vec<AgentPayload>),
    /// It was not JSON.
    Unparseable {
        /// What arrived, verbatim, so it can be logged rather than guessed at.
        data: String,
    },
    /// An event type this source has no mapping for.
    ///
    /// Reported rather than dropped: opencode ships roughly ninety event types
    /// and omt models a dozen of them on purpose, but a source that silently
    /// discarded the rest would be indistinguishable from one that had quietly
    /// stopped working.
    Unmapped {
        /// opencode's own name for it.
        event: String,
    },
}

/// Strip the `.1`-style version suffix the durable log adds.
///
/// `session.updated.1` and `session.updated` are the same event seen from the
/// database and from the stream. Only a trailing all-digits segment is removed,
/// so `question.v2.asked` — where the version is in the middle and part of the
/// name — is left alone.
#[must_use]
pub fn unversioned(event: &str) -> &str {
    match event.rsplit_once('.') {
        Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => event,
    }
}

/// Decode one SSE event's data into omt's vocabulary.
#[must_use]
pub fn decode(event: &sse::Event) -> Decoded {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&event.data) else {
        return Decoded::Unparseable {
            data: event.data.clone(),
        };
    };
    // A `sync` envelope carries the real event one level down, under the
    // versioned name. Unwrapping here rather than in every arm below keeps the
    // mapping written once.
    let inner = value
        .get("syncEvent")
        .filter(|_| value.get("type").and_then(|t| t.as_str()) == Some("sync"))
        .unwrap_or(&value);
    let Some(name) = inner.get("type").and_then(|t| t.as_str()) else {
        return Decoded::Unmapped {
            event: String::new(),
        };
    };
    let data = inner
        .get("data")
        .unwrap_or(&serde_json::Value::Null)
        .clone();
    map(unversioned(name), &data).map_or_else(
        || Decoded::Unmapped {
            event: name.to_owned(),
        },
        Decoded::Payloads,
    )
}

/// The mapping itself. `None` means opencode said something omt does not model.
fn map(event: &str, data: &serde_json::Value) -> Option<Vec<AgentPayload>> {
    fn opt(data: &serde_json::Value, key: &str) -> Option<String> {
        data.get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }
    let text = |key: &str| {
        data.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    match event {
        // ---------- session ----------
        "session.created" => Some(vec![AgentPayload::SessionStart {
            reason: StartReason::Startup,
            model: data
                .get("info")
                .and_then(|i| i.get("model"))
                .and_then(|m| m.as_str())
                .map(str::to_owned),
        }]),
        "session.deleted" => Some(vec![AgentPayload::SessionEnd {
            reason: "deleted".to_owned(),
        }]),
        "session.compacted" | "session.next.compaction.ended" => {
            Some(vec![AgentPayload::Compaction {
                phase: omt_events::CompactionPhase::After,
                trigger: Some("automatic".to_owned()),
            }])
        }
        "session.next.compaction.started" => Some(vec![AgentPayload::Compaction {
            phase: omt_events::CompactionPhase::Before,
            trigger: Some("automatic".to_owned()),
        }]),

        // ---------- turns ----------
        // A step is opencode's turn: one model call, start to finish. The
        // prompt is what a human did, and arrives separately, so both are
        // mapped — `TurnStart` from the prompt where there is one, because
        // that is the moment the user is waiting from.
        "session.next.prompted" => Some(vec![AgentPayload::TurnStart {
            turn: opt(data, "messageID"),
            trigger: if data.get("delivery").and_then(|d| d.as_str()) == Some("queue") {
                TurnTrigger::Queued
            } else {
                TurnTrigger::Human
            },
        }]),
        "session.next.step.started" => Some(vec![AgentPayload::TurnStart {
            turn: opt(data, "assistantMessageID"),
            trigger: TurnTrigger::Human,
        }]),
        "session.next.step.ended" => {
            let mut out = vec![AgentPayload::TurnEnd {
                turn: opt(data, "assistantMessageID"),
                outcome: TurnOutcome::Completed,
            }];
            if let Some(usage) = usage_from(data) {
                out.push(usage);
            }
            Some(out)
        }
        "session.next.step.failed" => Some(vec![AgentPayload::TurnEnd {
            turn: opt(data, "assistantMessageID"),
            outcome: TurnOutcome::Error,
        }]),
        // The session going idle is the one signal that means "nothing is
        // running" regardless of how the last step ended. It is what a phone
        // showing a spinner needs, and no step event covers an aborted turn.
        "session.idle" => Some(vec![AgentPayload::TurnEnd {
            turn: opt(data, "sessionID"),
            outcome: TurnOutcome::Completed,
        }]),
        "session.error" => Some(vec![AgentPayload::Notification {
            kind: "error".to_owned(),
            message: data
                .get("error")
                .and_then(|e| e.get("name").or_else(|| e.get("message")))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_owned(),
        }]),

        // ---------- content ----------
        "session.next.text.delta" => Some(vec![AgentPayload::AssistantText {
            text: text("delta"),
            partial: true,
        }]),
        // The whole message, once. Emitted as complete rather than skipped:
        // a surface that joined the deltas has it already, and one that did
        // not — a phone that attached mid-turn — has nothing otherwise.
        "session.next.text.ended" => Some(vec![AgentPayload::AssistantText {
            text: text("text"),
            partial: false,
        }]),
        "session.next.reasoning.delta" => Some(vec![AgentPayload::Reasoning {
            text: text("delta"),
            partial: true,
        }]),
        "session.next.synthetic" => Some(vec![AgentPayload::UserMessage {
            text: text("text"),
            // Named for what it is. opencode calls it synthetic because no
            // human typed it, and a surface that showed it as a user message
            // would have somebody arguing with a line they never wrote.
            origin: MessageOrigin::Synthetic,
        }]),

        // ---------- tools ----------
        "session.next.tool.called" => Some(vec![AgentPayload::ToolCall {
            call: text("callID"),
            name: text("tool"),
            input: data
                .get("input")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        }]),
        "session.next.tool.success" => Some(vec![AgentPayload::ToolResult {
            call: text("callID"),
            output: data
                .get("structured")
                .or_else(|| data.get("result"))
                .cloned(),
            error: None,
            duration_ms: None,
        }]),
        "session.next.tool.failed" => Some(vec![AgentPayload::ToolResult {
            call: text("callID"),
            output: None,
            error: Some(
                data.get("error")
                    .and_then(|e| e.get("message").or_else(|| e.get("name")))
                    .and_then(|m| m.as_str())
                    .unwrap_or("failed")
                    .to_owned(),
            ),
            duration_ms: None,
        }]),
        "file.edited" => Some(vec![AgentPayload::FileChanged {
            path: text("file"),
            change: omt_events::FileChange::Modified,
            tool: None,
        }]),

        // ---------- interactions ----------
        // The finding that made this source worth writing. opencode 1.18 asks
        // structured questions with their own options — header, label,
        // description, multi-select, free text — which is the exact shape omt
        // renders as a card. ACP has no word for it, so driving opencode over
        // ACP loses it and degrades the question to a permission prompt.
        "question.v2.asked" | "question.asked" => Some(vec![AgentPayload::Question {
            call: data
                .get("tool")
                .and_then(|t| t.get("callID"))
                .and_then(|c| c.as_str())
                .unwrap_or_else(|| data.get("id").and_then(|i| i.as_str()).unwrap_or_default())
                .to_owned(),
            questions: questions_from(data),
        }]),
        "permission.v2.asked" | "permission.asked" => Some(vec![AgentPayload::Notification {
            kind: "permission".to_owned(),
            message: text("action"),
        }]),

        _ => None,
    }
}

/// opencode's token counts, where a step reported them.
fn usage_from(data: &serde_json::Value) -> Option<AgentPayload> {
    let tokens = data.get("tokens")?;
    let n = |key: &str| {
        tokens
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let cache = |key: &str| {
        tokens
            .get("cache")
            .and_then(|c| c.get(key))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    Some(AgentPayload::Usage {
        input: n("input"),
        output: n("output"),
        cache_read: cache("read"),
        cache_write: cache("write"),
        // Only if opencode itself stated one. omt never computes a price.
        cost_usd: data.get("cost").and_then(serde_json::Value::as_f64),
    })
}

/// The questions off a `question.v2.asked`.
fn questions_from(data: &serde_json::Value) -> Vec<ChoiceQuestion> {
    data.get("questions")
        .and_then(|q| q.as_array())
        .map(|arr| {
            arr.iter()
                .map(|q| {
                    let s = |key: &str| {
                        q.get(key)
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_owned()
                    };
                    let flag =
                        |key: &str| q.get(key).and_then(serde_json::Value::as_bool) == Some(true);
                    ChoiceQuestion {
                        question: s("question"),
                        header: s("header"),
                        multi_select: flag("multiple"),
                        allow_free_text: flag("custom"),
                        options: q
                            .get("options")
                            .and_then(|o| o.as_array())
                            .map(|opts| {
                                opts.iter()
                                    .map(|o| ChoiceOption {
                                        label: o
                                            .get("label")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .to_owned(),
                                        description: o
                                            .get("description")
                                            .and_then(|v| v.as_str())
                                            .filter(|d| !d.is_empty())
                                            .map(str::to_owned),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The pipe: bytes off a `serve` instance's event stream, decoded in order.
///
/// Holds the SSE decoder so a chunk that ends mid-event does not become two
/// half events, and remembers the last id so a reconnect can resume rather
/// than replay.
#[derive(Debug, Default)]
pub struct EventStream {
    decoder: sse::Decoder,
}

impl EventStream {
    /// A stream with nothing buffered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The URL of the event stream on a `serve` instance.
    #[must_use]
    pub fn url(base: &str) -> String {
        format!("{}{EVENT_PATH}", base.trim_end_matches('/'))
    }

    /// The id a reconnect should resume from.
    #[must_use]
    pub fn last_id(&self) -> Option<&str> {
        self.decoder.last_id()
    }

    /// Feed a chunk of the stream, returning what it decoded to.
    pub fn feed(&mut self, chunk: &str) -> Vec<Decoded> {
        self.decoder.feed(chunk).iter().map(decode).collect()
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

    fn one(json: &str) -> Decoded {
        decode(&sse::Event {
            name: None,
            data: json.to_owned(),
            id: None,
        })
    }

    fn payloads(json: &str) -> Vec<AgentPayload> {
        match one(json) {
            Decoded::Payloads(p) => p,
            other => panic!("expected payloads, got {other:?}"),
        }
    }

    #[test]
    fn the_versioned_and_unversioned_spellings_are_the_same_event() {
        // The database stores `session.idle.1`; the stream sends
        // `session.idle`. An adapter that knew one spelling would work for
        // exactly one of the two ways to observe opencode.
        assert_eq!(unversioned("session.idle.1"), "session.idle");
        assert_eq!(unversioned("session.idle"), "session.idle");
    }

    #[test]
    fn a_version_in_the_middle_of_a_name_is_not_a_suffix() {
        // `question.v2.asked` is a name, not a versioned `question.v2`.
        assert_eq!(unversioned("question.v2.asked"), "question.v2.asked");
    }

    #[test]
    fn a_text_delta_is_a_partial_assistant_message() {
        let out = payloads(
            r#"{"id":"evt_1","type":"session.next.text.delta","data":{"sessionID":"ses_1","assistantMessageID":"msg_1","textID":"t","delta":"hel"}}"#,
        );
        assert_eq!(
            out,
            vec![AgentPayload::AssistantText {
                text: "hel".to_owned(),
                partial: true
            }]
        );
    }

    #[test]
    fn a_finished_message_arrives_whole_and_not_partial() {
        // A client that attached mid-turn never saw the deltas.
        let out = payloads(
            r#"{"type":"session.next.text.ended","data":{"textID":"t","text":"hello there"}}"#,
        );
        assert_eq!(
            out,
            vec![AgentPayload::AssistantText {
                text: "hello there".to_owned(),
                partial: false
            }]
        );
    }

    #[test]
    fn a_tool_call_keeps_its_id_so_the_result_can_be_matched_to_it() {
        let call = payloads(
            r#"{"type":"session.next.tool.called","data":{"callID":"c1","tool":"bash","input":{"command":"ls"}}}"#,
        );
        let AgentPayload::ToolCall { call: id, name, .. } = &call[0] else {
            panic!("expected a tool call, got {call:?}");
        };
        assert_eq!((id.as_str(), name.as_str()), ("c1", "bash"));

        let done = payloads(
            r#"{"type":"session.next.tool.success","data":{"callID":"c1","structured":{"ok":true}}}"#,
        );
        let AgentPayload::ToolResult {
            call: id, error, ..
        } = &done[0]
        else {
            panic!("expected a tool result, got {done:?}");
        };
        assert_eq!(id, "c1");
        assert!(error.is_none());
    }

    #[test]
    fn a_failed_tool_carries_why_rather_than_only_that_it_failed() {
        let out = payloads(
            r#"{"type":"session.next.tool.failed","data":{"callID":"c1","error":{"message":"permission denied"}}}"#,
        );
        let AgentPayload::ToolResult { error, .. } = &out[0] else {
            panic!("expected a tool result, got {out:?}");
        };
        assert_eq!(error.as_deref(), Some("permission denied"));
    }

    #[test]
    fn a_structured_question_becomes_a_card_rather_than_a_permission_prompt() {
        // This is what driving opencode over ACP loses: ACP has no word for a
        // question with its own options, so the card degrades to "allow?".
        let out = payloads(
            r#"{"type":"question.v2.asked","data":{"id":"que_1","sessionID":"ses_1",
                "tool":{"messageID":"msg_1","callID":"call_9"},
                "questions":[{"question":"Which database?","header":"Database","multiple":false,"custom":true,
                  "options":[{"label":"Postgres","description":"Already in the compose file"},
                             {"label":"SQLite","description":"No server to run"}]}]}}"#,
        );
        let AgentPayload::Question { call, questions } = &out[0] else {
            panic!("expected a question, got {out:?}");
        };
        // The tool call, not the question id: an answer has to be correlated
        // back to the thing that is actually waiting for it.
        assert_eq!(call, "call_9");
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].header, "Database");
        assert!(!questions[0].multi_select);
        assert!(questions[0].allow_free_text);
        assert_eq!(questions[0].options.len(), 2);
        assert_eq!(
            questions[0].options[1].description.as_deref(),
            Some("No server to run")
        );
    }

    #[test]
    fn a_question_with_no_tool_falls_back_to_its_own_id() {
        // Better than an empty correlation key, which would make two concurrent
        // questions indistinguishable.
        let out = payloads(r#"{"type":"question.v2.asked","data":{"id":"que_7","questions":[]}}"#);
        let AgentPayload::Question { call, .. } = &out[0] else {
            panic!("expected a question, got {out:?}");
        };
        assert_eq!(call, "que_7");
    }

    #[test]
    fn a_step_that_ended_reports_its_tokens_but_never_invents_a_price() {
        let out = payloads(
            r#"{"type":"session.next.step.ended","data":{"assistantMessageID":"msg_1","finish":"stop",
                "tokens":{"input":10,"output":20,"reasoning":0,"cache":{"read":5,"write":1}}}}"#,
        );
        assert!(matches!(out[0], AgentPayload::TurnEnd { .. }));
        let AgentPayload::Usage {
            input,
            cache_read,
            cost_usd,
            ..
        } = &out[1]
        else {
            panic!("expected usage, got {out:?}");
        };
        assert_eq!((*input, *cache_read), (10, 5));
        // opencode stated no cost, so omt states none. A computed price is a
        // wrong price the moment a plan differs.
        assert_eq!(*cost_usd, None);
    }

    #[test]
    fn a_queued_prompt_is_not_reported_as_somebody_typing() {
        let out = payloads(
            r#"{"type":"session.next.prompted","data":{"messageID":"msg_2","delivery":"queue"}}"#,
        );
        assert_eq!(
            out,
            vec![AgentPayload::TurnStart {
                turn: Some("msg_2".to_owned()),
                trigger: TurnTrigger::Queued
            }]
        );
    }

    #[test]
    fn a_sync_envelope_is_unwrapped_to_the_event_inside_it() {
        // The durable log wraps events and versions their names. Both arrive on
        // the same stream.
        let out = payloads(
            r#"{"type":"sync","id":"evt_1","syncEvent":{"type":"session.idle.1","id":"evt_2","seq":3,
                "aggregateID":"ses_1","data":{"sessionID":"ses_1"}}}"#,
        );
        assert!(matches!(
            out[0],
            AgentPayload::TurnEnd {
                outcome: TurnOutcome::Completed,
                ..
            }
        ));
    }

    #[test]
    fn an_event_omt_does_not_model_names_itself_rather_than_vanishing() {
        // opencode ships around ninety event types and omt maps a dozen on
        // purpose. A source that dropped the rest silently would look exactly
        // like one that had stopped working.
        assert_eq!(
            one(r#"{"type":"lsp.updated","data":{}}"#),
            Decoded::Unmapped {
                event: "lsp.updated".to_owned()
            }
        );
    }

    #[test]
    fn data_that_is_not_json_is_reported_with_its_contents() {
        assert_eq!(
            one("not json at all"),
            Decoded::Unparseable {
                data: "not json at all".to_owned()
            }
        );
    }

    #[test]
    fn the_stream_decodes_events_in_order_and_ignores_the_heartbeat() {
        // Verbatim shapes from a real `opencode serve` — including the bare
        // `data:` with no `event:` field and the `: heartbeat` comment, which
        // is what the connection actually sends when nothing is happening.
        let mut stream = EventStream::new();
        let out = stream.feed(concat!(
            "data: {\"id\":\"evt_1\",\"type\":\"server.connected\",\"data\":{}}\n",
            "\n",
            ": heartbeat\n",
            "\n",
            "data: {\"type\":\"session.next.text.delta\",\"data\":{\"delta\":\"hi\"}}\n",
            "\n",
        ));
        assert_eq!(out.len(), 2, "{out:?}");
        // `server.connected` is a real event omt has no word for, and says so.
        assert_eq!(
            out[0],
            Decoded::Unmapped {
                event: "server.connected".to_owned()
            }
        );
        assert!(matches!(out[1], Decoded::Payloads(_)));
    }

    #[test]
    fn an_event_split_across_chunks_arrives_once_and_whole() {
        let mut stream = EventStream::new();
        assert!(
            stream
                .feed("data: {\"type\":\"session.next.text.delta\",\"data\":")
                .is_empty()
        );
        let out = stream.feed("{\"delta\":\"whole\"}}\n\n");
        assert_eq!(
            out,
            vec![Decoded::Payloads(vec![AgentPayload::AssistantText {
                text: "whole".to_owned(),
                partial: true
            }])]
        );
    }

    #[test]
    fn the_event_url_is_built_off_whatever_base_the_server_reported() {
        assert_eq!(
            EventStream::url("http://127.0.0.1:4096"),
            "http://127.0.0.1:4096/api/event"
        );
        // A trailing slash on the base must not produce a double one.
        assert_eq!(
            EventStream::url("http://127.0.0.1:4096/"),
            "http://127.0.0.1:4096/api/event"
        );
    }
}
