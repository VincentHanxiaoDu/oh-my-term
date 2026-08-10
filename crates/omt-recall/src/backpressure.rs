//! Keeping the indexer out of the way of the terminal.
//!
//! `docs/architecture/20-recall-and-usage.md` §13.2.
//!
//! A `cargo build` at full tilt produces tens of megabytes a second. The
//! indexer must never be in that path — not "should usually be fast", never in
//! it at all. A terminal that stutters while indexing is a terminal people turn
//! indexing off in, and then the feature is gone for exactly the sessions that
//! generated the most to search.
//!
//! **The queue is bounded and shedding, not blocking.** That is the decision
//! everything else follows from. A blocking queue puts the indexer back in the
//! PTY path by the back door: the moment it falls behind, the thing writing to
//! it waits, and the terminal stutters after all. Shedding means output is
//! occasionally missing from the index, and that is the right trade — a
//! searchable history with gaps is worth far more than a terminal that pauses.
//!
//! **What gets shed is chosen, not arbitrary.** Dropping the oldest would lose
//! the command that started the burst; dropping at random makes the gap
//! unexplainable. So the low-value bulk goes first and the small high-value
//! units are kept, which is the same value-per-byte argument the indexing
//! policy is built on, applied under pressure instead of at rest.

use serde::Serialize;

/// How much a unit is worth keeping when the queue is full.
///
/// Ordered so the cheapest to lose sorts first. A command is forty bytes and is
/// what somebody searches for; a build log is megabytes and is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    /// Block output, tool results — the bulk.
    Bulk,
    /// Agent messages, interactions.
    Normal,
    /// Commands, changed paths.
    High,
}

/// Something waiting to be indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// Its id.
    pub id: String,
    /// How much it is worth keeping.
    pub value: Value,
    /// Roughly how many bytes it will cost to index.
    pub bytes: usize,
}

/// What happened to a unit that was offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Accepted {
    /// It was queued.
    Queued,
    /// It was queued and something less valuable was dropped to make room.
    Displaced,
    /// It was refused, because everything already queued is worth more.
    Refused,
}

/// A bounded, shedding queue.
#[derive(Debug)]
pub struct Queue {
    capacity: usize,
    units: Vec<Unit>,
    dropped: u64,
    dropped_bytes: u64,
}

impl Queue {
    /// A queue of this capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            units: Vec::new(),
            dropped: 0,
            dropped_bytes: 0,
        }
    }

    /// How many units are waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// Whether anything is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// How many units have been dropped since the queue was made.
    ///
    /// Counted and reportable, never silent. A search index with unexplained
    /// gaps is one nobody can trust; one that says "I dropped 4,000 units of
    /// build output during that build" is one somebody can reason about.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// How many bytes those were.
    #[must_use]
    pub const fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }

    /// Offer a unit. Never blocks.
    pub fn offer(&mut self, unit: Unit) -> Accepted {
        if self.units.len() < self.capacity {
            self.units.push(unit);
            return Accepted::Queued;
        }

        // Full. Find the least valuable thing queued; if the newcomer beats it,
        // it takes its place. Ties go to the incumbent, so a burst of equal-
        // value units does not churn the queue replacing each other and
        // indexing none of them.
        let weakest = self
            .units
            .iter()
            .enumerate()
            .min_by_key(|(index, u)| (u.value, std::cmp::Reverse(*index)))
            .map(|(index, u)| (index, u.value));

        match weakest {
            Some((index, value)) if unit.value > value => {
                let evicted = self.units.remove(index);
                self.dropped += 1;
                self.dropped_bytes += evicted.bytes as u64;
                self.units.push(unit);
                Accepted::Displaced
            }
            _ => {
                self.dropped += 1;
                self.dropped_bytes += unit.bytes as u64;
                Accepted::Refused
            }
        }
    }

    /// Take everything waiting, highest value first.
    ///
    /// Drained in value order so that if the process dies mid-drain, what was
    /// indexed is the part worth having.
    pub fn drain(&mut self) -> Vec<Unit> {
        let mut out = std::mem::take(&mut self.units);
        out.sort_by(|a, b| b.value.cmp(&a.value));
        out
    }
}

/// A report a client can show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pressure {
    /// How many units are waiting.
    pub queued: usize,
    /// The ceiling.
    pub capacity: usize,
    /// How many have been dropped.
    pub dropped: u64,
    /// How many bytes those were.
    pub dropped_bytes: u64,
    /// What to tell somebody, when there is anything to tell.
    pub note: Option<String>,
}

/// Describe the current state.
#[must_use]
pub fn pressure(queue: &Queue) -> Pressure {
    Pressure {
        queued: queue.len(),
        capacity: queue.capacity,
        dropped: queue.dropped(),
        dropped_bytes: queue.dropped_bytes(),
        note: if queue.dropped() == 0 {
            None
        } else {
            Some(format!(
                "{} unit(s) of low-value output were not indexed while the queue was full. Commands and file changes were kept.",
                queue.dropped()
            ))
        },
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

    fn unit(id: &str, value: Value) -> Unit {
        Unit {
            id: id.to_owned(),
            value,
            bytes: 1_000,
        }
    }

    #[test]
    fn offering_never_blocks_even_when_full() {
        // A blocking queue puts the indexer back in the PTY path by the back
        // door: the moment it falls behind, the writer waits and the terminal
        // stutters after all.
        let mut q = Queue::new(2);
        for i in 0..100 {
            let _ = q.offer(unit(&format!("u{i}"), Value::Bulk));
        }
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn bulk_is_shed_before_anything_valuable() {
        // The same value-per-byte argument as the indexing policy, applied
        // under pressure instead of at rest.
        let mut q = Queue::new(2);
        q.offer(unit("log1", Value::Bulk));
        q.offer(unit("log2", Value::Bulk));
        assert_eq!(q.offer(unit("cmd", Value::High)), Accepted::Displaced);
        let drained = q.drain();
        assert!(drained.iter().any(|u| u.id == "cmd"));
    }

    #[test]
    fn a_command_is_never_dropped_for_a_build_log() {
        let mut q = Queue::new(1);
        q.offer(unit("cmd", Value::High));
        assert_eq!(q.offer(unit("log", Value::Bulk)), Accepted::Refused);
        assert_eq!(q.drain()[0].id, "cmd");
    }

    #[test]
    fn a_burst_of_equal_value_does_not_churn_the_queue() {
        // Replacing each other endlessly would index none of them, which is
        // worse than indexing the first few.
        let mut q = Queue::new(2);
        q.offer(unit("a", Value::Bulk));
        q.offer(unit("b", Value::Bulk));
        for i in 0..50 {
            assert_eq!(
                q.offer(unit(&format!("c{i}"), Value::Bulk)),
                Accepted::Refused
            );
        }
        let ids: Vec<String> = q.drain().into_iter().map(|u| u.id).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn everything_dropped_is_counted_and_never_silent() {
        // A search index with unexplained gaps is one nobody can trust.
        let mut q = Queue::new(1);
        q.offer(unit("kept", Value::High));
        for i in 0..10 {
            q.offer(unit(&format!("shed{i}"), Value::Bulk));
        }
        assert_eq!(q.dropped(), 10);
        assert_eq!(q.dropped_bytes(), 10_000);
    }

    #[test]
    fn a_report_says_what_was_lost_and_what_was_kept() {
        let mut q = Queue::new(1);
        q.offer(unit("cmd", Value::High));
        q.offer(unit("log", Value::Bulk));
        let p = pressure(&q);
        let note = p.note.expect("something to say");
        assert!(note.contains("not indexed"), "{note}");
        assert!(note.contains("Commands"), "{note}");
    }

    #[test]
    fn a_quiet_queue_has_nothing_to_report() {
        // A permanent warning about an empty problem is a warning people learn
        // to ignore.
        let mut q = Queue::new(10);
        q.offer(unit("a", Value::High));
        assert!(pressure(&q).note.is_none());
    }

    #[test]
    fn draining_gives_the_valuable_units_first() {
        // If the process dies mid-drain, what got indexed is the part worth
        // having.
        let mut q = Queue::new(10);
        q.offer(unit("log", Value::Bulk));
        q.offer(unit("msg", Value::Normal));
        q.offer(unit("cmd", Value::High));
        let ids: Vec<String> = q.drain().into_iter().map(|u| u.id).collect();
        assert_eq!(ids, ["cmd", "msg", "log"]);
    }

    #[test]
    fn draining_empties_the_queue() {
        let mut q = Queue::new(10);
        q.offer(unit("a", Value::High));
        assert_eq!(q.drain().len(), 1);
        assert!(q.is_empty());
        assert_eq!(q.drain().len(), 0);
    }

    #[test]
    fn value_orders_cheapest_to_lose_first() {
        assert!(Value::Bulk < Value::Normal);
        assert!(Value::Normal < Value::High);
    }
}
