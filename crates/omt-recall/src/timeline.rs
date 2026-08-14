//! What happened in a session, assembled rather than observed.
//!
//! `docs/architecture/20-recall-and-usage.md` §8. Every input already exists in
//! the agent event stream, the block log and the interaction ledger. Nothing
//! here watches anything new — it is assembly, which is why it can be rebuilt
//! from the log at any time and why a bug in it is never a lost recording.
//!
//! The feature is collapsing. A raw event stream for a twenty-minute agent turn
//! is hundreds of entries, most of them the same tool called again, and reading
//! it is worse than reading the scrollback it was meant to replace. `Bash(cargo
//! test) ×7` is one line that says more than seven lines saying `Bash(cargo
//! test)`.
//!
//! Collapsing is only safe because of the canonicaliser. Two calls collapse
//! when they are the *same call*, not when they merely look similar — and the
//! canonicaliser is deliberately conservative: it strips whitespace and nothing
//! else. A cleverer one that ignored arguments would collapse `rm -rf build`
//! and `rm -rf /`, and the resulting line would be a lie about what happened.

use serde::Serialize;

/// What a timeline entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Something the user said.
    UserMessage,
    /// Something the agent said.
    AssistantMessage,
    /// A tool the agent called.
    ToolCall,
    /// A file that changed.
    FileChanged,
    /// A question the agent stopped to ask.
    Interaction,
    /// A command that exited non-zero.
    FailedBlock,
}

/// One thing that happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Its per-session sequence number, which is what orders the timeline.
    pub seq: u64,
    /// When, in seconds since the epoch.
    pub at: u64,
    /// What kind.
    pub kind: Kind,
    /// What it was, in the agent's own words.
    pub label: String,
}

/// One line of the assembled timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The sequence number of the first event in it.
    pub seq: u64,
    /// When the first one happened.
    pub at: u64,
    /// What kind.
    pub kind: Kind,
    /// What it was.
    pub label: String,
    /// How many identical events were collapsed into this line.
    ///
    /// One for an ordinary entry. A count is shown rather than hidden, because
    /// "this happened seven times" is usually the most interesting fact on the
    /// line — it is how a loop looks from outside.
    pub count: u32,
    /// How long from the first to the last of them, in seconds.
    pub spanned_secs: u64,
}

/// Counts over a whole timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Stats {
    /// How many events went in.
    pub events: u32,
    /// How many lines came out.
    pub entries: u32,
    /// How many tool calls.
    pub tool_calls: u32,
    /// How many files changed.
    pub files_changed: u32,
    /// How many questions were asked.
    pub interactions: u32,
    /// How many commands failed.
    pub failed_blocks: u32,
}

/// An assembled timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Timeline {
    /// The lines, in sequence order.
    pub entries: Vec<Entry>,
    /// Counts over the whole thing.
    pub stats: Stats,
}

/// Reduce a label to the form two identical calls share.
///
/// Whitespace only. A cleverer canonicaliser that ignored arguments would
/// collapse `rm -rf build` and `rm -rf /` into one line, and that line would be
/// a lie about what happened — which is the single worst thing a timeline can
/// be, because it is read exactly when somebody is trying to find out what
/// happened.
#[must_use]
pub fn canonical(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Assemble a timeline from an event stream.
///
/// Only *consecutive* identical events collapse. Two runs of the same command
/// separated by something else are two separate facts — an agent that ran the
/// tests, edited a file, and ran them again did something different from one
/// that ran them twice in a row.
#[must_use]
pub fn assemble(events: &[Event]) -> Timeline {
    let mut ordered: Vec<&Event> = events.iter().collect();
    // Sorted by seq rather than timestamp. Two events in the same second are
    // common, and their order matters; the sequence number is the causality
    // record and the clock is not.
    ordered.sort_by_key(|e| e.seq);

    let mut entries: Vec<Entry> = Vec::new();
    let mut stats = Stats {
        #[allow(clippy::cast_possible_truncation, reason = "counts are small")]
        events: events.len() as u32,
        ..Stats::default()
    };

    for event in ordered {
        match event.kind {
            Kind::ToolCall => stats.tool_calls += 1,
            Kind::FileChanged => stats.files_changed += 1,
            Kind::Interaction => stats.interactions += 1,
            Kind::FailedBlock => stats.failed_blocks += 1,
            _ => {}
        }

        let canonical_label = canonical(&event.label);
        match entries.last_mut() {
            Some(last) if last.kind == event.kind && canonical(&last.label) == canonical_label => {
                last.count += 1;
                last.spanned_secs = event.at.saturating_sub(last.at);
            }
            _ => entries.push(Entry {
                seq: event.seq,
                at: event.at,
                kind: event.kind,
                label: canonical_label,
                count: 1,
                spanned_secs: 0,
            }),
        }
    }

    #[allow(clippy::cast_possible_truncation, reason = "counts are small")]
    {
        stats.entries = entries.len() as u32;
    }
    Timeline { entries, stats }
}

/// A short account of a timeline, for somebody who was away.
///
/// Facts in a fixed order, not prose. A generated sentence would be omt
/// narrating work it did not do, and the first time the narration is subtly
/// wrong is the last time anybody reads one.
#[must_use]
pub fn digest(timeline: &Timeline) -> String {
    let s = timeline.stats;
    let mut parts = Vec::new();
    if s.tool_calls > 0 {
        parts.push(format!(
            "{} tool call{}",
            s.tool_calls,
            plural(s.tool_calls)
        ));
    }
    if s.files_changed > 0 {
        parts.push(format!(
            "{} file{} changed",
            s.files_changed,
            plural(s.files_changed)
        ));
    }
    if s.interactions > 0 {
        parts.push(format!(
            "{} question{}",
            s.interactions,
            plural(s.interactions)
        ));
    }
    if s.failed_blocks > 0 {
        parts.push(format!(
            "{} failure{}",
            s.failed_blocks,
            plural(s.failed_blocks)
        ));
    }
    if parts.is_empty() {
        return "Nothing happened.".to_owned();
    }
    // The largest collapsed run, when there is one worth mentioning. It is the
    // shape of a loop, and somebody returning to a session wants to know about
    // it before anything else.
    let repeated = timeline.entries.iter().max_by_key(|e| e.count);
    if let Some(entry) = repeated
        && entry.count >= 3
    {
        parts.push(format!("{} ×{}", entry.label, entry.count));
    }
    parts.join(", ")
}

const fn plural(n: u32) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn event(seq: u64, at: u64, kind: Kind, label: &str) -> Event {
        Event {
            seq,
            at,
            kind,
            label: label.to_owned(),
        }
    }

    #[test]
    fn repeated_calls_collapse_into_one_line_with_a_count() {
        // A raw stream for a twenty-minute turn is hundreds of entries, and
        // reading it is worse than reading the scrollback it replaced.
        let events: Vec<Event> = (0..7)
            .map(|i| event(i, 100 + i * 10, Kind::ToolCall, "Bash(cargo test)"))
            .collect();
        let t = assemble(&events);
        assert_eq!(t.entries.len(), 1);
        assert_eq!(t.entries[0].count, 7);
        assert_eq!(t.entries[0].spanned_secs, 60);
    }

    #[test]
    fn only_consecutive_repeats_collapse() {
        // An agent that ran the tests, edited a file, and ran them again did
        // something different from one that ran them twice in a row.
        let events = vec![
            event(0, 0, Kind::ToolCall, "Bash(cargo test)"),
            event(1, 1, Kind::FileChanged, "src/a.rs"),
            event(2, 2, Kind::ToolCall, "Bash(cargo test)"),
        ];
        let t = assemble(&events);
        assert_eq!(t.entries.len(), 3);
        assert!(t.entries.iter().all(|e| e.count == 1));
    }

    #[test]
    fn two_different_calls_never_collapse_however_similar() {
        // The canonicaliser strips whitespace and nothing else. One that
        // ignored arguments would collapse `rm -rf build` and `rm -rf /`, and
        // the line would be a lie about what happened.
        let events = vec![
            event(0, 0, Kind::ToolCall, "Bash(rm -rf build)"),
            event(1, 1, Kind::ToolCall, "Bash(rm -rf /)"),
        ];
        assert_eq!(assemble(&events).entries.len(), 2);
    }

    #[test]
    fn whitespace_alone_does_not_prevent_a_collapse() {
        let events = vec![
            event(0, 0, Kind::ToolCall, "Bash(cargo  test)"),
            event(1, 1, Kind::ToolCall, "Bash(cargo test)"),
        ];
        assert_eq!(assemble(&events).entries.len(), 1);
    }

    #[test]
    fn the_same_label_of_a_different_kind_stays_separate() {
        let events = vec![
            event(0, 0, Kind::ToolCall, "src/a.rs"),
            event(1, 1, Kind::FileChanged, "src/a.rs"),
        ];
        assert_eq!(assemble(&events).entries.len(), 2);
    }

    #[test]
    fn ordering_follows_the_sequence_number_not_the_clock() {
        // Two events in the same second are common and their order matters.
        // The sequence number is the causality record; the clock is not.
        let events = vec![
            event(2, 10, Kind::UserMessage, "second"),
            event(1, 10, Kind::UserMessage, "first"),
        ];
        let t = assemble(&events);
        assert_eq!(t.entries[0].label, "first");
    }

    #[test]
    fn the_stats_count_the_events_not_the_collapsed_lines() {
        // Otherwise seven runs of a failing command would report as one
        // failure, which is the opposite of what somebody needs to know.
        let events: Vec<Event> = (0..5)
            .map(|i| event(i, i, Kind::FailedBlock, "cargo test"))
            .collect();
        let t = assemble(&events);
        assert_eq!(t.stats.failed_blocks, 5);
        assert_eq!(t.stats.entries, 1);
        assert_eq!(t.stats.events, 5);
    }

    #[test]
    fn a_digest_is_facts_in_a_fixed_order_rather_than_prose() {
        // A generated sentence would be omt narrating work it did not do, and
        // the first time the narration is subtly wrong is the last time
        // anybody reads one.
        let events = vec![
            event(0, 0, Kind::ToolCall, "Read(a.rs)"),
            event(1, 1, Kind::FileChanged, "a.rs"),
            event(2, 2, Kind::Interaction, "May I write?"),
        ];
        let text = digest(&assemble(&events));
        assert!(text.contains("1 tool call"), "{text}");
        assert!(text.contains("1 file changed"), "{text}");
        assert!(text.contains("1 question"), "{text}");
    }

    #[test]
    fn a_digest_leads_with_nothing_when_nothing_happened() {
        assert_eq!(digest(&assemble(&[])), "Nothing happened.");
    }

    #[test]
    fn a_loop_shows_up_in_the_digest() {
        // Somebody returning to a session wants to know about it before
        // anything else.
        let events: Vec<Event> = (0..6)
            .map(|i| event(i, i, Kind::ToolCall, "Bash(cargo test)"))
            .collect();
        let text = digest(&assemble(&events));
        assert!(text.contains("×6"), "{text}");
    }

    #[test]
    fn a_pair_of_repeats_is_not_called_a_loop() {
        // Twice is a retry. Calling it a loop would put a scary line on an
        // ordinary session.
        let events: Vec<Event> = (0..2)
            .map(|i| event(i, i, Kind::ToolCall, "Bash(cargo test)"))
            .collect();
        assert!(!digest(&assemble(&events)).contains('×'));
    }

    #[test]
    fn assembling_is_deterministic_and_rebuildable() {
        // It is derived, so the same log must always give the same timeline —
        // otherwise a cached one and a rebuilt one disagree and there is no way
        // to tell which is right.
        let events: Vec<Event> = (0..20)
            .map(|i| {
                event(
                    i,
                    i,
                    if i % 3 == 0 {
                        Kind::ToolCall
                    } else {
                        Kind::FileChanged
                    },
                    "x",
                )
            })
            .collect();
        let once = assemble(&events);
        for _ in 0..3 {
            assert_eq!(assemble(&events), once);
        }
    }
}
