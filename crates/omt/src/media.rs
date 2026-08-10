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

// ---------------------------------------------------------------------------
// Resolving a recognised target — 18 §3, §4
// ---------------------------------------------------------------------------

/// Input to `open.resolve`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveIn {
    /// The path as it was recognised.
    pub path: String,
    /// A line, where the text carried one.
    #[serde(default)]
    pub line: Option<u32>,
    /// The roots it may resolve against.
    ///
    /// Passed in rather than taken from the daemon's open workspaces, because
    /// the caller knows which pane the text came from and the daemon does not.
    pub roots: Vec<String>,
}

/// What `open.resolve` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ResolveOut {
    /// The absolute path.
    pub path: String,
    /// Which root it resolved against, so a wrong answer is visible.
    pub root: Option<String>,
    /// Whether it is there — or was not looked for.
    pub existence: String,
    /// A line, where there was one.
    pub line: Option<u32>,
    /// What can be done, best first.
    pub actions: Vec<Offer>,
}

/// One thing that could be done.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Offer {
    /// Which handler.
    pub handler: String,
    /// What to call it in a menu.
    pub label: String,
    /// Higher is a better default.
    pub rank: i16,
    /// Whether it changes anything, so a confirm sheet is decidable without
    /// running it.
    pub mutates: bool,
}

omt_catalog::capability! {
    /// Turn a recognised path into a concrete target, or an honest failure.
    pub struct OpenResolve;
    input  = ResolveIn,
    output = ResolveOut,
    decl = omt_catalog::Decl {
        name: "open.resolve",
        group: "open",
        verb: "resolve",
        title: "Resolve a recognised path",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A path resolved against the wrong root opens somebody else's file, so the root that was used is part of the answer and anything escaping every root is refused rather than clamped.",
    },
}

impl CapabilityHandler<OpenResolve> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ResolveIn,
    ) -> Result<ResolveOut, CapabilityError> {
        let roots: Vec<std::path::PathBuf> =
            input.roots.iter().map(std::path::PathBuf::from).collect();
        let resolved = omt_open::resolve::resolve(&input.path, input.line, &roots, true)
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        Ok(ResolveOut {
            path: resolved.path,
            root: resolved.root,
            existence: format!("{:?}", resolved.existence).to_lowercase(),
            line: resolved.line,
            actions: resolved
                .actions
                .into_iter()
                .map(|a| Offer {
                    handler: a.handler,
                    label: a.label,
                    rank: a.rank,
                    mutates: a.mutates,
                })
                .collect(),
        })
    }
}

/// Input to `open.click`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClickIn {
    /// Whether the program in the pane has enabled mouse reporting.
    #[serde(default)]
    pub mouse_reporting: bool,
    /// Whether Shift was held.
    #[serde(default)]
    pub shift: bool,
    /// Cmd on macOS, Super elsewhere.
    #[serde(default)]
    pub super_key: bool,
    /// Alt / Option.
    #[serde(default)]
    pub alt: bool,
}

/// What `open.click` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ClickOut {
    /// Who owns this click.
    pub owner: String,
    /// Whether omt took it, or the program under it did.
    pub consumed: bool,
}

omt_catalog::capability! {
    /// Who owns a click at these coordinates.
    pub struct OpenClick;
    input  = ClickIn,
    output = ClickOut,
    decl = omt_catalog::Decl {
        name: "open.click",
        group: "open",
        verb: "click",
        title: "Who owns this click",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "When the program in the pane has mouse reporting on, every press is its input. A multiplexer that swallows clicks so its own link handler can run has broken vim, and no link feature is worth that.",
    },
}

impl CapabilityHandler<OpenClick> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ClickIn,
    ) -> Result<ClickOut, CapabilityError> {
        let owner = omt_open::click::owner(
            input.mouse_reporting,
            omt_open::click::Modifiers {
                shift: input.shift,
                super_key: input.super_key,
                alt: input.alt,
            },
        );
        Ok(ClickOut {
            owner: format!("{owner:?}").to_lowercase(),
            consumed: omt_open::click::consumed(owner),
        })
    }
}

/// Input to `open.rules_list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RulesListIn {}

/// One recognition rule.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RuleInfo {
    /// Its id.
    pub id: String,
    /// Higher wins.
    pub precedence: u16,
    /// What it recognises, in words.
    pub about: String,
}

/// What `open.rules_list` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RulesListOut {
    /// Every rule, in the order they are tried.
    pub rules: Vec<RuleInfo>,
}

omt_catalog::capability! {
    /// Every recognition rule omt runs.
    pub struct OpenRulesList;
    input  = RulesListIn,
    output = RulesListOut,
    decl = omt_catalog::Decl {
        name: "open.rules_list",
        group: "open",
        verb: "rules_list",
        title: "List recognition rules",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A recognition system nobody can enumerate is one where `why did that not become a link` has no answer short of reading the source.",
    },
}

impl CapabilityHandler<OpenRulesList> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: RulesListIn,
    ) -> Result<RulesListOut, CapabilityError> {
        Ok(RulesListOut {
            rules: omt_open::rules::catalog()
                .into_iter()
                .map(|r| RuleInfo {
                    id: r.id.to_owned(),
                    precedence: r.precedence,
                    about: r.about.to_owned(),
                })
                .collect(),
        })
    }
}

/// Input to `open.rules_test`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RulesTestIn {
    /// The line to try.
    pub text: String,
}

/// What `open.rules_test` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RulesTestOut {
    /// Which rule fired, if any.
    ///
    /// Named rather than a bare boolean: "it matched" is not an answer
    /// somebody can use to fix a rule that did not.
    pub rule: Option<String>,
    /// What was recognised, if anything.
    pub matches: Vec<OpenMatchOut>,
}

/// One match, as the preview shows it.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct OpenMatchOut {
    /// What kind of thing.
    pub kind: String,
    /// The text of it.
    pub target: String,
    /// Byte offset of the first character.
    pub start: usize,
    /// One past the last.
    pub end: usize,
    /// A line number, where the text carried one.
    pub line_number: Option<u32>,
}

omt_catalog::capability! {
    /// Try the recognition rules against a line and say what fired.
    pub struct OpenRulesTest;
    input  = RulesTestIn,
    output = RulesTestOut,
    decl = omt_catalog::Decl {
        name: "open.rules_test",
        group: "open",
        verb: "rules_test",
        title: "Test a line against the rules",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Powers the config editor's live preview. Names the rule that fired, because a preview that only says whether something matched cannot help fix one that did not.",
    },
}

impl CapabilityHandler<OpenRulesTest> for MediaHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RulesTestIn,
    ) -> Result<RulesTestOut, CapabilityError> {
        Ok(RulesTestOut {
            rule: omt_open::rules::which(&input.text).map(str::to_owned),
            matches: omt_open::recognize(&input.text)
                .into_iter()
                .map(|m| {
                    let (kind, target, line_number) = match &m.target {
                        omt_open::Target::Path { raw, line, .. } => {
                            ("path".to_owned(), raw.clone(), *line)
                        }
                        omt_open::Target::Url { raw } => ("url".to_owned(), raw.clone(), None),
                    };
                    OpenMatchOut {
                        kind,
                        target,
                        start: m.start,
                        end: m.end,
                        line_number,
                    }
                })
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Remote mirrors — 18 §6.3
// ---------------------------------------------------------------------------

/// Every mirror this instance holds, keyed by local path.
pub type Mirrors = std::sync::Arc<
    std::sync::Mutex<std::collections::BTreeMap<String, omt_media::mirror::Mirror>>,
>;

/// Input to `open.remote_list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoteListIn {}

/// One mirrored file.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MirrorOut {
    /// Where it is locally.
    pub local_path: String,
    /// Where it came from.
    pub remote_path: String,
    /// Which host.
    pub host: String,
    /// When it was fetched.
    pub fetched_at: u64,
    /// A hash of what was fetched, so staleness is checkable.
    pub digest: String,
    /// Whether omt will refuse to write it back.
    pub read_only: bool,
}

/// What `open.remote_list` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RemoteListOut {
    /// Every mirror, newest first.
    pub files: Vec<MirrorOut>,
}

omt_catalog::capability! {
    /// Every remote file mirrored locally.
    pub struct RemoteList;
    input  = RemoteListIn,
    output = RemoteListOut,
    decl = omt_catalog::Decl {
        name: "open.remote_list",
        group: "open",
        verb: "remote_list",
        title: "List mirrored remote files",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A mirror nobody can see is a copy of somebody's file sitting on disk with no way to tell where it came from or to get rid of it.",
    },
}

/// Serves the mirror registry.
pub struct MirrorHandler(pub Mirrors);

impl CapabilityHandler<RemoteList> for MirrorHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: RemoteListIn,
    ) -> Result<RemoteListOut, CapabilityError> {
        let held = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the mirror lock was poisoned"))?;
        let mut files: Vec<MirrorOut> = held
            .iter()
            .map(|(local, m)| MirrorOut {
                local_path: local.clone(),
                remote_path: m.remote_path.clone(),
                host: m.host.clone(),
                fetched_at: m.fetched_at,
                digest: m.digest.clone(),
                read_only: m.read_only,
            })
            .collect();
        files.sort_by(|a, b| b.fetched_at.cmp(&a.fetched_at).then(a.local_path.cmp(&b.local_path)));
        Ok(RemoteListOut { files })
    }
}

/// Input to `open.remote_discard`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoteDiscardIn {
    /// The local path of the mirror to drop.
    pub path: String,
}

/// What `open.remote_discard` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RemoteDiscardOut {
    /// Whether anything was there to drop.
    pub discarded: bool,
    /// How many mirrors remain.
    pub remaining: usize,
}

omt_catalog::capability! {
    /// Drop a mirrored file and stop watching it.
    pub struct RemoteDiscard;
    input  = RemoteDiscardIn,
    output = RemoteDiscardOut,
    decl = omt_catalog::Decl {
        name: "open.remote_discard",
        group: "open",
        verb: "remote_discard",
        title: "Discard a mirrored file",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_FS,
        // Cas: discarding twice is the same as discarding once, so a retry
        // after a dropped acknowledgement is safe and returns the same answer.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Unpins the blob so the sweeper can take it. Discarding something already gone is reported, not an error — a retry after a lost acknowledgement must not look like a failure.",
    },
}

impl CapabilityHandler<RemoteDiscard> for MirrorHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RemoteDiscardIn,
    ) -> Result<RemoteDiscardOut, CapabilityError> {
        let mut held = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the mirror lock was poisoned"))?;
        let discarded = held.remove(&input.path).is_some();
        Ok(RemoteDiscardOut {
            discarded,
            remaining: held.len(),
        })
    }
}

// ---------------------------------------------------------------------------
// Keymap conflicts — 16 §5
// ---------------------------------------------------------------------------

/// Input to `keys.conflicts`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConflictsIn {
    /// The chord omt would take.
    pub chord: String,
}

/// One thing a binding would break.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ConflictOut {
    /// Which program.
    pub program: String,
    /// What the key does there, in that program's own words.
    pub does: String,
    /// How much losing it matters.
    pub severity: String,
    /// What that keymap was checked against, so a stale warning is visible.
    pub verified_against: String,
}

/// What `keys.conflicts` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ConflictsOut {
    /// The canonical form of the chord that was asked about.
    pub chord: String,
    /// Everything it would break, worst first.
    pub conflicts: Vec<ConflictOut>,
    /// Whether omt refuses to take it by default.
    pub refused: bool,
}

omt_catalog::capability! {
    /// What binding this chord would break in the program underneath.
    pub struct KeysConflicts;
    input  = ConflictsIn,
    output = ConflictsOut,
    decl = omt_catalog::Decl {
        name: "keys.conflicts",
        group: "keys",
        verb: "conflicts",
        title: "What a binding would break",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "`Conflicts with something` is not actionable — every chord collides somewhere. This says which program, what the key does there, and how badly it would be missed.",
    },
}

/// Serves the keymap conflict registry.
pub struct KeysHandler;

impl CapabilityHandler<KeysConflicts> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ConflictsIn,
    ) -> Result<ConflictsOut, CapabilityError> {
        let chord = omt_input::key::Chord::parse(&input.chord).map_err(|e| {
            CapabilityError::invalid_input(format!("`{}` is not a chord: {e}", input.chord))
        })?;
        let registry = omt_input::conflicts::builtin();
        Ok(ConflictsOut {
            refused: registry.should_refuse(&chord),
            chord: chord.canonical(),
            conflicts: registry
                .conflicts(&chord)
                .into_iter()
                .map(|c| ConflictOut {
                    program: c.program,
                    does: c.does,
                    severity: format!("{:?}", c.severity).to_lowercase(),
                    verified_against: c.verified_against,
                })
                .collect(),
        })
    }
}

/// Input to `keys.audit`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AuditIn {}

/// What `keys.audit` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AuditOut {
    /// Everything omt's own defaults would take, worst first.
    pub conflicts: Vec<ConflictOut>,
    /// How many programs the registry knows about.
    ///
    /// Reported so an empty result can be told apart from a registry that
    /// knows nothing — "no conflicts" from a check that examined nothing looks
    /// exactly like "no conflicts" from one that examined everything.
    pub programs_known: usize,
}

omt_catalog::capability! {
    /// What omt's own default bindings would take from programs underneath.
    pub struct KeysAudit;
    input  = AuditIn,
    output = AuditOut,
    decl = omt_catalog::Decl {
        name: "keys.audit",
        group: "keys",
        verb: "audit",
        title: "Audit omt's own bindings",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A registry nobody runs the defaults against documents a problem instead of preventing one.",
    },
}

impl CapabilityHandler<KeysAudit> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: AuditIn,
    ) -> Result<AuditOut, CapabilityError> {
        let registry = omt_input::conflicts::builtin();
        Ok(AuditOut {
            programs_known: registry.len(),
            conflicts: omt_input::conflicts::audit(&omt_input::keymap::defaults(), &registry)
                .into_iter()
                .map(|c| ConflictOut {
                    program: c.program,
                    does: c.does,
                    severity: format!("{:?}", c.severity).to_lowercase(),
                    verified_against: c.verified_against,
                })
                .collect(),
        })
    }
}

/// Input to `keys.flavour`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FlavourIn {
    /// `plain`, `vim` or `emacs`.
    pub flavour: String,
}

/// One binding a flavour adds.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FlavourBinding {
    /// Which of omt's own surfaces.
    pub mode: String,
    /// The chord.
    pub chord: String,
    /// What it invokes.
    pub capability: String,
}

/// What `keys.flavour` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FlavourOut {
    /// Everything it binds.
    pub bindings: Vec<FlavourBinding>,
    /// Which modes a modal keymap is allowed to touch at all.
    ///
    /// Reported because it is the rule that makes the feature safe, and a rule
    /// nobody can read is one nobody can check.
    pub governs: Vec<String>,
}

omt_catalog::capability! {
    /// What a modal keymap flavour binds, and where it is allowed to.
    pub struct KeysFlavour;
    input  = FlavourIn,
    output = FlavourOut,
    decl = omt_catalog::Decl {
        name: "keys.flavour",
        group: "keys",
        verb: "flavour",
        title: "vim or emacs bindings",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A modal keymap governs omt's own surfaces only. It never governs a pane whose keys are passing through, which is what stops omt and an inner vim from contending over hjkl.",
    },
}

impl CapabilityHandler<KeysFlavour> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: FlavourIn,
    ) -> Result<FlavourOut, CapabilityError> {
        let flavour = match input.flavour.to_lowercase().as_str() {
            "vim" => omt_input::modal::Flavour::Vim,
            "emacs" => omt_input::modal::Flavour::Emacs,
            "plain" => omt_input::modal::Flavour::Plain,
            other => {
                return Err(CapabilityError::invalid_input(format!(
                    "no keymap flavour `{other}`; omt has plain, vim and emacs"
                )));
            }
        };
        Ok(FlavourOut {
            governs: omt_input::modal::OMT_OWNED
                .iter()
                .map(|m| format!("{m:?}").to_lowercase())
                .collect(),
            bindings: omt_input::modal::bindings(flavour)
                .into_iter()
                .map(|(mode, chord, capability)| FlavourBinding {
                    mode: format!("{mode:?}").to_lowercase(),
                    chord: chord.to_owned(),
                    capability: capability.to_owned(),
                })
                .collect(),
        })
    }
}

/// Input to `keys.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct KeysListIn {}

/// One binding.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BindingOut {
    /// Which surface it applies on.
    pub mode: String,
    /// The chord.
    pub chord: String,
    /// What it does.
    pub action: String,
}

/// What `keys.list` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct KeysListOut {
    /// Every binding, in a stable order.
    pub bindings: Vec<BindingOut>,
}

omt_catalog::capability! {
    /// Every binding omt currently has.
    pub struct KeysList;
    input  = KeysListIn,
    output = KeysListOut,
    decl = omt_catalog::Decl {
        name: "keys.list",
        group: "keys",
        verb: "list",
        title: "List every binding",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "In a stable order, so two calls can be compared. An unordered list makes a diff between two configurations unreadable.",
    },
}

impl CapabilityHandler<KeysList> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: KeysListIn,
    ) -> Result<KeysListOut, CapabilityError> {
        let keymap = omt_input::keymap::defaults();
        let mut bindings: Vec<BindingOut> = keymap
            .bindings()
            .into_iter()
            .map(|(mode, chord, action)| BindingOut {
                mode: format!("{mode:?}").to_lowercase(),
                chord: chord.canonical(),
                action: match action {
                    omt_input::keymap::Action::Capability { name } => name.clone(),
                    omt_input::keymap::Action::Passthrough => "passthrough".to_owned(),
                    omt_input::keymap::Action::Unbound => "unbound".to_owned(),
                },
            })
            .collect();
        bindings.sort_by(|a, b| a.mode.cmp(&b.mode).then(a.chord.cmp(&b.chord)));
        Ok(KeysListOut { bindings })
    }
}

/// Input to `keys.explain`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplainIn {
    /// The chord to explain.
    pub chord: String,
}

/// What `keys.explain` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ExplainOut {
    /// Its canonical form.
    pub chord: String,
    /// What omt does with it, if anything.
    pub action: Option<String>,
    /// On which surface.
    pub mode: Option<String>,
    /// What it would take from the program underneath.
    pub conflicts: Vec<ConflictOut>,
    /// Whether it needs an AltGr on a common non-US layout.
    pub awkward: bool,
    /// The alias installed beside it, where there is one.
    pub alias: Option<String>,
    /// What happens in a pane that is passing through.
    ///
    /// Said plainly, because "not bound" and "bound but the pane owns it" look
    /// identical from a user's chair and have different fixes.
    pub in_a_pane: String,
}

omt_catalog::capability! {
    /// Everything omt knows about one chord.
    pub struct KeysExplain;
    input  = ExplainIn,
    output = ExplainOut,
    decl = omt_catalog::Decl {
        name: "keys.explain",
        group: "keys",
        verb: "explain",
        title: "Explain a key",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "`Not bound` and `bound, but the pane underneath owns it` look identical from the user's chair and have different fixes, so this says which.",
    },
}

impl CapabilityHandler<KeysExplain> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ExplainIn,
    ) -> Result<ExplainOut, CapabilityError> {
        let chord = omt_input::key::Chord::parse(&input.chord).map_err(|e| {
            CapabilityError::invalid_input(format!("`{}` is not a chord: {e}", input.chord))
        })?;
        let keymap = omt_input::keymap::defaults();
        let found = keymap
            .bindings()
            .into_iter()
            .find(|(_, c, _)| c.canonical() == chord.canonical())
            .map(|(mode, _, action)| {
                (
                    format!("{mode:?}").to_lowercase(),
                    match action {
                        omt_input::keymap::Action::Capability { name } => name.clone(),
                        omt_input::keymap::Action::Passthrough => "passthrough".to_owned(),
                        omt_input::keymap::Action::Unbound => "unbound".to_owned(),
                    },
                )
            });
        let global = found.as_ref().is_some_and(|(mode, _)| mode == "global");
        Ok(ExplainOut {
            in_a_pane: if global {
                "omt handles this before the pane sees it.".to_owned()
            } else {
                "The program in the pane receives this untouched.".to_owned()
            },
            mode: found.as_ref().map(|(mode, _)| mode.clone()),
            action: found.map(|(_, action)| action),
            conflicts: omt_input::conflicts::builtin()
                .conflicts(&chord)
                .into_iter()
                .map(|c| ConflictOut {
                    program: c.program,
                    does: c.does,
                    severity: format!("{:?}", c.severity).to_lowercase(),
                    verified_against: c.verified_against,
                })
                .collect(),
            awkward: omt_input::layout::is_awkward(&chord),
            alias: omt_input::layout::alias_for(&chord).map(str::to_owned),
            chord: chord.canonical(),
        })
    }
}

/// Input to `keys.registry`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegistryIn {}

/// What `keys.registry` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RegistryOut {
    /// How many programs omt knows the keymap of.
    pub programs: usize,
    /// Which keymap flavours exist.
    pub flavours: Vec<String>,
}

omt_catalog::capability! {
    /// What omt knows about other programs' keys.
    pub struct KeysRegistry;
    input  = RegistryIn,
    output = RegistryOut,
    decl = omt_catalog::Decl {
        name: "keys.registry",
        group: "keys",
        verb: "registry",
        title: "Known inner keymaps",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The registry is data, so a new agent CLI is a file rather than a release. This says how much of it is loaded.",
    },
}

impl CapabilityHandler<KeysRegistry> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: RegistryIn,
    ) -> Result<RegistryOut, CapabilityError> {
        Ok(RegistryOut {
            programs: omt_input::conflicts::builtin().len(),
            flavours: vec!["plain".to_owned(), "vim".to_owned(), "emacs".to_owned()],
        })
    }
}

// ---------------------------------------------------------------------------
// Directional focus — 17 §6
// ---------------------------------------------------------------------------

/// One placed pane, as the caller describes it.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlacedIn {
    /// Which pane.
    pub id: String,
    /// Column of the left edge.
    pub left: u16,
    /// Row of the top edge.
    pub top: u16,
    /// One past the right edge.
    pub right: u16,
    /// One past the bottom edge.
    pub bottom: u16,
    /// Whether it is currently drawn.
    #[serde(default)]
    pub hidden: bool,
    /// Its stable pre-order index, the total tie-break.
    pub placement_index: u32,
}

/// Input to `pane.navigate`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct NavigateIn {
    /// Every placed pane.
    pub panes: Vec<PlacedIn>,
    /// Where focus is now.
    pub from: String,
    /// `left`, `right`, `up` or `down`.
    pub direction: String,
    /// Whether to jump to the far side when there is nothing that way.
    #[serde(default)]
    pub wrap: bool,
}

/// What `pane.navigate` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct NavigateOut {
    /// Where focus should go, or nothing.
    pub focus: Option<String>,
}

omt_catalog::capability! {
    /// Move focus in a direction, geometrically.
    pub struct PaneNavigate;
    input  = NavigateIn,
    output = NavigateOut,
    decl = omt_catalog::Decl {
        name: "pane.navigate",
        group: "pane",
        verb: "navigate",
        title: "Move focus in a direction",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Geometric, over the placed rectangles rather than tree order — tree order gives wrong answers in asymmetric layouts. Deterministic, because breaking ties by recency makes the same keypress do two different things for reasons the user cannot see.",
    },
}

impl CapabilityHandler<PaneNavigate> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: NavigateIn,
    ) -> Result<NavigateOut, CapabilityError> {
        let direction = match input.direction.to_lowercase().as_str() {
            "left" => omt_tui::navigate::Direction::Left,
            "right" => omt_tui::navigate::Direction::Right,
            "up" => omt_tui::navigate::Direction::Up,
            "down" => omt_tui::navigate::Direction::Down,
            other => {
                return Err(CapabilityError::invalid_input(format!(
                    "no direction `{other}`; omt has left, right, up and down"
                )));
            }
        };
        let panes: Vec<omt_tui::navigate::Placed> = input
            .panes
            .into_iter()
            .map(|p| omt_tui::navigate::Placed {
                id: p.id,
                rect: omt_tui::navigate::Rect {
                    left: p.left,
                    top: p.top,
                    right: p.right,
                    bottom: p.bottom,
                },
                hidden: p.hidden,
                placement_index: p.placement_index,
            })
            .collect();
        Ok(NavigateOut {
            focus: omt_tui::navigate::navigate(&panes, &input.from, direction, input.wrap),
        })
    }
}

/// Saved layouts, by name.
pub type Layouts = std::sync::Arc<
    std::sync::Mutex<std::collections::BTreeMap<String, omt_tui::geometry::Saved>>,
>;

/// Input to `layout.save`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayoutSaveIn {
    /// What to call it.
    pub name: String,
    /// The panes, in the order they should be arranged.
    pub panes: Vec<String>,
    /// `even_vertical`, `even_horizontal`, `main_vertical` or `main_horizontal`.
    pub preset: String,
}

/// What `layout.save` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LayoutSaveOut {
    /// What it was called.
    pub name: String,
    /// How many layouts are saved now.
    pub saved: usize,
}

omt_catalog::capability! {
    /// Save an arrangement under a name.
    pub struct LayoutSave;
    input  = LayoutSaveIn,
    output = LayoutSaveOut,
    decl = omt_catalog::Decl {
        name: "layout.save",
        group: "layout",
        verb: "save",
        title: "Save this layout",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_FS,
        // Cas: saving the same name twice is a replace, so a retry after a
        // dropped acknowledgement lands on the same result.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The tree and nothing else. Rects are a function of the tree and the screen, so saving them would restore a desktop layout onto a phone as a desktop layout.",
    },
}

/// Serves saved layouts.
pub struct LayoutHandler(pub Layouts);

fn preset_of(name: &str) -> Option<omt_tui::geometry::Preset> {
    match name {
        "even_vertical" => Some(omt_tui::geometry::Preset::EvenVertical),
        "even_horizontal" => Some(omt_tui::geometry::Preset::EvenHorizontal),
        "main_vertical" => Some(omt_tui::geometry::Preset::MainVertical),
        "main_horizontal" => Some(omt_tui::geometry::Preset::MainHorizontal),
        _ => None,
    }
}

impl CapabilityHandler<LayoutSave> for LayoutHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: LayoutSaveIn,
    ) -> Result<LayoutSaveOut, CapabilityError> {
        if input.name.trim().is_empty() {
            return Err(CapabilityError::invalid_input(
                "a layout needs a name to be restored by",
            ));
        }
        let preset = preset_of(&input.preset).ok_or_else(|| {
            CapabilityError::invalid_input(format!(
                "no preset `{}`; omt has even_vertical, even_horizontal, main_vertical and main_horizontal",
                input.preset
            ))
        })?;
        let tree = omt_tui::geometry::preset(preset, &input.panes)
            .ok_or_else(|| CapabilityError::invalid_input("an empty layout is not a layout"))?;
        let mut held = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the layout lock was poisoned"))?;
        held.insert(
            input.name.clone(),
            omt_tui::geometry::Saved {
                name: input.name.clone(),
                tree: omt_tui::geometry::SavedNode::from(&tree),
            },
        );
        Ok(LayoutSaveOut {
            name: input.name,
            saved: held.len(),
        })
    }
}

/// Input to `layout.restore`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayoutRestoreIn {
    /// Which saved layout.
    pub name: String,
    /// Which panes still exist.
    pub alive: Vec<String>,
    /// How wide the screen is.
    pub width: u16,
    /// How tall.
    pub height: u16,
}

/// What `layout.restore` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LayoutRestoreOut {
    /// Where each surviving pane goes.
    pub panes: Vec<PlacementOut>,
    /// Panes the layout named that are no longer running.
    ///
    /// Named rather than counted, because a restore that quietly drops two
    /// panes leaves somebody wondering what they have forgotten.
    pub missing: Vec<String>,
    /// Panes that survived but do not fit this screen.
    pub hidden: Vec<String>,
}

/// One placed pane.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PlacementOut {
    /// Which pane.
    pub pane: String,
    /// Column of the left edge.
    pub left: u16,
    /// Row of the top edge.
    pub top: u16,
    /// How wide.
    pub width: u16,
    /// How tall.
    pub height: u16,
}

omt_catalog::capability! {
    /// Restore a saved layout onto this screen.
    pub struct LayoutRestore;
    input  = LayoutRestoreIn,
    output = LayoutRestoreOut,
    decl = omt_catalog::Decl {
        name: "layout.restore",
        group: "layout",
        verb: "restore",
        title: "Restore a saved layout",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Keeps the panes that still exist and names the rest. Failing outright gives somebody nothing; keeping dead names gives them panes they have to close one by one.",
    },
}

impl CapabilityHandler<LayoutRestore> for LayoutHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: LayoutRestoreIn,
    ) -> Result<LayoutRestoreOut, CapabilityError> {
        let held = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the layout lock was poisoned"))?;
        let saved = held
            .get(&input.name)
            .ok_or_else(|| CapabilityError::not_found(format!("no layout `{}`", input.name)))?;
        let (tree, missing) = omt_tui::geometry::restore(saved, &input.alive).ok_or_else(|| {
            CapabilityError::precondition_failed(format!(
                "none of the panes in `{}` are still running",
                input.name
            ))
        })?;
        let geometry = omt_tui::geometry::compute(
            &tree,
            omt_tui::geometry::Rect {
                left: 0,
                top: 0,
                width: input.width,
                height: input.height,
            },
        );
        Ok(LayoutRestoreOut {
            panes: geometry
                .panes
                .into_iter()
                .map(|p| PlacementOut {
                    pane: p.pane,
                    left: p.outer.left,
                    top: p.outer.top,
                    width: p.outer.width,
                    height: p.outer.height,
                })
                .collect(),
            hidden: geometry.hidden,
            missing,
        })
    }
}

// ---------------------------------------------------------------------------
// Why one result beat another — 20 §4
// ---------------------------------------------------------------------------

/// One candidate, as the caller describes it.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CandidateIn {
    /// Its id.
    pub id: String,
    /// `command`, `output`, `agent_message` or `file_change`.
    pub kind: String,
    /// FTS5's bm25, negative and more-negative-is-better.
    pub bm25: f64,
    /// How many seconds ago.
    pub age_secs: u64,
    /// Whether it is in the workspace the search came from.
    #[serde(default)]
    pub in_scope: bool,
    /// The exit code, where there was one.
    #[serde(default)]
    pub exit: Option<i32>,
    /// Whether its session is still running.
    #[serde(default)]
    pub session_active: bool,
}

/// Input to `search.explain`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplainSearchIn {
    /// The candidates to rank.
    pub candidates: Vec<CandidateIn>,
}

/// One factor's contribution.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FactorOut {
    /// What it is called.
    pub name: String,
    /// What it multiplied by.
    pub value: f64,
    /// Why, in words.
    pub because: String,
}

/// One scored result.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ScoredOut {
    /// Which candidate.
    pub id: String,
    /// The product of every factor.
    pub score: f64,
    /// Each factor, in the order applied.
    pub factors: Vec<FactorOut>,
}

/// What `search.explain` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ExplainSearchOut {
    /// Every candidate, best first.
    pub results: Vec<ScoredOut>,
}

omt_catalog::capability! {
    /// Rank some candidates and show the arithmetic.
    pub struct SearchExplain;
    input  = ExplainSearchIn,
    output = ExplainSearchOut,
    decl = omt_catalog::Decl {
        name: "search.explain",
        group: "search",
        verb: "explain",
        title: "Why this result ranked here",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A transparent weighted product with no learning and no hidden state. Search over somebody's own history is where a wrong answer looks exactly like a right one, and a ranker that cannot explain itself gives them no way to notice.",
    },
}

/// Serves search explanation.
pub struct SearchHandler;

impl CapabilityHandler<SearchExplain> for SearchHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ExplainSearchIn,
    ) -> Result<ExplainSearchOut, CapabilityError> {
        let candidates: Vec<omt_recall::ranking::Candidate> = input
            .candidates
            .into_iter()
            .map(|c| omt_recall::ranking::Candidate {
                id: c.id,
                kind: match c.kind.as_str() {
                    "output" => omt_recall::ranking::Kind::Output,
                    "agent_message" => omt_recall::ranking::Kind::AgentMessage,
                    "file_change" => omt_recall::ranking::Kind::FileChange,
                    _ => omt_recall::ranking::Kind::Command,
                },
                bm25: c.bm25,
                age_secs: c.age_secs,
                in_scope: c.in_scope,
                exit: c.exit,
                session_active: c.session_active,
            })
            .collect();
        Ok(ExplainSearchOut {
            results: omt_recall::ranking::rank(&candidates)
                .into_iter()
                .map(|s| ScoredOut {
                    id: s.id,
                    score: s.score,
                    factors: s
                        .factors
                        .into_iter()
                        .map(|f| FactorOut {
                            name: f.name,
                            value: f.value,
                            because: f.because,
                        })
                        .collect(),
                })
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Is this agent stuck? — 20 §10
// ---------------------------------------------------------------------------

/// Input to `agent.attention`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AttentionIn {
    /// How long it has been working, in seconds.
    #[serde(default)]
    pub working_secs: u64,
    /// The usual working time for this binding.
    #[serde(default)]
    pub baseline_working_secs: u64,
    /// How long since anything happened.
    #[serde(default)]
    pub silent_secs: u64,
    /// The usual gap between events.
    #[serde(default)]
    pub baseline_gap_secs: u64,
    /// Recent tool calls, canonicalised.
    #[serde(default)]
    pub recent_calls: Vec<String>,
    /// How many results in a row have failed.
    #[serde(default)]
    pub consecutive_failures: u32,
    /// The most recent failure.
    #[serde(default)]
    pub last_failure: Option<String>,
    /// Tokens spent since a file last changed.
    #[serde(default)]
    pub tokens_since_file_change: u64,
    /// How long since a file last changed.
    #[serde(default)]
    pub since_file_change_secs: u64,
}

/// What `agent.attention` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AttentionOut {
    /// Whether anything fired.
    pub needs_attention: bool,
    /// Every reason, never just the first.
    pub reasons: Vec<String>,
    /// `weak` or `strong`.
    pub confidence: Option<String>,
    /// A line somebody can act on.
    pub detail: Option<String>,
}

omt_catalog::capability! {
    /// Whether an agent has stopped making progress.
    pub struct AgentAttention;
    input  = AttentionIn,
    output = AttentionOut,
    decl = omt_catalog::Decl {
        name: "agent.attention",
        group: "agent",
        verb: "attention",
        title: "Is this agent stuck",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Baselines are per binding and learned from its own history: thirty seconds is an eternity for a linter and a blink for a test suite, so a fixed threshold is wrong for everybody.",
    },
}

/// Serves attention detection.
pub struct AttentionHandler;

impl CapabilityHandler<AgentAttention> for AttentionHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: AttentionIn,
    ) -> Result<AttentionOut, CapabilityError> {
        let observation = omt_recall::stuck::Observation {
            working_secs: input.working_secs,
            baseline_working_secs: input.baseline_working_secs,
            silent_secs: input.silent_secs,
            baseline_gap_secs: input.baseline_gap_secs,
            recent_calls: input.recent_calls.into_iter().collect(),
            consecutive_failures: input.consecutive_failures,
            last_failure: input.last_failure,
            tokens_since_file_change: input.tokens_since_file_change,
            since_file_change_secs: input.since_file_change_secs,
        };
        let signal = omt_recall::stuck::examine(&observation, &omt_recall::stuck::Thresholds::default());
        Ok(match signal {
            None => AttentionOut {
                needs_attention: false,
                reasons: Vec::new(),
                confidence: None,
                detail: None,
            },
            Some(s) => AttentionOut {
                needs_attention: true,
                reasons: s
                    .reasons
                    .iter()
                    .map(|r| {
                        serde_json::to_value(r)
                            .ok()
                            .and_then(|v| v.get("reason").and_then(|x| x.as_str()).map(str::to_owned))
                            .unwrap_or_else(|| "unknown".to_owned())
                    })
                    .collect(),
                confidence: Some(format!("{:?}", s.confidence).to_lowercase()),
                detail: Some(s.detail),
            },
        })
    }
}

// ---------------------------------------------------------------------------
// Comparing parallel runs — 20 §9
// ---------------------------------------------------------------------------

/// One run, as the caller describes it.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunIn {
    /// Which session.
    pub session: String,
    /// Its branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// The resolved base it is measured against.
    #[serde(default)]
    pub base_ref: Option<String>,
    /// Which agent.
    #[serde(default)]
    pub agent: Option<String>,
    /// How long, in seconds.
    #[serde(default)]
    pub elapsed_secs: u64,
    /// Files git reports changed.
    #[serde(default)]
    pub files_from_git: Vec<String>,
    /// Files the agent said it changed.
    #[serde(default)]
    pub files_from_agent: Vec<String>,
    /// Lines added.
    #[serde(default)]
    pub insertions: u32,
    /// Lines removed.
    #[serde(default)]
    pub deletions: u32,
    /// Turns.
    #[serde(default)]
    pub turns: u32,
    /// Times it stopped to ask.
    #[serde(default)]
    pub interactions: u32,
    /// How many went unanswered.
    #[serde(default)]
    pub interactions_timed_out: u32,
    /// Commands that exited non-zero.
    #[serde(default)]
    pub failed_blocks: u32,
    /// Tokens, agent-reported.
    #[serde(default)]
    pub tokens: Option<u64>,
    /// The agent's own summary.
    #[serde(default)]
    pub summary: Option<String>,
}

/// Input to `agent.compare`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompareIn {
    /// The runs to put side by side.
    pub runs: Vec<RunIn>,
}

/// One row of the comparison.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RowOut {
    /// Which session.
    pub session: String,
    /// Its branch.
    pub branch: Option<String>,
    /// What it was measured against.
    pub base_ref: Option<String>,
    /// Which agent.
    pub agent: Option<String>,
    /// How long, in seconds.
    pub elapsed_secs: u64,
    /// Every file either record mentions.
    pub files_touched: Vec<String>,
    /// Files the two records disagree about.
    pub files_disagreement: Vec<String>,
    /// Lines added.
    pub insertions: u32,
    /// Lines removed.
    pub deletions: u32,
    /// Turns.
    pub turns: u32,
    /// Times it stopped to ask.
    pub interactions: u32,
    /// How many went unanswered.
    pub interactions_timed_out: u32,
    /// Commands that exited non-zero.
    pub failed_blocks: u32,
    /// Tokens, agent-reported. Absent means unreported, not zero.
    pub tokens: Option<u64>,
    /// The agent's own summary, verbatim.
    pub summary: Option<String>,
}

/// What `agent.compare` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CompareOut {
    /// One row per run, in the order given.
    pub rows: Vec<RowOut>,
    /// What is not comparable, and why.
    pub caveats: Vec<String>,
}

omt_catalog::capability! {
    /// Put parallel agent runs side by side.
    pub struct AgentCompare;
    input  = CompareIn,
    output = CompareOut,
    decl = omt_catalog::Decl {
        name: "agent.compare",
        group: "agent",
        verb: "compare",
        title: "Compare parallel runs",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Presents comparable facts; never ranks, scores or recommends. Judging requires understanding the code, and a plausible wrong recommendation about which branch to keep costs somebody real work.",
    },
}

impl CapabilityHandler<AgentCompare> for AttentionHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: CompareIn,
    ) -> Result<CompareOut, CapabilityError> {
        let runs: Vec<omt_recall::compare::Run> = input
            .runs
            .into_iter()
            .map(|r| omt_recall::compare::Run {
                session: r.session,
                branch: r.branch,
                base_ref: r.base_ref,
                agent: r.agent,
                elapsed_secs: r.elapsed_secs,
                files_from_git: r.files_from_git,
                files_from_agent: r.files_from_agent,
                insertions: r.insertions,
                deletions: r.deletions,
                turns: r.turns,
                interactions: r.interactions,
                interactions_timed_out: r.interactions_timed_out,
                failed_blocks: r.failed_blocks,
                tokens: r.tokens,
                summary: r.summary,
            })
            .collect();
        let comparison = omt_recall::compare::compare(&runs);
        Ok(CompareOut {
            rows: comparison
                .rows
                .into_iter()
                .map(|r| RowOut {
                    session: r.session,
                    branch: r.branch,
                    base_ref: r.base_ref,
                    agent: r.agent,
                    elapsed_secs: r.elapsed_secs,
                    files_touched: r.files_touched,
                    files_disagreement: r.files_disagreement,
                    insertions: r.insertions,
                    deletions: r.deletions,
                    turns: r.turns,
                    interactions: r.interactions,
                    interactions_timed_out: r.interactions_timed_out,
                    failed_blocks: r.failed_blocks,
                    tokens: r.tokens,
                    summary: r.summary,
                })
                .collect(),
            caveats: comparison.caveats,
        })
    }
}

// ---------------------------------------------------------------------------
// Timeline and digest — 20 §8
// ---------------------------------------------------------------------------

/// One event, as the caller describes it.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TimelineEventIn {
    /// Its per-session sequence number.
    pub seq: u64,
    /// When, in seconds since the epoch.
    pub at: u64,
    /// `user_message`, `assistant_message`, `tool_call`, `file_changed`,
    /// `interaction` or `failed_block`.
    pub kind: String,
    /// What it was, in the agent's own words.
    pub label: String,
}

/// Input to `session.timeline`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TimelineIn {
    /// The events to assemble.
    pub events: Vec<TimelineEventIn>,
}

/// One assembled line.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TimelineEntryOut {
    /// Where it starts.
    pub seq: u64,
    /// When.
    pub at: u64,
    /// What kind.
    pub kind: String,
    /// What it was.
    pub label: String,
    /// How many identical events collapsed into it.
    pub count: u32,
    /// From the first to the last, in seconds.
    pub spanned_secs: u64,
}

/// What `session.timeline` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TimelineOut {
    /// The lines, in sequence order.
    pub entries: Vec<TimelineEntryOut>,
    /// How many events went in.
    pub events: u32,
    /// How many tool calls.
    pub tool_calls: u32,
    /// How many files changed.
    pub files_changed: u32,
    /// How many questions were asked.
    pub interactions: u32,
    /// How many commands failed.
    pub failed_blocks: u32,
    /// A short account for somebody who was away.
    pub digest: String,
}

omt_catalog::capability! {
    /// What happened in a session, assembled and collapsed.
    pub struct SessionTimeline;
    input  = TimelineIn,
    output = TimelineOut,
    decl = omt_catalog::Decl {
        name: "session.timeline",
        group: "session",
        verb: "timeline",
        title: "What happened here",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Derived from the event log, so it can be rebuilt at any time. Only consecutive identical calls collapse, and the canonicaliser strips whitespace and nothing else — one that ignored arguments would collapse `rm -rf build` and `rm -rf /`.",
    },
}

impl CapabilityHandler<SessionTimeline> for AttentionHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: TimelineIn,
    ) -> Result<TimelineOut, CapabilityError> {
        let events: Vec<omt_recall::timeline::Event> = input
            .events
            .into_iter()
            .map(|e| omt_recall::timeline::Event {
                seq: e.seq,
                at: e.at,
                kind: match e.kind.as_str() {
                    "user_message" => omt_recall::timeline::Kind::UserMessage,
                    "assistant_message" => omt_recall::timeline::Kind::AssistantMessage,
                    "file_changed" => omt_recall::timeline::Kind::FileChanged,
                    "interaction" => omt_recall::timeline::Kind::Interaction,
                    "failed_block" => omt_recall::timeline::Kind::FailedBlock,
                    _ => omt_recall::timeline::Kind::ToolCall,
                },
                label: e.label,
            })
            .collect();
        let timeline = omt_recall::timeline::assemble(&events);
        let digest = omt_recall::timeline::digest(&timeline);
        Ok(TimelineOut {
            entries: timeline
                .entries
                .iter()
                .map(|e| TimelineEntryOut {
                    seq: e.seq,
                    at: e.at,
                    kind: format!("{:?}", e.kind).to_lowercase(),
                    label: e.label.clone(),
                    count: e.count,
                    spanned_secs: e.spanned_secs,
                })
                .collect(),
            events: timeline.stats.events,
            tool_calls: timeline.stats.tool_calls,
            files_changed: timeline.stats.files_changed,
            interactions: timeline.stats.interactions,
            failed_blocks: timeline.stats.failed_blocks,
            digest,
        })
    }
}

// ---------------------------------------------------------------------------
// What is indexed — 20 §2
// ---------------------------------------------------------------------------

/// Input to `recall.policy`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyIn {
    /// Whether the agent's reasoning is indexed.
    #[serde(default)]
    pub index_reasoning: bool,
}

/// What omt does with one source.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SourcePolicy {
    /// Which source.
    pub source: String,
    /// `full`, `capped`, `windowed`, `structured_only` or `excluded`.
    pub treatment: String,
    /// The cap or window, in bytes, where there is one.
    pub bytes: Option<usize>,
}

/// What `recall.policy` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PolicyOut {
    /// Every source and what happens to it.
    pub sources: Vec<SourcePolicy>,
    /// Which fields of a tool call are indexed.
    pub indexed_tool_fields: Vec<String>,
}

omt_catalog::capability! {
    /// What goes into the search index, and what does not.
    pub struct RecallPolicy;
    input  = PolicyIn,
    output = PolicyOut,
    decl = omt_catalog::Decl {
        name: "recall.policy",
        group: "recall",
        verb: "policy",
        title: "What is indexed",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A search index nobody can enumerate is one nobody can reason about. Reasoning is excluded by default: it is the most privacy-sensitive text an agent produces.",
    },
}

impl CapabilityHandler<RecallPolicy> for AttentionHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: PolicyIn,
    ) -> Result<PolicyOut, CapabilityError> {
        use omt_recall::policy::{Source, Treatment};
        let settings = omt_recall::policy::Settings {
            index_reasoning: input.index_reasoning,
            ..omt_recall::policy::Settings::default()
        };
        let sources = [
            Source::BlockCommand,
            Source::BlockOutput,
            Source::UserMessage,
            Source::AssistantText,
            Source::Reasoning,
            Source::ToolCallInput,
            Source::ToolResultOutput,
            Source::FileChanged,
            Source::Interaction,
        ];
        Ok(PolicyOut {
            sources: sources
                .into_iter()
                .map(|source| {
                    let t = omt_recall::policy::treatment(source, &settings);
                    SourcePolicy {
                        source: format!("{source:?}").to_lowercase(),
                        treatment: match t {
                            Treatment::Full => "full",
                            Treatment::Capped { .. } => "capped",
                            Treatment::Windowed { .. } => "windowed",
                            Treatment::StructuredOnly => "structured_only",
                            Treatment::Excluded => "excluded",
                        }
                        .to_owned(),
                        bytes: match t {
                            Treatment::Capped { bytes } => Some(bytes),
                            Treatment::Windowed { head, tail } => Some(head + tail),
                            _ => None,
                        },
                    }
                })
                .collect(),
            indexed_tool_fields: omt_recall::policy::INDEXED_TOOL_FIELDS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Bind policy — 13 §2
// ---------------------------------------------------------------------------

/// Input to `security.bind_check`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct BindCheckIn {
    /// Where omt would listen: `unix`, `loopback`, `tailnet`, or an address.
    pub binds: Vec<String>,
    /// Whether a password or bearer token is configured.
    #[serde(default)]
    pub auth_backend: bool,
    /// Whether Tailscale's LocalAPI supplies identity.
    #[serde(default)]
    pub tailnet_identity: bool,
    /// Whether omt terminates TLS.
    #[serde(default)]
    pub tls: bool,
    /// Whether a reverse proxy is declared to terminate it.
    #[serde(default)]
    pub trust_proxy: bool,
    /// What an unauthenticated caller gets: `none`, `viewer` or `operator`.
    #[serde(default)]
    pub default_role: Option<String>,
}

/// Something that must be fixed first.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RefusalOut {
    /// What is wrong.
    pub problem: String,
    /// What to do about it.
    pub remedy: String,
}

/// What `security.bind_check` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BindCheckOut {
    /// Whether omt would start.
    pub may_start: bool,
    /// Everything that must be fixed, not just the first.
    pub refusals: Vec<RefusalOut>,
    /// Things worth knowing that do not stop it.
    pub warnings: Vec<String>,
}

omt_catalog::capability! {
    /// Whether a listen configuration is one omt will start on.
    pub struct BindCheck;
    input  = BindCheckIn,
    output = BindCheckOut,
    decl = omt_catalog::Decl {
        name: "security.bind_check",
        group: "security",
        verb: "bind_check",
        title: "Check a listen configuration",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A misconfiguration fails the daemon rather than warning. A warning about an open port is printed once, into a log nobody reads, on a machine that is now reachable from the network.",
    },
}

/// Serves the bind check.
pub struct SecurityHandler;

impl CapabilityHandler<BindCheck> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: BindCheckIn,
    ) -> Result<BindCheckOut, CapabilityError> {
        use omt_auth::bind::{Auth, Bind, DefaultRole, Listen, Transport, check};
        let mut binds = Vec::new();
        for spec in &input.binds {
            binds.push(match spec.as_str() {
                "unix" => Bind::UnixSocket {
                    path: "/run/omt.sock".to_owned(),
                },
                "loopback" => Bind::Loopback { port: 7878 },
                "tailnet" => Bind::Tailnet { port: 7878 },
                other => {
                    let addr = other.parse().map_err(|_| {
                        CapabilityError::invalid_input(format!(
                            "`{other}` is not `unix`, `loopback`, `tailnet` or an IP address"
                        ))
                    })?;
                    Bind::Interface { addr, port: 7878 }
                }
            });
        }
        let verdict = check(&Listen {
            binds,
            auth: Auth {
                backend: input.auth_backend,
                tailnet_identity: input.tailnet_identity,
            },
            transport: Transport {
                tls: input.tls,
                trust_proxy: input.trust_proxy,
            },
            default_role: match input.default_role.as_deref() {
                Some("viewer") => DefaultRole::Viewer,
                Some("operator") => DefaultRole::Operator,
                _ => DefaultRole::None,
            },
        });
        Ok(BindCheckOut {
            may_start: verdict.may_start(),
            refusals: verdict
                .refusals
                .into_iter()
                .map(|r| RefusalOut {
                    problem: r.problem,
                    remedy: r.remedy,
                })
                .collect(),
            warnings: verdict.warnings.into_iter().map(|w| w.note).collect(),
        })
    }
}

/// Input to `security.publish_check`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PublishCheckIn {
    /// A password or bearer backend is configured.
    #[serde(default)]
    pub auth_backend: bool,
    /// The only identity available is the tailnet's.
    #[serde(default)]
    pub tailnet_identity_only: bool,
    /// How long the configured password is.
    #[serde(default)]
    pub password_length: Option<usize>,
    /// Bearer tokens are bound to a device key.
    #[serde(default)]
    pub bearer_device_bound: bool,
    /// Rate limiting and lockout are on.
    #[serde(default)]
    pub rate_limited: bool,
    /// The origins configured, verbatim.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// What an unauthenticated caller gets.
    #[serde(default)]
    pub default_role: Option<String>,
}

omt_catalog::capability! {
    /// Whether this instance may be put on the public internet.
    pub struct PublishCheck;
    input  = PublishCheckIn,
    output = BindCheckOut,
    decl = omt_catalog::Decl {
        name: "security.publish_check",
        group: "security",
        verb: "publish_check",
        title: "Check before publishing",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Funnel puts the instance on the open internet, and the difference between that and a tailnet is not visible in any configuration omt reads. Somebody who enabled it for a demo and forgot is who this is for.",
    },
}

impl CapabilityHandler<PublishCheck> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: PublishCheckIn,
    ) -> Result<BindCheckOut, CapabilityError> {
        use omt_auth::bind::{DefaultRole, Publication, check_publication};
        let verdict = check_publication(&Publication {
            auth_backend: input.auth_backend,
            tailnet_identity_only: input.tailnet_identity_only,
            password_length: input.password_length,
            bearer_device_bound: input.bearer_device_bound,
            rate_limited: input.rate_limited,
            allowed_origins: input.allowed_origins,
            default_role: match input.default_role.as_deref() {
                Some("viewer") => DefaultRole::Viewer,
                Some("operator") => DefaultRole::Operator,
                _ => DefaultRole::None,
            },
        });
        Ok(BindCheckOut {
            may_start: verdict.may_start(),
            refusals: verdict
                .refusals
                .into_iter()
                .map(|r| RefusalOut {
                    problem: r.problem,
                    remedy: r.remedy,
                })
                .collect(),
            warnings: verdict.warnings.into_iter().map(|w| w.note).collect(),
        })
    }
}

/// Input to `config.reload_class`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReloadClassIn {
    /// The keys to ask about.
    pub keys: Vec<String>,
}

/// One key and when a change to it takes effect.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReloadEntry {
    /// The key.
    pub key: String,
    /// `hot`, `restart_server` or `restart`.
    pub class: String,
}

/// What `config.reload_class` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReloadClassOut {
    /// One entry per key asked about.
    pub keys: Vec<ReloadEntry>,
    /// Which of them will not take effect until something restarts.
    ///
    /// Listed separately because that is the answer somebody needs. A setting
    /// that quietly waits for a restart is the worst of the three: it is
    /// changed, nothing happens, and the conclusion is that the setting does
    /// not work.
    pub pending: Vec<String>,
}

omt_catalog::capability! {
    /// When a change to a setting takes effect.
    pub struct ReloadClass;
    input  = ReloadClassIn,
    output = ReloadClassOut,
    decl = omt_catalog::Decl {
        name: "config.reload_class",
        group: "config",
        verb: "reload_class",
        title: "When a setting takes effect",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A setting that quietly waits for a restart is changed, nothing happens, and the conclusion is that the setting does not work.",
    },
}

impl CapabilityHandler<ReloadClass> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ReloadClassIn,
    ) -> Result<ReloadClassOut, CapabilityError> {
        let entries: Vec<ReloadEntry> = input
            .keys
            .into_iter()
            .map(|key| {
                let class = omt_config::schema::reload_class(&key);
                ReloadEntry {
                    key,
                    class: format!("{class:?}")
                        .chars()
                        .flat_map(|c| {
                            if c.is_uppercase() {
                                vec!['_', c.to_ascii_lowercase()]
                            } else {
                                vec![c]
                            }
                        })
                        .collect::<String>()
                        .trim_start_matches('_')
                        .to_owned(),
                }
            })
            .collect();
        Ok(ReloadClassOut {
            pending: entries
                .iter()
                .filter(|e| e.class != "hot")
                .map(|e| e.key.clone())
                .collect(),
            keys: entries,
        })
    }
}

/// Input to `launch.preview`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct LaunchPreviewIn {
    /// The launch configuration, as JSON.
    pub launch: serde_json::Value,
}

/// One pane omt would open.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LaunchPane {
    /// Where it starts.
    pub cwd: Option<String>,
    /// What runs in it: `shell`, `commands` or `agent`.
    pub start: String,
    /// The argument vector, for an agent pane.
    ///
    /// A vector, never a string. There is no shell in this path, so a prompt
    /// containing a backtick is a prompt rather than an execution.
    pub argv: Vec<String>,
}

/// What `launch.preview` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LaunchPreviewOut {
    /// What omt would open.
    pub panes: Vec<LaunchPane>,
    /// Which pane focus lands in.
    pub focused: Option<usize>,
    /// Everything wrong with it, all at once.
    pub problems: Vec<RefusalOut>,
}

omt_catalog::capability! {
    /// What a launch configuration would start.
    pub struct LaunchPreview;
    input  = LaunchPreviewIn,
    output = LaunchPreviewOut,
    decl = omt_catalog::Decl {
        name: "launch.preview",
        group: "launch",
        verb: "preview",
        title: "Preview a launch configuration",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "An agent is spawned with an argument vector, never by typing into a shell — a launch file is copied from a colleague and the prompt is the part people edit.",
    },
}

impl CapabilityHandler<LaunchPreview> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: LaunchPreviewIn,
    ) -> Result<LaunchPreviewOut, CapabilityError> {
        let launch: omt_config::launch::Launch = serde_json::from_value(input.launch)
            .map_err(|e| CapabilityError::invalid_input(format!("that is not a launch configuration: {e}")))?;
        let problems = omt_config::launch::check(&launch);
        Ok(LaunchPreviewOut {
            focused: omt_config::launch::focused(&launch),
            panes: launch
                .panes
                .iter()
                .map(|p| LaunchPane {
                    cwd: p.cwd.clone(),
                    start: match &p.start {
                        omt_config::launch::Start::Shell => "shell".to_owned(),
                        omt_config::launch::Start::Commands { .. } => "commands".to_owned(),
                        omt_config::launch::Start::Agent { .. } => "agent".to_owned(),
                    },
                    argv: match &p.start {
                        omt_config::launch::Start::Agent { id, args, prompt } => {
                            omt_config::launch::agent_argv(id, args, prompt.as_deref())
                        }
                        _ => Vec::new(),
                    },
                })
                .collect(),
            problems: problems
                .into_iter()
                .map(|r| RefusalOut {
                    problem: r.problem,
                    remedy: r.help,
                })
                .collect(),
        })
    }
}

/// Input to `keys.check_file`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CheckFileIn {
    /// The leader chord.
    pub leader: String,
    /// Which flavour.
    #[serde(default)]
    pub keymap: Option<String>,
    /// Trigger to action.
    pub bindings: Vec<(String, String)>,
}

/// Something wrong with a keybinding file.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BindingProblemOut {
    /// Which trigger.
    pub trigger: String,
    /// What is wrong.
    pub message: String,
    /// What to do.
    pub help: String,
}

/// What `keys.check_file` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CheckFileOut {
    /// Everything wrong, in file order.
    pub problems: Vec<BindingProblemOut>,
}

omt_catalog::capability! {
    /// Check a keybinding file against the capabilities that exist.
    pub struct CheckBindingFile;
    input  = CheckFileIn,
    output = CheckFileOut,
    decl = omt_catalog::Decl {
        name: "keys.check_file",
        group: "keys",
        verb: "check_file",
        title: "Check a keybinding file",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A binding naming a capability that is not there parses, loads and does nothing — and the person who wrote it concludes their key is broken rather than their spelling.",
    },
}

/// Serves the keybinding file check, which needs the catalog to check against.
///
/// A cell filled *after* registration finishes rather than a list captured
/// during it. Capturing mid-registration gives whatever happened to be
/// registered so far, which is a subset that changes with the order of the
/// lines above — and the failure is a valid binding reported as naming a
/// capability that does not exist. Found exactly that way.
pub struct BindingFileHandler(pub std::sync::Arc<std::sync::OnceLock<Vec<String>>>);

impl CapabilityHandler<CheckBindingFile> for BindingFileHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: CheckFileIn,
    ) -> Result<CheckFileOut, CapabilityError> {
        let file = omt_input::keymap::BindingFile {
            leader: input.leader,
            keymap: input.keymap.unwrap_or_else(|| "default".to_owned()),
            bindings: input.bindings,
        };
        Ok(CheckFileOut {
            problems: omt_input::keymap::check_bindings(
                &file,
                self.0.get().map_or(&[][..], Vec::as_slice),
            )
                .into_iter()
                .map(|p| BindingProblemOut {
                    trigger: p.trigger,
                    message: p.message,
                    help: p.help,
                })
                .collect(),
        })
    }
}

/// Input to `identity.reauth_check`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReauthIn {
    /// Which capability is being called.
    pub capability: String,
    /// Its effects.
    #[serde(default)]
    pub effects: Vec<String>,
    /// How long since this device verified, in seconds. Absent means never.
    #[serde(default)]
    pub verified_secs_ago: Option<u64>,
    /// Whether step-up is on.
    #[serde(default)]
    pub enabled: bool,
    /// Capability names that always ask.
    #[serde(default)]
    pub require: Vec<String>,
    /// Effects that ask.
    #[serde(default)]
    pub require_effects: Vec<String>,
    /// How long a verification counts for.
    #[serde(default)]
    pub freshness_secs: Option<u64>,
}

/// What `identity.reauth_check` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReauthOut {
    /// Whether the call proceeds without a prompt.
    pub proceeds: bool,
    /// Why it is asking, when it is.
    pub because: Option<String>,
    /// Whether failing narrows what the device may do.
    ///
    /// Always false, and reported so a client cannot implement it as if it
    /// might be true. A failed step-up is "not now", never "not you".
    pub failing_narrows_authority: bool,
}

omt_catalog::capability! {
    /// Whether this call needs a fresh proof that a human is here.
    pub struct ReauthCheck;
    input  = ReauthIn,
    output = ReauthOut,
    decl = omt_catalog::Decl {
        name: "identity.reauth_check",
        group: "identity",
        verb: "reauth_check",
        title: "Does this need a fresh check",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A proof-of-presence timer, not an authorisation narrowing. Failing it means the device must show a human is holding it, and then it may do everything again.",
    },
}

impl CapabilityHandler<ReauthCheck> for SecurityHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ReauthIn,
    ) -> Result<ReauthOut, CapabilityError> {
        let policy = omt_auth::reauth::Policy {
            mode: if input.enabled {
                omt_auth::reauth::Mode::On
            } else {
                omt_auth::reauth::Mode::Off
            },
            require: input.require.into_iter().collect(),
            require_effects: input.require_effects,
            freshness_secs: input.freshness_secs.unwrap_or(300),
            fallback: omt_auth::reauth::Fallback::Passphrase,
        };
        let decision = omt_auth::reauth::decide(
            &policy,
            &input.capability,
            &input.effects,
            input.verified_secs_ago,
        );
        Ok(ReauthOut {
            proceeds: decision.proceeds(),
            because: match decision {
                omt_auth::reauth::Decision::Challenge { because } => Some(because),
                omt_auth::reauth::Decision::Proceed => None,
            },
            failing_narrows_authority: omt_auth::reauth::failing_narrows_authority(),
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
