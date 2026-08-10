//! Delivering events to plugins without letting them hold up the terminal.
//!
//! `docs/architecture/11-plugins.md` §1. A plugin sits on the event path, not
//! the PTY byte path, and the host budgets one warm process per plugin with a
//! bounded queue in front of it.
//!
//! The rule that matters: **a plugin can never back-pressure the terminal.**
//! Fan-out is asynchronous and lossy by policy — a slow plugin gets a bounded
//! queue and then dropped events *with a counter*. The counter is the whole
//! difference between a policy and a bug: a plugin that missed events is told
//! how many, so it can resynchronize rather than carry on with a view of the
//! world that quietly lost a piece.
//!
//! The alternative, blocking the producer until the slowest subscriber catches
//! up, means any third-party plugin can freeze the user's typing. That is not a
//! trade worth making for delivery guarantees to a notification sink.

use std::collections::VecDeque;

/// A bounded queue in front of one plugin.
#[derive(Debug, Clone)]
pub struct Mailbox {
    capacity: usize,
    queued: VecDeque<String>,
    dropped: u64,
}

/// What happened to an event offered to a mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// It is queued.
    Queued,
    /// The queue was full and the event was dropped.
    ///
    /// Never a stall: the producer is the terminal.
    Dropped {
        /// How many this plugin has now missed in total.
        total: u64,
    },
}

impl Mailbox {
    /// A mailbox holding at most `capacity` events.
    ///
    /// A capacity of zero would drop everything while looking configured, so it
    /// is raised to one — a plugin that is subscribed receives something.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            queued: VecDeque::new(),
            dropped: 0,
        }
    }

    /// Offer an event. Never blocks.
    pub fn offer(&mut self, event: impl Into<String>) -> Delivery {
        if self.queued.len() >= self.capacity {
            // The newest is dropped rather than the oldest: dropping the head
            // would reorder what the plugin does receive, and a plugin that
            // sees event 5 before event 3 has no way to tell.
            self.dropped += 1;
            return Delivery::Dropped {
                total: self.dropped,
            };
        }
        self.queued.push_back(event.into());
        Delivery::Queued
    }

    /// Take the next event.
    pub fn take(&mut self) -> Option<String> {
        self.queued.pop_front()
    }

    /// How many events this plugin has missed.
    ///
    /// Reported to the plugin so it can resynchronize rather than carry on with
    /// a view that quietly lost a piece.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// How many are waiting.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.queued.len()
    }
}

/// Why an entrypoint kind is or is not allowed.
///
/// In-process stays first-party only. A dynamically-linked `cdylib` plugin ABI
/// is explicitly rejected: Rust has no stable ABI, and a version skew produces
/// silent undefined behaviour rather than an error — a failure mode with no
/// diagnostic, on the user's machine, in third-party code.
#[must_use]
pub fn entrypoint_refusal(kind: &str) -> Option<&'static str> {
    match kind {
        "process" | "wasm" => None,
        "cdylib" | "dylib" | "so" => Some(
            "Rust has no stable ABI, so a linked plugin that is one version out is undefined behaviour rather than an error",
        ),
        _ => Some("an entrypoint is a subprocess or a wasm component"),
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
    fn events_queue_up_to_the_capacity() {
        let mut mailbox = Mailbox::new(2);
        assert_eq!(mailbox.offer("a"), Delivery::Queued);
        assert_eq!(mailbox.offer("b"), Delivery::Queued);
        assert_eq!(mailbox.depth(), 2);
    }

    #[test]
    fn a_slow_plugin_is_dropped_rather_than_allowed_to_stall_the_terminal() {
        // Blocking the producer until the slowest subscriber catches up means
        // any third-party plugin can freeze the user's typing.
        let mut mailbox = Mailbox::new(1);
        mailbox.offer("a");
        assert_eq!(mailbox.offer("b"), Delivery::Dropped { total: 1 });
        assert_eq!(mailbox.offer("c"), Delivery::Dropped { total: 2 });
    }

    #[test]
    fn what_was_dropped_is_counted_and_reported() {
        // The counter is the difference between a policy and a bug: the plugin
        // is told it missed events and can resynchronize.
        let mut mailbox = Mailbox::new(1);
        mailbox.offer("a");
        mailbox.offer("b");
        assert_eq!(mailbox.dropped(), 1);
    }

    #[test]
    fn dropping_never_reorders_what_does_arrive() {
        // A plugin that sees event 5 before event 3 has no way to tell.
        let mut mailbox = Mailbox::new(2);
        mailbox.offer("first");
        mailbox.offer("second");
        mailbox.offer("third");
        assert_eq!(mailbox.take().as_deref(), Some("first"));
        assert_eq!(mailbox.take().as_deref(), Some("second"));
        assert_eq!(mailbox.take(), None);
    }

    #[test]
    fn draining_makes_room_again() {
        let mut mailbox = Mailbox::new(1);
        mailbox.offer("a");
        mailbox.take();
        assert_eq!(mailbox.offer("b"), Delivery::Queued);
    }

    #[test]
    fn a_capacity_of_zero_does_not_silently_drop_everything() {
        // It would look configured and deliver nothing.
        let mut mailbox = Mailbox::new(0);
        assert_eq!(mailbox.offer("a"), Delivery::Queued);
    }

    #[test]
    fn a_linked_plugin_abi_is_refused_with_the_reason() {
        // Rust has no stable ABI, and a version skew is undefined behaviour
        // rather than an error — no diagnostic, on the user's machine, in
        // third-party code.
        let refusal = entrypoint_refusal("cdylib").expect("refused");
        assert!(refusal.contains("ABI"), "{refusal}");
        assert!(entrypoint_refusal("process").is_none());
    }

    #[test]
    fn a_wasm_component_is_allowed_on_the_same_terms_as_a_subprocess() {
        // The WASM host is a transport swap, not a second plugin model.
        assert!(entrypoint_refusal("wasm").is_none());
    }
}
