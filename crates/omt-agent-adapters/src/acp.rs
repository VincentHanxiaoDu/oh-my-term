//! Speaking the Agent Client Protocol.
//!
//! JSON-RPC 2.0 over the agent's stdio. Five adapters declare `acp_spawn` and
//! until now nothing spoke the protocol, so declaring native mode meant
//! offering the user a mode that could not be entered.
//!
//! The framing is line-delimited JSON, which matters more than it sounds: a
//! response and a notification can arrive in either order, and a client that
//! assumed the next line answers its last request would attribute one turn's
//! result to another.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A JSON-RPC request, response or notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Present on requests and responses, absent on notifications.
    ///
    /// Its absence is what distinguishes a notification, so it is optional
    /// rather than defaulted — a default id would make every notification look
    /// like an answer to request zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// The method, on a request or notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// The parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// The result, on a successful response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// The error, on a failed response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// Its code.
    pub code: i32,
    /// What went wrong.
    pub message: String,
    /// Anything else the agent said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// What a message turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The agent answered something omt asked.
    Response {
        /// Which request.
        id: u64,
        /// What it said.
        result: Result<serde_json::Value, RpcError>,
    },
    /// The agent said something unprompted.
    Notification {
        /// Its method.
        method: String,
        /// Its parameters.
        params: serde_json::Value,
    },
    /// The agent asked omt something and expects an answer.
    ///
    /// This is the direction people forget: ACP is bidirectional, and a
    /// permission request arrives as a *request to the client*. A reader that
    /// treated it as a notification would leave the agent waiting forever.
    Request {
        /// Which request, so the reply can carry the same id.
        id: u64,
        /// Its method.
        method: String,
        /// Its parameters.
        params: serde_json::Value,
    },
}

/// Why a line could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AcpError {
    /// It was not JSON, or not a JSON-RPC message.
    #[error("not a JSON-RPC message: {0}")]
    Malformed(String),
    /// It claimed a version this build does not speak.
    #[error("JSON-RPC version `{0}` is not understood")]
    Version(String),
}

/// Classify one line from the agent.
///
/// # Errors
/// Fails if the line is not a JSON-RPC message this build understands.
pub fn parse(line: &str) -> Result<Incoming, AcpError> {
    // Bounded before parsing, like every other thing an agent writes.
    crate::bounded::check_structure(line).map_err(|e| AcpError::Malformed(e.to_string()))?;

    let message: Message =
        serde_json::from_str(line).map_err(|e| AcpError::Malformed(e.to_string()))?;
    if message.jsonrpc != "2.0" {
        return Err(AcpError::Version(message.jsonrpc));
    }

    match (message.id, message.method) {
        // An id and a method together: the agent is asking, not answering.
        (Some(id), Some(method)) => Ok(Incoming::Request {
            id,
            method,
            params: message.params.unwrap_or(serde_json::Value::Null),
        }),
        (Some(id), None) => Ok(Incoming::Response {
            id,
            result: match (message.result, message.error) {
                (_, Some(e)) => Err(e),
                (Some(r), None) => Ok(r),
                // Neither is not a valid response, but treating it as an empty
                // success would silently resolve a request with nothing.
                (None, None) => Err(RpcError {
                    code: -32_603,
                    message: "the agent answered with neither a result nor an error".to_owned(),
                    data: None,
                }),
            },
        }),
        (None, Some(method)) => Ok(Incoming::Notification {
            method,
            params: message.params.unwrap_or(serde_json::Value::Null),
        }),
        (None, None) => Err(AcpError::Malformed("no id and no method".to_owned())),
    }
}

/// Builds requests and matches their answers.
#[derive(Debug, Default)]
pub struct Rpc {
    next_id: u64,
    /// Which method each outstanding request used, so a late answer can be
    /// attributed rather than guessed at.
    pending: BTreeMap<u64, String>,
}

impl Rpc {
    /// A fresh connection.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: 1,
            pending: BTreeMap::new(),
        }
    }

    /// Build a request and remember it.
    pub fn request(&mut self, method: &str, params: serde_json::Value) -> (u64, String) {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, method.to_owned());
        let message = Message {
            jsonrpc: "2.0".to_owned(),
            id: Some(id),
            method: Some(method.to_owned()),
            params: Some(params),
            result: None,
            error: None,
        };
        (
            id,
            serde_json::to_string(&message).unwrap_or_default() + "\n",
        )
    }

    /// Build the reply to something the agent asked.
    #[must_use]
    pub fn respond(id: u64, result: serde_json::Value) -> String {
        let message = Message {
            jsonrpc: "2.0".to_owned(),
            id: Some(id),
            method: None,
            params: None,
            result: Some(result),
            error: None,
        };
        serde_json::to_string(&message).unwrap_or_default() + "\n"
    }

    /// Which method an answer belongs to, clearing it.
    ///
    /// Returns `None` for an id nothing is waiting on — an agent that answered
    /// twice, or answered something omt never asked. Neither is a reason to
    /// stop, and both are worth not acting on.
    pub fn resolve(&mut self, id: u64) -> Option<String> {
        self.pending.remove(&id)
    }

    /// How many requests are outstanding.
    #[must_use]
    pub fn outstanding(&self) -> usize {
        self.pending.len()
    }
}

/// The initialize request every ACP session opens with.
#[must_use]
pub fn initialize_params() -> serde_json::Value {
    serde_json::json!({
        "protocolVersion": 1,
        "clientCapabilities": {
            "fs": { "readTextFile": true, "writeTextFile": true },
            // Declared because omt can genuinely render one and deliver the
            // answer. Claiming a capability omt cannot honour would have the
            // agent wait on a permission nobody can grant.
            "terminal": true,
        },
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

    #[test]
    fn a_response_is_matched_to_its_request() {
        let mut rpc = Rpc::new();
        let (id, line) = rpc.request("session/prompt", serde_json::json!({ "prompt": "hi" }));
        assert!(line.ends_with('\n'), "framing is line-delimited");
        assert_eq!(rpc.outstanding(), 1);
        assert_eq!(rpc.resolve(id).as_deref(), Some("session/prompt"));
        assert_eq!(rpc.outstanding(), 0);
    }

    #[test]
    fn answers_arriving_out_of_order_go_to_the_right_request() {
        // A client that assumed the next line answers its last request would
        // attribute one turn's result to another.
        let mut rpc = Rpc::new();
        let (first, _) = rpc.request("session/prompt", serde_json::Value::Null);
        let (second, _) = rpc.request("session/cancel", serde_json::Value::Null);
        assert_eq!(rpc.resolve(second).as_deref(), Some("session/cancel"));
        assert_eq!(rpc.resolve(first).as_deref(), Some("session/prompt"));
    }

    #[test]
    fn an_answer_to_something_never_asked_is_ignored_rather_than_acted_on() {
        let mut rpc = Rpc::new();
        assert_eq!(rpc.resolve(999), None);
    }

    #[test]
    fn answering_twice_only_resolves_once() {
        // The second would otherwise re-run whatever the first triggered.
        let mut rpc = Rpc::new();
        let (id, _) = rpc.request("session/prompt", serde_json::Value::Null);
        assert!(rpc.resolve(id).is_some());
        assert!(rpc.resolve(id).is_none());
    }

    #[test]
    fn a_notification_is_told_apart_from_a_response() {
        // Absence of an id is the only difference, which is why the field is
        // optional rather than defaulted.
        let n = parse(r#"{"jsonrpc":"2.0","method":"session/update","params":{"a":1}}"#)
            .expect("parse");
        assert!(
            matches!(n, Incoming::Notification { ref method, .. } if method == "session/update")
        );

        let r = parse(r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}"#).expect("parse");
        assert!(matches!(r, Incoming::Response { id: 7, .. }));
    }

    #[test]
    fn a_request_from_the_agent_is_recognised_as_one() {
        // The direction people forget: ACP is bidirectional, and a permission
        // request arrives as a request *to the client*. Treating it as a
        // notification leaves the agent waiting forever.
        let m =
            parse(r#"{"jsonrpc":"2.0","id":3,"method":"session/request_permission","params":{}}"#)
                .expect("parse");
        let Incoming::Request { id, method, .. } = m else {
            panic!("{m:?}");
        };
        assert_eq!(id, 3);
        assert_eq!(method, "session/request_permission");
    }

    #[test]
    fn a_reply_carries_the_id_the_agent_asked_with() {
        // Any other id leaves the agent's request outstanding forever.
        let line = Rpc::respond(3, serde_json::json!({ "outcome": "allow" }));
        let back: Message = serde_json::from_str(line.trim()).expect("parse");
        assert_eq!(back.id, Some(3));
        assert!(back.method.is_none(), "a reply is not a request");
    }

    #[test]
    fn an_error_response_is_an_error_not_an_empty_success() {
        let m = parse(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}}"#)
            .expect("parse");
        let Incoming::Response { result, .. } = m else {
            panic!("{m:?}");
        };
        assert!(result.is_err());
    }

    #[test]
    fn a_response_with_neither_result_nor_error_is_treated_as_a_failure() {
        // Treating it as an empty success would silently resolve a request
        // with nothing, and the caller would act on it.
        let m = parse(r#"{"jsonrpc":"2.0","id":1}"#).expect("parse");
        let Incoming::Response { result, .. } = m else {
            panic!("{m:?}");
        };
        assert!(result.is_err());
    }

    #[test]
    fn a_wrong_protocol_version_is_refused_rather_than_guessed_at() {
        let err = parse(r#"{"jsonrpc":"1.0","id":1,"result":{}}"#).expect_err("must refuse");
        assert!(matches!(err, AcpError::Version(_)), "{err:?}");
    }

    #[test]
    fn a_line_that_is_not_a_message_is_refused() {
        assert!(parse("not json").is_err());
        assert!(
            parse(r#"{"jsonrpc":"2.0"}"#).is_err(),
            "no id and no method"
        );
    }

    #[test]
    fn an_over_structured_line_is_refused_before_parsing() {
        // Same bound as every other thing an agent writes: one line carries a
        // bomb as readily as a whole file.
        let bomb = format!(
            r#"{{"jsonrpc":"2.0","method":"x","params":{}}}"#,
            "[".repeat(crate::bounded::MAX_NESTING_DEPTH + 5)
        );
        assert!(matches!(parse(&bomb), Err(AcpError::Malformed(_))));
    }

    #[test]
    fn omt_only_declares_capabilities_it_can_honour() {
        // Claiming one it cannot would have the agent wait on a permission
        // nobody can grant.
        let params = initialize_params();
        assert_eq!(params["clientCapabilities"]["terminal"], true);
        assert_eq!(params["protocolVersion"], 1);
    }
}
