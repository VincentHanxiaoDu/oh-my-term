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
// Automation primitives (22 §8.3)
// ---------------------------------------------------------------------------

/// Input to `agent.wait`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct WaitIn {
    /// Which session.
    pub session: String,
    /// Any of these ends the wait.
    pub until: Vec<crate::automation::WaitCondition>,
    /// How long to wait, in milliseconds.
    pub timeout_ms: u64,
    /// Resolve at once if the condition already holds.
    ///
    /// Default true: the alternative is a guaranteed race, because a script
    /// that checks and then waits misses the transition in between.
    #[serde(default = "waits_immediately")]
    pub immediate: bool,
}

const fn waits_immediately() -> bool {
    true
}

/// What `agent.wait` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct WaitOut {
    /// Which condition ended it, if one did.
    pub matched: Option<crate::automation::WaitCondition>,
    /// The state when it returned.
    pub state: String,
    /// How long it waited, in milliseconds.
    pub waited_ms: u64,
    /// Whether the deadline passed first.
    pub timed_out: bool,
}

omt_catalog::capability! {
    /// Block until a session's agent reaches one of the requested states.
    pub struct AgentWait;
    input  = WaitIn,
    output = WaitOut,
    decl = omt_catalog::Decl {
        name: "agent.wait",
        group: "agent",
        verb: "wait",
        title: "Wait for an agent",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        // It observes; it mutates nothing.
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The primitive that stops every script being a sleep loop. `blocked` is a state and matches at once; `interaction` is an edge and matches only a card that opens after the wait began.",
    },
}

/// Serves `agent.wait`.
pub struct AgentWaitHandler(pub State);

impl CapabilityHandler<AgentWait> for AgentWaitHandler {
    fn call(&self, _ctx: &omt_catalog::CallContext, input: WaitIn) -> Result<WaitOut, CapabilityError> {
        use crate::automation::{already_satisfied, WaitCondition};

        let id = session_id(&input.session)?;
        if input.until.is_empty() {
            // A wait with nothing to wait for is a sleep, and a script that
            // wrote one meant something else.
            return Err(CapabilityError::invalid_input(
                "name at least one condition to wait for",
            ));
        }
        // A cap rather than an unbounded wait: a request that never returns
        // holds a connection thread for as long as the caller lives.
        let timeout_ms = input.timeout_ms.min(24 * 60 * 60 * 1_000);

        let started = std::time::Instant::now();
        let deadline = started + std::time::Duration::from_millis(timeout_ms);
        let mut first = true;

        loop {
            let state = {
                let instance = self.0.lock()?;
                if instance.session(id).is_none() {
                    return Err(CapabilityError::not_found(format!(
                        "no session {}",
                        input.session
                    )));
                }
                // A session with no agent in it has no state to reach. Reported
                // as a refusal rather than waited out, because waiting thirty
                // minutes for an agent that was never there is the sleep loop
                // this capability exists to replace.
                let Some(roster) = instance.threads(id) else {
                    return Err(CapabilityError::precondition_failed(
                        "no agent is bound to that session",
                    ));
                };
                roster
                    .threads()
                    .into_iter()
                    .find(|t| !t.is_subagent)
                    .map(|t| t.state.clone())
                    .ok_or_else(|| {
                        CapabilityError::precondition_failed("no agent is bound to that session")
                    })?
            };

            // On the first pass `immediate` decides; after that any state match
            // is by definition a transition the wait saw.
            let matched = already_satisfied(&input.until, &state, !first || input.immediate);
            if let Some(matched) = matched {
                return Ok(WaitOut {
                    matched: Some(matched),
                    state: crate::capabilities::describe_agent_state(&state),
                    waited_ms: started.elapsed().as_millis() as u64,
                    timed_out: false,
                });
            }
            first = false;

            if std::time::Instant::now() >= deadline {
                return Ok(WaitOut {
                    matched: None,
                    state: crate::capabilities::describe_agent_state(&state),
                    waited_ms: started.elapsed().as_millis() as u64,
                    // Reported rather than raised: a timeout is an outcome a
                    // script branches on, and the state it timed out in is the
                    // useful part.
                    timed_out: true,
                });
            }
            let _ = WaitCondition::Idle;
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}

/// Input to `session.capture`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CaptureIn {
    /// Which session.
    pub session: String,
    /// `text` or `jsonl`.
    #[serde(default)]
    pub format: Option<String>,
    /// Most lines to return, counting back from the end.
    #[serde(default = "default_capture_lines")]
    pub lines: u32,
}

const fn default_capture_lines() -> u32 {
    1_000
}

/// What `session.capture` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CaptureOut {
    /// The capture.
    pub content: String,
    /// How many lines it holds.
    pub lines: u32,
    /// Which format it is in.
    pub format: String,
    /// How many lines the session has in total.
    ///
    /// So a script can tell a complete capture from a truncated one, which a
    /// line count alone cannot.
    pub total: u64,
}

omt_catalog::capability! {
    /// A session's scrollback as text a script can read.
    pub struct SessionCapture;
    input  = CaptureIn,
    output = CaptureOut,
    decl = omt_catalog::Decl {
        name: "session.capture",
        group: "session",
        verb: "capture",
        title: "Capture scrollback",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The thing every migrating script asks for first. Reports the total as well as what it returned, so a truncated capture is visible rather than looking complete.",
    },
}

/// Serves `session.capture`.
pub struct SessionCaptureHandler(pub State);

impl CapabilityHandler<SessionCapture> for SessionCaptureHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: CaptureIn,
    ) -> Result<CaptureOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let format = input.format.unwrap_or_else(|| "text".to_owned());
        if format != "text" && format != "jsonl" {
            return Err(CapabilityError::invalid_input(
                "format must be `text` or `jsonl`",
            ));
        }
        let instance = self.0.lock()?;
        let terminal = instance
            .runtime(id)
            .map(omt_daemon::SessionRuntime::terminal)
            .or_else(|| instance.orphan_terminal(id))
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;

        let total = terminal.total_rows();
        let wanted = u64::from(input.lines);
        let from = total.saturating_sub(wanted);
        let mut rows = Vec::new();
        for absolute in from..total {
            if let Some(line) = terminal.line_at(absolute) {
                let mut text = String::new();
                for cell in line.cells() {
                    // The spacer half of a wide character carries no glyph, and
                    // emitting one would shift the rest of the line right.
                    if cell.flags.contains(omt_term::Flags::WIDE_SPACER) {
                        continue;
                    }
                    match cell.resolve() {
                        // An untouched cell is a space, not a NUL: a script
                        // grepping the capture should not find control bytes
                        // the terminal never showed.
                        omt_term::Resolved::Char('\0') => text.push(' '),
                        omt_term::Resolved::Char(c) => text.push(c),
                        // A grapheme lives in the terminal's side table, which
                        // a capture has no handle on. `?` is what the styled
                        // path already emits, and inventing a different
                        // placeholder here would make two views disagree.
                        omt_term::Resolved::Grapheme(_) => text.push('?'),
                    }
                }
                rows.push(text.trim_end().to_owned());
            }
        }

        let content = if format == "jsonl" {
            rows.iter()
                .enumerate()
                .map(|(i, text)| {
                    serde_json::json!({ "row": from + i as u64, "text": text }).to_string()
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            rows.join("\n")
        };

        Ok(CaptureOut {
            lines: u32::try_from(rows.len()).unwrap_or(u32::MAX),
            content,
            format,
            total,
        })
    }
}
