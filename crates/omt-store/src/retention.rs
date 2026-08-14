//! Compact first, delete second.
//!
//! `docs/architecture/21-data-lifecycle.md` §3. Old data loses value unevenly,
//! which is the whole reason this is a ladder rather than one age cutoff.
//!
//! The output of a `cargo build` from three months ago is worthless. The *fact*
//! that you ran it, in that directory, on that branch, that it took ninety-four
//! seconds and failed, is still useful and costs two hundred bytes. A single
//! cutoff throws both away together, and the thing somebody actually searches
//! for months later is the second one.
//!
//! Two rules shape what survives compaction.
//!
//! **A failed block keeps its output.** The failures are what people come back
//! to; dropping them is dropping the answer to almost every question anybody
//! asks of old data.
//!
//! **A block an agent touched keeps its output.** Somebody reconstructing what
//! an agent did needs what it saw, and reconstructing it is a thing people do
//! after something has gone wrong — which is exactly when the record has to
//! still be there.

use serde::Serialize;

/// What kind of data this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Scrollback and block output.
    Output,
    /// Block metadata: the command, cwd, branch, exit, timing.
    BlockMetadata,
    /// Command history.
    History,
    /// The agent event log.
    AgentEvents,
    /// The interaction ledger.
    Interactions,
    /// A native session transcript.
    Transcript,
}

/// The ladder for one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ladder {
    /// Days after which it is compacted. `None` means never.
    pub compact_after_days: Option<u32>,
    /// Days after which it is deleted.
    pub delete_after_days: u32,
}

/// What happens to a piece of data of this age.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fate {
    /// Nothing yet.
    Keep,
    /// Reduced to the parts that still carry value.
    Compact,
    /// Gone.
    Delete,
}

/// The ladder each kind is on.
#[must_use]
pub const fn ladder(kind: Kind) -> Ladder {
    match kind {
        Kind::Output => Ladder {
            compact_after_days: Some(7),
            delete_after_days: 30,
        },
        // Never compacted: it is already the compacted form of something else,
        // and there is nothing left to drop that would not make it useless.
        Kind::BlockMetadata => Ladder {
            compact_after_days: None,
            delete_after_days: 365,
        },
        // Two years, and never compacted. A command is forty bytes and is the
        // single thing people search their own history for.
        Kind::History => Ladder {
            compact_after_days: None,
            delete_after_days: 730,
        },
        Kind::AgentEvents | Kind::Transcript => Ladder {
            compact_after_days: Some(14),
            delete_after_days: 90,
        },
        Kind::Interactions => Ladder {
            compact_after_days: Some(30),
            delete_after_days: 365,
        },
    }
}

/// What should happen to something of this age.
#[must_use]
pub fn fate(kind: Kind, age_days: u32) -> Fate {
    let ladder = ladder(kind);
    if age_days >= ladder.delete_after_days {
        return Fate::Delete;
    }
    match ladder.compact_after_days {
        Some(days) if age_days >= days => Fate::Compact,
        _ => Fate::Keep,
    }
}

/// One block, as retention sees it.
#[derive(Debug, Clone, Copy)]
pub struct Block {
    /// How old.
    pub age_days: u32,
    /// Whether it exited non-zero.
    pub failed: bool,
    /// Whether an agent was attributed to it.
    pub agent_touched: bool,
    /// How large its output is.
    pub output_bytes: u64,
}

/// The largest output kept regardless of anything else.
///
/// Small outputs cost nothing to keep and are often the whole answer — a
/// version string, a path, a single error line. Compacting them saves bytes
/// nobody was short of and loses the thing somebody was looking for.
pub const KEEP_OUTPUT_BYTES: u64 = 4 * 1024;

/// Whether a block's output survives compaction.
#[must_use]
pub const fn keeps_output(block: &Block) -> bool {
    // The failures are what people come back to.
    block.failed
        // Reconstructing what an agent did is something people do after
        // something has gone wrong, which is exactly when the record has to
        // still be there.
        || block.agent_touched
        || block.output_bytes <= KEEP_OUTPUT_BYTES
}

/// What compaction does to one block.
#[must_use]
pub fn compact(block: &Block) -> Fate {
    match fate(Kind::Output, block.age_days) {
        Fate::Delete => Fate::Delete,
        Fate::Compact if keeps_output(block) => Fate::Keep,
        other => other,
    }
}

/// Caps, in bytes, beyond which the oldest is compacted early.
#[derive(Debug, Clone, Copy)]
pub struct Caps {
    /// Per session.
    pub per_session: u64,
    /// Per instance.
    pub per_instance: u64,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            per_session: 8 * 1024 * 1024,
            per_instance: 4 * 1024 * 1024 * 1024,
        }
    }
}

/// How much needs to go to get back under a cap.
///
/// Returned as a number rather than acted on, so the caller decides what to
/// give up — and so a caller that decides to give up nothing still knows it is
/// over, which is the state a silent trim would hide.
#[must_use]
pub const fn over_by(used: u64, cap: u64) -> u64 {
    used.saturating_sub(cap)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn block(age_days: u32) -> Block {
        Block {
            age_days,
            failed: false,
            agent_touched: false,
            output_bytes: 1024 * 1024,
        }
    }

    #[test]
    fn recent_output_is_kept_whole() {
        assert_eq!(fate(Kind::Output, 1), Fate::Keep);
    }

    #[test]
    fn week_old_output_is_compacted_and_month_old_is_deleted() {
        assert_eq!(fate(Kind::Output, 7), Fate::Compact);
        assert_eq!(fate(Kind::Output, 30), Fate::Delete);
    }

    #[test]
    fn the_fact_that_you_ran_it_outlives_what_it_printed() {
        // The whole reason this is a ladder. The output of a build from three
        // months ago is worthless; that you ran it, where, on what branch, and
        // that it failed, is still useful and costs two hundred bytes.
        assert_eq!(fate(Kind::Output, 60), Fate::Delete);
        assert_eq!(fate(Kind::BlockMetadata, 60), Fate::Keep);
    }

    #[test]
    fn a_command_outlives_almost_everything() {
        // Forty bytes, and the single thing people search their own history
        // for.
        assert_eq!(fate(Kind::History, 365), Fate::Keep);
        assert_eq!(ladder(Kind::History).compact_after_days, None);
    }

    #[test]
    fn a_failed_block_keeps_its_output_past_compaction() {
        // The failures are what people come back to; dropping them drops the
        // answer to almost every question anybody asks of old data.
        let mut failed = block(10);
        failed.failed = true;
        assert_eq!(compact(&failed), Fate::Keep);
        assert_eq!(compact(&block(10)), Fate::Compact);
    }

    #[test]
    fn a_block_an_agent_touched_keeps_its_output() {
        // Reconstructing what an agent did is a thing people do after
        // something has gone wrong.
        let mut touched = block(10);
        touched.agent_touched = true;
        assert_eq!(compact(&touched), Fate::Keep);
    }

    #[test]
    fn a_small_output_is_not_worth_compacting() {
        // Often the whole answer — a version string, a path, one error line.
        // Compacting it saves bytes nobody was short of.
        let mut small = block(10);
        small.output_bytes = 100;
        assert_eq!(compact(&small), Fate::Keep);
    }

    #[test]
    fn deletion_still_applies_to_a_failed_block() {
        // Keeping a failure forever is a different promise from keeping it
        // past compaction, and the caps exist because the second one is
        // affordable and the first is not.
        let mut failed = block(60);
        failed.failed = true;
        assert_eq!(compact(&failed), Fate::Delete);
    }

    #[test]
    fn a_transcript_and_the_event_log_age_together() {
        // Same content in the same shape, so a different ladder would leave
        // one half of a record without the other.
        assert_eq!(ladder(Kind::Transcript), ladder(Kind::AgentEvents));
    }

    #[test]
    fn every_kind_is_deleted_eventually() {
        // Data with no delete date is data that grows without bound, and the
        // machine it grows on is somebody's laptop.
        for kind in [
            Kind::Output,
            Kind::BlockMetadata,
            Kind::History,
            Kind::AgentEvents,
            Kind::Interactions,
            Kind::Transcript,
        ] {
            assert!(ladder(kind).delete_after_days > 0, "{kind:?}");
        }
    }

    #[test]
    fn nothing_is_deleted_before_it_is_compacted() {
        // A ladder where the order inverted would delete data that was about
        // to become cheap to keep.
        for kind in [
            Kind::Output,
            Kind::AgentEvents,
            Kind::Interactions,
            Kind::Transcript,
        ] {
            let l = ladder(kind);
            let compact_at = l.compact_after_days.expect("compacted");
            assert!(compact_at < l.delete_after_days, "{kind:?}");
        }
    }

    #[test]
    fn being_over_a_cap_is_reported_rather_than_acted_on() {
        // So a caller that decides to give up nothing still knows it is over,
        // which is the state a silent trim would hide.
        let caps = Caps::default();
        assert_eq!(over_by(caps.per_session, caps.per_session), 0);
        assert_eq!(over_by(caps.per_session + 100, caps.per_session), 100);
        assert_eq!(over_by(0, caps.per_session), 0);
    }
}
