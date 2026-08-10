//! Who is connected, what they have done, and how the process is bearing up.
//!
//! Three things the documents describe and nothing recorded.
//!
//! **Presence** (`docs/architecture/12-collaboration.md` §2) is first-class
//! state: "is somebody else looking at this" is a question people ask before
//! they type, and a terminal that cannot answer it is one where two people
//! interleave keystrokes into the same shell and each blames the other.
//!
//! **The audit log** (§8) is what makes a shared credential accountable. Every
//! call already carries an actor and an intent id; nothing wrote them down.
//!
//! **Metrics** (`22-operations.md` §4) is the difference between "the process
//! is up" — which `/api/health` says, and is all it says — and knowing whether
//! it is doing anything.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use omt_types::{Role, Timestamp};
use serde::Serialize;

/// One connected client.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Connected {
    /// Its connection id, unique for as long as it is connected.
    pub id: String,
    /// What the client called itself in the handshake.
    pub client: String,
    /// What it may do.
    pub role: String,
    /// When it connected.
    pub since: String,
}

/// One thing somebody did.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Audited {
    /// When.
    pub at: String,
    /// Which capability.
    pub capability: String,
    /// Which connection.
    pub client: String,
    /// The intent id, where the call carried one — which is what lets a repeat
    /// be recognised as the same intent rather than counted as a second.
    pub intent: Option<String>,
    /// Whether it succeeded.
    pub ok: bool,
}

/// How many audit entries are kept.
///
/// Bounded, because this is in memory in a process expected to run for days.
/// Old entries are dropped from the front and the drop is counted, so a reader
/// is never shown a partial log as if it were whole.
pub const AUDIT_LIMIT: usize = 2_000;

/// What the instance has seen.
#[derive(Debug, Default)]
pub struct Watch {
    connected: BTreeMap<String, Connected>,
    audit: Vec<Audited>,
    dropped: u64,
    calls: u64,
    failures: u64,
    started: Option<Timestamp>,
}

/// Shared, because the socket layer writes it and a capability reads it.
pub type Shared = Arc<Mutex<Watch>>;

impl Watch {
    /// Note the process starting.
    pub fn started(&mut self, at: Timestamp) {
        self.started.get_or_insert(at);
    }

    /// Note a client arriving.
    pub fn arrived(&mut self, id: &str, client: &str, role: Role, at: Timestamp) {
        self.connected.insert(
            id.to_owned(),
            Connected {
                id: id.to_owned(),
                client: client.to_owned(),
                role: format!("{role:?}").to_lowercase(),
                since: at.to_string(),
            },
        );
    }

    /// Note a client leaving.
    ///
    /// Removed rather than marked gone: presence is about who is here now, and
    /// a list that accumulates everybody who ever connected answers a different
    /// question badly.
    pub fn left(&mut self, id: &str) {
        self.connected.remove(id);
    }

    /// Who is here.
    #[must_use]
    pub fn present(&self) -> Vec<Connected> {
        self.connected.values().cloned().collect()
    }

    /// Record a call.
    pub fn record(&mut self, entry: Audited) {
        if !entry.ok {
            self.failures += 1;
        }
        self.calls += 1;
        self.audit.push(entry);
        if self.audit.len() > AUDIT_LIMIT {
            let over = self.audit.len() - AUDIT_LIMIT;
            self.audit.drain(..over);
            self.dropped += over as u64;
        }
    }

    /// The log, newest last.
    #[must_use]
    pub fn audited(&self) -> &[Audited] {
        &self.audit
    }

    /// How many entries fell off the front.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Calls seen, and how many failed.
    #[must_use]
    pub const fn counts(&self) -> (u64, u64) {
        (self.calls, self.failures)
    }

    /// When the process started, where it said.
    #[must_use]
    pub const fn started_at(&self) -> Option<Timestamp> {
        self.started
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

    fn record() -> Audited {
        Audited {
            at: "2026-01-01T00:00:00Z".to_owned(),
            capability: "session.write_bytes".to_owned(),
            client: "device-1".to_owned(),
            intent: Some("intent-1".to_owned()),
            ok: true,
        }
    }

    #[test]
    fn an_audit_record_carries_no_free_text_to_leak() {
        // 13 §8's guarantee is that no path emits an unredacted value. The
        // audit log is the easiest one to get wrong, because "also log the
        // arguments" is the obvious next feature request and the arguments are
        // where the secrets are.
        //
        // The defence is shape rather than filtering: the record has no field
        // that can hold caller-supplied text at all. The capability name comes
        // from the catalog, the ids are generated, `ok` is a bool. A field that
        // could hold a password fails this test before it can fail somebody's
        // threat model.
        let json = serde_json::to_value(record()).expect("json");
        let object = json.as_object().expect("an object");
        assert_eq!(
            object.len(),
            5,
            "a field was added to the audit record: {object:?}"
        );
        for key in ["at", "capability", "client", "intent", "ok"] {
            assert!(object.contains_key(key), "`{key}` went missing");
        }
    }

    #[test]
    fn the_audit_log_records_the_capability_not_its_input() {
        // The whole distinction. "`session.write_bytes` was called" is an
        // audit record; "`session.write_bytes` was called with these bytes" is
        // a keylogger.
        let json = serde_json::to_string(&record()).expect("json");
        assert!(json.contains("session.write_bytes"));
        assert!(!json.contains("input"), "{json}");
        assert!(!json.contains("payload"), "{json}");
    }
}
