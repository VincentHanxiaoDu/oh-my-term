//! Lines: right-trimmed cell runs plus how they ended.

use crate::cell::{Cell, Color};

/// How a line ended.
///
/// This tri-state is what makes reflow a pure function of the stored lines and
/// a width. Without it, unwrapping cannot tell a line the user ended from one
/// the margin ended, and re-wrapping at a new width either joins paragraphs
/// that were separate or splits ones that were not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wrap {
    /// A real newline ended this logical line.
    #[default]
    Hard,
    /// It hit the right margin and continues on the next line.
    Soft,
    /// It wrapped because a double-width cluster would not fit. The last cell
    /// is padding and must be dropped on unwrap, not re-emitted.
    SoftWide,
}

impl Wrap {
    /// Whether the next stored line is a continuation of this one.
    #[must_use]
    pub const fn continues(self) -> bool {
        matches!(self, Self::Soft | Self::SoftWide)
    }
}

/// A hyperlink identity, interned per terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HyperlinkId(pub u32);

/// An image placement identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImagePlacementId(pub u32);

/// Run-structured attributes that would be wasteful per cell.
///
/// Hyperlinks and images are rare and always cover a contiguous run, so they
/// are stored as runs. Paying two bytes on every cell for something almost no
/// cell has is the trade this avoids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtraAttrs {
    /// OSC 8 hyperlink runs, by column range.
    pub hyperlinks: Vec<(std::ops::Range<u16>, HyperlinkId)>,
    /// Inline image placements, by column range.
    pub images: Vec<(std::ops::Range<u16>, ImagePlacementId)>,
}

impl ExtraAttrs {
    fn is_empty(&self) -> bool {
        self.hyperlinks.is_empty() && self.images.is_empty()
    }

    /// The hyperlink covering a column, if any.
    #[must_use]
    pub fn hyperlink_at(&self, col: u16) -> Option<HyperlinkId> {
        self.hyperlinks
            .iter()
            .find(|(r, _)| r.contains(&col))
            .map(|(_, id)| *id)
    }
}

/// A monotonic mutation counter, so a renderer can skip an untouched line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Generation(pub u64);

/// One logical or displayed line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    cells: Vec<Cell>,
    /// What this line reads as past its stored end.
    ///
    /// Trimming means a line does not store its whole width, so "erased to a
    /// painted background" cannot be represented by the stored cells alone —
    /// one stored cell would leave the rest of the row default-coloured. The
    /// fill is what makes an erase O(1) and still correct at every column.
    fill: Cell,
    wrap: Wrap,
    extras: Option<Box<ExtraAttrs>>,
    tab_runs: Option<Vec<(u16, u16)>>,
    // Named `generation` rather than the design note's `gen`, which edition
    // 2024 reserved for generator blocks.
    generation: Generation,
}

impl Line {
    /// An empty line.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A line holding these cells, trimmed.
    #[must_use]
    pub fn from_cells(cells: Vec<Cell>) -> Self {
        let mut l = Self {
            cells,
            ..Self::default()
        };
        l.trim();
        l
    }

    /// The stored cells. Shorter than the terminal's width, usually much.
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// How this line ended.
    #[must_use]
    pub const fn wrap(&self) -> Wrap {
        self.wrap
    }

    /// Set how it ended.
    pub fn set_wrap(&mut self, w: Wrap) {
        if self.wrap != w {
            self.wrap = w;
            self.touch();
        }
    }

    /// Its mutation counter.
    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    /// Its run-structured attributes, if it has any.
    #[must_use]
    pub fn extras(&self) -> Option<&ExtraAttrs> {
        self.extras.as_deref()
    }

    fn touch(&mut self) {
        self.generation = Generation(self.generation.0.wrapping_add(1));
    }

    /// The cell at a column. Past the stored end this is blank, which is what
    /// makes trimming invisible to every reader.
    #[must_use]
    pub fn cell(&self, col: u16) -> Cell {
        self.cells.get(col as usize).copied().unwrap_or(self.fill)
    }

    /// Write a cell, growing the line if the column is past its stored end.
    pub fn set(&mut self, col: u16, cell: Cell) {
        let col = col as usize;
        if col >= self.cells.len() {
            if cell == self.fill {
                // Writing a blank past the end changes nothing observable, and
                // materializing the gap would defeat trimming entirely.
                return;
            }
            self.cells.resize(col + 1, self.fill);
        }
        self.cells[col] = cell;
        self.touch();
    }

    /// Drop trailing cells that render as nothing.
    ///
    /// The single biggest memory win in the crate: a 200-column terminal
    /// showing a twelve-character line stores twelve cells.
    pub fn trim(&mut self) {
        while self
            .cells
            .last()
            .is_some_and(|c| c.is_blank_with(self.fill) && !c.is_wrap_padding())
        {
            self.cells.pop();
        }
        if self.extras.as_deref().is_some_and(ExtraAttrs::is_empty) {
            self.extras = None;
        }
    }

    /// Blank the line, keeping the background of the template cell.
    pub fn reset(&mut self, template: Cell) {
        self.cells.clear();
        // A screen painted with a background colour must keep it, at every
        // column rather than the one that happens to be stored.
        self.fill = template;
        self.wrap = Wrap::Hard;
        self.extras = None;
        self.tab_runs = None;
        self.touch();
    }

    /// Make everything past the stored end read as this cell.
    ///
    /// What "erase to end of line" actually means on a trimmed line: there is
    /// no run of cells to paint, only a tail that must answer with the erase
    /// background at every column the caller asks about.
    pub fn set_fill(&mut self, cell: Cell) {
        if self.fill != cell {
            self.fill = cell;
            self.touch();
        }
    }

    /// What this line reads as past its stored end.
    #[must_use]
    pub const fn fill(&self) -> Cell {
        self.fill
    }

    /// Record that a tab produced the spaces in `start..end`.
    ///
    /// The whole run, not just where it began: a copy has to know which spaces
    /// the tab stands for, or restoring it would swallow real spaces the user
    /// typed after it. Storing the run rather than a filler character is what
    /// makes `cat` of a tab-indented file round trip.
    pub fn mark_tab_run(&mut self, start: u16, end: u16) {
        if end <= start {
            return;
        }
        let runs = self.tab_runs.get_or_insert_with(Vec::new);
        if let Err(i) = runs.binary_search_by_key(&start, |(s, _)| *s) {
            runs.insert(i, (start, end));
        }
    }

    /// Whether a tab produced the run starting here.
    #[must_use]
    pub fn is_tab_origin(&self, col: u16) -> bool {
        self.tab_runs
            .as_ref()
            .is_some_and(|r| r.binary_search_by_key(&col, |(s, _)| *s).is_ok())
    }

    /// The end of the tab run starting at this column, if one does.
    #[must_use]
    fn tab_run_end(&self, col: u16) -> Option<u16> {
        self.tab_runs
            .as_ref()?
            .iter()
            .find(|(s, _)| *s == col)
            .map(|(_, e)| *e)
    }

    /// Attach a hyperlink to a column range.
    pub fn set_hyperlink(&mut self, range: std::ops::Range<u16>, id: HyperlinkId) {
        if range.is_empty() {
            return;
        }
        self.extras
            .get_or_insert_with(Box::default)
            .hyperlinks
            .push((range, id));
        self.touch();
    }

    /// The line's plain text, styling stripped and the padding dropped.
    ///
    /// This is what a search matches and what a copy produces, so it must read
    /// the way the user sees it: no spacer halves, no wrap padding, and tabs
    /// restored where a tab put them.
    #[must_use]
    pub fn text(&self) -> String {
        use crate::cell::{Flags, Resolved};
        let mut out = String::with_capacity(self.cells.len());
        let mut skip_until = 0usize;
        for (i, c) in self.cells.iter().enumerate() {
            if i < skip_until {
                continue;
            }
            if c.flags.contains(Flags::WIDE_SPACER) {
                // The right half of a wide cluster and the wrap padding are
                // both storage artifacts, not characters the user sees.
                continue;
            }
            let col = u16::try_from(i).unwrap_or(u16::MAX);
            if let Some(end) = self.tab_run_end(col) {
                // The whole run collapses back to the one character that made
                // it, so the text is what the user typed rather than what the
                // terminal drew.
                out.push('\t');
                skip_until = end as usize;
                continue;
            }
            match c.resolve() {
                Resolved::Char('\0') => out.push(' '),
                Resolved::Char(ch) => out.push(ch),
                // A cluster the caller must widen through the grapheme table;
                // a placeholder keeps columns aligned for anyone who does not.
                Resolved::Grapheme(_) => out.push('\u{fffc}'),
            }
        }
        out
    }

    /// How many cells this line occupies, for the memory cap.
    #[must_use]
    pub fn byte_size(&self) -> usize {
        self.cells.len() * size_of::<Cell>()
            + self.extras.as_ref().map_or(0, |e| {
                e.hyperlinks.len() * 12 + e.images.len() * 12 + size_of::<ExtraAttrs>()
            })
            + self.tab_runs.as_ref().map_or(0, |t| t.len() * 4)
    }

    /// Append another line's cells to this one.
    ///
    /// Used by unwrap. The padding cell is dropped rather than carried, which
    /// is the whole reason [`Wrap::SoftWide`] is distinct from [`Wrap::Soft`].
    pub fn append(&mut self, other: &Self) {
        if self.wrap == Wrap::SoftWide {
            while self.cells.last().is_some_and(|c| c.is_wrap_padding()) {
                self.cells.pop();
            }
        }
        let base = u16::try_from(self.cells.len()).unwrap_or(u16::MAX);
        self.cells.extend_from_slice(&other.cells);
        if let Some(extras) = other.extras.as_deref() {
            let mine = self.extras.get_or_insert_with(Box::default);
            for (r, id) in &extras.hyperlinks {
                mine.hyperlinks.push((r.start + base..r.end + base, *id));
            }
            for (r, id) in &extras.images {
                mine.images.push((r.start + base..r.end + base, *id));
            }
        }
        if let Some(runs) = other.tab_runs.as_ref() {
            let mine = self.tab_runs.get_or_insert_with(Vec::new);
            mine.extend(runs.iter().map(|(s, e)| (s + base, e + base)));
            mine.sort_unstable();
        }
        self.wrap = other.wrap;
        self.touch();
    }

    /// Split off everything from a column onward as a new line.
    #[must_use]
    pub fn split_off(&mut self, col: u16) -> Self {
        let at = (col as usize).min(self.cells.len());
        let tail = self.cells.split_off(at);
        let base = col;
        let mut extras = ExtraAttrs::default();
        if let Some(mine) = self.extras.as_deref_mut() {
            mine.hyperlinks.retain(|(r, id)| {
                if r.start >= base {
                    extras
                        .hyperlinks
                        .push((r.start - base..r.end.saturating_sub(base), *id));
                    false
                } else {
                    true
                }
            });
        }
        let mut tail_runs: Option<Vec<(u16, u16)>> = None;
        if let Some(mine) = self.tab_runs.as_mut() {
            let moved: Vec<(u16, u16)> = mine
                .iter()
                .filter(|(s, _)| *s >= base)
                .map(|(s, e)| (s - base, e - base))
                .collect();
            mine.retain(|(s, _)| *s < base);
            if !moved.is_empty() {
                tail_runs = Some(moved);
            }
        }
        self.touch();
        Self {
            cells: tail,
            wrap: self.wrap,
            fill: self.fill,
            extras: (!extras.is_empty()).then(|| Box::new(extras)),
            tab_runs: tail_runs,
            generation: Generation(0),
        }
    }

    /// Pad the line out to a width with whatever it reads as past its end.
    ///
    /// Only for callers that genuinely need a dense row; the grid does not.
    /// The pad cell is the line's own fill rather than one the caller supplies,
    /// so a densified row of an erased line keeps its background.
    #[must_use]
    pub fn densified(&self, cols: u16) -> Vec<Cell> {
        let mut v = self.cells.clone();
        v.resize(cols as usize, self.fill);
        v
    }
}

/// The background to erase with — the current SGR background, not the default.
#[must_use]
pub fn erase_template(bg: Color) -> Cell {
    Cell::new(
        ' ',
        crate::cell::Color::DEFAULT,
        bg,
        crate::cell::Flags::empty(),
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::cell::{Color, Flags};

    fn ch(c: char) -> Cell {
        Cell::new(c, Color::DEFAULT, Color::DEFAULT, Flags::empty())
    }

    fn line(s: &str) -> Line {
        Line::from_cells(s.chars().map(ch).collect())
    }

    #[test]
    fn a_short_line_stores_only_what_it_holds() {
        // The memory claim: a 200-column terminal showing 12 characters must
        // not be storing 200 cells.
        let mut l = line("hello");
        l.set(199, ch('!'));
        assert_eq!(
            l.cells().len(),
            200,
            "a real character past the end grows it"
        );

        let l = line("hello");
        assert_eq!(l.cells().len(), 5);
        assert_eq!(l.cell(150), Cell::BLANK, "reads past the end are blank");
    }

    #[test]
    fn writing_a_blank_past_the_end_does_not_materialize_the_gap() {
        let mut l = line("hi");
        l.set(500, Cell::BLANK);
        assert_eq!(l.cells().len(), 2, "otherwise trimming buys nothing");
    }

    #[test]
    fn trimming_keeps_a_styled_blank() {
        let mut cells: Vec<Cell> = "ab".chars().map(ch).collect();
        cells.push(Cell::new(
            ' ',
            Color::DEFAULT,
            Color::rgb(1, 2, 3),
            Flags::empty(),
        ));
        cells.push(ch(' '));
        let l = Line::from_cells(cells);
        assert_eq!(
            l.cells().len(),
            3,
            "the highlighted trailing space is visible and must survive"
        );
    }

    #[test]
    fn erasing_preserves_the_painted_background() {
        let mut l = line("text");
        l.reset(erase_template(Color::rgb(5, 5, 5)));
        assert_eq!(l.cell(0).bg, Color::rgb(5, 5, 5));
        assert_eq!(
            l.cell(80).bg,
            Color::rgb(5, 5, 5),
            "the whole row, not one cell"
        );
    }

    #[test]
    fn erasing_to_the_default_stores_nothing() {
        let mut l = line("text");
        l.reset(Cell::BLANK);
        assert!(l.cells().is_empty());
    }

    #[test]
    fn a_soft_wrapped_line_continues_and_a_hard_one_does_not() {
        assert!(Wrap::Soft.continues());
        assert!(Wrap::SoftWide.continues());
        assert!(!Wrap::Hard.continues());
    }

    #[test]
    fn appending_drops_the_wide_wrap_padding() {
        // Re-emitting it would grow a space out of nothing every time the
        // window is resized twice.
        let mut a = Line::from_cells(vec![ch('a'), Cell::wrap_padding(Color::DEFAULT)]);
        a.set_wrap(Wrap::SoftWide);
        let b = line("b");
        a.append(&b);
        assert_eq!(a.text(), "ab");
    }

    #[test]
    fn appending_a_plain_soft_wrap_keeps_every_cell() {
        let mut a = line("ab");
        a.set_wrap(Wrap::Soft);
        a.append(&line("cd"));
        assert_eq!(a.text(), "abcd");
        assert_eq!(
            a.wrap(),
            Wrap::Hard,
            "the tail's ending becomes the joined one"
        );
    }

    #[test]
    fn splitting_moves_the_hyperlink_runs_with_their_cells() {
        let mut l = line("abcdef");
        l.set_hyperlink(4..6, HyperlinkId(9));
        let tail = l.split_off(3);
        assert!(
            l.extras().is_none_or(|e| e.hyperlinks.is_empty()),
            "the link left with its text"
        );
        assert_eq!(
            tail.extras().expect("tail extras").hyperlink_at(1),
            Some(HyperlinkId(9)),
            "and is still on the same characters"
        );
    }

    #[test]
    fn split_and_append_round_trip() {
        let mut l = line("hello world");
        let tail = l.split_off(5);
        l.append(&tail);
        assert_eq!(l.text(), "hello world");
    }

    #[test]
    fn a_densified_erased_line_keeps_its_background() {
        let mut l = line("x");
        l.reset(erase_template(Color::rgb(4, 4, 4)));
        let dense = l.densified(3);
        assert!(dense.iter().all(|c| c.bg == Color::rgb(4, 4, 4)));
    }

    #[test]
    fn a_tab_does_not_swallow_the_spaces_after_it() {
        // The run is stored, not just its origin: two spaces the user typed
        // after a tab are text, and collapsing them into the tab would change
        // what a copy produces.
        let mut l = Line::from_cells("    ab".chars().map(ch).collect::<Vec<_>>());
        l.mark_tab_run(0, 4);
        assert_eq!(l.text(), "\tab");

        let mut l = Line::from_cells("      x".chars().map(ch).collect::<Vec<_>>());
        l.mark_tab_run(0, 4);
        assert_eq!(l.text(), "\t  x", "the two typed spaces survive");
    }

    #[test]
    fn a_tab_survives_a_copy() {
        // Storing tabs as spaces plus origins is what makes `cat` of a
        // tab-indented file round trip instead of silently expanding.
        let mut l = Line::from_cells(vec![ch(' '), ch(' '), ch(' '), ch(' '), ch('x')]);
        l.mark_tab_run(0, 4);
        assert_eq!(l.text(), "\tx");
    }

    #[test]
    fn wide_spacers_do_not_appear_in_text() {
        // The right half of a wide cluster is storage, not a character; a
        // search for "ab" must match across it.
        let mut cells = vec![ch('a')];
        cells.push(Cell::new(
            ' ',
            Color::DEFAULT,
            Color::DEFAULT,
            Flags::WIDE_SPACER,
        ));
        cells.push(ch('b'));
        let l = Line::from_cells(cells);
        assert_eq!(l.text(), "ab");
    }

    #[test]
    fn a_generation_moves_only_when_the_line_does() {
        let mut l = line("a");
        let before = l.generation();
        assert_eq!(l.cell(0), ch('a'));
        assert_eq!(l.generation(), before, "reading is not a mutation");
        l.set(1, ch('b'));
        assert!(l.generation() > before);
    }

    #[test]
    fn densifying_pads_without_storing() {
        let l = line("ab");
        assert_eq!(l.densified(5).len(), 5);
        assert_eq!(l.cells().len(), 2, "the line itself is untouched");
    }
}
