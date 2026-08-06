//! Dividing a terminal between panes.
//!
//! Geometry only, and deliberately: where a pane goes is a pure function of the
//! screen size and how many panes there are, so it can be tested without a
//! terminal, a pty or a running session.
//!
//! The rule that shapes everything here is that **a pane too small to use is
//! worse than a pane that is not shown**. A terminal split six ways on an
//! 80-column screen gives every pane thirteen columns, which no program can
//! draw in — so past a point this stops splitting and says so.

/// Where one pane sits, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Leftmost column, counting from zero.
    pub col: u16,
    /// Top row.
    pub row: u16,
    /// Width.
    pub cols: u16,
    /// Height.
    pub rows: u16,
}

/// How panes are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    /// Side by side, dividing the columns.
    Vertical,
    /// Stacked, dividing the rows.
    Horizontal,
}

/// The narrowest pane worth drawing.
///
/// Below this a shell prompt wraps on itself and any program with a table is
/// unreadable. Chosen as the width at which `git log --oneline` still fits.
pub const MIN_PANE_COLS: u16 = 40;

/// The shortest.
///
/// Three lines of output and a prompt. Less is a pane you cannot follow.
pub const MIN_PANE_ROWS: u16 = 4;

/// One cell of separator between panes.
const SEPARATOR: u16 = 1;

/// How many panes actually fit.
///
/// Returning a smaller number than asked for is the honest answer: the
/// alternative is drawing eight unusable panes and leaving the user to work out
/// why nothing is legible.
#[must_use]
pub fn how_many_fit(cols: u16, rows: u16, split: Split, wanted: usize) -> usize {
    if wanted <= 1 {
        return wanted;
    }
    let (available, minimum) = match split {
        Split::Vertical => (cols, MIN_PANE_COLS),
        Split::Horizontal => (rows, MIN_PANE_ROWS),
    };
    // Each pane after the first also costs a separator column or row.
    let mut fits = 1usize;
    while fits < wanted {
        let needed = (fits as u16 + 1) * minimum + fits as u16 * SEPARATOR;
        if needed > available {
            break;
        }
        fits += 1;
    }
    fits
}

/// Lay out `count` panes in a screen.
///
/// The remainder goes to the first panes rather than being dropped: three panes
/// in eighty columns is 26, 26, 26 with two columns spare, and leaving them
/// unused puts a ragged gap at the edge of the screen.
#[must_use]
pub fn tile(cols: u16, rows: u16, split: Split, count: usize) -> Vec<Rect> {
    if count == 0 || cols == 0 || rows == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![Rect {
            col: 0,
            row: 0,
            cols,
            rows,
        }];
    }

    let n = count as u16;
    let separators = n - 1;
    match split {
        Split::Vertical => {
            let usable = cols.saturating_sub(separators);
            let base = usable / n;
            let extra = usable % n;
            let mut out = Vec::with_capacity(count);
            let mut x = 0u16;
            for i in 0..n {
                let width = base + u16::from(i < extra);
                out.push(Rect {
                    col: x,
                    row: 0,
                    cols: width,
                    rows,
                });
                x += width + SEPARATOR;
            }
            out
        }
        Split::Horizontal => {
            let usable = rows.saturating_sub(separators);
            let base = usable / n;
            let extra = usable % n;
            let mut out = Vec::with_capacity(count);
            let mut y = 0u16;
            for i in 0..n {
                let height = base + u16::from(i < extra);
                out.push(Rect {
                    col: 0,
                    row: y,
                    cols,
                    rows: height,
                });
                y += height + SEPARATOR;
            }
            out
        }
    }
}

/// Which pane a click or tap landed in.
#[must_use]
pub fn pane_at(rects: &[Rect], col: u16, row: u16) -> Option<usize> {
    rects.iter().position(|r| {
        col >= r.col && col < r.col + r.cols && row >= r.row && row < r.row + r.rows
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    #[test]
    fn one_pane_gets_the_whole_screen() {
        let rects = tile(80, 24, Split::Vertical, 1);
        assert_eq!(
            rects[0],
            Rect {
                col: 0,
                row: 0,
                cols: 80,
                rows: 24
            }
        );
    }

    #[test]
    fn two_panes_side_by_side_leave_a_separator_between_them() {
        let rects = tile(81, 24, Split::Vertical, 2);
        assert_eq!(rects[0].cols + rects[1].cols, 80, "{rects:?}");
        assert_eq!(
            rects[1].col,
            rects[0].cols + 1,
            "the second pane overlaps the separator"
        );
    }

    #[test]
    fn the_remainder_is_distributed_rather_than_dropped() {
        // Leaving spare columns unused puts a ragged gap at the edge of the
        // screen that looks like a rendering bug.
        let rects = tile(82, 24, Split::Vertical, 3);
        let used: u16 = rects.iter().map(|r| r.cols).sum::<u16>() + 2;
        assert_eq!(used, 82, "{rects:?}");
    }

    #[test]
    fn panes_never_overlap() {
        // The failure this prevents is one pane painting over another's last
        // column, which reads as corruption rather than as a layout bug.
        for count in 2..6usize {
            let rects = tile(200, 60, Split::Vertical, count);
            for pair in rects.windows(2) {
                assert!(
                    pair[0].col + pair[0].cols < pair[1].col,
                    "{count} panes overlap: {pair:?}"
                );
            }
        }
    }

    #[test]
    fn a_horizontal_split_divides_the_rows_instead() {
        let rects = tile(80, 25, Split::Horizontal, 2);
        assert_eq!(rects[0].cols, 80);
        assert_eq!(rects[0].rows + rects[1].rows, 24);
        assert_eq!(rects[1].row, rects[0].rows + 1);
    }

    #[test]
    fn a_screen_too_narrow_admits_fewer_panes_than_asked_for() {
        // Eight unusable panes is worse than three usable ones, and the user
        // cannot tell why nothing is legible.
        assert_eq!(how_many_fit(80, 24, Split::Vertical, 8), 1);
        assert_eq!(how_many_fit(81, 24, Split::Vertical, 8), 2);
    }

    #[test]
    fn a_phone_width_screen_admits_exactly_one_pane() {
        // Which is the whole argument for the mobile client's design: there is
        // no split worth showing at fifty columns.
        assert_eq!(how_many_fit(50, 40, Split::Vertical, 4), 1);
    }

    #[test]
    fn asking_for_one_pane_always_fits_however_small_the_screen() {
        // Refusing to draw anything on a tiny terminal would leave a blank
        // screen where a cramped session is at least usable.
        assert_eq!(how_many_fit(10, 3, Split::Vertical, 1), 1);
    }

    #[test]
    fn a_zero_sized_screen_produces_no_panes_rather_than_a_panic() {
        // A terminal reports zero during a drag, and dividing by it is how a
        // resize crashes the whole TUI.
        assert!(tile(0, 0, Split::Vertical, 3).is_empty());
    }

    #[test]
    fn a_click_lands_in_the_pane_it_was_over() {
        let rects = tile(81, 24, Split::Vertical, 2);
        assert_eq!(pane_at(&rects, 0, 0), Some(0));
        assert_eq!(pane_at(&rects, 80, 23), Some(1));
    }

    #[test]
    fn a_click_on_the_separator_belongs_to_nobody() {
        // Better than guessing: a click that silently focuses the wrong pane
        // sends the next keystroke somewhere unexpected.
        let rects = tile(81, 24, Split::Vertical, 2);
        assert_eq!(pane_at(&rects, rects[0].cols, 0), None);
    }
}
