//! The capabilities the structured client surfaces are built on.
//!
//! Block view, transcript view and search are described in
//! `docs/architecture/08-web-client.md` as complete surfaces rather than
//! previews of the terminal, and none of them could be built: a block reported
//! the *rows* its output occupied and no way to fetch them, there was no
//! scrollback over the wire at all, and nothing searched anything.
//!
//! So a phone could render a grid of 40 columns and nothing else. The block
//! model is the escape from that — output is already segmented into records —
//! and it was unreachable.

use omt_catalog::{CapabilityError, CapabilityHandler};
use serde::{Deserialize, Serialize};

use crate::capabilities::{StyledRun, runs_of, session_id};
use crate::state::State;

// ---------------------------------------------------------------------------
// Block bodies
// ---------------------------------------------------------------------------

/// Input to `block.body`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct BlockBodyIn {
    /// Which session.
    pub session: String,
    /// Which block.
    pub block: String,
    /// Most lines to return.
    ///
    /// A block can hold a whole test run, and a phone that asks for one over a
    /// slow link is a phone that stalls. The default is what fits a screen and
    /// a scroll; a caller that wants everything asks for it.
    #[serde(default = "default_lines")]
    pub lines: u32,
    /// Skip this many lines from the start, for paging through a long one.
    #[serde(default)]
    pub from: u32,
}

const fn default_lines() -> u32 {
    200
}

/// What `block.body` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct BlockBodyOut {
    /// The output, as styled runs — the same encoding the screen uses, so a
    /// client has one renderer rather than two.
    pub lines: Vec<Vec<StyledRun>>,
    /// The command that produced it, where the shell marked one.
    pub command: String,
    /// How many lines the block holds in total.
    pub total: u32,
    /// Whether more remain after what was returned.
    pub truncated: bool,
}

omt_catalog::capability! {
    /// One command block's output.
    pub struct BlockBody;
    input  = BlockBodyIn,
    output = BlockBodyOut,
    decl = omt_catalog::Decl {
        name: "block.body",
        group: "block",
        verb: "body",
        title: "Read a block's output",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The output of one command block, as styled lines. Paged, because a block can hold a whole test run and a phone asking for one over a slow link stalls.",
    },
}

/// Serves `block.body`.
pub struct BlockBodyHandler(pub State);

impl CapabilityHandler<BlockBody> for BlockBodyHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: BlockBodyIn,
    ) -> Result<BlockBodyOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let instance = self.0.lock()?;
        let terminal = instance
            .runtime(id)
            .map(omt_daemon::SessionRuntime::terminal)
            .or_else(|| instance.orphan_terminal(id))
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;

        let block = terminal
            .blocks()
            .iter()
            .find(|b| b.id.to_wire() == input.block)
            .ok_or_else(|| CapabilityError::not_found(format!("no block {}", input.block)))?;

        let start = block.output_row.unwrap_or(0);
        // A running block has no end yet, so it ends wherever the screen does.
        // Reporting nothing until it finishes would make live tailing — the one
        // thing somebody watches a running command for — impossible.
        let end = block.end_row.unwrap_or_else(|| terminal.total_rows());
        let total = u32::try_from(end.saturating_sub(start)).unwrap_or(u32::MAX);

        let first = start + u64::from(input.from);
        let mut lines = Vec::new();
        for absolute in first..end {
            if lines.len() >= input.lines as usize {
                break;
            }
            if let Some(line) = terminal.line_at(absolute) {
                lines.push(runs_of(line.cells()));
            }
        }
        let truncated = (input.from as u64 + lines.len() as u64) < u64::from(total);

        Ok(BlockBodyOut {
            lines,
            command: block.command.clone(),
            total,
            truncated,
        })
    }
}

// ---------------------------------------------------------------------------
// Scrollback
// ---------------------------------------------------------------------------

/// Input to `session.scrollback`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScrollbackIn {
    /// Which session.
    pub session: String,
    /// The first absolute row wanted.
    #[serde(default)]
    pub from: u64,
    /// How many rows.
    #[serde(default = "default_lines")]
    pub lines: u32,
}

/// What `session.scrollback` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ScrollbackOut {
    /// The rows, as styled runs.
    pub lines: Vec<Vec<StyledRun>>,
    /// Which absolute row the first one is.
    pub from: u64,
    /// How many rows the session has ever written, so a client can size a
    /// scrollbar without fetching everything behind it.
    pub total: u64,
}

omt_catalog::capability! {
    /// A session's history, as styled lines.
    pub struct SessionScrollback;
    input  = ScrollbackIn,
    output = ScrollbackOut,
    decl = omt_catalog::Decl {
        name: "session.scrollback",
        group: "session",
        verb: "scrollback",
        title: "Read scrollback",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "History as styled lines, by absolute row. Structured rather than raw bytes, so a browser needs no second terminal emulator to show what scrolled past.",
    },
}

/// Serves `session.scrollback`.
pub struct SessionScrollbackHandler(pub State);

impl CapabilityHandler<SessionScrollback> for SessionScrollbackHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ScrollbackIn,
    ) -> Result<ScrollbackOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let instance = self.0.lock()?;
        let terminal = instance
            .runtime(id)
            .map(omt_daemon::SessionRuntime::terminal)
            .or_else(|| instance.orphan_terminal(id))
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;

        let total = terminal.total_rows();
        let mut lines = Vec::new();
        for absolute in input.from..total {
            if lines.len() >= input.lines as usize {
                break;
            }
            if let Some(line) = terminal.line_at(absolute) {
                lines.push(runs_of(line.cells()));
            }
        }
        Ok(ScrollbackOut {
            lines,
            from: input.from,
            total,
        })
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Input to `search.session`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchIn {
    /// Which session.
    pub session: String,
    /// What to look for.
    pub query: String,
    /// Match case exactly.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Most matches to return.
    #[serde(default = "default_matches")]
    pub limit: u32,
}

const fn default_matches() -> u32 {
    100
}

/// One hit.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SearchHit {
    /// Which absolute row.
    pub row: u64,
    /// Where in the row the match starts, in characters.
    pub start: u32,
    /// How long it is, in characters.
    pub len: u32,
    /// The whole line, so a result list can show context without a second call
    /// per hit.
    pub text: String,
}

/// What `search.session` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SearchOut {
    /// Oldest first.
    pub hits: Vec<SearchHit>,
    /// Whether the limit cut the answer short, so a client can say "more"
    /// rather than implying it found everything.
    pub truncated: bool,
}

omt_catalog::capability! {
    /// Find text in a session's output.
    pub struct SearchSession;
    input  = SearchIn,
    output = SearchOut,
    decl = omt_catalog::Decl {
        name: "search.session",
        group: "search",
        verb: "session",
        title: "Search a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Find text across a session's screen and history, with the row and offsets of each hit so a client can scroll to it and highlight exactly that span.",
    },
}

/// Serves `search.session`.
pub struct SearchSessionHandler(pub State);

impl CapabilityHandler<SearchSession> for SearchSessionHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: SearchIn,
    ) -> Result<SearchOut, CapabilityError> {
        if input.query.is_empty() {
            // An empty query matches every position on every line. Refused
            // rather than answered, because the answer is useless and enormous.
            return Err(CapabilityError::invalid_input(
                "a search needs something to look for",
            ));
        }
        let id = session_id(&input.session)?;
        let instance = self.0.lock()?;
        let terminal = instance
            .runtime(id)
            .map(omt_daemon::SessionRuntime::terminal)
            .or_else(|| instance.orphan_terminal(id))
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;

        let needle = if input.case_sensitive {
            input.query.clone()
        } else {
            input.query.to_lowercase()
        };

        let mut hits = Vec::new();
        let mut truncated = false;
        for absolute in 0..terminal.total_rows() {
            let Some(line) = terminal.line_at(absolute) else {
                continue;
            };
            let text: String = runs_of(line.cells())
                .into_iter()
                .map(|r| r.text)
                .collect::<String>()
                .trim_end()
                .to_owned();
            let haystack = if input.case_sensitive {
                text.clone()
            } else {
                text.to_lowercase()
            };

            let mut at = 0usize;
            while let Some(found) = haystack[at..].find(&needle) {
                if hits.len() >= input.limit as usize {
                    truncated = true;
                    break;
                }
                let byte = at + found;
                // Character offsets, not byte offsets: a client highlighting by
                // byte index puts the highlight in the wrong place on any line
                // with a non-ASCII character in it.
                let start = haystack[..byte].chars().count();
                hits.push(SearchHit {
                    row: absolute,
                    start: u32::try_from(start).unwrap_or(0),
                    len: u32::try_from(input.query.chars().count()).unwrap_or(0),
                    text: text.clone(),
                });
                at = byte + needle.len().max(1);
                if at >= haystack.len() {
                    break;
                }
            }
            if truncated {
                break;
            }
        }

        Ok(SearchOut { hits, truncated })
    }
}

// ---------------------------------------------------------------------------
// Transcript
// ---------------------------------------------------------------------------

/// Input to `session.transcript`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TranscriptIn {
    /// Which session.
    pub session: String,
    /// Skip this many entries from the start.
    #[serde(default)]
    pub from: u32,
    /// Most entries to return.
    #[serde(default = "default_entries")]
    pub limit: u32,
}

const fn default_entries() -> u32 {
    500
}

/// What `session.transcript` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct TranscriptOut {
    /// Oldest first.
    pub entries: Vec<crate::transcript::Entry>,
    /// How many the session holds in total.
    pub total: u32,
    /// How many were dropped off the front of a long session, so a client says
    /// the history is incomplete rather than presenting a partial one as whole.
    pub dropped: u64,
}

omt_catalog::capability! {
    /// What an agent has said and done.
    pub struct SessionTranscript;
    input  = TranscriptIn,
    output = TranscriptOut,
    decl = omt_catalog::Decl {
        name: "session.transcript",
        group: "session",
        verb: "transcript",
        title: "Read an agent transcript",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Messages, reasoning, tool calls and results for an agent session. An agent emits no command marks, so blocks do not apply to it; this is what it is read through instead.",
    },
}

/// Serves `session.transcript`.
pub struct SessionTranscriptHandler(pub State);

impl CapabilityHandler<SessionTranscript> for SessionTranscriptHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: TranscriptIn,
    ) -> Result<TranscriptOut, CapabilityError> {
        let id = session_id(&input.session)?;
        // The session has to exist; having no transcript does not make it an
        // error. A shell with no agent in it has an empty transcript, which is
        // a true answer rather than a failure.
        if self.0.lock()?.session(id).is_none() {
            return Err(CapabilityError::not_found(format!(
                "no session {}",
                input.session
            )));
        }
        let transcripts = self.0.transcripts()?;
        let Some(transcript) = transcripts.get(id) else {
            return Ok(TranscriptOut {
                entries: Vec::new(),
                total: 0,
                dropped: 0,
            });
        };
        let all = transcript.entries();
        let entries = all
            .iter()
            .skip(input.from as usize)
            .take(input.limit as usize)
            .cloned()
            .collect();
        Ok(TranscriptOut {
            entries,
            total: u32::try_from(all.len()).unwrap_or(u32::MAX),
            dropped: transcript.dropped(),
        })
    }
}

// ---------------------------------------------------------------------------
// What an agent offers
// ---------------------------------------------------------------------------

/// Input to `agent.offers`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct OffersIn {
    /// Which session.
    pub session: String,
}

/// What `agent.offers` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct OffersOut {
    /// The slash commands, as the agent itself resolved them.
    pub commands: Vec<omt_events::SlashCommand>,
    /// Its tools.
    pub tools: Vec<String>,
    /// The models it offers.
    pub models: Vec<String>,
    /// The messages waiting to be sent.
    pub queue: Vec<String>,
}

omt_catalog::capability! {
    /// The commands, tools and queue an agent has.
    pub struct AgentOffers;
    input  = OffersIn,
    output = OffersOut,
    decl = omt_catalog::Decl {
        name: "agent.offers",
        group: "agent",
        verb: "offers",
        title: "What an agent offers",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Slash commands, tools, models and the pending message queue, as the agent itself reported them. Never a list omt maintains: a completion popup offering a command the agent does not have is worse than none.",
    },
}

/// Serves `agent.offers`.
pub struct AgentOffersHandler(pub State);

impl CapabilityHandler<AgentOffers> for AgentOffersHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: OffersIn,
    ) -> Result<OffersOut, CapabilityError> {
        let id = session_id(&input.session)?;
        if self.0.lock()?.session(id).is_none() {
            return Err(CapabilityError::not_found(format!(
                "no session {}",
                input.session
            )));
        }
        let transcripts = self.0.transcripts()?;
        let Some(transcript) = transcripts.get(id) else {
            // Nothing has told omt what this agent offers. Empty rather than
            // guessed: a popup listing commands from a table omt keeps would
            // offer things the agent in front of somebody does not have.
            return Ok(OffersOut {
                commands: Vec::new(),
                tools: Vec::new(),
                models: Vec::new(),
                queue: Vec::new(),
            });
        };
        Ok(OffersOut {
            commands: transcript.commands().to_vec(),
            tools: transcript.tools().to_vec(),
            models: transcript.models().to_vec(),
            queue: transcript.queue().to_vec(),
        })
    }
}

// ---------------------------------------------------------------------------
// Re-running a block
// ---------------------------------------------------------------------------

/// Input to `block.rerun`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RerunIn {
    /// Which session.
    pub session: String,
    /// Which block.
    pub block: String,
    /// The writer epoch, checked like any other write.
    pub epoch: u64,
}

/// What `block.rerun` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct RerunOut {
    /// What was sent, so a caller can show it rather than assuming.
    pub command: String,
}

omt_catalog::capability! {
    /// Run a block's command again.
    pub struct BlockRerun;
    input  = RerunIn,
    output = RerunOut,
    decl = omt_catalog::Decl {
        name: "block.rerun",
        group: "block",
        verb: "rerun",
        title: "Re-run a command",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_PTY,
        intent: Some(omt_catalog::Intent::RawStream),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Send a block's command to its session again. Refused for a block omt never saw the command of, because the alternative is scraping the screen for something that looks like a prompt.",
    },
}

/// Serves `block.rerun`.
pub struct BlockRerunHandler(pub State);

impl CapabilityHandler<BlockRerun> for BlockRerunHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RerunIn,
    ) -> Result<RerunOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;

        let command = {
            let terminal = instance
                .runtime(id)
                .map(omt_daemon::SessionRuntime::terminal)
                .or_else(|| instance.orphan_terminal(id))
                .ok_or_else(|| CapabilityError::not_found("no such session"))?;
            let block = terminal
                .blocks()
                .iter()
                .find(|b| b.id.to_wire() == input.block)
                .ok_or_else(|| CapabilityError::not_found(format!("no block {}", input.block)))?;
            // A block opened from output alone — a shell with no marks, a bare
            // `ssh`, a container — has no command omt ever saw. Re-running it
            // would mean guessing from the screen, and guessing wrong here runs
            // something nobody asked for.
            if !block.attributed || block.command.trim().is_empty() {
                return Err(CapabilityError::precondition_failed(
                    "omt never saw the command behind that block, so it cannot run it again",
                ));
            }
            block.command.clone()
        };

        instance
            .write_session_input(
                id,
                omt_session::Epoch(input.epoch),
                format!("{command}\r").as_bytes(),
            )
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;

        Ok(RerunOut { command })
    }
}

// ---------------------------------------------------------------------------
// The agent's message queue
// ---------------------------------------------------------------------------

/// Input to `queue.enqueue`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct EnqueueIn {
    /// Which session.
    pub session: String,
    /// The message.
    pub text: String,
}

/// What a queue operation reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct QueueOut {
    /// The queue as it now stands, as the agent reports it.
    pub pending: Vec<String>,
}

omt_catalog::capability! {
    /// Add a message to an agent's queue.
    pub struct QueueEnqueue;
    input  = EnqueueIn,
    output = QueueOut,
    decl = omt_catalog::Decl {
        name: "queue.enqueue",
        group: "queue",
        verb: "enqueue",
        title: "Queue a message",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_PTY,
        intent: Some(omt_catalog::Intent::Append {
            dedup: omt_catalog::DedupKey::IntentId,
            ttl_secs: 600,
        }),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Add a message to an agent's queue. Refused where omt only mirrors the queue rather than owning it: a client must be told it cannot write rather than shown a control that silently does nothing.",
    },
}

/// Serves `queue.enqueue`.
pub struct QueueEnqueueHandler(pub State);

impl CapabilityHandler<QueueEnqueue> for QueueEnqueueHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: EnqueueIn,
    ) -> Result<QueueOut, CapabilityError> {
        let id = session_id(&input.session)?;
        if self.0.lock()?.session(id).is_none() {
            return Err(CapabilityError::not_found(format!(
                "no session {}",
                input.session
            )));
        }
        // omt *mirrors* a queue it learned about from the agent's own events;
        // it does not own one. Writing into a mirror produces a queue the agent
        // does not have, and the divergence shows up as a message that is never
        // sent — with omt's copy insisting it is pending.
        //
        // So this refuses, and says which agents can be written to, rather than
        // accepting and dropping it. `agent.offers` reports the same fact up
        // front so a client hides the control instead of offering it.
        Err(CapabilityError::unsupported(
            "this agent's queue is mirrored, not owned: omt can show what is pending and cannot add to it",
        ))
    }
}

/// Input to `queue.remove`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct QueueRemoveIn {
    /// Which session.
    pub session: String,
    /// Which entry, by position in the pending list.
    pub index: u32,
}

omt_catalog::capability! {
    /// Remove a message from an agent's queue.
    pub struct QueueRemove;
    input  = QueueRemoveIn,
    output = QueueOut,
    decl = omt_catalog::Decl {
        name: "queue.remove",
        group: "queue",
        verb: "remove",
        title: "Unqueue a message",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_PTY,
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Take a message out of an agent's queue. Refused on a mirrored queue for the same reason enqueue is: a removal omt cannot confirm loses the message with no record of what it was.",
    },
}

/// Serves `queue.remove`.
pub struct QueueRemoveHandler(pub State);

impl CapabilityHandler<QueueRemove> for QueueRemoveHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: QueueRemoveIn,
    ) -> Result<QueueOut, CapabilityError> {
        let id = session_id(&input.session)?;
        if self.0.lock()?.session(id).is_none() {
            return Err(CapabilityError::not_found(format!(
                "no session {}",
                input.session
            )));
        }
        Err(CapabilityError::unsupported(
            "this agent's queue is mirrored, not owned: a removal omt cannot confirm would lose the message",
        ))
    }
}
