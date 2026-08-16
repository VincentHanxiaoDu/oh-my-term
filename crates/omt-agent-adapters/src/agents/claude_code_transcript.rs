//! One record of Claude Code's own session file, in omt's vocabulary.
//!
//! The file is JSON Lines. Most of its records are the agent's bookkeeping —
//! mode changes, permission modes, file-history snapshots, queue operations,
//! titles — and omt has no opinion on any of them. Two record types carry the
//! conversation, and they are the only two read here. Everything else produces
//! nothing, which is the ordinary answer rather than a failure.

use omt_events::{AgentPayload, MessageOrigin};

/// Map one record. Returns nothing for the records omt has no opinion on.
pub fn transcript_record(record: &serde_json::Value) -> Vec<AgentPayload> {
    // A sidechain is a subagent's conversation written into the same file.
    // Attributing it to the main thread would put a subagent's words in the
    // conversation somebody is reading, so it is dropped rather than guessed
    // at: the roster models threads, and this reader does not know which.
    if record
        .get("isSidechain")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Vec::new();
    }
    let kind = record.get("type").and_then(serde_json::Value::as_str);
    let Some(message) = record.get("message") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match record_content(message) {
        // A string body is a person's message written plainly, which is how
        // the first message of a conversation appears.
        Content::Text(text) if kind == Some("user") => out.push(AgentPayload::UserMessage {
            text,
            origin: MessageOrigin::Human,
        }),
        Content::Text(text) if kind == Some("assistant") => {
            out.push(AgentPayload::AssistantText {
                text,
                // Never partial. A file is written a message at a time, and
                // saying otherwise would have a surface wait for a rest that
                // is not coming.
                partial: false,
            });
        }
        Content::Blocks(blocks) => {
            for block in blocks {
                out.extend(content_block(&block, kind));
            }
        }
        Content::Text(_) | Content::None => {}
    }
    if let Some(usage) = message.get("usage") {
        out.extend(usage_of(usage));
    }
    out
}

/// The shapes a message body arrives in.
enum Content {
    Text(String),
    Blocks(Vec<serde_json::Value>),
    None,
}

fn record_content(message: &serde_json::Value) -> Content {
    match message.get("content") {
        Some(serde_json::Value::String(text)) => Content::Text(text.clone()),
        Some(serde_json::Value::Array(blocks)) => Content::Blocks(blocks.clone()),
        _ => Content::None,
    }
}

fn content_block(block: &serde_json::Value, kind: Option<&str>) -> Vec<AgentPayload> {
    let text = |k: &str| {
        block
            .get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match block.get("type").and_then(serde_json::Value::as_str) {
        Some("text") if kind == Some("user") => vec![AgentPayload::UserMessage {
            text: text("text"),
            origin: MessageOrigin::Human,
        }],
        Some("text") => vec![AgentPayload::AssistantText {
            text: text("text"),
            partial: false,
        }],
        Some("thinking") => vec![AgentPayload::Reasoning {
            text: text("thinking"),
            partial: false,
        }],
        Some("tool_use") => vec![AgentPayload::ToolCall {
            // The agent's own call id. It is what correlates this with its
            // result, and — because a hook reports the same id — what lets a
            // call already known from a stronger source be recognized rather
            // than shown twice.
            call: text("id"),
            name: text("name"),
            input: block
                .get("input")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        }],
        Some("tool_result") => vec![AgentPayload::ToolResult {
            call: text("tool_use_id"),
            output: block.get("content").cloned(),
            // The flag, not a guess from the text. Output that mentions an
            // error is not the same as a call that failed.
            error: (block.get("is_error").and_then(serde_json::Value::as_bool) == Some(true))
                .then(|| text("content")),
            // The file does not say, and a duration worked out from record
            // timestamps would be the gap between two writes, not the time a
            // tool took.
            duration_ms: None,
        }],
        // Images, and anything a later version adds. Dropped rather than
        // rendered as an empty message.
        _ => Vec::new(),
    }
}

fn usage_of(usage: &serde_json::Value) -> Option<AgentPayload> {
    let n = |k: &str| {
        usage
            .get(k)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let (input, output) = (n("input_tokens"), n("output_tokens"));
    let (cache_read, cache_write) = (
        n("cache_read_input_tokens"),
        n("cache_creation_input_tokens"),
    );
    // A record with no usage block at all is every user message; reporting
    // four zeros for each would make a session's totals look like activity.
    (input | output | cache_read | cache_write != 0).then_some(AgentPayload::Usage {
        input,
        output,
        cache_read,
        cache_write,
        // Only if the agent stated one, and it does not state one here.
        cost_usd: None,
    })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn read(record: serde_json::Value) -> Vec<AgentPayload> {
        transcript_record(&record)
    }

    #[test]
    fn a_person_message_is_what_the_person_said() {
        let out = read(serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": "run the tests"}
        }));
        assert!(matches!(
            out.as_slice(),
            [AgentPayload::UserMessage { text, origin: MessageOrigin::Human }] if text == "run the tests"
        ));
    }

    #[test]
    fn an_assistant_turn_carries_its_words_its_reasoning_and_its_calls() {
        let out = read(serde_json::json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "weighing it", "signature": "s"},
                {"type": "text", "text": "running them now"},
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "cargo test"}}
            ]}
        }));
        assert_eq!(out.len(), 3);
        assert!(matches!(out[0], AgentPayload::Reasoning { .. }));
        assert!(matches!(
            out[1],
            AgentPayload::AssistantText { partial: false, .. }
        ));
        let AgentPayload::ToolCall { call, name, input } = &out[2] else {
            panic!("a tool call, got {:?}", out[2]);
        };
        assert_eq!(call, "toolu_1");
        assert_eq!(name, "Bash");
        assert_eq!(input["command"], "cargo test");
    }

    #[test]
    fn a_result_correlates_by_the_agents_own_call_id() {
        // The same id a hook reports. That is what makes "already known" a
        // fact rather than a comparison of two pieces of text.
        let out = read(serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok", "is_error": false}
            ]}
        }));
        assert!(matches!(
            out.as_slice(),
            [AgentPayload::ToolResult { call, error: None, .. }] if call == "toolu_1"
        ));
    }

    #[test]
    fn a_failed_tool_says_so_from_the_flag_not_from_its_text() {
        // Output that mentions an error is not a call that failed, and the
        // difference is what a person scanning a conversation relies on.
        let out = read(serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t", "content": "error: no such file", "is_error": true}
            ]}
        }));
        assert!(matches!(
            out.as_slice(),
            [AgentPayload::ToolResult { error: Some(_), .. }]
        ));
        let fine = read(serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t", "content": "no errors found"}
            ]}
        }));
        assert!(matches!(
            fine.as_slice(),
            [AgentPayload::ToolResult { error: None, .. }]
        ));
    }

    #[test]
    fn what_the_turn_spent_is_taken_from_the_agents_own_count() {
        let out = read(serde_json::json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": [], "usage": {
                "input_tokens": 12, "output_tokens": 34,
                "cache_read_input_tokens": 5, "cache_creation_input_tokens": 6
            }}
        }));
        assert!(matches!(
            out.as_slice(),
            [AgentPayload::Usage {
                input: 12,
                output: 34,
                cache_read: 5,
                cache_write: 6,
                cost_usd: None
            }]
        ));
    }

    #[test]
    fn a_turn_that_spent_nothing_reports_no_usage_at_all() {
        // Four zeros per message would make a quiet session's totals look
        // like activity.
        let out = read(serde_json::json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}], "usage": {}}
        }));
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_subagents_words_do_not_join_the_conversation() {
        // A sidechain is written into the same file. The roster models
        // threads; this reader does not know which one, so it says nothing
        // rather than attributing it to the thread somebody is reading.
        let out = read(serde_json::json!({
            "type": "assistant",
            "isSidechain": true,
            "message": {"role": "assistant", "content": [{"type": "text", "text": "from a subagent"}]}
        }));
        assert!(out.is_empty());
    }

    #[test]
    fn the_bookkeeping_records_produce_nothing() {
        // Most of the file. "Nothing to say" is the ordinary answer here, and
        // treating it as an error would fill a log with the common case.
        for record in [
            serde_json::json!({"type": "mode", "mode": "normal"}),
            serde_json::json!({"type": "file-history-snapshot", "snapshot": {}}),
            serde_json::json!({"type": "queue-operation", "op": "add"}),
            serde_json::json!({"type": "ai-title", "title": "something"}),
            serde_json::json!({"type": "assistant"}),
        ] {
            assert!(read(record.clone()).is_empty(), "{record}");
        }
    }

    #[test]
    fn an_attachment_block_is_dropped_rather_than_shown_as_an_empty_line() {
        let out = read(serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "image", "source": {}}]}
        }));
        assert!(out.is_empty());
    }
}
