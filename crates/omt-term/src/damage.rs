//! What changed, so a renderer redraws that and not everything.
//!
//! `docs/architecture/04-terminal-core.md` §4. The core hands out two things
//! and nothing else: an immutable snapshot, and a record of what changed since
//! the last one a renderer acknowledged.
//!
//! The reason this exists rather than "redraw the screen each frame" is the
//! remote case. A local renderer redrawing eighty by twenty-four costs nothing
//! anyone can measure; sending it over a phone connection sixty times a second
//! does. Damage is what makes the same core serve both.
//!
//! Two design points are load-bearing.
//!
//! **There is a ceiling, and past it the answer is `Full`.** A record of two
//! thousand changed spans is larger than the screen it describes, so tracking
//! precisely past that point costs more than it saves. Degrading to "redraw
//! everything" is correct and cheap; the mistake would be to keep accumulating.
//!
//! **A structural change is `Reset`, not `Full`.** They differ in what the
//! renderer must throw away. `Full` means every row changed; the renderer's
//! cached geometry, scroll position and size are still valid. `Reset` means the
//! shape changed underneath it — a resize, an alternate-screen swap — and any
//! cached state is about a screen that no longer exists. A renderer told `Full`
//! when it should have been told `Reset` draws the new content into the old
//! geometry.

use crate::line::Generation;
use std::ops::Range;

/// Bumped whenever the shape of the screen changes.
///
/// A renderer holding damage from a previous epoch cannot apply it: the rows it
/// names are not the rows that exist now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Epoch(pub u64);

/// One row that changed, and how much of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowDamage {
    /// Which row of the viewport.
    pub row: u16,
    /// The columns that changed.
    ///
    /// A span rather than a bitmap: terminal writes are overwhelmingly runs,
    /// and a span is two numbers where a bitmap is a hundred bits.
    pub cols: Range<u16>,
}

/// How much changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageKind {
    /// Nothing.
    None,
    /// These rows, each with the columns that changed.
    Rows(Vec<RowDamage>),
    /// Too much to describe; redraw the content.
    ///
    /// Cached geometry is still valid — this is about content, not shape.
    Full,
    /// The shape changed. Re-fetch a snapshot and discard cached state.
    Reset,
}

/// What changed between two points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Damage {
    /// Which epoch this describes.
    pub epoch: Epoch,
    /// The sequence it starts from.
    pub from_seq: u64,
    /// The sequence it runs to.
    pub to_seq: u64,
    /// What changed.
    pub kind: DamageKind,
}

impl Damage {
    /// Whether a renderer needs to do anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self.kind, DamageKind::None)
    }

    /// Whether the renderer must discard what it has cached.
    #[must_use]
    pub const fn is_structural(&self) -> bool {
        matches!(self.kind, DamageKind::Reset)
    }
}

/// Past this many changed rows, tracking costs more than redrawing.
///
/// A record larger than the screen it describes is a record not worth sending.
pub const MAX_TRACKED_ROWS: usize = 64;

/// Accumulates what changed between snapshots.
#[derive(Debug, Clone, Default)]
pub struct Tracker {
    epoch: Epoch,
    seq: u64,
    from_seq: u64,
    rows: Vec<RowDamage>,
    /// Set once too much has changed to describe.
    saturated: bool,
    /// Set by a resize, a screen swap, or a reset.
    structural: bool,
    /// The generation each row was last seen at.
    seen: Vec<Generation>,
}

impl Tracker {
    /// A tracker for a screen of this height.
    #[must_use]
    pub fn new(rows: u16) -> Self {
        Self {
            seen: vec![Generation::default(); rows as usize],
            ..Self::default()
        }
    }

    /// The epoch it is in.
    #[must_use]
    pub const fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// Note that the screen's shape changed.
    ///
    /// Bumps the epoch, which is what stops a renderer applying damage that
    /// names rows of a screen that no longer exists.
    pub fn structural(&mut self, rows: u16) {
        self.epoch = Epoch(self.epoch.0 + 1);
        self.structural = true;
        self.rows.clear();
        self.saturated = false;
        self.seen = vec![Generation::default(); rows as usize];
        self.seq += 1;
    }

    /// Note that a row changed, over these columns.
    pub fn row(&mut self, row: u16, cols: Range<u16>) {
        self.seq += 1;
        if self.saturated || self.structural {
            return;
        }
        if let Some(existing) = self.rows.iter_mut().find(|d| d.row == row) {
            // Widened rather than appended: two writes to one row are one row
            // to redraw, and a list with the same row twice makes a renderer
            // do the work twice.
            existing.cols.start = existing.cols.start.min(cols.start);
            existing.cols.end = existing.cols.end.max(cols.end);
            return;
        }
        if self.rows.len() >= MAX_TRACKED_ROWS {
            self.saturated = true;
            self.rows.clear();
            return;
        }
        self.rows.push(RowDamage { row, cols });
    }

    /// Note that everything changed, without the shape changing.
    pub fn full(&mut self) {
        self.seq += 1;
        if !self.structural {
            self.saturated = true;
            self.rows.clear();
        }
    }

    /// Compare against the generations last seen and record what moved.
    ///
    /// The cheap path: a line's generation moves only when the line does, so
    /// this is a scan of `rows` integers rather than a comparison of cells.
    pub fn observe(&mut self, generations: &[Generation]) {
        if self.seen.len() != generations.len() {
            // The screen changed height without anybody saying so. Treated as
            // structural rather than guessed at, because a row-indexed record
            // against the wrong height points at the wrong rows.
            self.structural(u16::try_from(generations.len()).unwrap_or(u16::MAX));
            self.seen = generations.to_vec();
            return;
        }
        for (index, generation) in generations.iter().enumerate() {
            if self.seen[index] != *generation {
                self.seen[index] = *generation;
                // The whole row: a generation says a line moved, not where.
                self.row(u16::try_from(index).unwrap_or(u16::MAX), 0..u16::MAX);
            }
        }
    }

    /// Take what has accumulated and start again.
    #[must_use]
    pub fn take(&mut self) -> Damage {
        let kind = if self.structural {
            DamageKind::Reset
        } else if self.saturated {
            DamageKind::Full
        } else if self.rows.is_empty() {
            DamageKind::None
        } else {
            let mut rows = std::mem::take(&mut self.rows);
            // In order, so a renderer walks the screen once.
            rows.sort_by_key(|d| d.row);
            DamageKind::Rows(rows)
        };
        let damage = Damage {
            epoch: self.epoch,
            from_seq: self.from_seq,
            to_seq: self.seq,
            kind,
        };
        self.from_seq = self.seq;
        self.rows.clear();
        self.saturated = false;
        self.structural = false;
        damage
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
    fn nothing_changing_is_nothing_to_draw() {
        let mut t = Tracker::new(24);
        assert_eq!(t.take().kind, DamageKind::None);
    }

    #[test]
    fn a_changed_row_is_reported_with_its_span() {
        let mut t = Tracker::new(24);
        t.row(3, 10..20);
        let DamageKind::Rows(rows) = t.take().kind else {
            panic!("rows");
        };
        assert_eq!(rows, vec![RowDamage { row: 3, cols: 10..20 }]);
    }

    #[test]
    fn two_writes_to_one_row_are_one_row_to_redraw() {
        // A list with the same row twice makes a renderer do the work twice.
        let mut t = Tracker::new(24);
        t.row(3, 10..20);
        t.row(3, 5..12);
        let DamageKind::Rows(rows) = t.take().kind else {
            panic!("rows");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cols, 5..20, "the span covers both");
    }

    #[test]
    fn rows_come_back_in_order_so_a_renderer_walks_the_screen_once() {
        let mut t = Tracker::new(24);
        t.row(9, 0..1);
        t.row(2, 0..1);
        t.row(5, 0..1);
        let DamageKind::Rows(rows) = t.take().kind else {
            panic!("rows");
        };
        assert_eq!(rows.iter().map(|d| d.row).collect::<Vec<_>>(), vec![2, 5, 9]);
    }

    #[test]
    fn taking_damage_clears_it() {
        let mut t = Tracker::new(24);
        t.row(1, 0..1);
        let _ = t.take();
        assert_eq!(t.take().kind, DamageKind::None, "it is not reported twice");
    }

    #[test]
    fn too_much_change_degrades_to_a_full_redraw() {
        // A record of two thousand spans is larger than the screen it
        // describes; tracking past that point costs more than it saves.
        let mut t = Tracker::new(4096);
        for row in 0..u16::try_from(MAX_TRACKED_ROWS).expect("fits") + 1 {
            t.row(row, 0..80);
        }
        assert_eq!(t.take().kind, DamageKind::Full);
    }

    #[test]
    fn a_full_redraw_does_not_keep_accumulating_spans() {
        // The saving is in not holding them, not only in not sending them.
        let mut t = Tracker::new(4096);
        for row in 0..500u16 {
            t.row(row, 0..80);
        }
        assert_eq!(t.rows.len(), 0);
    }

    #[test]
    fn a_resize_is_reset_and_not_full() {
        // They differ in what the renderer throws away. Told `Full`, it draws
        // the new content into the old geometry.
        let mut t = Tracker::new(24);
        t.structural(40);
        assert_eq!(t.take().kind, DamageKind::Reset);
    }

    #[test]
    fn a_structural_change_outranks_whatever_else_happened() {
        // Row damage from before a resize names rows of a screen that is gone.
        let mut t = Tracker::new(24);
        t.row(3, 0..10);
        t.structural(40);
        t.row(1, 0..10);
        assert_eq!(t.take().kind, DamageKind::Reset);
    }

    #[test]
    fn the_epoch_moves_only_on_a_structural_change() {
        // It is what stops a renderer applying damage to the wrong screen, so
        // moving it for ordinary writes would make every frame a full redraw.
        let mut t = Tracker::new(24);
        let before = t.epoch();
        t.row(1, 0..5);
        assert_eq!(t.epoch(), before);
        t.structural(24);
        assert!(t.epoch() > before);
    }

    #[test]
    fn generations_say_which_rows_moved_without_comparing_cells() {
        // The cheap path: a scan of `rows` integers rather than a comparison of
        // eighty by twenty-four cells.
        let mut t = Tracker::new(3);
        let mut generations = vec![Generation::default(); 3];
        t.observe(&generations);
        let _ = t.take();

        generations[1] = Generation(7);
        t.observe(&generations);
        let DamageKind::Rows(rows) = t.take().kind else {
            panic!("rows");
        };
        assert_eq!(rows.iter().map(|d| d.row).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn an_unchanged_screen_observes_as_nothing() {
        let mut t = Tracker::new(3);
        let generations = vec![Generation(4); 3];
        t.observe(&generations);
        let _ = t.take();
        t.observe(&generations);
        assert_eq!(t.take().kind, DamageKind::None);
    }

    #[test]
    fn a_height_change_nobody_announced_is_treated_as_structural() {
        // A row-indexed record against the wrong height points at the wrong
        // rows, which is worse than redrawing.
        let mut t = Tracker::new(3);
        t.observe(&[Generation::default(); 5]);
        assert_eq!(t.take().kind, DamageKind::Reset);
    }

    #[test]
    fn the_sequence_range_covers_what_the_record_describes() {
        // A renderer that acknowledged up to `to_seq` knows exactly what it has
        // seen, which is what makes resuming a stream possible.
        let mut t = Tracker::new(24);
        t.row(1, 0..1);
        let first = t.take();
        t.row(2, 0..1);
        let second = t.take();
        assert_eq!(
            first.to_seq, second.from_seq,
            "the ranges meet with no gap and no overlap"
        );
    }
}
