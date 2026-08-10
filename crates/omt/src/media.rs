//! Reaching the blob store.
//!
//! `docs/architecture/09-ssh-and-media.md` §2. The store itself lives in
//! `omt-media`; without these it would be the sixth thing in this project that
//! was written, tested and unreachable.
//!
//! Bytes cross the wire base64-encoded. That is not free, and it is still the
//! right answer here: the alternative is a second binary framing beside the
//! terminal stream, and one more framing is one more place for a length prefix
//! to disagree with its payload.

use omt_catalog::{CapabilityError, CapabilityHandler};
use omt_media::store::{BlobStore, Class, Origin};
use serde::{Deserialize, Serialize};

/// The store, shared.
pub type Shared = std::sync::Arc<std::sync::Mutex<BlobStore>>;

/// Input to `media.put`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PutIn {
    /// The bytes, base64.
    pub content: String,
    /// Where they came from.
    ///
    /// Taken from the caller because only the caller knows, and recorded rather
    /// than acted on: what may be done with a blob depends on where it came
    /// from, and that decision belongs at the point of use.
    pub origin: String,
}

/// What `media.put` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PutOut {
    /// Its content hash, which is its name.
    pub digest: String,
    /// How many bytes.
    pub size: usize,
    /// What it appears to be, sniffed rather than declared.
    pub kind: String,
    /// Its media type, where one could be determined.
    pub media_type: Option<String>,
}

omt_catalog::capability! {
    /// Land some bytes in the blob store.
    pub struct MediaPut;
    input  = PutIn,
    output = PutOut,
    decl = omt_catalog::Decl {
        name: "media.put",
        group: "media",
        verb: "put",
        title: "Upload a blob",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_FS,
        // Cas: content-addressed, so a retried upload lands on the same digest
        // and returns the same answer. A paste retried after a dropped
        // acknowledgement must not consume the quota twice.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The type is sniffed here and never taken from the sender. A sender that names its own content type decides how the receiver treats it.",
    },
}

/// Serves the blob store.
pub struct MediaHandler(pub Shared);

/// Decode base64 without pulling in a dependency for it.
fn from_base64(text: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        let value = TABLE.iter().position(|c| *c == byte)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// Encode base64, likewise.
fn to_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = |i: usize| chunk.get(i).copied().unwrap_or(0) as u32;
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn origin_of(name: &str) -> Origin {
    match name {
        "web_upload" => Origin::WebUpload,
        "osc_bridge" => Origin::OscBridge,
        "agent_output" => Origin::AgentOutput,
        _ => Origin::LocalClipboard,
    }
}

impl CapabilityHandler<MediaPut> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: PutIn,
    ) -> Result<PutOut, CapabilityError> {
        let bytes = from_base64(&input.content)
            .ok_or_else(|| CapabilityError::invalid_input("the content is not valid base64"))?;
        let mut store = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the blob lock was poisoned"))?;
        let meta = store
            .put(&bytes, origin_of(&input.origin), Class::Runtime)
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(PutOut {
            digest: meta.digest,
            size: meta.size,
            kind: format!("{:?}", meta.kind).to_lowercase(),
            media_type: meta.media_type,
        })
    }
}

/// Input to `media.get`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetIn {
    /// Which blob.
    pub digest: String,
}

/// What `media.get` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct GetOut {
    /// The bytes, base64.
    pub content: String,
    /// How many bytes they decode to.
    pub size: usize,
    /// What it appears to be.
    pub kind: String,
}

omt_catalog::capability! {
    /// Fetch a blob by its digest.
    pub struct MediaGet;
    input  = GetIn,
    output = GetOut,
    decl = omt_catalog::Decl {
        name: "media.get",
        group: "media",
        verb: "get",
        title: "Fetch a blob",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "By digest, which is the only name a blob has. There is no path to traverse because there is no path.",
    },
}

impl CapabilityHandler<MediaGet> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: GetIn,
    ) -> Result<GetOut, CapabilityError> {
        let store = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the blob lock was poisoned"))?;
        let bytes = store
            .get(&input.digest)
            .map_err(|e| CapabilityError::not_found(e.to_string()))?;
        let kind = store
            .meta(&input.digest)
            .map_or_else(|| "binary".to_owned(), |r| {
                format!("{:?}", r.meta.kind).to_lowercase()
            });
        Ok(GetOut {
            content: to_base64(bytes),
            size: bytes.len(),
            kind,
        })
    }
}

/// Input to `media.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListIn {}

/// One blob, as the list shows it.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Landed {
    /// Its digest.
    pub digest: String,
    /// How many bytes.
    pub size: usize,
    /// What it appears to be.
    pub kind: String,
    /// Where it came from.
    pub origin: String,
    /// How many things are holding it open.
    pub refs: u32,
}

/// What `media.list` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListOut {
    /// Every blob, newest first.
    pub blobs: Vec<Landed>,
    /// How many bytes are held in total.
    pub used: u64,
}

omt_catalog::capability! {
    /// Everything currently in the landing zone.
    pub struct MediaList;
    input  = ListIn,
    output = ListOut,
    decl = omt_catalog::Decl {
        name: "media.list",
        group: "media",
        verb: "list",
        title: "List blobs",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A landing zone nobody can see the contents of is one that has quietly become the place things live.",
    },
}

impl CapabilityHandler<MediaList> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: ListIn,
    ) -> Result<ListOut, CapabilityError> {
        let store = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the blob lock was poisoned"))?;
        Ok(ListOut {
            used: store.used(),
            blobs: store
                .list()
                .into_iter()
                .map(|r| Landed {
                    digest: r.meta.digest.clone(),
                    size: r.meta.size,
                    kind: format!("{:?}", r.meta.kind).to_lowercase(),
                    origin: format!("{:?}", r.origin).to_lowercase(),
                    refs: r.refs,
                })
                .collect(),
        })
    }
}

/// Input to `media.clipboard`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClipIn {
    /// The text to put on the local clipboard.
    pub text: String,
    /// The terminal the human is actually sitting in front of.
    ///
    /// Supplied by the caller because only the client knows it. Keyed off
    /// `TERM` rather than `$TMUX`, which does not survive ssh — getting that
    /// backwards yields a copy that works locally and silently does nothing
    /// remotely.
    #[serde(default)]
    pub term: Option<String>,
}

/// What `media.clipboard` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ClipOut {
    /// The bytes to write into the session's output stream.
    pub sequence: String,
    /// What the surface may say about it.
    ///
    /// Not "copied". OSC 52 has no acknowledgement, Terminal.app does not
    /// implement it at all, and kitty refuses silently above its own limit.
    pub message: String,
}

omt_catalog::capability! {
    /// Put text on the local clipboard, across ssh.
    pub struct MediaClipboard;
    input  = ClipIn,
    output = ClipOut,
    decl = omt_catalog::Decl {
        name: "media.clipboard",
        group: "media",
        verb: "clipboard",
        title: "Copy to local clipboard",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: Some(omt_catalog::Intent::LwwFreeText),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "OSC 52, wrapped for tmux when TERM says so. omt does not claim the copy succeeded: there is no acknowledgement to base that on.",
    },
}

impl CapabilityHandler<MediaClipboard> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ClipIn,
    ) -> Result<ClipOut, CapabilityError> {
        let term = input.term.unwrap_or_else(|| "xterm-256color".to_owned());
        let sequence = omt_media::clipboard::osc52(
            &input.text,
            &term,
            omt_media::clipboard::DEFAULT_MAX_BYTES,
        )
        .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(ClipOut {
            sequence,
            message: omt_media::clipboard::what_to_say().to_owned(),
        })
    }
}

// ---------------------------------------------------------------------------
// Handing a blob to an agent — §7.1
// ---------------------------------------------------------------------------

/// How one agent wants to be told about a file.
///
/// Per-adapter data, not a global format. Every one of these was observed from
/// the agent's own interface rather than guessed: `@"path"` is what Claude
/// Code's own TUI writes for a pasted image, `/add` is Aider's documented
/// command. A single format applied to all of them would be silently ignored
/// by most, which looks exactly like an agent choosing not to read the file.
#[must_use]
pub fn attachment_reference(agent: &str, path: &str) -> (String, &'static str) {
    match agent {
        // Quoted, because a path with a space in it is otherwise two arguments
        // and the reference resolves to nothing.
        "claudecode" | "claude" => (format!("@\"{path}\""), "high"),
        "aider" => (format!("/add {path}"), "high"),
        "geminicli" | "gemini" | "qwencode" | "qwen" => (format!("@{path}"), "medium"),
        "codex" => (format!("{path}"), "medium"),
        // The only ones with a real structured attachment channel. A path in
        // the prompt would work less well than the channel they already have.
        "opencode" => (path.to_owned(), "high"),
        // The fallback is a bare absolute path with a human sentence in front,
        // because an agent that does not recognise any syntax still reads
        // English and can be asked to open a path.
        _ => (
            format!("Please look at this file: {path}"),
            "fallback",
        ),
    }
}

/// Input to `media.attach`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AttachIn {
    /// Which agent is being told.
    pub agent: String,
    /// The absolute path the blob was materialised at.
    pub path: String,
}

/// What `media.attach` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AttachOut {
    /// The text to inject, in the agent's own syntax.
    pub reference: String,
    /// How well established this form is, rather than a claim it will work.
    pub confidence: String,
}

omt_catalog::capability! {
    /// How to reference a file to a particular agent.
    pub struct MediaAttach;
    input  = AttachIn,
    output = AttachOut,
    decl = omt_catalog::Decl {
        name: "media.attach",
        group: "media",
        verb: "attach",
        title: "Reference a file to an agent",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Per-adapter, because one format applied to all of them is silently ignored by most — which looks exactly like an agent choosing not to read the file.",
    },
}

impl CapabilityHandler<MediaAttach> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: AttachIn,
    ) -> Result<AttachOut, CapabilityError> {
        if !input.path.starts_with('/') {
            return Err(CapabilityError::invalid_input(
                "an attachment reference needs an absolute path; a relative one resolves against whatever directory the agent happens to be in",
            ));
        }
        let (reference, confidence) =
            attachment_reference(&input.agent.to_lowercase().replace(['-', '_'], ""), &input.path);
        Ok(AttachOut {
            reference,
            confidence: confidence.to_owned(),
        })
    }
}

/// Input to `media.paste_tier`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TierIn {
    /// omt answered on the far side of the connection.
    #[serde(default)]
    pub omt_on_both_ends: bool,
    /// A forwarded socket exists *and* an omt instance answered on it.
    #[serde(default)]
    pub reverse_socket_answered: bool,
    /// A companion on the near side acknowledged the OSC handshake.
    #[serde(default)]
    pub osc_companion_answered: bool,
    /// The terminal replied about its own upload protocol.
    #[serde(default)]
    pub terminal_upload_confirmed: bool,
}

/// What `media.paste_tier` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TierOut {
    /// Which mechanism is actually available.
    pub tier: String,
    /// Why paste behaves the way it does, in words for a person.
    pub explanation: String,
}

omt_catalog::capability! {
    /// Which paste mechanism this connection actually achieved.
    pub struct PasteTier;
    input  = TierIn,
    output = TierOut,
    decl = omt_catalog::Decl {
        name: "media.paste_tier",
        group: "media",
        verb: "paste_tier",
        title: "How paste reaches this machine",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A tier is never claimed without a positive handshake. `Probably` means a paste that silently does nothing, and no way for the user to tell whether they mis-copied or omt mis-guessed.",
    },
}

impl CapabilityHandler<PasteTier> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: TierIn,
    ) -> Result<TierOut, CapabilityError> {
        let probe = omt_media::tier::Probe {
            omt_on_both_ends: input.omt_on_both_ends,
            reverse_socket_answered: input.reverse_socket_answered,
            osc_companion_answered: input.osc_companion_answered,
            terminal_upload_confirmed: input.terminal_upload_confirmed,
        };
        let tier = omt_media::tier::achieved(&probe);
        Ok(TierOut {
            tier: format!("{tier:?}").to_lowercase(),
            explanation: tier.explain().to_owned(),
        })
    }
}

/// Input to `ssh.plan`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SshPlanIn {
    /// The ssh target.
    pub target: String,
    /// Whether a probe found omt on the far side.
    #[serde(default)]
    pub remote_has_omt: bool,
    /// What to do if it did not.
    #[serde(default)]
    pub bootstrap: Option<String>,
}

/// What `ssh.plan` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SshPlanOut {
    /// How the session would be run.
    pub mode: String,
    /// The argument vector, with no shell involved.
    pub argv: Vec<String>,
    /// Why, in words somebody can act on.
    pub reason: String,
    /// Whether paste crosses directly, or goes through the escape bridge.
    pub direct_media: bool,
}

omt_catalog::capability! {
    /// What `omt ssh <target>` would run, and why.
    pub struct SshPlan;
    input  = SshPlanIn,
    output = SshPlanOut,
    decl = omt_catalog::Decl {
        name: "ssh.plan",
        group: "ssh",
        verb: "plan",
        title: "Preview an omt ssh session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "An argument vector, never a shell string: a hostname from a config file with a quote in it would otherwise become remote execution.",
    },
}

impl CapabilityHandler<SshPlan> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: SshPlanIn,
    ) -> Result<SshPlanOut, CapabilityError> {
        let bootstrap = match input.bootstrap.as_deref() {
            Some("never") => crate::thin::Bootstrap::Never,
            Some("assume_installed") => crate::thin::Bootstrap::AssumeInstalled,
            _ => crate::thin::Bootstrap::Ask,
        };
        let plan = crate::thin::plan(&input.target, input.remote_has_omt, bootstrap)
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(SshPlanOut {
            mode: format!("{:?}", plan.mode).to_lowercase(),
            direct_media: plan.direct_media(),
            argv: plan.argv,
            reason: plan.reason,
        })
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

    #[test]
    fn claude_code_gets_the_form_its_own_tui_writes() {
        let (reference, confidence) = attachment_reference("claudecode", "/tmp/omt/shot.png");
        assert_eq!(reference, "@\"/tmp/omt/shot.png\"");
        assert_eq!(confidence, "high");
    }

    #[test]
    fn a_path_with_a_space_is_quoted_where_the_syntax_needs_it() {
        // Otherwise it is two arguments and the reference resolves to nothing.
        let (reference, _) = attachment_reference("claudecode", "/tmp/my shot.png");
        assert!(reference.contains("\"/tmp/my shot.png\""), "{reference}");
    }

    #[test]
    fn aider_gets_its_documented_command() {
        let (reference, confidence) = attachment_reference("aider", "/tmp/a.png");
        assert_eq!(reference, "/add /tmp/a.png");
        assert_eq!(confidence, "high");
    }

    #[test]
    fn the_file_inclusion_agents_get_their_own_syntax() {
        for agent in ["gemini", "geminicli", "qwen", "qwencode"] {
            let (reference, _) = attachment_reference(agent, "/tmp/a.png");
            assert_eq!(reference, "@/tmp/a.png", "{agent}");
        }
    }

    #[test]
    fn an_agent_omt_does_not_know_is_asked_in_english() {
        // One that recognises no syntax still reads English and can be asked
        // to open a path. Reported as a fallback rather than as a confident
        // format, because it is one.
        let (reference, confidence) = attachment_reference("something-new", "/tmp/a.png");
        assert!(reference.contains("/tmp/a.png"));
        assert!(reference.contains("look at"));
        assert_eq!(confidence, "fallback");
    }

    #[test]
    fn every_reference_contains_the_path_it_is_about() {
        // A reference that lost the path is one the agent cannot act on, and
        // the failure is silent.
        for agent in ["claudecode", "aider", "gemini", "codex", "opencode", "whatever"] {
            let (reference, _) = attachment_reference(agent, "/tmp/needle.png");
            assert!(reference.contains("/tmp/needle.png"), "{agent}: {reference}");
        }
    }

    #[test]
    fn base64_round_trips_including_the_padded_lengths() {
        // The lengths that need padding are exactly where a hand-rolled codec
        // goes wrong, so all three residues are covered.
        for bytes in [
            &b"a"[..],
            &b"ab"[..],
            &b"abc"[..],
            &b"abcd"[..],
            &[0u8, 255, 128, 1, 2, 3, 4][..],
        ] {
            let encoded = to_base64(bytes);
            assert_eq!(
                from_base64(&encoded).expect("decode"),
                bytes,
                "round trip of {} bytes",
                bytes.len()
            );
        }
    }

    #[test]
    fn something_that_is_not_base64_is_refused_rather_than_decoded_to_rubbish() {
        assert!(from_base64("not base64!").is_none());
    }
}
