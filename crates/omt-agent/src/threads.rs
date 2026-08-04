//! The subagent roster — what a dense grid renders from.
//!
//! Claude Code's desktop client has a subagent switcher: when it spawns
//! subagents you can cycle each one's transcript. Its mobile client does not —
//! with five agents running it shows one at a time. That asymmetry is the gap
//! this exists to close, and closing it on a phone is the harder half.
//!
//! **What is honestly steerable.** A running subagent has no free-text input
//! channel: the parent agent drives it, and nothing in Claude Code exposes a
//! prompt for one. What *is* actionable is real and is most of the value —
//! a card a subagent raised is answerable, and the whole tree is interruptible.
//! Modelling a text box omt cannot deliver into would be the same mistake as
//! offering an answering affordance for an undeliverable card.

use std::collections::BTreeMap;

use omt_events::{AgentEvent, AgentPayload, ThreadRef};
use omt_types::{AgentState, InteractionId, Timestamp};

/// What one thread is doing.
#[derive(Debug, Clone, PartialEq)]
pub struct Thread {
    /// The agent's own id for it.
    pub id: String,
    /// The tool call that spawned it, for a subagent.
    pub parent: Option<String>,
    /// Whether this is a subagent rather than the main thread.
    pub is_subagent: bool,
    /// What it is doing.
    pub state: AgentState,
    /// A short label — the tool it is running, or what it was asked to do.
    pub label: Option<String>,
    /// When it was last heard from.
    pub last_seen: Timestamp,
    /// Cards it raised that are still waiting.
    ///
    /// Attributed to the thread rather than the session, which is the whole
    /// point: five subagents blocked on five different questions is five cards
    /// that have to be told apart, not one session that "needs you".
    pub open_interactions: Vec<InteractionId>,
}

impl Thread {
    /// Whether a human is needed here specifically.
    #[must_use]
    pub const fn needs_human(&self) -> bool {
        matches!(self.state, AgentState::Blocked { .. })
    }

    /// Whether this thread can still do anything.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        matches!(self.state, AgentState::Exited { .. })
    }
}

/// Every thread in one session, main and subagents alike.
#[derive(Debug, Default, Clone)]
pub struct ThreadRoster {
    threads: BTreeMap<String, Thread>,
    /// The main thread's id, once anything has identified it.
    main: Option<String>,
}

/// The id used for a session's own thread when the agent does not name one.
///
/// Agents that never spawn subagents report no thread at all; giving that work
/// a reserved id means a grid has one uniform thing to render rather than a
/// special case for "the session itself".
pub const MAIN_THREAD: &str = "main";

impl ThreadRoster {
    /// An empty roster.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold an event in.
    pub fn observe(&mut self, event: &AgentEvent) {
        let reference = event.thread.clone().unwrap_or(ThreadRef {
            id: MAIN_THREAD.to_owned(),
            parent: None,
            is_subagent: false,
            label: None,
        });
        let is_subagent = reference.is_subagent;
        let id = reference.id.clone();

        if !is_subagent {
            self.main.get_or_insert_with(|| id.clone());
        }

        let thread = self.threads.entry(id.clone()).or_insert_with(|| Thread {
            id,
            parent: reference.parent.clone(),
            is_subagent,
            state: AgentState::Starting,
            // The agent's own name for the thread, where it gave one, beats a
            // tool name: "review the auth module" says more than "Grep".
            label: reference.label.clone(),
            last_seen: event.ts,
            open_interactions: Vec::new(),
        });
        thread.last_seen = event.ts;

        match &event.payload {
            AgentPayload::TurnStart { .. } => {
                thread.state = AgentState::Working { detail: None };
            }
            AgentPayload::ToolCall { name, .. } => {
                thread.state = AgentState::Working {
                    detail: Some(name.clone()),
                };
                // Only as a fallback: an agent that named the thread said
                // something more useful than the tool it happens to be in.
                thread.label.get_or_insert_with(|| name.clone());
            }
            AgentPayload::TurnEnd { .. } => {
                thread.state = AgentState::Idle;
            }
            AgentPayload::SessionEnd { .. } => {
                thread.state = AgentState::Exited { code: None };
                // A finished thread cannot still be waiting on anybody. Leaving
                // its cards open would keep a dot lit for something nobody can
                // answer any more.
                thread.open_interactions.clear();
            }
            AgentPayload::InteractionRaised { interaction } => {
                if !thread.open_interactions.contains(interaction) {
                    thread.open_interactions.push(*interaction);
                }
                thread.state = AgentState::Blocked {
                    reason: omt_types::BlockReason::Unspecified,
                    interaction: Some(*interaction),
                };
            }
            AgentPayload::InteractionResolved { interaction } => {
                thread.open_interactions.retain(|i| i != interaction);
                if thread.open_interactions.is_empty() {
                    thread.state = AgentState::Working { detail: None };
                } else if let Some(next) = thread.open_interactions.first() {
                    // Still blocked, on a different card. Reporting idle here
                    // would drop a dot that still needs somebody.
                    thread.state = AgentState::Blocked {
                        reason: omt_types::BlockReason::Unspecified,
                        interaction: Some(*next),
                    };
                }
            }
            _ => {}
        }
    }

    /// Every thread, main first and then subagents in spawn order.
    ///
    /// Stable, because this is what a grid draws: cells that reshuffle between
    /// two renders are cells the user mistaps.
    #[must_use]
    pub fn threads(&self) -> Vec<&Thread> {
        let mut out: Vec<&Thread> = self.threads.values().collect();
        out.sort_by(|a, b| {
            a.is_subagent
                .cmp(&b.is_subagent)
                .then_with(|| a.id.cmp(&b.id))
        });
        out
    }

    /// One thread.
    #[must_use]
    pub fn thread(&self, id: &str) -> Option<&Thread> {
        self.threads.get(id)
    }

    /// Only the subagents.
    #[must_use]
    pub fn subagents(&self) -> Vec<&Thread> {
        self.threads()
            .into_iter()
            .filter(|t| t.is_subagent)
            .collect()
    }

    /// Threads waiting on a human, in the order they started waiting.
    ///
    /// What a phone's "next" button walks: five blocked subagents is five
    /// separate answers, and the user needs them one at a time in a stable
    /// order rather than a session that merely says "needs you".
    #[must_use]
    pub fn awaiting_human(&self) -> Vec<&Thread> {
        let mut out: Vec<&Thread> = self.threads.values().filter(|t| t.needs_human()).collect();
        out.sort_by_key(|t| t.last_seen);
        out
    }

    /// How many threads there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.threads.len()
    }

    /// Whether nothing has been observed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    /// A compact summary, for a grid header or a notification.
    #[must_use]
    pub fn summary(&self) -> RosterSummary {
        let mut s = RosterSummary::default();
        for t in self.threads.values() {
            match t.state {
                AgentState::Blocked { .. } => s.blocked += 1,
                AgentState::Working { .. } => s.working += 1,
                AgentState::Idle => s.idle += 1,
                AgentState::Exited { .. } => s.finished += 1,
                AgentState::Starting | AgentState::Unknown => s.unknown += 1,
            }
        }
        s
    }
}

/// How many threads are in each state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RosterSummary {
    /// Waiting on a human.
    pub blocked: usize,
    /// Doing something.
    pub working: usize,
    /// Waiting for input but not blocked on a card.
    pub idle: usize,
    /// Done.
    pub finished: usize,
    /// Cannot say.
    pub unknown: usize,
}

impl RosterSummary {
    /// Total threads.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.blocked + self.working + self.idle + self.finished + self.unknown
    }

    /// Whether anything needs a human right now.
    #[must_use]
    pub const fn needs_human(&self) -> bool {
        self.blocked > 0
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
    use omt_types::{AgentKind, BindingId, Seq, SessionId, Tier};

    fn event(thread: Option<ThreadRef>, at: i64, payload: AgentPayload) -> AgentEvent {
        AgentEvent {
            session: SessionId::new(),
            binding: BindingId::new(),
            agent: AgentKind::ClaudeCode,
            agent_version: None,
            agent_session: None,
            thread,
            seq: Seq::new(1),
            ts: Timestamp::from_unix_seconds(at),
            tier: Tier::Hook,
            payload,
        }
    }

    fn subagent(id: &str) -> Option<ThreadRef> {
        Some(ThreadRef {
            id: id.to_owned(),
            parent: Some("toolu_parent".to_owned()),
            is_subagent: true,
            label: None,
        })
    }

    fn tool(name: &str) -> AgentPayload {
        AgentPayload::ToolCall {
            call: "c1".to_owned(),
            name: name.to_owned(),
            input: serde_json::Value::Null,
        }
    }

    #[test]
    fn an_agent_with_no_subagents_still_has_one_thread_to_draw() {
        // A grid should not need a special case for "the session itself".
        let mut r = ThreadRoster::new();
        r.observe(&event(None, 1, tool("Bash")));
        assert_eq!(r.len(), 1);
        assert_eq!(r.threads()[0].id, MAIN_THREAD);
        assert!(!r.threads()[0].is_subagent);
    }

    #[test]
    fn five_subagents_are_five_cells() {
        // The thing Claude Code's own mobile client does not do: five agents in
        // parallel shown as five, not one at a time.
        let mut r = ThreadRoster::new();
        for i in 0..5 {
            r.observe(&event(subagent(&format!("sub{i}")), 1, tool("Read")));
        }
        assert_eq!(r.subagents().len(), 5);
    }

    #[test]
    fn the_main_thread_sorts_before_its_subagents() {
        let mut r = ThreadRoster::new();
        r.observe(&event(subagent("sub1"), 1, tool("Read")));
        r.observe(&event(None, 2, tool("Bash")));
        assert_eq!(r.threads()[0].id, MAIN_THREAD);
    }

    #[test]
    fn the_order_is_stable_across_renders() {
        // Cells that reshuffle between two renders are cells the user mistaps.
        let mut r = ThreadRoster::new();
        for i in 0..5 {
            r.observe(&event(subagent(&format!("sub{i}")), 1, tool("Read")));
        }
        let first: Vec<&str> = r.threads().iter().map(|t| t.id.as_str()).collect();
        let second: Vec<&str> = r.threads().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn a_card_is_attributed_to_the_subagent_that_raised_it() {
        // Five subagents blocked on five questions is five answers to give, not
        // one session that vaguely "needs you".
        let mut r = ThreadRoster::new();
        let a = InteractionId::new();
        let b = InteractionId::new();
        r.observe(&event(
            subagent("sub1"),
            1,
            AgentPayload::InteractionRaised { interaction: a },
        ));
        r.observe(&event(
            subagent("sub2"),
            2,
            AgentPayload::InteractionRaised { interaction: b },
        ));

        assert_eq!(r.thread("sub1").expect("present").open_interactions, [a]);
        assert_eq!(r.thread("sub2").expect("present").open_interactions, [b]);
        assert_eq!(r.awaiting_human().len(), 2);
    }

    #[test]
    fn threads_waiting_are_ordered_so_a_next_button_is_predictable() {
        let mut r = ThreadRoster::new();
        r.observe(&event(
            subagent("later"),
            200,
            AgentPayload::InteractionRaised {
                interaction: InteractionId::new(),
            },
        ));
        r.observe(&event(
            subagent("earlier"),
            100,
            AgentPayload::InteractionRaised {
                interaction: InteractionId::new(),
            },
        ));
        let waiting: Vec<&str> = r.awaiting_human().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(waiting, ["earlier", "later"]);
    }

    #[test]
    fn resolving_one_card_leaves_a_thread_blocked_on_the_next() {
        // Reporting idle with a card still open drops a dot that needs
        // somebody.
        let mut r = ThreadRoster::new();
        let a = InteractionId::new();
        let b = InteractionId::new();
        for i in [a, b] {
            r.observe(&event(
                subagent("sub1"),
                1,
                AgentPayload::InteractionRaised { interaction: i },
            ));
        }
        r.observe(&event(
            subagent("sub1"),
            2,
            AgentPayload::InteractionResolved { interaction: a },
        ));
        let t = r.thread("sub1").expect("present");
        assert!(t.needs_human(), "still waiting on the second card");
        assert_eq!(t.open_interactions, [b]);
    }

    #[test]
    fn resolving_the_last_card_unblocks_the_thread() {
        let mut r = ThreadRoster::new();
        let a = InteractionId::new();
        r.observe(&event(
            subagent("sub1"),
            1,
            AgentPayload::InteractionRaised { interaction: a },
        ));
        r.observe(&event(
            subagent("sub1"),
            2,
            AgentPayload::InteractionResolved { interaction: a },
        ));
        assert!(!r.thread("sub1").expect("present").needs_human());
    }

    #[test]
    fn a_finished_thread_stops_asking_for_anybody() {
        // A lit dot for something nobody can answer any more is worse than no
        // dot at all.
        let mut r = ThreadRoster::new();
        r.observe(&event(
            subagent("sub1"),
            1,
            AgentPayload::InteractionRaised {
                interaction: InteractionId::new(),
            },
        ));
        r.observe(&event(
            subagent("sub1"),
            2,
            AgentPayload::SessionEnd {
                reason: "done".to_owned(),
            },
        ));
        let t = r.thread("sub1").expect("present");
        assert!(t.is_finished());
        assert!(t.open_interactions.is_empty());
        assert!(r.awaiting_human().is_empty());
    }

    #[test]
    fn a_tool_call_labels_the_cell_with_what_it_is_doing() {
        // A grid of identical dots says how many; a labelled one says which to
        // look at.
        let mut r = ThreadRoster::new();
        r.observe(&event(subagent("sub1"), 1, tool("Grep")));
        let t = r.thread("sub1").expect("present");
        assert_eq!(t.label.as_deref(), Some("Grep"));
        assert_eq!(
            t.state,
            AgentState::Working {
                detail: Some("Grep".to_owned())
            }
        );
    }

    #[test]
    fn the_summary_counts_every_thread_exactly_once() {
        let mut r = ThreadRoster::new();
        r.observe(&event(subagent("a"), 1, tool("Read")));
        r.observe(&event(
            subagent("b"),
            1,
            AgentPayload::InteractionRaised {
                interaction: InteractionId::new(),
            },
        ));
        r.observe(&event(
            subagent("c"),
            1,
            AgentPayload::TurnEnd {
                turn: None,
                outcome: omt_events::TurnOutcome::Completed,
            },
        ));
        let s = r.summary();
        assert_eq!(s.total(), r.len());
        assert_eq!(s.working, 1);
        assert_eq!(s.blocked, 1);
        assert_eq!(s.idle, 1);
        assert!(s.needs_human());
    }

    #[test]
    fn a_duplicate_raise_does_not_double_count() {
        // Events arrive twice after a reconnect.
        let mut r = ThreadRoster::new();
        let a = InteractionId::new();
        for _ in 0..3 {
            r.observe(&event(
                subagent("sub1"),
                1,
                AgentPayload::InteractionRaised { interaction: a },
            ));
        }
        assert_eq!(r.thread("sub1").expect("present").open_interactions, [a]);
    }

    #[test]
    fn a_subagent_remembers_which_call_spawned_it() {
        // So a transcript can nest it under that call rather than inline.
        let mut r = ThreadRoster::new();
        r.observe(&event(subagent("sub1"), 1, tool("Read")));
        assert_eq!(
            r.thread("sub1").expect("present").parent.as_deref(),
            Some("toolu_parent")
        );
    }
}
