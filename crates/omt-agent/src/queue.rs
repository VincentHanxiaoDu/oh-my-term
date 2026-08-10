//! The message queue, mirrored or held.
//!
//! `docs/architecture/06-agent-layer.md` §8. Queueing work for an agent that is
//! mid-turn is exactly what one wants to do from a phone, and the semantics are
//! not the obvious ones.
//!
//! The inversion worth naming rather than smoothing over: **the flagship agent
//! is the one that cannot reorder or edit its queue, and every other agent can,
//! precisely because omt is holding their queue for them.** Claude Code owns its
//! own queue and emits two verbs, `enqueue` and `remove`; there is no third one
//! to drive and no receipt that would confirm one. For every other agent omt is
//! the sink, so an edit is an ordinary compare-and-swap over a row it owns.
//!
//! Two rules run through everything here.
//!
//! **A `queue-operation` line is a receipt, not mirror data.** After an injected
//! enqueue omt waits a bounded window for the agent to write the matching line.
//! On a match the entry is `Queued`; on timeout it is `NotConfirmed` with the
//! text preserved, and it is *never retried automatically* — a repeated removal
//! against a queue that has since drained removes a different message.
//!
//! **An empty queue is a claim, and a broken reader must not be able to make
//! it.** When the transcript reader is stale, disabled, or has never seen a
//! `queue-operation` line for a binding that should have one, the queue reads
//! `Unknown`, which surfaces render distinctly from empty.

use omt_types::{AgentState, BindingId, Timestamp};
use serde::Serialize;

/// Who owns the queue behind a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    /// The agent owns it; omt mirrors what it reads and injects to change it.
    Mirrored,
    /// omt owns it, because the agent has no queue of its own.
    ///
    /// Clearly labelled as omt-managed on every surface: the text has not
    /// reached the agent, and presenting it as if it had would be a lie about
    /// where the work is.
    Managed,
}

/// Where an entry has got to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum EntryState {
    /// Injected; waiting for the agent's receipt.
    Pending,
    /// The agent wrote the matching `queue-operation` line.
    Queued,
    /// The window closed without a receipt.
    ///
    /// The text is preserved and the offered action is to open the terminal
    /// view, never to try again.
    NotConfirmed,
    /// Held by omt, never delivered to an agent.
    Held,
}

/// One queued message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// Position-independent identity.
    pub id: u64,
    /// The text.
    pub text: String,
    /// Where it has got to.
    pub state: EntryState,
    /// When it was queued.
    pub created_at: Timestamp,
    /// When it stops being obviously still wanted.
    ///
    /// Past this an entry is presented for re-confirmation rather than
    /// silently flushed: text written an hour ago against a state of the world
    /// that has moved on is not something to submit unasked.
    pub valid_until: Option<Timestamp>,
    /// Bumped on every edit, so a CAS has something to compare.
    pub version: u64,
}

/// What a surface should render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "entries")]
pub enum View {
    /// The queue, as read.
    Known(Vec<Entry>),
    /// The reader cannot say.
    ///
    /// Distinct from an empty queue, and deliberately so.
    Unknown,
}

/// Why a call was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "refusal")]
pub enum Refusal {
    /// The named binding is not the session's current binding.
    ///
    /// The session id alone is not a sufficient target. A replayed or delayed
    /// enqueue against a session whose agent has since exited types the text
    /// into the shell prompt, where it is executed as a command.
    NotCurrentBinding,
    /// The agent is not working, so there is no turn to queue behind.
    NotWorking,
    /// The entry at that id no longer carries that text.
    ///
    /// An id alone is a position, and a position in a draining queue is not a
    /// stable target.
    TextMoved,
    /// No such entry.
    NoSuchEntry,
    /// The version moved under the caller.
    VersionMoved,
    /// The agent's own queue has no primitive for this.
    Unsupported {
        /// What to tell the user, and what the surface hides on.
        reason: &'static str,
    },
}

/// A queue behind one binding.
#[derive(Debug, Clone)]
pub struct Queue {
    binding: BindingId,
    ownership: Ownership,
    entries: Vec<Entry>,
    next_id: u64,
    /// Whether the reader has ever produced a usable answer.
    readable: bool,
    /// CAS token for the ordering as a whole.
    order_version: u64,
}

/// A stable hash of an entry's text, used to name it alongside its id.
#[must_use]
pub fn text_hash(text: &str) -> u64 {
    // FNV-1a: a few lines against a dependency, and the value never leaves the
    // process except as an opaque token.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

impl Queue {
    /// A queue for a binding.
    #[must_use]
    pub const fn new(binding: BindingId, ownership: Ownership) -> Self {
        Self {
            binding,
            ownership,
            entries: Vec::new(),
            next_id: 1,
            // A managed queue is readable by construction: omt is the sink.
            readable: matches!(ownership, Ownership::Managed),
            order_version: 0,
        }
    }

    /// Who owns it.
    #[must_use]
    pub const fn ownership(&self) -> Ownership {
        self.ownership
    }

    /// The CAS token for the ordering.
    #[must_use]
    pub const fn order_version(&self) -> u64 {
        self.order_version
    }

    /// Record that the transcript reader is working and current.
    pub const fn mark_readable(&mut self) {
        self.readable = true;
    }

    /// Record that the reader is stale or disabled.
    pub const fn mark_unreadable(&mut self) {
        self.readable = false;
    }

    /// What a surface should render.
    #[must_use]
    pub fn view(&self) -> View {
        if self.readable {
            View::Known(self.entries.clone())
        } else {
            View::Unknown
        }
    }

    fn check_target(&self, binding: BindingId, state: &AgentState) -> Result<(), Refusal> {
        if binding != self.binding {
            return Err(Refusal::NotCurrentBinding);
        }
        if !matches!(state, AgentState::Working { .. }) {
            return Err(Refusal::NotWorking);
        }
        Ok(())
    }

    /// Queue text behind the agent's current turn.
    ///
    /// # Errors
    /// Refused unless the binding is current and working.
    pub fn enqueue(
        &mut self,
        binding: BindingId,
        state: &AgentState,
        text: &str,
        created_at: Timestamp,
        valid_until: Option<Timestamp>,
    ) -> Result<u64, Refusal> {
        self.check_target(binding, state)?;
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            text: text.to_owned(),
            state: match self.ownership {
                Ownership::Mirrored => EntryState::Pending,
                Ownership::Managed => EntryState::Held,
            },
            created_at,
            valid_until,
            version: 1,
        });
        self.order_version += 1;
        Ok(id)
    }

    /// The agent wrote a matching `queue-operation` line.
    ///
    /// Returns whether it named an entry that was waiting for one.
    pub fn receipt(&mut self, text: &str) -> bool {
        let hash = text_hash(text);
        for entry in &mut self.entries {
            if entry.state == EntryState::Pending && text_hash(&entry.text) == hash {
                entry.state = EntryState::Queued;
                self.readable = true;
                return true;
            }
        }
        false
    }

    /// The confirmation window closed.
    ///
    /// Never retried automatically: a repeated injection against a queue that
    /// has since drained acts on a different message.
    pub fn window_closed(&mut self, id: u64) {
        for entry in &mut self.entries {
            if entry.id == id && entry.state == EntryState::Pending {
                entry.state = EntryState::NotConfirmed;
            }
        }
    }

    /// Remove an entry, naming it by id *and* by the hash of its text.
    ///
    /// # Errors
    /// Refused if the binding is not current and working, or if the entry at
    /// that id no longer carries that text.
    pub fn remove(
        &mut self,
        binding: BindingId,
        state: &AgentState,
        id: u64,
        expected_text_hash: u64,
    ) -> Result<(), Refusal> {
        self.check_target(binding, state)?;
        let found = self
            .entries
            .iter()
            .position(|e| e.id == id)
            .ok_or(Refusal::NoSuchEntry)?;
        if text_hash(&self.entries[found].text) != expected_text_hash {
            return Err(Refusal::TextMoved);
        }
        self.entries.remove(found);
        self.order_version += 1;
        Ok(())
    }

    /// Rewrite an entry's text in place.
    ///
    /// # Errors
    /// `unsupported` on a mirrored queue: the agent emits `enqueue` and
    /// `remove` and no third verb. Decomposing an edit into remove plus
    /// re-enqueue is not an acceptable substitute — the removal is the
    /// unconfirmable half, and one that lands followed by an enqueue that does
    /// not leaves a queue that has silently lost a message.
    pub fn edit(
        &mut self,
        binding: BindingId,
        state: &AgentState,
        id: u64,
        expected_version: u64,
        text: &str,
    ) -> Result<(), Refusal> {
        self.check_target(binding, state)?;
        if self.ownership == Ownership::Mirrored {
            return Err(Refusal::Unsupported {
                reason: "the agent's own queue has no edit primitive and no receipt for one",
            });
        }
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(Refusal::NoSuchEntry)?;
        if entry.version != expected_version {
            return Err(Refusal::VersionMoved);
        }
        entry.text = text.to_owned();
        entry.version += 1;
        Ok(())
    }

    /// Reorder the whole queue under a CAS on the ordering.
    ///
    /// A CAS over the *whole* ordering rather than a move of one entry, which
    /// is the only shape that is safe when two clients drag at once.
    ///
    /// # Errors
    /// `unsupported` on a mirrored queue: there is no `queue-operation move`
    /// and no reorder gesture omt could drive position-independently.
    pub fn reorder(
        &mut self,
        binding: BindingId,
        state: &AgentState,
        order: &[u64],
        expected_version: u64,
    ) -> Result<(), Refusal> {
        self.check_target(binding, state)?;
        if self.ownership == Ownership::Mirrored {
            return Err(Refusal::Unsupported {
                reason: "the agent's own queue cannot be reordered position-independently",
            });
        }
        if expected_version != self.order_version {
            return Err(Refusal::VersionMoved);
        }
        if order.len() != self.entries.len() {
            return Err(Refusal::NoSuchEntry);
        }
        let mut next = Vec::with_capacity(order.len());
        for id in order {
            let found = self
                .entries
                .iter()
                .position(|e| e.id == *id)
                .ok_or(Refusal::NoSuchEntry)?;
            next.push(self.entries.remove(found));
        }
        self.entries = next;
        self.order_version += 1;
        Ok(())
    }

    /// Entries past their `valid_until`, to be re-confirmed rather than sent.
    #[must_use]
    pub fn stale(&self, now: Timestamp) -> Vec<u64> {
        self.entries
            .iter()
            .filter(|e| e.valid_until.is_some_and(|until| now > until))
            .map(|e| e.id)
            .collect()
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

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_seconds(seconds)
    }

    fn working() -> AgentState {
        AgentState::Working { detail: None }
    }

    fn queue(ownership: Ownership) -> (BindingId, Queue) {
        let binding = BindingId::new();
        (binding, Queue::new(binding, ownership))
    }

    fn with_one(ownership: Ownership) -> (BindingId, Queue, u64) {
        let (binding, mut q) = queue(ownership);
        let id = q
            .enqueue(binding, &working(), "run the tests", at(0), None)
            .expect("enqueued");
        // The reader has spoken for this binding; tests that care about the
        // unreadable case build their own queue.
        q.mark_readable();
        (binding, q, id)
    }

    #[test]
    fn an_enqueue_against_another_binding_is_refused() {
        // A replayed enqueue against a session whose agent has since exited
        // types the text into the shell prompt, where it is executed as a
        // command. The session id alone is not a sufficient target.
        let (_, mut q) = queue(Ownership::Mirrored);
        assert_eq!(
            q.enqueue(BindingId::new(), &working(), "rm -rf /", at(0), None),
            Err(Refusal::NotCurrentBinding)
        );
    }

    #[test]
    fn an_enqueue_against_an_idle_agent_is_refused() {
        // There is no turn to queue behind, and the prompt is a shell.
        let (binding, mut q) = queue(Ownership::Mirrored);
        assert_eq!(
            q.enqueue(binding, &AgentState::Idle, "text", at(0), None),
            Err(Refusal::NotWorking)
        );
    }

    #[test]
    fn a_mirrored_entry_waits_for_the_agents_receipt() {
        // The `queue-operation` line is delivery confirmation, not mirror data.
        let (_, q, _) = with_one(Ownership::Mirrored);
        let View::Known(entries) = q.view() else {
            panic!("readable");
        };
        assert_eq!(entries[0].state, EntryState::Pending);
    }

    #[test]
    fn a_matching_receipt_marks_it_queued() {
        let (_, mut q, _) = with_one(Ownership::Mirrored);
        assert!(q.receipt("run the tests"));
        let View::Known(entries) = q.view() else {
            panic!("readable");
        };
        assert_eq!(entries[0].state, EntryState::Queued);
    }

    #[test]
    fn a_closed_window_fails_the_entry_and_keeps_its_text() {
        // Never retried automatically: a repeated injection against a queue
        // that has since drained acts on a different message.
        let (_, mut q, id) = with_one(Ownership::Mirrored);
        q.window_closed(id);
        q.mark_readable();
        let View::Known(entries) = q.view() else {
            panic!("readable");
        };
        assert_eq!(entries[0].state, EntryState::NotConfirmed);
        assert_eq!(entries[0].text, "run the tests", "the text is preserved");
    }

    #[test]
    fn a_receipt_after_the_window_closed_does_not_resurrect_it() {
        // The entry has already been reported as unconfirmed and the user has
        // been told to look at the terminal; flipping it back silently would
        // contradict what they were shown.
        let (_, mut q, id) = with_one(Ownership::Mirrored);
        q.window_closed(id);
        assert!(!q.receipt("run the tests"));
    }

    #[test]
    fn a_broken_reader_reports_unknown_never_empty() {
        // An empty queue is a claim, and a broken reader must not be able to
        // make it.
        let (_, mut q) = queue(Ownership::Mirrored);
        assert_eq!(q.view(), View::Unknown);
        q.mark_readable();
        assert_eq!(q.view(), View::Known(Vec::new()));
        q.mark_unreadable();
        assert_eq!(q.view(), View::Unknown, "and it goes back on staleness");
    }

    #[test]
    fn an_omt_managed_queue_is_readable_by_construction() {
        // omt is the sink, so there is no reader to be stale.
        let (_, q) = queue(Ownership::Managed);
        assert_eq!(q.view(), View::Known(Vec::new()));
    }

    #[test]
    fn a_managed_entry_is_never_reported_as_delivered() {
        // It has not reached the agent, and saying otherwise is a lie about
        // where the work is.
        let (_, q, _) = with_one(Ownership::Managed);
        let View::Known(entries) = q.view() else {
            panic!("readable");
        };
        assert_eq!(entries[0].state, EntryState::Held);
    }

    #[test]
    fn a_removal_names_the_entry_by_text_as_well_as_by_id() {
        let (binding, mut q, id) = with_one(Ownership::Mirrored);
        assert!(q
            .remove(binding, &working(), id, text_hash("run the tests"))
            .is_ok());
    }

    #[test]
    fn a_removal_whose_text_has_moved_is_refused_and_writes_nothing() {
        // An id alone is a position, and a position in a draining queue is not
        // a stable target — the agent consumed the entry and the id now names
        // somebody else's message.
        let (binding, mut q, id) = with_one(Ownership::Mirrored);
        assert_eq!(
            q.remove(binding, &working(), id, text_hash("something else")),
            Err(Refusal::TextMoved)
        );
        q.mark_readable();
        let View::Known(entries) = q.view() else {
            panic!("readable");
        };
        assert_eq!(entries.len(), 1, "nothing was written");
    }

    #[test]
    fn the_flagship_agent_is_the_one_that_cannot_edit_its_queue() {
        // The inversion worth naming: omt holds every other agent's queue, so
        // an edit is a row rewrite; Claude Code holds its own and emits no
        // third verb.
        let (binding, mut q, id) = with_one(Ownership::Mirrored);
        assert!(matches!(
            q.edit(binding, &working(), id, 1, "different"),
            Err(Refusal::Unsupported { .. })
        ));

        let (binding, mut q, id) = with_one(Ownership::Managed);
        assert!(q.edit(binding, &working(), id, 1, "different").is_ok());
    }

    #[test]
    fn an_edit_will_not_decompose_into_remove_plus_enqueue() {
        // Refused rather than emulated: the removal is the unconfirmable half,
        // and one that lands followed by an enqueue that does not leaves a
        // queue that has silently lost a message.
        let (binding, mut q, id) = with_one(Ownership::Mirrored);
        let before = q.view();
        let _ = q.edit(binding, &working(), id, 1, "different");
        q.mark_readable();
        assert_eq!(q.view(), before, "the entry is untouched");
    }

    #[test]
    fn an_edit_under_a_stale_version_is_refused() {
        let (binding, mut q, id) = with_one(Ownership::Managed);
        q.edit(binding, &working(), id, 1, "first").expect("first");
        assert_eq!(
            q.edit(binding, &working(), id, 1, "second"),
            Err(Refusal::VersionMoved)
        );
    }

    #[test]
    fn reorder_is_a_cas_over_the_whole_ordering() {
        // The only shape that is safe when two clients drag at once.
        let (binding, mut q) = queue(Ownership::Managed);
        let a = q.enqueue(binding, &working(), "a", at(0), None).expect("a");
        let b = q.enqueue(binding, &working(), "b", at(0), None).expect("b");
        let version = q.order_version();
        q.reorder(binding, &working(), &[b, a], version).expect("reordered");
        let View::Known(entries) = q.view() else {
            panic!("readable");
        };
        assert_eq!(entries[0].id, b);
        assert_eq!(
            q.reorder(binding, &working(), &[a, b], version),
            Err(Refusal::VersionMoved),
            "the second dragger loses rather than both winning"
        );
    }

    #[test]
    fn a_mirrored_queue_cannot_be_reordered_at_all() {
        // No `queue-operation move`, and an inferred gesture is what D3
        // forbids.
        let (binding, mut q, _) = with_one(Ownership::Mirrored);
        assert!(matches!(
            q.reorder(binding, &working(), &[1], 1),
            Err(Refusal::Unsupported { .. })
        ));
    }

    #[test]
    fn an_expired_entry_is_offered_for_re_confirmation_not_flushed() {
        // Text written an hour ago against a state of the world that has moved
        // on is not something to submit unasked.
        let (binding, mut q) = queue(Ownership::Managed);
        let id = q
            .enqueue(binding, &working(), "deploy", at(0), Some(at(100)))
            .expect("enqueued");
        assert!(q.stale(at(50)).is_empty());
        assert_eq!(q.stale(at(200)), vec![id]);
        let View::Known(entries) = q.view() else {
            panic!("readable");
        };
        assert_eq!(entries.len(), 1, "and it is still there");
    }

    #[test]
    fn an_entry_without_an_expiry_never_goes_stale() {
        let (_, q, _) = with_one(Ownership::Managed);
        assert!(q.stale(at(i64::from(u32::MAX))).is_empty());
    }
}
