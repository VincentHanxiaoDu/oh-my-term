//! File transfer, Interactions, Protocol agents.
//!
//! Split out of one 6454-line module, which P1's own file-length rule
//! calls a design smell and which nothing measured until the check that
//! enforces it was written.

use super::*;

// ---------------------------------------------------------------------------
// File transfer
// ---------------------------------------------------------------------------

/// Input to `fs.read`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FsReadIn {
    /// Which workspace.
    pub workspace: String,
    /// A workspace-relative path.
    pub path: String,
    /// Which chunk, counting from zero.
    #[serde(default)]
    pub chunk: u32,
}

/// What `fs.read` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FsReadOut {
    /// This chunk, base64.
    pub data: String,
    /// Which chunk this is.
    pub chunk: u32,
    /// How many there are, so a client can show a bar rather than a spinner.
    pub chunks: u32,
    /// The whole file's size.
    pub total_bytes: u64,
    /// What it appears to be, sniffed from the bytes rather than the name — an
    /// extension is a claim and the content is the fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

capability! {
    /// Read a file out of a workspace, one chunk at a time.
    pub struct FsRead;
    input  = FsReadIn,
    output = FsReadOut,
    decl = Decl {
        name: "fs.read",
        group: "fs",
        verb: "read",
        title: "Read a file",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Read one chunk of a file. Chunked so a large file over a slow link can show progress and resume rather than restarting.",
    },
}

pub(crate) struct FsReadHandler(pub(crate) State);

impl CapabilityHandler<FsRead> for FsReadHandler {
    fn call(&self, _ctx: &CallContext, input: FsReadIn) -> Result<FsReadOut, CapabilityError> {
        let path = resolve_in_workspace(&self.0, &input.workspace, &input.path)?;
        let bytes = std::fs::read(&path).map_err(|e| CapabilityError::not_found(e.to_string()))?;

        let plan = omt_media::TransferPlan::of(&bytes)
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        let index = input.chunk as usize;
        let chunk = bytes
            .chunks(omt_media::CHUNK_BYTES)
            .nth(index)
            .ok_or_else(|| {
                CapabilityError::invalid_input(format!("there is no chunk {}", input.chunk))
            })?;

        Ok(FsReadOut {
            data: b64_encode(chunk),
            chunk: input.chunk,
            chunks: plan.chunk_count() as u32,
            total_bytes: plan.total_bytes,
            media_type: plan.media_type,
        })
    }
}

/// Input to `fs.write`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FsWriteIn {
    /// Which workspace.
    pub workspace: String,
    /// Where to put it, workspace-relative.
    pub path: String,
    /// The chunk, base64.
    pub data: String,
    /// Which chunk this is.
    #[serde(default)]
    pub chunk: u32,
    /// How many there are in total.
    #[serde(default = "one")]
    pub chunks: u32,
}

pub(crate) fn one() -> u32 {
    1
}

/// What `fs.write` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct FsWriteOut {
    /// How many chunks have arrived.
    pub received: u32,
    /// How many are still missing, which is what a progress bar is drawn from.
    pub remaining: u32,
    /// Whether the file is now complete and on disk.
    pub complete: bool,
}

capability! {
    /// Write a file into a workspace, one chunk at a time.
    pub struct FsWrite;
    input  = FsWriteIn,
    output = FsWriteOut,
    decl = Decl {
        name: "fs.write",
        group: "fs",
        verb: "write",
        title: "Write a file",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_FS,
        // Keyed on the chunk as well as the intent, because the retry this
        // protects against is one chunk of many: deduplicating on the intent
        // alone would drop the rest of the file.
        intent: Some(Intent::Append {
            dedup: DedupKey::IntentIdAndTarget,
            ttl_secs: 600,
        }),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Write one chunk of a file into a workspace. The file appears only once every chunk has arrived.",
    },
}

pub(crate) struct FsWriteHandler(pub(crate) State);

impl CapabilityHandler<FsWrite> for FsWriteHandler {
    fn call(&self, _ctx: &CallContext, input: FsWriteIn) -> Result<FsWriteOut, CapabilityError> {
        let path = resolve_for_write(&self.0, &input.workspace, &input.path)?;
        let bytes = b64_decode(&input.data)
            .ok_or_else(|| CapabilityError::invalid_input("the chunk is not valid base64"))?;

        // Written beside the destination and renamed at the end. A partial file
        // under the real name is one another program will happily open, and a
        // half-copied image is worse than a missing one.
        let partial = path.with_extension("omt-partial");
        if input.chunk == 0 {
            if let Some(parent) = partial.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| CapabilityError::internal(e.to_string()))?;
            }
            std::fs::write(&partial, &bytes)
                .map_err(|e| CapabilityError::internal(e.to_string()))?;
        } else {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&partial)
                .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
            file.write_all(&bytes)
                .map_err(|e| CapabilityError::internal(e.to_string()))?;
        }

        let received = input.chunk + 1;
        let complete = received >= input.chunks;
        if complete {
            std::fs::rename(&partial, &path)
                .map_err(|e| CapabilityError::internal(e.to_string()))?;
        }
        Ok(FsWriteOut {
            received,
            remaining: input.chunks.saturating_sub(received),
            complete,
        })
    }
}

/// Resolve a path for writing, which cannot require the file to exist yet.
///
/// The read path defers to the workspace layer, which refuses `..`, absolute
/// paths and symlinks that leave the tree — but it does that by resolving a
/// path that is already there. A destination is by definition not, so
/// containment is checked twice here instead: lexically on the components, and
/// again on the deepest ancestor that does exist, which is what catches a
/// symlink pointing out of the tree.
pub(crate) fn resolve_for_write(
    state: &State,
    workspace: &str,
    rel: &str,
) -> Result<std::path::PathBuf, CapabilityError> {
    use std::path::Component;

    let id = WorkspaceId::from_wire(workspace).ok_or_else(|| {
        CapabilityError::invalid_input(format!("`{workspace}` is not a workspace id"))
    })?;
    let root = {
        let instance = state.lock()?;
        instance
            .workspace_root(id)
            .ok_or_else(|| CapabilityError::not_found("no such workspace"))?
    };
    let root = std::path::Path::new(&root)
        .canonicalize()
        .map_err(|e| CapabilityError::not_found(e.to_string()))?;

    let candidate = std::path::Path::new(rel);
    for component in candidate.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            // Refused rather than normalised away. Normalising `..` silently
            // turns a path that meant to escape into one that did not, which
            // hides an attempt that is worth failing loudly.
            _ => {
                return Err(CapabilityError::invalid_input(format!(
                    "`{rel}` must stay inside the workspace"
                )));
            }
        }
    }
    let full = root.join(candidate);

    // The deepest existing ancestor, canonicalised: a component along the way
    // may be a symlink out of the tree, and the lexical check above cannot see
    // that.
    let mut existing = full.as_path();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => break,
        }
    }
    let anchored = existing
        .canonicalize()
        .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
    if !anchored.starts_with(&root) {
        return Err(CapabilityError::invalid_input(format!(
            "`{rel}` resolves outside the workspace"
        )));
    }
    Ok(full)
}

/// Resolve a workspace-relative path, refusing anything outside it.
pub(crate) fn resolve_in_workspace(
    state: &State,
    workspace: &str,
    rel: &str,
) -> Result<std::path::PathBuf, CapabilityError> {
    let id = WorkspaceId::from_wire(workspace).ok_or_else(|| {
        CapabilityError::invalid_input(format!("`{workspace}` is not a workspace id"))
    })?;
    let root = {
        let instance = state.lock()?;
        instance
            .workspace_root(id)
            .ok_or_else(|| CapabilityError::not_found("no such workspace"))?
    };
    let fs = omt_workspace_fs::WorkspaceFs::new(std::path::Path::new(&root))
        .map_err(|e| CapabilityError::not_found(e.to_string()))?;
    // The workspace layer decides, not this function: it already refuses
    // `..`, absolute paths and symlinks that leave the tree, and a second
    // implementation of that rule here is a second chance to get it wrong.
    fs.resolve(rel)
        .map_err(|e| CapabilityError::invalid_input(e.to_string()))
}

/// Base64, because JSON cannot carry bytes.
pub(crate) fn b64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let b = [
            group[0],
            group.get(1).copied().unwrap_or(0),
            group.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= group.len() {
                let index = ((n >> (18 - i * 6)) & 0x3f) as usize;
                out.push(ALPHABET[index] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The inverse, refusing anything that is not base64 rather than guessing.
pub(crate) fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let cleaned: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    let body: Vec<u8> = cleaned.iter().copied().take_while(|c| *c != b'=').collect();
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for group in body.chunks(4) {
        let mut n = 0u32;
        for (i, c) in group.iter().enumerate() {
            n |= value(*c)? << (18 - i * 6);
        }
        let bytes = match group.len() {
            2 => 1,
            3 => 2,
            4 => 3,
            // A single trailing character encodes nothing and means the input
            // was truncated. Returning the bytes before it would hand back a
            // silently short file.
            _ => return None,
        };
        for i in 0..bytes {
            out.push(((n >> (16 - i * 8)) & 0xff) as u8);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Interactions
// ---------------------------------------------------------------------------

/// Input to `interaction.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct InteractionListIn {
    /// Only this session's, when given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// One card, as a remote surface needs it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InteractionSummary {
    /// Its id, which is how it is answered.
    pub id: String,
    /// Which session raised it.
    pub session: String,
    /// What kind of card it is.
    pub kind: String,
    /// Whether omt can deliver an answer, and over what.
    ///
    /// A surface renders its buttons from this and never from the state: a card
    /// that is open but has no channel must not offer a button, or the user
    /// finds out by the wrong option being selected.
    pub deliverable: String,
    /// Why not, when it cannot be answered here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_deliverable_because: Option<String>,
    /// Where it is in its lifecycle.
    pub state: String,
    /// The question or the command, rendered for a small screen.
    pub prompt: String,
    /// The options, verbatim and in the agent's own order.
    pub options: Vec<String>,
}

/// What `interaction.list` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InteractionListOut {
    /// The cards still waiting.
    pub interactions: Vec<InteractionSummary>,
}

capability! {
    /// Every card waiting for a human.
    pub struct InteractionList;
    input  = InteractionListIn,
    output = InteractionListOut,
    decl = Decl {
        name: "interaction.list",
        group: "interaction",
        verb: "list",
        title: "List open questions",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every interaction waiting for a human, with whether omt can answer it from here and why not when it cannot.",
    },
}

pub(crate) struct InteractionListHandler(pub(crate) State);

impl CapabilityHandler<InteractionList> for InteractionListHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: InteractionListIn,
    ) -> Result<InteractionListOut, CapabilityError> {
        let filter = match input.session.as_deref() {
            Some(raw) => Some(session_id(raw)?),
            None => None,
        };
        let instance = self.0.lock()?;
        let interactions = instance
            .ledger
            .open_interactions()
            .into_iter()
            .filter(|i| filter.is_none_or(|f| i.session == f))
            .map(summarize_interaction)
            .collect();
        Ok(InteractionListOut { interactions })
    }
}

pub(crate) fn summarize_interaction(i: &omt_events::Interaction) -> InteractionSummary {
    let (kind, prompt, options) = match &i.kind {
        omt_events::InteractionKind::Choice { questions } => (
            "choice",
            questions
                .first()
                .map(|q| q.question.clone())
                .unwrap_or_default(),
            questions
                .first()
                .map(|q| q.options.iter().map(|o| o.label.clone()).collect())
                .unwrap_or_default(),
        ),
        omt_events::InteractionKind::Permission {
            tool,
            command,
            options,
            ..
        } => (
            "permission",
            // The command if there is one, because that is what a person is
            // actually approving; the tool name alone is not enough to decide.
            command.clone().unwrap_or_else(|| tool.clone()),
            options.iter().map(|o| o.label.clone()).collect(),
        ),
        omt_events::InteractionKind::PlanReview { plan } => {
            ("plan_review", plan.clone(), Vec::new())
        }
        other => (kind_name(other), String::new(), Vec::new()),
    };

    let (deliverable, because) = match &i.deliverable {
        omt_events::Deliverable::Native => ("native".to_owned(), None),
        omt_events::Deliverable::Synthetic { .. } => ("synthetic".to_owned(), None),
        omt_events::Deliverable::None { reason } => {
            ("none".to_owned(), Some(format!("{reason:?}")))
        }
    };

    InteractionSummary {
        id: i.id.to_wire(),
        session: i.session.to_wire(),
        kind: kind.to_owned(),
        deliverable,
        not_deliverable_because: because,
        state: state_name(&i.state).to_owned(),
        prompt,
        options,
    }
}

pub(crate) fn kind_name(kind: &omt_events::InteractionKind) -> &'static str {
    match kind {
        omt_events::InteractionKind::Choice { .. } => "choice",
        omt_events::InteractionKind::Permission { .. } => "permission",
        omt_events::InteractionKind::PlanReview { .. } => "plan_review",
        _ => "other",
    }
}

pub(crate) fn state_name(state: &omt_events::InteractionState) -> &'static str {
    match state {
        omt_events::InteractionState::Open => "open",
        omt_events::InteractionState::Resolving { .. } => "resolving",
        omt_events::InteractionState::Submitted { .. } => "submitted",
        // The only state that means the agent actually took the answer.
        // Everything above it means omt sent bytes and nothing more.
        omt_events::InteractionState::Resolved { .. } => "resolved",
        omt_events::InteractionState::Undelivered { .. } => "undelivered",
        omt_events::InteractionState::Cancelled { .. } => "cancelled",
        omt_events::InteractionState::Abandoned { .. } => "abandoned",
    }
}

/// Input to `interaction.resolve`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct InteractionRespondIn {
    /// Which card.
    pub interaction: String,
    /// The chosen option, by the agent's own label — verbatim, so the recorded
    /// answer can be compared against what was sent when confirming delivery.
    pub option: String,
    /// Free text, where the question allowed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// What `interaction.resolve` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InteractionRespondOut {
    /// Its state after the answer.
    ///
    /// `resolving`, not `confirmed`: omt has claimed the right to answer and
    /// nothing more. For a synthetic responder the far side is a UI omt does
    /// not own, and confirmation comes from observing it, never from the write
    /// succeeding.
    pub state: String,
}

capability! {
    /// Answer a card.
    pub struct InteractionRespond;
    input  = InteractionRespondIn,
    output = InteractionRespondOut,
    decl = Decl {
        name: "interaction.resolve",
        group: "interaction",
        verb: "resolve",
        title: "Answer a question",
        // The name it shipped under. Every document calls this `resolve` and
        // none of them calls it `respond`, so the code was the odd one out --
        // but a capability name is a wire contract, and dropping the old one
        // outright would break a client mid-upgrade.
        aliases: &["interaction.respond"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        // Compare-and-swap against the card's state, which is what makes two
        // people answering the same card at once resolve to one answer and one
        // loser who is told so — rather than two answers, both delivered.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Answer an open interaction. Exactly once: a second answer is refused with who won, and answerability comes from the deliverable rather than the state.",
    },
}

pub(crate) struct InteractionRespondHandler(pub(crate) State);

impl CapabilityHandler<InteractionRespond> for InteractionRespondHandler {
    fn call(
        &self,
        ctx: &CallContext,
        input: InteractionRespondIn,
    ) -> Result<InteractionRespondOut, CapabilityError> {
        let id = omt_types::InteractionId::from_wire(&input.interaction).ok_or_else(|| {
            CapabilityError::invalid_input(format!(
                "`{}` is not an interaction id",
                input.interaction
            ))
        })?;
        let mut instance = self.0.lock()?;

        let response = match instance.ledger.get(id).map(|i| &i.kind) {
            Some(omt_events::InteractionKind::Permission { .. }) => {
                omt_events::InteractionResponse::Permission {
                    option: input.option.clone(),
                    updated_input: None,
                }
            }
            Some(_) => omt_events::InteractionResponse::Choice {
                answers: vec![omt_events::ChoiceAnswer {
                    labels: vec![input.option.clone()],
                    other: input.text.clone(),
                    comment: None,
                }],
            },
            None => {
                return Err(CapabilityError::not_found(format!(
                    "no interaction {}",
                    input.interaction
                )));
            }
        };

        let interaction = instance
            .ledger
            .resolve(id, ctx.actor.clone(), omt_types::Timestamp::now(), response)
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;

        Ok(InteractionRespondOut {
            state: state_name(&interaction.state).to_owned(),
        })
    }
}

// ---------------------------------------------------------------------------
// Protocol agents
// ---------------------------------------------------------------------------

/// Input to `agent.connect`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentConnectIn {
    /// Which session the agent belongs to.
    pub session: String,
    /// Which agent, by the name the catalog uses — `codex`, `opencode`.
    pub agent: String,
    /// The program to run, with arguments.
    pub command: Vec<String>,
}

/// What `agent.connect` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AgentConnectOut {
    /// How many notifications it produced before ending.
    pub events: u32,
    /// Methods the adapter had no mapping for, verbatim.
    ///
    /// Reported rather than dropped, because a protocol source that silently
    /// ignores what it does not understand degrades into a worse one while
    /// still claiming its tier — and this is the list that says which.
    pub unmapped: Vec<String>,
}

capability! {
    /// Drive an agent over its own protocol.
    pub struct AgentConnect;
    input  = AgentConnectIn,
    output = AgentConnectOut,
    decl = Decl {
        name: "agent.connect",
        group: "agent",
        verb: "connect",
        title: "Connect a protocol agent",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::SPAWNS_PROCESS,
        intent: Some(Intent::Append {
            dedup: DedupKey::IntentIdAndTarget,
            ttl_secs: 600,
        }),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Run an agent's own protocol server and fold its notifications into the session's event stream. Anything the adapter cannot map is reported by name rather than dropped.",
    },
}

pub(crate) struct AgentConnectHandler(pub(crate) State);

impl CapabilityHandler<AgentConnect> for AgentConnectHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: AgentConnectIn,
    ) -> Result<AgentConnectOut, CapabilityError> {
        let session = session_id(&input.session)?;
        let kind = match input.agent.as_str() {
            "codex" => omt_types::AgentKind::Codex,
            "opencode" => omt_types::AgentKind::Opencode,
            "gemini" => omt_types::AgentKind::GeminiCli,
            "qwen" => omt_types::AgentKind::QwenCode,
            "goose" => omt_types::AgentKind::Goose,
            // Refused rather than guessed: connecting the wrong adapter would
            // decode one agent's notifications with another's vocabulary, and
            // the result is a transcript that is subtly wrong rather than
            // obviously broken.
            other => {
                return Err(CapabilityError::invalid_input(format!(
                    "`{other}` is not an agent omt can drive over a protocol"
                )));
            }
        };
        let adapters = omt_agent_adapters::builtin();
        let adapter = adapters
            .get(kind)
            .ok_or_else(|| CapabilityError::not_found("no adapter for that agent"))?;

        let (program, args) = input
            .command
            .split_first()
            .ok_or_else(|| CapabilityError::invalid_input("no command to run"))?;
        let mut server = omt_agent_adapters::AppServer::spawn(program, args)
            .map_err(|e| CapabilityError::internal(e.to_string()))?;

        let mut events = 0u32;
        let mut unmapped = Vec::new();
        let mut payloads = Vec::new();
        server
            .drive(adapter, |line| match line {
                omt_agent_adapters::ServerLine::Payloads(p) => {
                    events += p.len() as u32;
                    payloads.extend(p);
                }
                omt_agent_adapters::ServerLine::Unknown { method } => unmapped.push(method),
                // A line that is not JSON is the agent's own diagnostics on the
                // wrong stream. Counted with the unmapped rather than dropped,
                // because it means the two sides disagree about the channel.
                omt_agent_adapters::ServerLine::Unparseable { line } => {
                    unmapped.push(format!("<not json> {line}"));
                }
            })
            .map_err(|e| CapabilityError::internal(e.to_string()))?;
        server.stop().ok();

        // Folded into the session's stream, which is what makes them reach
        // every surface: there is one event path and this is it.
        let mut instance = self.0.lock()?;
        let binding = omt_types::BindingId::new();
        {
            // Kept as well as emitted. An event that is only emitted is gone
            // the moment it is sent: a client that connects a minute later sees
            // nothing that happened, and there was no transcript to render an
            // agent session from. The ledger was in the same position — nothing
            // outside a test ever fed it, so every token counted was dropped a
            // line later.
            let mut transcripts = self.0.transcripts()?;
            let transcript = transcripts.entry(session);
            for payload in &payloads {
                transcript.observe(payload);
                instance.observe_usage(session, payload);
            }
        }
        // Named after what is running in it. A session that has an agent in it
        // and is still called `shell` is indistinguishable on a roster from the
        // four shells beside it, which is the one thing the roster exists to
        // tell apart.
        if let Some(record) = instance.session_mut(session) {
            record.title = kind.to_string();
        }

        // Folded into the roster as well as the stream. `observe_thread` had no
        // caller outside a test, so an agent could run a whole turn in a session
        // and the session still reported no threads — which is what every
        // surface reads to decide whether it is looking at an agent at all. The
        // conversation stayed disabled with "nothing in this session is an
        // agent" while the agent's own words sat in the transcript.
        for payload in &payloads {
            instance.observe_thread(
                session,
                &omt_events::AgentEvent {
                    session,
                    binding,
                    agent: kind,
                    agent_version: None,
                    agent_session: None,
                    thread: None,
                    seq: omt_types::Seq::new(0),
                    ts: omt_types::Timestamp::now(),
                    tier: omt_types::Tier::Protocol,
                    payload: payload.clone(),
                },
            );
        }

        for payload in payloads {
            instance.emit(
                session,
                omt_events::EventSourceTag::Protocol,
                omt_events::EventPayload::Agent {
                    event: Box::new(omt_events::AgentEvent {
                        session,
                        binding,
                        agent: kind,
                        agent_version: None,
                        agent_session: None,
                        thread: None,
                        seq: omt_types::Seq::new(0),
                        ts: omt_types::Timestamp::now(),
                        tier: omt_types::Tier::Protocol,
                        payload,
                    }),
                },
            );
        }

        Ok(AgentConnectOut { events, unmapped })
    }
}

// ---------------------------------------------------------------------------
// Protocol agents
// ---------------------------------------------------------------------------
