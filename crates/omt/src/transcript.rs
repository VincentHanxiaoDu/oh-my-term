//! What an agent has said and done, kept so a client can render it.
//!
//! The agent layer produced typed events — assistant messages, tool calls, tool
//! results, plans, queue changes — and every one of them was emitted into the
//! stream and then forgotten. A client that connected a minute late saw nothing
//! that had happened, and a client that reconnected saw nothing at all.
//!
//! Transcript view (`docs/architecture/08-web-client.md` §4.4) is the surface an
//! agent session is *meant* to be read through: an agent produces no OSC 133 at
//! all, so the block model does not apply to it, and what is available instead
//! is better than blocks — full messages and typed tool calls rather than a
//! byte grid. It could not be built, because nothing kept them.
//!
//! Bounded, because a long agent session would otherwise grow without limit in
//! a process that is expected to run for days.

use std::collections::BTreeMap;

use omt_events::AgentPayload;
use omt_types::SessionId;
use serde::Serialize;

/// How many entries one session keeps.
///
/// Large enough to hold a working day of a talkative agent, small enough that
/// a hundred sessions cannot exhaust memory. Older entries are dropped from the
/// front, and the drop is reported rather than hidden.
pub const LIMIT: usize = 5_000;

/// One thing that happened, as a client renders it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    /// A person said something.
    User {
        /// The text.
        text: String,
        /// Whether omt sent it on somebody's behalf rather than a human typing
        /// it — a synthetic turn must never be shown as a human one.
        synthetic: bool,
    },
    /// The agent said something.
    Assistant {
        /// The text.
        text: String,
    },
    /// The agent's own reasoning, kept apart from what it said.
    ///
    /// Separate because it is a different kind of claim and a client should be
    /// able to collapse it: reasoning rendered as speech reads as the agent
    /// telling you things it has not decided.
    Reasoning {
        /// The text.
        text: String,
    },
    /// A tool was called.
    ToolCall {
        /// Correlates with its result.
        call: String,
        /// The tool's name.
        name: String,
        /// Its arguments, verbatim.
        input: serde_json::Value,
    },
    /// A tool finished.
    ToolResult {
        /// Which call.
        call: String,
        /// What it produced, where it produced anything.
        output: Option<serde_json::Value>,
        /// What went wrong, if anything.
        error: Option<String>,
        /// How long it took.
        duration_ms: Option<u64>,
    },
    /// The agent's plan, as it now stands.
    Plan {
        /// The steps, re-rendered in place on each revision.
        steps: Vec<omt_events::PlanStep>,
    },
    /// A file changed.
    FileChanged {
        /// Which file.
        path: String,
        /// What happened to it.
        change: String,
        /// The tool that did it.
        tool: Option<String>,
    },
    /// Something omt does not model, reported by its own name rather than
    /// dropped.
    Other {
        /// The agent's own category for it.
        category: String,
        /// Its text.
        text: String,
    },
}

/// One session's transcript, plus what the agent offers.
#[derive(Debug, Default)]
pub struct Transcript {
    entries: Vec<Entry>,
    /// How many were dropped off the front, so a client can say the history is
    /// incomplete rather than presenting a partial one as whole.
    dropped: u64,
    /// The messages waiting to be sent, as the agent last reported them.
    queue: Vec<String>,
    /// The slash commands this agent offers, resolved by the agent rather than
    /// guessed from a list omt maintains.
    ///
    /// Kept whole rather than as names: a completion popup that cannot show
    /// what a command does is a list of words somebody has to remember.
    commands: Vec<omt_events::SlashCommand>,
    /// Its tools, likewise.
    tools: Vec<String>,
    /// The models it offers.
    models: Vec<String>,
    /// Whether the last message is still being streamed into.
    ///
    /// Without this, a fragment arriving after a completed message would be
    /// glued onto the end of it and two messages would read as one.
    open: bool,
}

impl Transcript {
    /// Everything kept, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// How many were dropped off the front.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// The messages waiting to be sent.
    #[must_use]
    pub fn queue(&self) -> &[String] {
        &self.queue
    }

    /// The slash commands the agent offers.
    #[must_use]
    pub fn commands(&self) -> &[omt_events::SlashCommand] {
        &self.commands
    }

    /// The tools it offers.
    #[must_use]
    pub fn tools(&self) -> &[String] {
        &self.tools
    }

    /// The models it offers.
    #[must_use]
    pub fn models(&self) -> &[String] {
        &self.models
    }

    fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
        if self.entries.len() > LIMIT {
            let over = self.entries.len() - LIMIT;
            self.entries.drain(..over);
            self.dropped += over as u64;
        }
    }

    /// Fold one agent payload in.
    ///
    /// Streaming fragments are appended to the message they belong to rather
    /// than each becoming an entry of its own: a protocol that streams a
    /// sentence in twelve chunks would otherwise render as twelve messages.
    pub fn observe(&mut self, payload: &AgentPayload) {
        match payload {
            AgentPayload::UserMessage { text, origin } => self.push(Entry::User {
                text: text.clone(),
                synthetic: !matches!(origin, omt_events::MessageOrigin::Human),
            }),
            AgentPayload::AssistantText { text, partial } => {
                self.append_text(text, *partial, false);
            }
            AgentPayload::Reasoning { text, partial } => {
                self.append_text(text, *partial, true);
            }
            AgentPayload::ToolCall { call, name, input } => self.push(Entry::ToolCall {
                call: call.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            AgentPayload::ToolResult {
                call,
                output,
                error,
                duration_ms,
            } => self.push(Entry::ToolResult {
                call: call.clone(),
                output: output.clone(),
                error: error.clone(),
                duration_ms: *duration_ms,
            }),
            AgentPayload::Plan { steps } => {
                // Re-rendered in place: a plan revised five times is one plan,
                // and five copies of it in a transcript is unreadable.
                if let Some(Entry::Plan { steps: existing }) = self
                    .entries
                    .iter_mut()
                    .rev()
                    .find(|e| matches!(e, Entry::Plan { .. }))
                {
                    existing.clone_from(steps);
                } else {
                    self.push(Entry::Plan {
                        steps: steps.clone(),
                    });
                }
            }
            AgentPayload::FileChanged { path, change, tool } => self.push(Entry::FileChanged {
                path: path.clone(),
                change: format!("{change:?}").to_lowercase(),
                tool: tool.clone(),
            }),
            AgentPayload::QueueChanged { pending, .. } => {
                // The agent's own view of its queue replaces omt's, always. A
                // queue omt maintained alongside would drift the first time a
                // message was sent by anything but this client.
                self.queue.clone_from(pending);
            }
            AgentPayload::Capabilities {
                tools,
                slash_commands,
                models,
            } => {
                self.tools.clone_from(tools);
                self.commands.clone_from(slash_commands);
                self.models.clone_from(models);
            }
            AgentPayload::Notification { kind, message } => self.push(Entry::Other {
                category: kind.clone(),
                text: message.clone(),
            }),
            // Everything else is state a client reads elsewhere — activity from
            // the merge machine, usage from the ledger, interactions from the
            // card list. Duplicating them here would give a client two answers
            // to one question.
            _ => {}
        }
    }

    /// Append a streaming fragment, or start a new message.
    fn append_text(&mut self, text: &str, partial: bool, reasoning: bool) {
        let extend = match self.entries.last_mut() {
            Some(Entry::Assistant { text: existing }) if !reasoning && self.open => {
                existing.push_str(text);
                true
            }
            Some(Entry::Reasoning { text: existing }) if reasoning && self.open => {
                existing.push_str(text);
                true
            }
            _ => false,
        };
        if !extend {
            self.push(if reasoning {
                Entry::Reasoning {
                    text: text.to_owned(),
                }
            } else {
                Entry::Assistant {
                    text: text.to_owned(),
                }
            });
        }
        // The next fragment continues this message only while more is coming.
        self.open = partial;
    }
}

/// Every session's transcript.
#[derive(Debug, Default)]
pub struct Transcripts(BTreeMap<SessionId, Transcript>);

impl Transcripts {
    /// One session's, creating it on first sight.
    pub fn entry(&mut self, session: SessionId) -> &mut Transcript {
        self.0.entry(session).or_default()
    }

    /// One session's, if it has one.
    #[must_use]
    pub fn get(&self, session: SessionId) -> Option<&Transcript> {
        self.0.get(&session)
    }

    /// Forget a session's transcript when the session goes.
    pub fn forget(&mut self, session: SessionId) {
        self.0.remove(&session);
    }
}
