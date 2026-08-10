//! Queue capabilities.
//!
//! `docs/architecture/06-agent-layer.md` §8. The doc writes these as
//! `agent.queue.enqueue`; the catalog takes a group and a verb and refuses a
//! third level, so they are declared as `queue.*`, which is the same two facts
//! in the shape the catalog holds.
//!
//! The semantics live in [`omt_agent::queue`]. This is the door to them.

use omt_agent::queue::{Ownership, Queue, Refusal};
use omt_catalog::{CapabilityError, CapabilityHandler};
use omt_types::{AgentState, BindingId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Queues, by binding.
pub type Queues = Arc<Mutex<BTreeMap<BindingId, Queue>>>;

/// Serves the queues.
pub struct QueueHandler(pub Queues);

fn refused(refusal: &Refusal) -> CapabilityError {
    match refusal {
        // Target identity, not state inference: the binding named is not the
        // one in the session, and typing into what is now a shell prompt runs
        // it as a command.
        Refusal::NotCurrentBinding => {
            CapabilityError::precondition_failed("that binding is not the session's current one")
        }
        Refusal::NotWorking => {
            CapabilityError::precondition_failed("the agent is not working, so there is no turn to queue behind")
        }
        Refusal::TextMoved => CapabilityError::precondition_failed(
            "that entry no longer carries that text; nothing was written",
        ),
        Refusal::NoSuchEntry => CapabilityError::precondition_failed("no such entry"),
        Refusal::VersionMoved => {
            CapabilityError::precondition_failed("the queue moved under this call")
        }
        Refusal::Unsupported { reason } => CapabilityError::unsupported(*reason),
    }
}

fn parse_binding(text: &str) -> Result<BindingId, CapabilityError> {
    text.parse::<BindingId>()
        .map_err(|_| CapabilityError::invalid_input("that is not a binding id"))
}

fn state_of(name: &str) -> Result<AgentState, CapabilityError> {
    match name {
        "working" => Ok(AgentState::Working { detail: None }),
        "idle" => Ok(AgentState::Idle),
        "starting" => Ok(AgentState::Starting),
        _ => Err(CapabilityError::invalid_input(
            "state must be one of working, idle, starting",
        )),
    }
}

fn ownership_of(name: &str) -> Ownership {
    // Mirrored is the conservative default: it refuses edit and reorder, and
    // guessing the permissive one would offer affordances that cannot work.
    if name == "managed" {
        Ownership::Managed
    } else {
        Ownership::Mirrored
    }
}

/// Input to `queue.enqueue`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct EnqueueIn {
    /// Which binding. Not the session: a session id alone is not a target.
    pub binding: String,
    /// What the agent is doing.
    pub state: String,
    /// Who owns the queue: `mirrored` or `managed`.
    #[serde(default)]
    pub ownership: Option<String>,
    /// The text to queue.
    pub text: String,
    /// Seconds after which the entry is re-confirmed rather than sent.
    #[serde(default)]
    pub valid_for_seconds: Option<i64>,
}

/// What `queue.enqueue` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct EnqueueOut {
    /// The entry's id.
    pub entry: u64,
    /// Where it has got to, which on a mirrored queue is `pending` until the
    /// agent writes its receipt.
    pub state: String,
}

omt_catalog::capability! {
    /// Queue work for an agent that is mid-turn.
    pub struct QueueEnqueue;
    input  = EnqueueIn,
    output = EnqueueOut,
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
        // ExternallyConfirmed: the agent's own `queue-operation` line is the
        // receipt, and until it arrives omt does not know the text landed.
        intent: Some(omt_catalog::Intent::ExternallyConfirmed {
            confirm_within_ms: 5_000,
        }),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Takes a binding rather than a session. A replayed enqueue against a session whose agent has exited types the text into a shell prompt, where it runs as a command.",
    },
}

impl CapabilityHandler<QueueEnqueue> for QueueHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: EnqueueIn,
    ) -> Result<EnqueueOut, CapabilityError> {
        let binding = parse_binding(&input.binding)?;
        let state = state_of(&input.state)?;
        let ownership = ownership_of(input.ownership.as_deref().unwrap_or("mirrored"));
        let mut queues = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the queue lock was poisoned"))?;
        let queue = queues
            .entry(binding)
            .or_insert_with(|| Queue::new(binding, ownership));
        let now = omt_types::Timestamp::from_unix_seconds(0);
        let valid_until = input
            .valid_for_seconds
            .map(omt_types::Timestamp::from_unix_seconds);
        let entry = queue
            .enqueue(binding, &state, &input.text, now, valid_until)
            .map_err(|e| refused(&e))?;
        let reported = match queue.view() {
            omt_agent::queue::View::Known(entries) => entries
                .iter()
                .find(|e| e.id == entry)
                .map_or_else(|| "pending".to_owned(), |e| state_name(&e.state)),
            // A managed queue is always readable, so this is the mirrored case
            // before the reader has spoken.
            omt_agent::queue::View::Unknown => "pending".to_owned(),
        };
        Ok(EnqueueOut {
            entry,
            state: reported,
        })
    }
}

fn state_name(state: &omt_agent::queue::EntryState) -> String {
    match state {
        omt_agent::queue::EntryState::Pending => "pending",
        omt_agent::queue::EntryState::Queued => "queued",
        omt_agent::queue::EntryState::NotConfirmed => "not_confirmed",
        omt_agent::queue::EntryState::Held => "held",
    }
    .to_owned()
}

/// Input to `queue.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListIn {
    /// Which binding.
    pub binding: String,
}

/// One entry, as reported.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListedEntry {
    /// Its id.
    pub entry: u64,
    /// Its text.
    pub text: String,
    /// Where it has got to.
    pub state: String,
    /// Its version, for a compare-and-swap edit.
    pub version: u64,
}

/// What `queue.list` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListOut {
    /// `known` or `unknown`. Never an empty list standing in for `unknown`.
    pub kind: String,
    /// The entries, when known.
    pub entries: Vec<ListedEntry>,
    /// Whether this queue can be edited and reordered.
    pub can_rearrange: bool,
}

omt_catalog::capability! {
    /// What is queued behind an agent.
    pub struct QueueList;
    input  = ListIn,
    output = ListOut,
    decl = omt_catalog::Decl {
        name: "queue.list",
        group: "queue",
        verb: "list",
        title: "List the queue",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Reports `unknown` when the reader is stale or has never spoken. An empty queue is a claim, and a broken reader must not be able to make it.",
    },
}

impl CapabilityHandler<QueueList> for QueueHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ListIn,
    ) -> Result<ListOut, CapabilityError> {
        let binding = parse_binding(&input.binding)?;
        let queues = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the queue lock was poisoned"))?;
        let Some(queue) = queues.get(&binding) else {
            // Never seen. That is not the same as empty, and saying `known: []`
            // here is the exact claim this capability refuses to make.
            return Ok(ListOut {
                kind: "unknown".to_owned(),
                entries: Vec::new(),
                can_rearrange: false,
            });
        };
        let can_rearrange = queue.ownership() == Ownership::Managed;
        match queue.view() {
            omt_agent::queue::View::Known(entries) => Ok(ListOut {
                kind: "known".to_owned(),
                entries: entries
                    .iter()
                    .map(|e| ListedEntry {
                        entry: e.id,
                        text: e.text.clone(),
                        state: state_name(&e.state),
                        version: e.version,
                    })
                    .collect(),
                can_rearrange,
            }),
            omt_agent::queue::View::Unknown => Ok(ListOut {
                kind: "unknown".to_owned(),
                entries: Vec::new(),
                can_rearrange,
            }),
        }
    }
}

/// Input to `queue.remove`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoveIn {
    /// Which binding.
    pub binding: String,
    /// What the agent is doing.
    pub state: String,
    /// Which entry.
    pub entry: u64,
    /// The text it is expected to still carry.
    ///
    /// An id alone is a position, and a position in a draining queue is not a
    /// stable target.
    pub expected_text: String,
}

/// What `queue.remove` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RemoveOut {
    /// Whether the entry is gone from omt's view.
    pub removed: bool,
}

omt_catalog::capability! {
    /// Take a message back out of the queue.
    pub struct QueueRemove;
    input  = RemoveIn,
    output = RemoveOut,
    decl = omt_catalog::Decl {
        name: "queue.remove",
        group: "queue",
        verb: "remove",
        title: "Remove a queued message",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_PTY,
        // ExternallyConfirmed rather than Cas: `intent` is static and must be
        // the strictest the capability can require, and on a mirrored queue the
        // agent's `remove` line is the only confirmation there is.
        intent: Some(omt_catalog::Intent::ExternallyConfirmed {
            confirm_within_ms: 5_000,
        }),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Never retried automatically: a repeated removal against a queue that has since drained removes a different message.",
    },
}

impl CapabilityHandler<QueueRemove> for QueueHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RemoveIn,
    ) -> Result<RemoveOut, CapabilityError> {
        let binding = parse_binding(&input.binding)?;
        let state = state_of(&input.state)?;
        let mut queues = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the queue lock was poisoned"))?;
        let queue = queues
            .get_mut(&binding)
            .ok_or_else(|| CapabilityError::precondition_failed("no queue for that binding"))?;
        queue
            .remove(
                binding,
                &state,
                input.entry,
                omt_agent::queue::text_hash(&input.expected_text),
            )
            .map_err(|e| refused(&e))?;
        Ok(RemoveOut { removed: true })
    }
}

/// Input to `queue.edit`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct EditIn {
    /// Which binding.
    pub binding: String,
    /// What the agent is doing.
    pub state: String,
    /// Which entry.
    pub entry: u64,
    /// The version it is expected to be at.
    pub expected_version: u64,
    /// The replacement text.
    pub text: String,
}

/// What `queue.edit` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct EditOut {
    /// The entry's new version.
    pub version: u64,
}

omt_catalog::capability! {
    /// Rewrite a queued message in place.
    pub struct QueueEdit;
    input  = EditIn,
    output = EditOut,
    decl = omt_catalog::Decl {
        name: "queue.edit",
        group: "queue",
        verb: "edit",
        title: "Edit a queued message",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        // Cas: offered only where omt owns the sink, so there is no
        // confirmation window and no loss mode.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Unsupported on an agent's own queue, which emits enqueue and remove and no third verb. The pair is not a substitute: the removal is the unconfirmable half.",
    },
}

impl CapabilityHandler<QueueEdit> for QueueHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: EditIn,
    ) -> Result<EditOut, CapabilityError> {
        let binding = parse_binding(&input.binding)?;
        let state = state_of(&input.state)?;
        let mut queues = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the queue lock was poisoned"))?;
        let queue = queues
            .get_mut(&binding)
            .ok_or_else(|| CapabilityError::precondition_failed("no queue for that binding"))?;
        queue
            .edit(
                binding,
                &state,
                input.entry,
                input.expected_version,
                &input.text,
            )
            .map_err(|e| refused(&e))?;
        Ok(EditOut {
            version: input.expected_version + 1,
        })
    }
}

/// Input to `queue.reorder`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReorderIn {
    /// Which binding.
    pub binding: String,
    /// What the agent is doing.
    pub state: String,
    /// Every entry id, in the order wanted.
    pub order: Vec<u64>,
    /// The ordering version this call was composed against.
    pub expected_version: u64,
}

/// What `queue.reorder` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReorderOut {
    /// The new ordering version.
    pub version: u64,
}

omt_catalog::capability! {
    /// Put the queue in a different order.
    pub struct QueueReorder;
    input  = ReorderIn,
    output = ReorderOut,
    decl = omt_catalog::Decl {
        name: "queue.reorder",
        group: "queue",
        verb: "reorder",
        title: "Reorder the queue",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::empty(),
        // A compare-and-swap over the whole ordering, which is the only shape
        // that is safe when two clients drag at once.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "This is what a client's queue-reorder check asks for. Unsupported on an agent's own queue, and the client hides the drag handles rather than disabling them.",
    },
}

impl CapabilityHandler<QueueReorder> for QueueHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ReorderIn,
    ) -> Result<ReorderOut, CapabilityError> {
        let binding = parse_binding(&input.binding)?;
        let state = state_of(&input.state)?;
        let mut queues = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the queue lock was poisoned"))?;
        let queue = queues
            .get_mut(&binding)
            .ok_or_else(|| CapabilityError::precondition_failed("no queue for that binding"))?;
        queue
            .reorder(binding, &state, &input.order, input.expected_version)
            .map_err(|e| refused(&e))?;
        Ok(ReorderOut {
            version: queue.order_version(),
        })
    }
}
