//! Scrollback: logical lines, wrapped on demand.
//!
//! The core decision this file exists to enforce:
//!
//! > Scrollback stores **logical** (unwrapped) lines. Wrapping to a display
//! > width is computed on demand and memoized, never stored.
//!
//! Everything else follows. Reflow becomes a pure function of the stored lines
//! and a width rather than a bespoke join-and-re-split over rows, and every
//! durable coordinate can be a [`Position`] that no resize can invalidate.

use std::collections::VecDeque;

use crate::cell::{Cell, Flags};
use crate::line::{Line, Wrap};

/// A width-independent location in a session's content.
///
/// Stable across reflow by construction, and invalidated only by eviction.
/// Everything durable is one of these — selections, search hits, block
/// boundaries, marks, agent attributions. Nothing durable is a row and column,
/// because a row and column mean something different after every resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// Absolute logical line index, counting lines already evicted.
    pub line: u64,
    /// Offset in cells from the start of that logical line.
    pub offset: u32,
    /// "The end of this line", wherever that lands.
    ///
    /// A selection dragged to end-of-line must stay at end-of-line at every
    /// width; an offset alone would freeze it at whatever column the line
    /// happened to end at when the user let go.
    pub to_eol: bool,
}

impl Position {
    /// A position at an offset.
    #[must_use]
    pub const fn new(line: u64, offset: u32) -> Self {
        Self {
            line,
            offset,
            to_eol: false,
        }
    }

    /// A position meaning the end of a line.
    #[must_use]
    pub const fn end_of(line: u64) -> Self {
        Self {
            line,
            offset: 0,
            to_eol: true,
        }
    }
}

/// A point on the current screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point {
    /// Wrapped-row index from the top of scrollback.
    pub row: u64,
    /// Column.
    pub col: u16,
}

/// What resolving a [`Position`] at the current width produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// It is on screen or in resident scrollback.
    Resolved(Point),
    /// Its line has been evicted; the content is gone.
    Evicted,
    /// It refers to a line that has not been written yet.
    NotYetVisible,
}

/// How much scrollback may cost.
///
/// Three independent caps, because each catches a case the others miss. The
/// byte cap is the one that matters: ten thousand lines of plain log is about
/// two megabytes, and the same ten thousand lines of emoji and hyperlinks is
/// ten times that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbackLimits {
    /// Most logical lines kept, to within one chunk.
    pub max_lines: u32,
    /// Most cell bytes kept, to within one chunk.
    pub max_bytes: usize,
    /// Most image payload kept for this session.
    pub max_image_bytes_total: usize,
}

impl Default for ScrollbackLimits {
    fn default() -> Self {
        Self {
            max_lines: 10_000,
            max_bytes: 64 << 20,
            max_image_bytes_total: 64 << 20,
        }
    }
}

/// How many logical lines live in one chunk.
///
/// Chunking gives cheap head eviction — drop one at the front — and keeps the
/// prefix-sum index short enough that finding a row is a binary search over
/// chunks and then a walk within one.
///
/// The chunk is also the unit of eviction, so the caps in
/// [`ScrollbackLimits`] are honoured to within one chunk rather than exactly.
/// At the default 64 MiB that overshoot is noise; at a cap smaller than a
/// chunk it is the whole budget, which is why the limits are documented as a
/// target and not a guarantee.
const CHUNK_LINES: usize = 256;

#[derive(Debug, Clone)]
struct LineChunk {
    lines: Vec<Line>,
    /// The wrapped-row count, and the width it was computed for.
    ///
    /// Recomputed lazily when the width no longer matches, so a drag-resize
    /// does not re-wrap a hundred thousand lines per frame.
    wrapped_at: Option<(u16, u64)>,
    /// Sticky: once a chunk has seen a double-width cluster it uses the
    /// counting path forever. Most sessions never trip it, and for those the
    /// wrapped count is arithmetic with no scan at all.
    may_have_wide: bool,
    bytes: usize,
}

impl LineChunk {
    fn new() -> Self {
        Self {
            lines: Vec::with_capacity(CHUNK_LINES),
            wrapped_at: None,
            may_have_wide: false,
            bytes: 0,
        }
    }

    fn push(&mut self, line: Line) {
        self.bytes += line.byte_size();
        self.may_have_wide |= line
            .cells()
            .iter()
            .any(|c| c.flags.intersects(Flags::WIDE | Flags::COMPLEX));
        self.lines.push(line);
        self.wrapped_at = None;
    }

    fn wrapped_rows(&mut self, width: u16) -> u64 {
        if let Some((w, n)) = self.wrapped_at
            && w == width
        {
            return n;
        }
        let n = if self.may_have_wide {
            self.lines.iter().map(|l| wrapped_rows(l, width)).sum()
        } else {
            // No wide clusters means every cell is one column, so the row count
            // is arithmetic rather than a scan.
            self.lines
                .iter()
                .map(|l| {
                    let len = l.cells().len() as u64;
                    len.div_ceil(u64::from(width)).max(1)
                })
                .sum()
        };
        self.wrapped_at = Some((width, n));
        n
    }
}

/// How many rows a logical line occupies at a width.
#[must_use]
pub fn wrapped_rows(line: &Line, width: u16) -> u64 {
    if width == 0 {
        return 1;
    }
    let mut rows = 1u64;
    let mut col = 0u16;
    for c in line.cells() {
        if c.flags.contains(Flags::WIDE_SPACER) {
            continue;
        }
        let w = if c.flags.contains(Flags::WIDE) { 2 } else { 1 };
        if col + w > width {
            rows += 1;
            col = 0;
        }
        col += w;
    }
    rows
}

/// Split a logical line into display rows at a width.
///
/// The inverse of [`unwrap_lines`], and the only place a `Wrap` value is
/// produced from content: a row that ended because a wide cluster would not
/// fit is marked [`Wrap::SoftWide`] and carries the padding, so unwrapping can
/// tell that padding from a space the user typed.
#[must_use]
pub fn wrap_line(line: &Line, width: u16) -> Vec<Line> {
    if width == 0 {
        return vec![line.clone()];
    }
    let mut out = Vec::new();
    let mut current: Vec<Cell> = Vec::new();
    let mut col = 0u16;

    for c in line.cells() {
        if c.flags.contains(Flags::WIDE_SPACER) && !c.is_wrap_padding() {
            // The right half travels with its left half, which was just
            // pushed; it never starts a row on its own.
            current.push(*c);
            continue;
        }
        if c.is_wrap_padding() {
            // Padding from a previous wrapping is not content. Dropping it
            // here is what makes re-wrapping idempotent.
            continue;
        }
        let w = if c.flags.contains(Flags::WIDE) { 2 } else { 1 };
        if col + w > width {
            // Untrimmed: this row ended at the margin, so a trailing space is
            // a space inside the text, not padding.
            let mut row = Line::from_cells_untrimmed(std::mem::take(&mut current));
            if w == 2 && col < width {
                // A wide cluster that could not fit in the last column: record
                // why, and pad, so the reason survives into storage.
                row.set(col, Cell::wrap_padding(line.fill().bg));
                row.set_wrap(Wrap::SoftWide);
            } else {
                row.set_wrap(Wrap::Soft);
            }
            out.push(row);
            col = 0;
        }
        current.push(*c);
        col += w;
    }

    let mut last = Line::from_cells(current);
    last.set_wrap(line.wrap());
    last.set_fill(line.fill());
    out.push(last);
    out
}

/// Join display rows back into logical lines.
///
/// A row whose wrap continues is appended to the one before it. This is what
/// makes reflow a pure function: unwrap, change the width, wrap again.
#[must_use]
pub fn unwrap_lines(rows: &[Line]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for row in rows {
        match out.last_mut() {
            Some(prev) if prev.wrap().continues() => prev.append(row),
            _ => out.push(row.clone()),
        }
    }
    out
}

/// The scrollback itself.
#[derive(Debug, Clone)]
pub struct Scrollback {
    chunks: VecDeque<LineChunk>,
    /// How many logical lines have been evicted, ever.
    ///
    /// Never reset. Every externally held coordinate is absolute, so evicting
    /// does not invalidate a mark or a selection — it makes it resolve to
    /// [`Resolution::Evicted`], which a caller can say something about.
    overflow: u64,
    limits: ScrollbackLimits,
    bytes: usize,
}

impl Scrollback {
    /// An empty scrollback.
    #[must_use]
    pub fn new(limits: ScrollbackLimits) -> Self {
        Self {
            chunks: VecDeque::new(),
            overflow: 0,
            limits,
            bytes: 0,
        }
    }

    /// How many logical lines are resident.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.iter().map(|c| c.lines.len()).sum()
    }

    /// Whether anything is resident.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.iter().all(|c| c.lines.is_empty())
    }

    /// How many logical lines have been evicted.
    #[must_use]
    pub const fn overflow(&self) -> u64 {
        self.overflow
    }

    /// The absolute index the next appended line will get.
    #[must_use]
    pub fn next_index(&self) -> u64 {
        self.overflow + self.len() as u64
    }

    /// Resident bytes.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// Append a logical line, evicting from the front if that broke a cap.
    pub fn push(&mut self, line: Line) {
        let size = line.byte_size();
        if self
            .chunks
            .back()
            .is_none_or(|c| c.lines.len() >= CHUNK_LINES)
        {
            self.chunks.push_back(LineChunk::new());
        }
        if let Some(chunk) = self.chunks.back_mut() {
            chunk.push(line);
        }
        self.bytes += size;
        self.enforce_limits();
    }

    /// Append display rows, joining the ones that were soft-wrapped.
    ///
    /// The entry point the grid uses: rows leaving the top of the screen are
    /// content, and content is stored logically.
    ///
    /// The join continues into what is already stored, not just within this
    /// batch. Rows scroll off one at a time, so a batch is usually a single
    /// row, and joining only within it would file every row of a wrapped line
    /// separately — which is exactly the geometry this whole module exists to
    /// avoid storing.
    pub fn push_rows(&mut self, rows: &[Line]) {
        for line in unwrap_lines(rows) {
            if self.last_continues() {
                self.append_to_last(&line);
            } else {
                self.push(line);
            }
        }
    }

    /// Whether the newest stored line was left mid-wrap.
    fn last_continues(&self) -> bool {
        self.chunks
            .back()
            .and_then(|c| c.lines.last())
            .is_some_and(|l| l.wrap().continues())
    }

    fn append_to_last(&mut self, line: &Line) {
        let Some(chunk) = self.chunks.back_mut() else {
            return;
        };
        let Some(last) = chunk.lines.last_mut() else {
            return;
        };
        let before = last.byte_size();
        last.append(line);
        let after = last.byte_size();
        let delta = after.saturating_sub(before);
        chunk.bytes += delta;
        chunk.wrapped_at = None;
        chunk.may_have_wide |= line
            .cells()
            .iter()
            .any(|c| c.flags.intersects(Flags::WIDE | Flags::COMPLEX));
        self.bytes += delta;
        self.enforce_limits();
    }

    fn enforce_limits(&mut self) {
        // Whole chunks, from the front. Evicting individual lines would make
        // the prefix-sum index useless and buy nothing: a chunk is the unit of
        // memoization as well as of storage.
        while self.len() > self.limits.max_lines as usize || self.bytes > self.limits.max_bytes {
            if self.chunks.len() <= 1 {
                // Never evict the only chunk. A single enormous line can break
                // any cap on its own, and dropping it would make the terminal
                // appear to erase itself rather than to have scrolled.
                break;
            }
            let Some(front) = self.chunks.pop_front() else {
                break;
            };
            self.overflow += front.lines.len() as u64;
            self.bytes = self.bytes.saturating_sub(front.bytes);
        }
    }

    /// A logical line by absolute index.
    #[must_use]
    pub fn line(&self, abs: u64) -> Option<&Line> {
        let rel = abs.checked_sub(self.overflow)?;
        let mut rel = usize::try_from(rel).ok()?;
        for chunk in &self.chunks {
            if rel < chunk.lines.len() {
                return chunk.lines.get(rel);
            }
            rel -= chunk.lines.len();
        }
        None
    }

    /// Every resident logical line, oldest first.
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.chunks.iter().flat_map(|c| c.lines.iter())
    }

    /// How many display rows the whole scrollback occupies at a width.
    pub fn wrapped_rows(&mut self, width: u16) -> u64 {
        self.chunks.iter_mut().map(|c| c.wrapped_rows(width)).sum()
    }

    /// Where a position lands at a width.
    pub fn resolve(&mut self, pos: Position, width: u16) -> Resolution {
        if pos.line < self.overflow {
            return Resolution::Evicted;
        }
        if pos.line >= self.next_index() {
            return Resolution::NotYetVisible;
        }
        // Rows before the target line, chunk by chunk, so a long scrollback is
        // a binary-search-shaped walk rather than a per-line scan.
        let mut rel = (pos.line - self.overflow) as usize;
        let mut row = 0u64;
        let mut target: Option<&Line> = None;
        for chunk in &mut self.chunks {
            if rel >= chunk.lines.len() {
                rel -= chunk.lines.len();
                row += chunk.wrapped_rows(width);
                continue;
            }
            for (i, line) in chunk.lines.iter().enumerate() {
                if i == rel {
                    target = Some(line);
                    break;
                }
                row += wrapped_rows(line, width);
            }
            break;
        }
        let Some(line) = target else {
            return Resolution::NotYetVisible;
        };
        let offset = if pos.to_eol {
            u32::try_from(line.cells().len()).unwrap_or(u32::MAX)
        } else {
            pos.offset
        };
        let (extra_rows, col) = offset_to_row_col(line, offset, width);
        Resolution::Resolved(Point {
            row: row + extra_rows,
            col,
        })
    }

    /// Take the newest logical lines back out, for pulling rows into a resized
    /// grid.
    #[must_use]
    pub fn take_last(&mut self, n: usize) -> Vec<Line> {
        let mut out = Vec::new();
        while out.len() < n {
            let Some(chunk) = self.chunks.back_mut() else {
                break;
            };
            match chunk.lines.pop() {
                Some(line) => {
                    self.bytes = self.bytes.saturating_sub(line.byte_size());
                    chunk.wrapped_at = None;
                    out.push(line);
                }
                None => {
                    if self.chunks.len() == 1 {
                        break;
                    }
                    self.chunks.pop_back();
                }
            }
        }
        out.reverse();
        out
    }
}

/// Which wrapped row of a line an offset falls on, and its column there.
fn offset_to_row_col(line: &Line, offset: u32, width: u16) -> (u64, u16) {
    if width == 0 {
        return (0, 0);
    }
    let mut row = 0u64;
    let mut col = 0u16;
    for (i, c) in line.cells().iter().enumerate() {
        if i as u32 >= offset {
            break;
        }
        if c.flags.contains(Flags::WIDE_SPACER) {
            continue;
        }
        let w = if c.flags.contains(Flags::WIDE) { 2 } else { 1 };
        if col + w > width {
            row += 1;
            col = 0;
        }
        col += w;
    }
    if col >= width {
        // An offset exactly at the margin belongs at the start of the next row,
        // which is where the next character would actually be drawn.
        row += 1;
        col = 0;
    }
    (row, col)
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
        let mut cell = Cell::new(c, Color::DEFAULT, Color::DEFAULT, Flags::empty());
        if unicode_width::UnicodeWidthChar::width(c) == Some(2) {
            cell.flags.insert(Flags::WIDE);
        }
        cell
    }

    fn logical(s: &str) -> Line {
        let mut cells = Vec::new();
        for c in s.chars() {
            cells.push(ch(c));
            if unicode_width::UnicodeWidthChar::width(c) == Some(2) {
                cells.push(Cell::new(
                    ' ',
                    Color::DEFAULT,
                    Color::DEFAULT,
                    Flags::WIDE_SPACER,
                ));
            }
        }
        Line::from_cells(cells)
    }

    fn texts(rows: &[Line]) -> Vec<String> {
        rows.iter().map(Line::text).collect()
    }

    #[test]
    fn wrapping_splits_at_the_width() {
        let rows = wrap_line(&logical("abcdefgh"), 3);
        assert_eq!(texts(&rows), ["abc", "def", "gh"]);
        assert_eq!(rows[0].wrap(), Wrap::Soft);
        assert_eq!(
            rows[2].wrap(),
            Wrap::Hard,
            "the last row ends as the line did"
        );
    }

    #[test]
    fn a_line_shorter_than_the_width_is_one_row() {
        let rows = wrap_line(&logical("hi"), 80);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text(), "hi");
    }

    #[test]
    fn an_empty_line_is_still_one_row() {
        // Otherwise a blank line between paragraphs would vanish on resize.
        assert_eq!(wrap_line(&Line::new(), 80).len(), 1);
        assert_eq!(wrapped_rows(&Line::new(), 80), 1);
    }

    #[test]
    fn a_wide_cluster_never_straddles_the_margin() {
        let rows = wrap_line(&logical("a漢b"), 2);
        assert_eq!(texts(&rows), ["a", "漢", "b"]);
        assert_eq!(
            rows[0].wrap(),
            Wrap::SoftWide,
            "the reason is recorded so unwrapping drops the padding"
        );
    }

    #[test]
    fn unwrapping_undoes_wrapping() {
        for width in [1u16, 2, 3, 7, 40] {
            let original = logical("the quick brown fox");
            let rows = wrap_line(&original, width);
            let back = unwrap_lines(&rows);
            assert_eq!(back.len(), 1, "width {width}");
            assert_eq!(back[0].text(), original.text(), "width {width}");
        }
    }

    #[test]
    fn unwrapping_undoes_a_wide_wrap_without_growing_spaces() {
        // The padding must be dropped, not turned into a space; otherwise
        // every resize adds one.
        let original = logical("a漢b漢c");
        for width in [2u16, 3, 4, 5] {
            let back = unwrap_lines(&wrap_line(&original, width));
            assert_eq!(back[0].text(), "a漢b漢c", "width {width}");
        }
    }

    #[test]
    fn rewrapping_is_idempotent_across_a_round_trip() {
        // The property that makes drag-resize safe: going out to a width and
        // back must produce exactly what was there.
        let original = logical("some text that is long enough to wrap several times over");
        for (a, b) in [(80u16, 20u16), (20, 80), (13, 7), (5, 200)] {
            let once = unwrap_lines(&wrap_line(&original, a));
            let twice = unwrap_lines(&wrap_line(&once[0], b));
            let back = unwrap_lines(&wrap_line(&twice[0], a));
            assert_eq!(back[0].text(), original.text(), "{a} -> {b} -> {a}");
        }
    }

    #[test]
    fn hard_and_soft_endings_stay_distinct_through_a_round_trip() {
        // Losing this distinction is what joins paragraphs that were separate.
        let mut a = logical("first");
        a.set_wrap(Wrap::Hard);
        let b = logical("second");
        let rows: Vec<Line> = wrap_line(&a, 3)
            .into_iter()
            .chain(wrap_line(&b, 3))
            .collect();
        let back = unwrap_lines(&rows);
        assert_eq!(back.len(), 2, "two paragraphs, not one");
        assert_eq!(back[0].text(), "first");
        assert_eq!(back[1].text(), "second");
    }

    #[test]
    fn the_row_count_matches_what_wrapping_produces() {
        for text in ["", "a", "abcdefgh", "a漢b漢c", "漢漢漢漢"] {
            for width in [1u16, 2, 3, 5, 80] {
                let l = logical(text);
                assert_eq!(
                    wrapped_rows(&l, width) as usize,
                    wrap_line(&l, width).len(),
                    "{text:?} at width {width}"
                );
            }
        }
    }

    #[test]
    fn positions_survive_a_width_change() {
        // The whole reason durable coordinates are positions: the same content
        // must be found at any width.
        let mut sb = Scrollback::new(ScrollbackLimits::default());
        sb.push(logical("0123456789"));
        let pos = Position::new(0, 7);
        let wide = sb.resolve(pos, 10);
        let narrow = sb.resolve(pos, 4);
        assert_eq!(wide, Resolution::Resolved(Point { row: 0, col: 7 }));
        assert_eq!(
            narrow,
            Resolution::Resolved(Point { row: 1, col: 3 }),
            "same character, different geometry"
        );
    }

    #[test]
    fn end_of_line_stays_at_end_of_line() {
        let mut sb = Scrollback::new(ScrollbackLimits::default());
        sb.push(logical("abcdef"));
        let Resolution::Resolved(p) = sb.resolve(Position::end_of(0), 3) else {
            panic!("must resolve");
        };
        assert_eq!(p, Point { row: 2, col: 0 }, "past the last character");
        let Resolution::Resolved(p) = sb.resolve(Position::end_of(0), 100) else {
            panic!("must resolve");
        };
        assert_eq!(p, Point { row: 0, col: 6 });
    }

    #[test]
    fn an_evicted_position_says_so_rather_than_resolving_wrongly() {
        // Silently resolving to some other line is how a selection ends up
        // highlighting text the user never chose.
        let mut sb = Scrollback::new(ScrollbackLimits {
            max_lines: 300,
            ..ScrollbackLimits::default()
        });
        for i in 0..600 {
            sb.push(logical(&format!("line {i}")));
        }
        assert!(sb.overflow() > 0, "something was evicted");
        assert_eq!(sb.resolve(Position::new(0, 0), 80), Resolution::Evicted);
        let live = sb.next_index() - 1;
        assert!(matches!(
            sb.resolve(Position::new(live, 0), 80),
            Resolution::Resolved(_)
        ));
    }

    #[test]
    fn a_position_beyond_the_end_is_not_yet_visible() {
        let mut sb = Scrollback::new(ScrollbackLimits::default());
        sb.push(logical("only line"));
        assert_eq!(
            sb.resolve(Position::new(50, 0), 80),
            Resolution::NotYetVisible,
            "distinct from evicted: this content may still arrive"
        );
    }

    #[test]
    fn absolute_indices_do_not_shift_when_lines_are_evicted() {
        // If they did, every mark and block boundary would silently slide.
        let mut sb = Scrollback::new(ScrollbackLimits {
            max_lines: 300,
            ..ScrollbackLimits::default()
        });
        for i in 0..600u64 {
            sb.push(logical(&format!("l{i}")));
        }
        let last = sb.next_index() - 1;
        assert_eq!(last, 599, "the newest line is still index 599");
        assert_eq!(sb.line(599).expect("resident").text(), "l599");
    }

    #[test]
    fn the_byte_cap_evicts_even_when_the_line_cap_would_not() {
        // Ten thousand lines of emoji is ten times ten thousand lines of log,
        // and only a byte cap catches that.
        let mut sb = Scrollback::new(ScrollbackLimits {
            max_lines: 100_000,
            max_bytes: 32 * 1024,
            ..ScrollbackLimits::default()
        });
        for _ in 0..2000 {
            sb.push(logical(&"x".repeat(100)));
        }
        assert!(sb.overflow() > 0, "the byte cap did the evicting");
        // Eviction is chunk-granular, so what is guaranteed is that the
        // residue is bounded by one chunk — not that it is under the cap. With
        // a 32 KiB cap and 1.6 KiB lines the cap is smaller than a chunk, so
        // this is the tightest true statement.
        let one_chunk = CHUNK_LINES * 100 * size_of::<crate::cell::Cell>();
        assert!(sb.bytes() <= one_chunk, "bytes: {}", sb.bytes());
        assert!(
            sb.len() <= CHUNK_LINES,
            "and never more than a chunk's worth of lines"
        );
    }

    #[test]
    fn eviction_never_empties_the_scrollback_entirely() {
        // A single enormous line must not make the terminal appear to erase
        // itself.
        let mut sb = Scrollback::new(ScrollbackLimits {
            max_lines: 10,
            max_bytes: 16,
            ..ScrollbackLimits::default()
        });
        sb.push(logical(&"y".repeat(10_000)));
        assert_eq!(sb.len(), 1, "the line that broke the cap is still there");
    }

    #[test]
    fn pushing_rows_stores_them_as_one_logical_line() {
        // This is what makes reflow pure: what is stored is content, not
        // geometry.
        let mut sb = Scrollback::new(ScrollbackLimits::default());
        let rows = wrap_line(&logical("a long line that wrapped"), 6);
        assert!(rows.len() > 1);
        sb.push_rows(&rows);
        assert_eq!(sb.len(), 1, "one logical line, not {}", rows.len());
        assert_eq!(sb.line(0).expect("line").text(), "a long line that wrapped");
    }

    #[test]
    fn a_row_pushed_alone_continues_the_line_it_belongs_to() {
        // Rows scroll off one at a time, so joining only within a batch would
        // file every row of a wrapped line separately.
        let mut sb = Scrollback::new(ScrollbackLimits::default());
        let rows = wrap_line(&logical("a long line that wrapped"), 6);
        for row in &rows {
            sb.push_rows(std::slice::from_ref(row));
        }
        assert_eq!(sb.len(), 1, "one logical line");
        assert_eq!(sb.line(0).expect("line").text(), "a long line that wrapped");
    }

    #[test]
    fn pushing_hard_ended_rows_keeps_them_separate() {
        let mut sb = Scrollback::new(ScrollbackLimits::default());
        sb.push_rows(&[logical("one"), logical("two")]);
        assert_eq!(sb.len(), 2);
    }

    #[test]
    fn the_wrapped_count_is_memoized_and_still_correct_after_a_width_change() {
        let mut sb = Scrollback::new(ScrollbackLimits::default());
        for _ in 0..10 {
            sb.push(logical("0123456789"));
        }
        assert_eq!(sb.wrapped_rows(10), 10);
        assert_eq!(sb.wrapped_rows(10), 10, "the memo agrees with itself");
        assert_eq!(sb.wrapped_rows(5), 20, "and is invalidated by a new width");
        assert_eq!(sb.wrapped_rows(10), 10, "and back again");
    }

    #[test]
    fn the_wide_fast_path_and_the_slow_path_agree() {
        // The sticky flag picks between arithmetic and a scan; if they
        // disagreed, scrollback geometry would depend on whether a chunk had
        // ever seen an emoji.
        let mut plain = Scrollback::new(ScrollbackLimits::default());
        let mut wide = Scrollback::new(ScrollbackLimits::default());
        for _ in 0..5 {
            plain.push(logical("abcdefgh"));
            wide.push(logical("abcdefgh"));
        }
        wide.push(logical("漢"));
        plain.push(logical("ab"));
        assert_eq!(plain.wrapped_rows(4), wide.wrapped_rows(4));
    }

    #[test]
    fn taking_lines_back_out_returns_the_newest_in_order() {
        let mut sb = Scrollback::new(ScrollbackLimits::default());
        for i in 0..5 {
            sb.push(logical(&format!("l{i}")));
        }
        let taken = sb.take_last(2);
        assert_eq!(
            texts(&taken),
            ["l3", "l4"],
            "oldest first, as rows are drawn"
        );
        assert_eq!(sb.len(), 3);
    }

    #[test]
    fn chunking_is_invisible_from_outside() {
        // Crossing a chunk boundary must not change any answer.
        let mut sb = Scrollback::new(ScrollbackLimits::default());
        for i in 0..(CHUNK_LINES * 2 + 5) {
            sb.push(logical(&format!("line{i}")));
        }
        assert_eq!(sb.len(), CHUNK_LINES * 2 + 5);
        assert_eq!(sb.line(0).expect("first").text(), "line0");
        assert_eq!(
            sb.line(CHUNK_LINES as u64).expect("boundary").text(),
            format!("line{CHUNK_LINES}")
        );
        let last = sb.next_index() - 1;
        assert!(matches!(
            sb.resolve(Position::new(last, 0), 80),
            Resolution::Resolved(_)
        ));
    }
}
