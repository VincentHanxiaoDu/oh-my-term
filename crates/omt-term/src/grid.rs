//! The viewport: rows, a cursor, and the margins everything is clipped to.

use crate::cell::{Cell, Color, Flags};
use crate::line::{Line, Wrap, erase_template};

/// How large a terminal is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

impl GridSize {
    /// A size, clamped to something a terminal can actually be.
    ///
    /// A zero-column grid would make every write a division by zero somewhere
    /// downstream, and a resize to zero is a thing window managers really do
    /// send during a drag.
    #[must_use]
    pub const fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: if cols == 0 { 1 } else { cols },
            rows: if rows == 0 { 1 } else { rows },
        }
    }
}

/// Where the cursor is and what it will write with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Column, zero-based.
    pub col: u16,
    /// Row within the grid, zero-based.
    pub row: u16,
    /// Foreground for the next write.
    pub fg: Color,
    /// Background for the next write.
    pub bg: Color,
    /// Style bits for the next write.
    pub flags: Flags,
    /// Whether the cursor is visible (DECTCEM).
    pub visible: bool,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            col: 0,
            row: 0,
            fg: Color::DEFAULT,
            bg: Color::DEFAULT,
            flags: Flags::empty(),
            visible: true,
        }
    }
}

impl Cursor {
    /// The cell this cursor would write.
    #[must_use]
    pub const fn template(&self) -> Cell {
        Cell::new(' ', self.fg, self.bg, self.flags)
    }

    /// What an erase should fill with: the background, but never the other
    /// attributes.
    ///
    /// Erasing with the current *underline* or *inverse* would leave invisible
    /// styling behind that reappears the moment something writes a space there.
    #[must_use]
    pub fn erase_cell(&self) -> Cell {
        erase_template(self.bg)
    }
}

/// How much of a line or screen an erase covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraseExtent {
    /// From the cursor to the end.
    ToEnd,
    /// From the start to the cursor, inclusive.
    ToStart,
    /// All of it.
    All,
}

/// The scrolling region, which is what every vertical operation is clipped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Margins {
    /// First row of the region.
    pub top: u16,
    /// Last row, inclusive.
    pub bottom: u16,
    /// First column (DECSLRM).
    pub left: u16,
    /// Last column, inclusive.
    pub right: u16,
}

impl Margins {
    /// The whole grid.
    #[must_use]
    pub const fn full(size: GridSize) -> Self {
        Self {
            top: 0,
            bottom: size.rows - 1,
            left: 0,
            right: size.cols - 1,
        }
    }

    /// Whether a row is inside the vertical region.
    #[must_use]
    pub const fn contains_row(&self, row: u16) -> bool {
        row >= self.top && row <= self.bottom
    }
}

/// One screen's worth of state.
#[derive(Debug, Clone)]
pub struct Grid {
    rows: Vec<Line>,
    size: GridSize,
    /// Where the cursor is.
    pub cursor: Cursor,
    saved_cursor: Option<Cursor>,
    /// The scrolling region.
    pub margins: Margins,
    /// DECOM: whether row addressing is relative to the region.
    pub origin_mode: bool,
    /// DECAWM: whether writing past the last column wraps.
    pub autowrap: bool,
    /// The cursor is logically past the last column, and the next character
    /// wraps before it is written.
    ///
    /// Explicit because the alternative — moving the cursor off-screen and
    /// fixing it up later — is the classic source of off-by-one corruption
    /// when a program writes exactly `cols` characters and then a control
    /// sequence.
    pending_wrap: bool,
}

impl Grid {
    /// A blank grid.
    #[must_use]
    pub fn new(size: GridSize) -> Self {
        Self {
            rows: vec![Line::new(); size.rows as usize],
            size,
            cursor: Cursor::default(),
            saved_cursor: None,
            margins: Margins::full(size),
            origin_mode: false,
            autowrap: true,
            pending_wrap: false,
        }
    }

    /// Its size.
    #[must_use]
    pub const fn size(&self) -> GridSize {
        self.size
    }

    /// A row, by index from the top.
    #[must_use]
    pub fn row(&self, row: u16) -> &Line {
        &self.rows[(row as usize).min(self.rows.len() - 1)]
    }

    /// Every row, top first.
    #[must_use]
    pub fn rows(&self) -> &[Line] {
        &self.rows
    }

    /// A row, mutably.
    pub fn row_mut(&mut self, row: u16) -> &mut Line {
        let i = (row as usize).min(self.rows.len() - 1);
        &mut self.rows[i]
    }

    /// Whether the next write will wrap first.
    #[must_use]
    pub const fn pending_wrap(&self) -> bool {
        self.pending_wrap
    }

    /// Move the cursor, clipped to the grid — and to the scrolling region when
    /// origin mode is on.
    pub fn goto(&mut self, row: u16, col: u16) {
        let (row, max_row) = if self.origin_mode {
            (self.margins.top.saturating_add(row), self.margins.bottom)
        } else {
            (row, self.size.rows - 1)
        };
        self.cursor.row = row.min(max_row);
        self.cursor.col = col.min(self.size.cols - 1);
        // Any explicit movement resolves the deferred wrap: a program that
        // writes to the last column and then repositions did not want a
        // newline.
        self.pending_wrap = false;
    }

    /// Move relative to where the cursor is, clipped the same way.
    pub fn move_by(&mut self, drow: i32, dcol: i32) {
        let row = i64::from(self.cursor.row) + i64::from(drow);
        let col = i64::from(self.cursor.col) + i64::from(dcol);
        // Vertical movement stops at the region edge rather than scrolling:
        // CUU/CUD are defined not to scroll, and a program relying on that to
        // draw a box would otherwise shred the screen above it.
        let (lo, hi) = if self.margins.contains_row(self.cursor.row) {
            (i64::from(self.margins.top), i64::from(self.margins.bottom))
        } else {
            (0, i64::from(self.size.rows - 1))
        };
        self.cursor.row = row.clamp(lo, hi) as u16;
        self.cursor.col = col.clamp(0, i64::from(self.size.cols - 1)) as u16;
        self.pending_wrap = false;
    }

    /// Save the cursor and its attributes (DECSC).
    pub fn save_cursor(&mut self) {
        self.saved_cursor = Some(self.cursor);
    }

    /// Restore it (DECRC). Restoring without a save homes the cursor, which is
    /// what the hardware did.
    pub fn restore_cursor(&mut self) {
        self.cursor = self.saved_cursor.unwrap_or_default();
        self.pending_wrap = false;
    }

    /// Write one cell at the cursor and advance, wrapping if allowed.
    ///
    /// Returns the row that was completed by a wrap, if any, so the caller can
    /// push it to scrollback.
    pub fn put(&mut self, cell: Cell, width: u16) -> Option<u16> {
        let mut scrolled = None;
        let last = self.size.cols - 1;

        if self.pending_wrap {
            if self.autowrap {
                self.row_mut(self.cursor.row).set_wrap(Wrap::Soft);
                scrolled = self.linefeed_scrolling();
                self.cursor.col = 0;
            }
            self.pending_wrap = false;
        }

        if width == 2 && self.cursor.col == last {
            // A double-width cluster cannot straddle the margin. The padding
            // marks why the line ended, so reflow can drop it rather than
            // turning it into a space.
            if self.autowrap {
                let bg = self.cursor.bg;
                self.row_mut(self.cursor.row)
                    .set(last, Cell::wrap_padding(bg));
                self.row_mut(self.cursor.row).set_wrap(Wrap::SoftWide);
                scrolled = self.linefeed_scrolling().or(scrolled);
                self.cursor.col = 0;
            } else {
                // No wrap: the cluster does not fit and is dropped rather than
                // being drawn as half of itself.
                return scrolled;
            }
        }

        let col = self.cursor.col;
        let mut cell = cell;
        if width == 2 {
            cell.flags.insert(Flags::WIDE);
        }
        self.row_mut(col_row(self.cursor)).set(col, cell);
        if width == 2 {
            let spacer = Cell::new(' ', cell.fg, cell.bg, Flags::WIDE_SPACER);
            self.row_mut(col_row(self.cursor)).set(col + 1, spacer);
        }

        let advance = width.max(1);
        if col + advance > last {
            // Sit *on* the last column with the wrap deferred, rather than
            // moving off the grid.
            self.cursor.col = last;
            self.pending_wrap = true;
        } else {
            self.cursor.col = col + advance;
        }
        scrolled
    }

    /// Move down one row, scrolling the region if that would leave it.
    ///
    /// Returns the row that scrolled off the top of the region, if any.
    pub fn linefeed_scrolling(&mut self) -> Option<u16> {
        if self.cursor.row == self.margins.bottom {
            self.scroll_up(1);
            Some(self.margins.top)
        } else if self.cursor.row + 1 < self.size.rows {
            self.cursor.row += 1;
            None
        } else {
            None
        }
    }

    /// Scroll the region up, blanking the rows that appear at the bottom.
    ///
    /// The rows that leave the top are returned so the caller can decide
    /// whether they belong in scrollback — which is a decision the grid must
    /// not make, since the alternate screen has none.
    pub fn scroll_up(&mut self, n: u16) -> Vec<Line> {
        let top = self.margins.top as usize;
        let bottom = self.margins.bottom as usize;
        let n = (n as usize).min(bottom - top + 1);
        if n == 0 {
            return Vec::new();
        }
        let evicted: Vec<Line> = self.rows.splice(top..top + n, Vec::new()).collect();
        let template = self.cursor.erase_cell();
        for i in 0..n {
            let mut blank = Line::new();
            blank.reset(template);
            self.rows.insert(bottom + 1 - n + i, blank);
        }
        evicted
    }

    /// Scroll the region down, blanking the rows that appear at the top.
    pub fn scroll_down(&mut self, n: u16) {
        let top = self.margins.top as usize;
        let bottom = self.margins.bottom as usize;
        let n = (n as usize).min(bottom - top + 1);
        if n == 0 {
            return;
        }
        self.rows.drain(bottom + 1 - n..=bottom);
        let template = self.cursor.erase_cell();
        for _ in 0..n {
            let mut blank = Line::new();
            blank.reset(template);
            self.rows.insert(top, blank);
        }
    }

    /// Erase part of the cursor's line.
    pub fn erase_line(&mut self, extent: EraseExtent) {
        let template = self.cursor.erase_cell();
        let col = self.cursor.col;
        let cols = self.size.cols;
        let row = self.row_mut(col_row(self.cursor));
        match extent {
            EraseExtent::All => row.reset(template),
            EraseExtent::ToEnd => {
                let _ = row.split_off(col);
                // The tail is gone, so everything from here on reads as the
                // fill — which is exactly the background the erase asked for,
                // at every column, without storing a cell per column.
                row.set_fill(template);
            }
            EraseExtent::ToStart => {
                for c in 0..=col.min(cols - 1) {
                    row.set(c, template);
                }
            }
        }
        self.pending_wrap = false;
    }

    /// Erase part of the screen.
    pub fn erase_screen(&mut self, extent: EraseExtent) -> Vec<Line> {
        let template = self.cursor.erase_cell();
        let row = self.cursor.row;
        let rows = self.size.rows;
        match extent {
            EraseExtent::All => {
                // The scrolled-off rows are handed back rather than dropped:
                // `clear` should push the screen into scrollback, not destroy
                // it, and only the caller knows whether this screen has any.
                let old = std::mem::replace(&mut self.rows, vec![Line::new(); rows as usize]);
                for r in &mut self.rows {
                    r.reset(template);
                }
                self.pending_wrap = false;
                return old;
            }
            EraseExtent::ToEnd => {
                self.erase_line(EraseExtent::ToEnd);
                for r in row + 1..rows {
                    self.row_mut(r).reset(template);
                }
            }
            EraseExtent::ToStart => {
                for r in 0..row {
                    self.row_mut(r).reset(template);
                }
                self.erase_line(EraseExtent::ToStart);
            }
        }
        self.pending_wrap = false;
        Vec::new()
    }

    /// Insert blank cells at the cursor, pushing the rest of the line right.
    pub fn insert_cells(&mut self, n: u16) {
        let template = self.cursor.erase_cell();
        let col = self.cursor.col;
        let cols = self.size.cols;
        let row = self.row_mut(col_row(self.cursor));
        let tail = row.split_off(col);
        for i in 0..n.min(cols - col) {
            row.set(col + i, template);
        }
        let mut shifted = Line::new();
        shifted.append(&tail);
        for (i, c) in shifted.cells().iter().enumerate() {
            let Ok(i) = u16::try_from(i) else { break };
            let dest = col + n + i;
            if dest >= cols {
                // Cells pushed past the margin fall off, rather than growing
                // the line past the terminal's width.
                break;
            }
            row.set(dest, *c);
        }
    }

    /// Delete cells at the cursor, pulling the rest of the line left.
    pub fn delete_cells(&mut self, n: u16) {
        let col = self.cursor.col;
        let cols = self.size.cols;
        let template = self.cursor.erase_cell();
        let row = self.row_mut(col_row(self.cursor));
        let cells: Vec<Cell> = row.cells().to_vec();
        let n = n as usize;
        let col_i = col as usize;
        for i in col_i..cols as usize {
            let src = i + n;
            let c = cells.get(src).copied().unwrap_or(template);
            row.set(u16::try_from(i).unwrap_or(u16::MAX), c);
        }
        row.trim();
    }

    /// Resize the viewport, returning rows that fell off the top.
    ///
    /// Rows leave from the top rather than the bottom when shrinking, because
    /// the cursor is usually near the bottom and taking rows from under it
    /// would make the shell prompt jump.
    pub fn resize(&mut self, size: GridSize) -> Vec<Line> {
        let mut evicted = Vec::new();
        let old_rows = self.rows.len();
        let new_rows = size.rows as usize;
        if new_rows < old_rows {
            let cursor_row = self.cursor.row as usize;
            let drop = old_rows - new_rows;
            // Only take rows the cursor is not standing on or below.
            let from_top = drop.min(cursor_row);
            evicted.extend(self.rows.drain(..from_top));
            let still = drop - from_top;
            self.rows.truncate(self.rows.len() - still);
            self.cursor.row = (self.cursor.row as usize - from_top) as u16;
        } else {
            self.rows.resize(new_rows, Line::new());
        }
        self.size = size;
        self.margins = Margins::full(size);
        self.cursor.row = self.cursor.row.min(size.rows - 1);
        self.cursor.col = self.cursor.col.min(size.cols - 1);
        self.pending_wrap = false;
        evicted
    }

    /// Reset everything a full reset (RIS) resets.
    pub fn hard_reset(&mut self) {
        let size = self.size;
        *self = Self::new(size);
    }
}

const fn col_row(c: Cursor) -> u16 {
    c.row
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn grid(cols: u16, rows: u16) -> Grid {
        Grid::new(GridSize::new(cols, rows))
    }

    fn write(g: &mut Grid, s: &str) {
        for c in s.chars() {
            let w = u16::from(unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u8);
            g.put(Cell::new(c, g.cursor.fg, g.cursor.bg, g.cursor.flags), w);
        }
    }

    fn text(g: &Grid, row: u16) -> String {
        g.row(row).text()
    }

    #[test]
    fn a_zero_sized_grid_is_impossible() {
        // Window managers really do send a zero during a drag, and every
        // downstream width calculation divides by this.
        let g = grid(0, 0);
        assert_eq!(g.size(), GridSize { cols: 1, rows: 1 });
    }

    #[test]
    fn writing_exactly_the_width_does_not_wrap_yet() {
        // The classic off-by-one: after `cols` characters the cursor sits on
        // the last column with the wrap deferred. A program that now sends a
        // control sequence must not have produced a newline.
        let mut g = grid(4, 3);
        write(&mut g, "abcd");
        assert_eq!(g.cursor.row, 0, "still on the first row");
        assert_eq!(g.cursor.col, 3);
        assert!(g.pending_wrap());
        assert_eq!(text(&g, 0), "abcd");
        assert_eq!(text(&g, 1), "");
    }

    #[test]
    fn the_deferred_wrap_resolves_on_the_next_character() {
        let mut g = grid(4, 3);
        write(&mut g, "abcde");
        assert_eq!(text(&g, 0), "abcd");
        assert_eq!(text(&g, 1), "e");
        assert_eq!(g.cursor.row, 1);
        assert_eq!(
            g.row(0).wrap(),
            Wrap::Soft,
            "so reflow can rejoin them at another width"
        );
    }

    #[test]
    fn repositioning_cancels_the_deferred_wrap() {
        // A program that fills the last column and then moves did not ask for
        // a newline; producing one here is what corrupts full-screen apps.
        let mut g = grid(4, 3);
        write(&mut g, "abcd");
        g.goto(0, 0);
        assert!(!g.pending_wrap());
        write(&mut g, "X");
        assert_eq!(text(&g, 0), "Xbcd");
        assert_eq!(text(&g, 1), "", "nothing spilled onto the next row");
    }

    #[test]
    fn a_wide_cluster_will_not_straddle_the_margin() {
        let mut g = grid(4, 3);
        write(&mut g, "abc漢");
        assert_eq!(
            g.row(0).wrap(),
            Wrap::SoftWide,
            "the reason it wrapped is recorded, so reflow drops the padding"
        );
        assert_eq!(text(&g, 0), "abc");
        assert_eq!(text(&g, 1), "漢");
    }

    #[test]
    fn a_wide_cluster_occupies_two_cells() {
        let mut g = grid(6, 2);
        write(&mut g, "漢x");
        assert!(g.row(0).cell(0).flags.contains(Flags::WIDE));
        assert!(g.row(0).cell(1).flags.contains(Flags::WIDE_SPACER));
        assert_eq!(g.row(0).cell(2).resolve(), crate::cell::Resolved::Char('x'));
        assert_eq!(text(&g, 0), "漢x", "but reads as what the user sees");
    }

    #[test]
    fn without_autowrap_the_last_column_is_overwritten() {
        let mut g = grid(4, 3);
        g.autowrap = false;
        write(&mut g, "abcdef");
        assert_eq!(text(&g, 0), "abcf");
        assert_eq!(text(&g, 1), "");
    }

    #[test]
    fn scrolling_returns_what_left_the_top() {
        // The grid must not decide where those rows go: the alternate screen
        // has no scrollback, and that is the caller's knowledge.
        let mut g = grid(8, 3);
        write(&mut g, "one");
        g.goto(1, 0);
        write(&mut g, "two");
        g.goto(2, 0);
        write(&mut g, "three");
        let off = g.scroll_up(1);
        assert_eq!(off.len(), 1);
        assert_eq!(off[0].text(), "one");
        assert_eq!(text(&g, 0), "two");
        assert_eq!(text(&g, 2), "", "a blank row appeared at the bottom");
    }

    #[test]
    fn scrolling_is_clipped_to_the_region() {
        let mut g = grid(8, 4);
        for r in 0..4 {
            g.goto(r, 0);
            write(&mut g, &format!("r{r}"));
        }
        g.margins.top = 1;
        g.margins.bottom = 2;
        let off = g.scroll_up(1);
        assert_eq!(off[0].text(), "r1");
        assert_eq!(text(&g, 0), "r0", "outside the region, untouched");
        assert_eq!(text(&g, 1), "r2");
        assert_eq!(text(&g, 2), "");
        assert_eq!(text(&g, 3), "r3", "and below it, untouched");
    }

    #[test]
    fn scrolling_down_blanks_the_top_of_the_region() {
        let mut g = grid(8, 3);
        for r in 0..3 {
            g.goto(r, 0);
            write(&mut g, &format!("r{r}"));
        }
        g.scroll_down(1);
        assert_eq!(text(&g, 0), "");
        assert_eq!(text(&g, 1), "r0");
        assert_eq!(text(&g, 2), "r1");
    }

    #[test]
    fn cursor_movement_does_not_scroll() {
        // CUU/CUD are defined not to scroll; a program drawing a box relies on
        // it, and scrolling here shreds everything above.
        let mut g = grid(8, 3);
        write(&mut g, "top");
        g.move_by(-5, 0);
        assert_eq!(g.cursor.row, 0);
        assert_eq!(text(&g, 0), "top", "nothing moved");
        g.move_by(99, 0);
        assert_eq!(g.cursor.row, 2);
    }

    #[test]
    fn origin_mode_addresses_relative_to_the_region() {
        let mut g = grid(8, 5);
        g.margins.top = 2;
        g.margins.bottom = 4;
        g.origin_mode = true;
        g.goto(0, 0);
        assert_eq!(g.cursor.row, 2, "row 0 is the top of the region");
        g.goto(9, 0);
        assert_eq!(g.cursor.row, 4, "and it cannot escape the bottom");
    }

    #[test]
    fn erasing_to_the_end_of_a_line_covers_every_column() {
        // Trimming means the line stops storing cells; the erased background
        // still has to be there at column 70.
        let mut g = grid(80, 2);
        write(&mut g, "hello");
        g.cursor.bg = Color::rgb(3, 3, 3);
        g.goto(0, 2);
        g.erase_line(EraseExtent::ToEnd);
        assert_eq!(text(&g, 0), "he");
        assert_eq!(g.row(0).cell(70).bg, Color::rgb(3, 3, 3));
    }

    #[test]
    fn erasing_to_the_start_keeps_the_tail() {
        let mut g = grid(10, 2);
        write(&mut g, "abcdef");
        g.goto(0, 2);
        g.erase_line(EraseExtent::ToStart);
        assert_eq!(text(&g, 0), "   def");
    }

    #[test]
    fn erasing_the_screen_hands_back_the_old_rows() {
        // `clear` should move the screen into scrollback, not destroy it.
        let mut g = grid(8, 3);
        write(&mut g, "keep me");
        let old = g.erase_screen(EraseExtent::All);
        assert_eq!(old.len(), 3);
        assert_eq!(old[0].text(), "keep me");
        assert_eq!(text(&g, 0), "");
    }

    #[test]
    fn erasing_with_an_attribute_leaves_only_the_background() {
        // Erasing with the current underline would leave invisible styling
        // that reappears the moment a space is written there.
        let mut g = grid(8, 2);
        g.cursor.flags = Flags::empty().with_underline(crate::cell::Underline::Single);
        g.cursor.bg = Color::rgb(1, 1, 1);
        g.erase_line(EraseExtent::All);
        let c = g.row(0).cell(3);
        assert_eq!(c.bg, Color::rgb(1, 1, 1));
        assert_eq!(c.flags.underline(), crate::cell::Underline::None);
    }

    #[test]
    fn inserting_cells_pushes_the_rest_right_and_off() {
        let mut g = grid(6, 2);
        write(&mut g, "abcdef");
        g.goto(0, 2);
        g.insert_cells(2);
        assert_eq!(text(&g, 0), "ab  cd", "ef fell off the margin");
    }

    #[test]
    fn deleting_cells_pulls_the_rest_left() {
        let mut g = grid(6, 2);
        write(&mut g, "abcdef");
        g.goto(0, 2);
        g.delete_cells(2);
        assert_eq!(text(&g, 0), "abef");
    }

    #[test]
    fn saving_and_restoring_carries_the_attributes() {
        let mut g = grid(8, 3);
        g.goto(1, 2);
        g.cursor.fg = Color::rgb(7, 7, 7);
        g.save_cursor();
        g.goto(0, 0);
        g.cursor.fg = Color::DEFAULT;
        g.restore_cursor();
        assert_eq!((g.cursor.row, g.cursor.col), (1, 2));
        assert_eq!(g.cursor.fg, Color::rgb(7, 7, 7));
    }

    #[test]
    fn restoring_without_a_save_homes_the_cursor() {
        let mut g = grid(8, 3);
        g.goto(2, 4);
        g.restore_cursor();
        assert_eq!((g.cursor.row, g.cursor.col), (0, 0));
    }

    #[test]
    fn shrinking_takes_rows_from_above_the_cursor() {
        // Taking them from under the cursor makes the shell prompt jump, which
        // is what every terminal that gets this wrong looks like.
        let mut g = grid(8, 4);
        for r in 0..4 {
            g.goto(r, 0);
            write(&mut g, &format!("r{r}"));
        }
        g.goto(3, 0);
        let off = g.resize(GridSize::new(8, 2));
        assert_eq!(off.len(), 2);
        assert_eq!(off[0].text(), "r0");
        assert_eq!(text(&g, 1), "r3");
        assert_eq!(g.cursor.row, 1, "the cursor stayed on its own line");
    }

    #[test]
    fn shrinking_below_the_cursor_takes_from_the_bottom() {
        let mut g = grid(8, 4);
        g.goto(0, 0);
        write(&mut g, "top");
        let off = g.resize(GridSize::new(8, 2));
        assert!(off.is_empty(), "nothing above the cursor to take");
        assert_eq!(text(&g, 0), "top");
    }

    #[test]
    fn growing_adds_blank_rows_at_the_bottom() {
        let mut g = grid(8, 2);
        write(&mut g, "one");
        g.resize(GridSize::new(8, 4));
        assert_eq!(g.rows().len(), 4);
        assert_eq!(text(&g, 0), "one");
    }

    #[test]
    fn resizing_resets_the_margins() {
        // A scrolling region left over from a dead full-screen app would clip
        // every subsequent scroll to a region the user cannot see.
        let mut g = grid(8, 6);
        g.margins.top = 2;
        g.margins.bottom = 3;
        g.resize(GridSize::new(8, 4));
        assert_eq!(g.margins, Margins::full(GridSize::new(8, 4)));
    }

    #[test]
    fn a_hard_reset_restores_every_mode() {
        let mut g = grid(8, 4);
        g.origin_mode = true;
        g.autowrap = false;
        g.margins.top = 1;
        g.cursor.fg = Color::rgb(1, 2, 3);
        write(&mut g, "junk");
        g.hard_reset();
        assert!(!g.origin_mode);
        assert!(g.autowrap);
        assert_eq!(g.margins, Margins::full(GridSize::new(8, 4)));
        assert_eq!(g.cursor.fg, Color::DEFAULT);
        assert_eq!(text(&g, 0), "");
    }
}
