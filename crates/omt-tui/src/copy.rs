//! Reading what has scrolled away, and taking a copy of it.
//!
//! A multiplexer that cannot scroll back is one people keep a second terminal
//! beside. The keys are already in the keymap and the help already advertises
//! them; this is the part that was missing underneath.
//!
//! Coordinates here are **absolute row numbers** — counted from the first line
//! the session ever wrote, not from the top of the screen. Screen-relative
//! positions move under you as output arrives: a selection anchored at "row 4"
//! is anchored to a different line a second later, and the text somebody
//! copies is not the text they highlighted.

/// Where the reader is looking, and what they have marked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Copy {
    /// The absolute row at the top of the viewport.
    pub top: u64,
    /// The absolute row the cursor is on.
    pub row: u64,
    /// The column the cursor is on.
    pub col: u16,
    /// Where a selection was started, if one was.
    pub anchor: Option<(u64, u16)>,
    /// What is being searched for, while somebody is typing it.
    ///
    /// `Some("")` is a search that has been opened and not yet typed into,
    /// which is a different state from no search at all: the keys mean
    /// different things in each.
    pub typing: Option<String>,
    /// The last thing searched for, so `n` has something to repeat.
    pub needle: String,
}

/// A direction to move in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// One line up.
    Up,
    /// One line down.
    Down,
    /// One column left.
    Left,
    /// One column right.
    Right,
    /// A screen up.
    PageUp,
    /// A screen down.
    PageDown,
    /// The first line ever written.
    Top,
    /// The last line written.
    Bottom,
    /// The start of the line.
    LineStart,
    /// The end of the line.
    LineEnd,
}

impl Copy {
    /// Enter, looking at the bottom of the history — where the session is.
    ///
    /// Not at the top: somebody who presses this key is almost always looking
    /// for something that just happened, and starting at the beginning of a
    /// long history means scrolling past everything they have already read.
    #[must_use]
    pub fn entering(total_rows: u64, rows: u16) -> Self {
        let bottom = total_rows.saturating_sub(1);
        Self {
            top: bottom.saturating_sub(u64::from(rows).saturating_sub(1)),
            row: bottom,
            col: 0,
            anchor: None,
            typing: None,
            needle: String::new(),
        }
    }

    /// Open the search field.
    pub fn search(&mut self) {
        self.typing = Some(String::new());
    }

    /// Take what was typed, if anything, and remember it for `n`.
    ///
    /// An empty query is dropped rather than searched for: it matches every
    /// position on every line, which is an answer nobody wants and a jump to
    /// wherever the cursor already was.
    pub fn submit(&mut self) -> Option<String> {
        let typed = self.typing.take()?;
        if typed.is_empty() {
            return None;
        }
        self.needle = typed.clone();
        Some(typed)
    }

    /// Move to the next row matching `needle`, in a direction.
    ///
    /// Rows rather than positions within a row: the cursor lands at the start
    /// of the match, which is where somebody reading wants to be, and a search
    /// that stopped at every occurrence on one long line would need three
    /// presses to leave it.
    ///
    /// Returns whether anything was found, so a caller can say "no matches"
    /// rather than leaving the cursor sitting still with no explanation.
    pub fn jump(
        &mut self,
        forwards: bool,
        total_rows: u64,
        rows: u16,
        read: impl Fn(u64) -> Option<String>,
    ) -> bool {
        if self.needle.is_empty() {
            return false;
        }
        let needle = self.needle.to_lowercase();
        let hit = if forwards {
            (self.row + 1..total_rows).find(|row| matches(*row, &needle, &read))
        } else {
            (0..self.row)
                .rev()
                .find(|row| matches(*row, &needle, &read))
        };
        let Some(row) = hit else {
            return false;
        };
        self.row = row;
        self.col = read(row)
            .and_then(|text| {
                let lower = text.to_lowercase();
                lower
                    .find(&needle)
                    .map(|byte| u16::try_from(lower[..byte].chars().count()).unwrap_or(0))
            })
            .unwrap_or(0);
        self.follow(rows, total_rows);
        true
    }

    /// Move the cursor, keeping it on screen.
    ///
    /// The viewport follows the cursor rather than the other way round, so
    /// there is one thing to think about and it is the one the keys move.
    pub fn go(&mut self, towards: Move, total_rows: u64, rows: u16, cols: u16) {
        let last = total_rows.saturating_sub(1);
        let page = u64::from(rows).max(1);
        match towards {
            Move::Up => self.row = self.row.saturating_sub(1),
            Move::Down => self.row = (self.row + 1).min(last),
            Move::Left => self.col = self.col.saturating_sub(1),
            Move::Right => self.col = (self.col + 1).min(cols.saturating_sub(1)),
            Move::PageUp => self.row = self.row.saturating_sub(page),
            Move::PageDown => self.row = (self.row + page).min(last),
            Move::Top => self.row = 0,
            Move::Bottom => self.row = last,
            Move::LineStart => self.col = 0,
            Move::LineEnd => self.col = cols.saturating_sub(1),
        }
        self.follow(rows, total_rows);
    }

    /// Keep the viewport around the cursor.
    fn follow(&mut self, rows: u16, total_rows: u64) {
        let height = u64::from(rows).max(1);
        if self.row < self.top {
            self.top = self.row;
        } else if self.row >= self.top + height {
            self.top = self.row + 1 - height;
        }
        // Never past the end: a viewport below the last line is a screen of
        // blank rows with the content above it, which reads as lost output.
        let furthest = total_rows.saturating_sub(height);
        self.top = self.top.min(furthest);
    }

    /// Start or clear a selection at the cursor.
    pub fn mark(&mut self) {
        self.anchor = if self.anchor.is_some() {
            None
        } else {
            Some((self.row, self.col))
        };
    }

    /// The selection, ordered, as (row, column) pairs.
    ///
    /// Ordered here rather than by every caller, because a selection dragged
    /// upwards has its anchor after its cursor and every consumer would
    /// otherwise have to know that.
    #[must_use]
    pub fn span(&self) -> Option<((u64, u16), (u64, u16))> {
        let anchor = self.anchor?;
        let here = (self.row, self.col);
        Some(if anchor <= here {
            (anchor, here)
        } else {
            (here, anchor)
        })
    }

    /// Whether a cell is inside the selection.
    #[must_use]
    pub fn selected(&self, row: u64, col: u16) -> bool {
        let Some((start, end)) = self.span() else {
            return false;
        };
        (row, col) >= start && (row, col) <= end
    }
}

/// Whether one row contains the needle, which is already lower-cased.
///
/// Case-insensitive, because somebody searching their own scrollback is
/// looking for a word and not for a spelling of it.
fn matches(row: u64, needle: &str, read: &impl Fn(u64) -> Option<String>) -> bool {
    read(row).is_some_and(|text| text.to_lowercase().contains(needle))
}

/// The selected text, given a way to read a line.
///
/// Takes a reader rather than the lines themselves: the history can be a very
/// long way back, and copying every line of it to select three of them is a
/// cost paid on every keystroke that moves the cursor.
///
/// Trailing blanks are trimmed per line. A terminal pads every row to its
/// width, so an untrimmed copy carries hundreds of spaces that were never
/// typed — and they reappear when it is pasted somewhere that keeps them.
#[must_use]
pub fn selected_text(copy: &Copy, read: impl Fn(u64) -> Option<String>) -> String {
    let Some(((from_row, from_col), (to_row, to_col))) = copy.span() else {
        return String::new();
    };
    let mut pieces = Vec::new();
    for row in from_row..=to_row {
        // A row the history no longer holds is dropped, not rendered as a
        // blank line: scrollback is bounded, a selection can outlive the lines
        // it covers, and an invented blank puts something in the copy that was
        // never on the screen.
        let Some(text) = read(row) else {
            continue;
        };
        let chars: Vec<char> = text.chars().collect();
        let start = if row == from_row {
            usize::from(from_col)
        } else {
            0
        };
        let end = if row == to_row {
            usize::from(to_col).saturating_add(1).min(chars.len())
        } else {
            chars.len()
        };
        let piece: String = if start < end {
            chars[start..end].iter().collect()
        } else {
            String::new()
        };
        pieces.push(piece.trim_end().to_owned());
    }
    pieces.join("\n")
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn lines() -> Vec<String> {
        (0..10)
            .map(|i| format!("line {i} of the output    "))
            .collect()
    }

    fn read(all: &[String]) -> impl Fn(u64) -> Option<String> + '_ {
        move |row| all.get(usize::try_from(row).ok()?).cloned()
    }

    #[test]
    fn entering_looks_at_the_end_where_the_session_is() {
        // Somebody pressing this key is looking for something that just
        // happened. Starting at the beginning of a long history means
        // scrolling past everything they have already read.
        let copy = Copy::entering(100, 10);
        assert_eq!(copy.row, 99);
        assert_eq!(copy.top, 90);
    }

    #[test]
    fn the_viewport_follows_the_cursor_in_both_directions() {
        let mut copy = Copy::entering(100, 10);
        copy.go(Move::Top, 100, 10, 80);
        assert_eq!(copy.row, 0);
        assert_eq!(copy.top, 0, "the cursor left the screen going up");
        copy.go(Move::Bottom, 100, 10, 80);
        assert_eq!(copy.top, 90, "the cursor left the screen going down");
    }

    #[test]
    fn the_viewport_never_shows_blank_rows_past_the_end() {
        // A screen of blanks below the last line reads as output that was
        // lost, which is the one thing a scrollback must never look like.
        let mut copy = Copy::entering(12, 10);
        copy.go(Move::PageDown, 12, 10, 80);
        assert!(copy.top <= 2, "top was {}", copy.top);
    }

    #[test]
    fn a_selection_dragged_upwards_reads_the_same_as_one_dragged_down() {
        // The anchor is wherever the mark was pressed, which is as often below
        // the cursor as above it.
        let all = lines();
        let mut down = Copy {
            top: 0,
            row: 1,
            col: 0,
            anchor: Some((3, 5)),
            ..Copy::default()
        };
        let mut up = Copy {
            top: 0,
            row: 3,
            col: 5,
            anchor: Some((1, 0)),
            ..Copy::default()
        };
        assert_eq!(
            selected_text(&down, read(&all)),
            selected_text(&up, read(&all))
        );
        down.mark();
        up.mark();
        assert!(down.anchor.is_none() && up.anchor.is_none());
    }

    #[test]
    fn nothing_marked_copies_nothing() {
        // Not "the line under the cursor": a yank with no selection that
        // silently took something is a clipboard somebody has lost the
        // contents of.
        let all = lines();
        let copy = Copy::entering(10, 5);
        assert_eq!(selected_text(&copy, read(&all)), "");
    }

    #[test]
    fn the_padding_a_terminal_adds_is_not_copied() {
        // Every row is padded to the width of the screen. Untrimmed, a copied
        // line carries hundreds of spaces nobody typed.
        let all = lines();
        let copy = Copy {
            top: 0,
            row: 0,
            col: 200,
            anchor: Some((0, 0)),
            ..Copy::default()
        };
        assert_eq!(selected_text(&copy, read(&all)), "line 0 of the output");
    }

    #[test]
    fn a_selection_across_lines_keeps_the_line_breaks() {
        let all = lines();
        let copy = Copy {
            top: 0,
            row: 2,
            col: 5,
            anchor: Some((1, 0)),
            ..Copy::default()
        };
        let text = selected_text(&copy, read(&all));
        assert_eq!(text.lines().count(), 2, "{text:?}");
        assert!(text.starts_with("line 1"), "{text:?}");
        assert!(text.ends_with("line 2"), "{text:?}");
    }

    #[test]
    fn a_row_the_history_no_longer_holds_is_skipped_rather_than_guessed() {
        // Scrollback is bounded, and a selection can outlive the lines it
        // covers. Inventing a blank line would put content in a copy that was
        // never on the screen.
        let copy = Copy {
            top: 0,
            row: 4,
            col: 3,
            anchor: Some((0, 0)),
            ..Copy::default()
        };
        let text = selected_text(&copy, |row| (row % 2 == 0).then(|| format!("kept {row}")));
        assert!(!text.contains("kept 1"), "{text:?}");
        // Row 4 is cut at the cursor's column, which is where the selection
        // ends — that is the selection, not a loss.
        assert!(text.starts_with("kept 0"), "{text:?}");
        assert_eq!(text.lines().count(), 3, "{text:?}");
    }
    #[test]
    fn a_search_finds_the_next_line_that_has_it() {
        let all = lines();
        let mut copy = Copy::entering(10, 4);
        copy.row = 0;
        copy.needle = "line 7".to_owned();
        assert!(copy.jump(true, 10, 4, read(&all)));
        assert_eq!(copy.row, 7);
        assert_eq!(copy.col, 0);
    }

    #[test]
    fn a_search_looks_for_a_word_and_not_for_a_spelling_of_it() {
        // Somebody searching their own scrollback is looking for the thing,
        // not for how they capitalised it an hour ago.
        let all = vec!["Error: it broke".to_owned(), "fine".to_owned()];
        let mut copy = Copy::entering(2, 2);
        copy.row = 1;
        copy.needle = "error".to_owned();
        assert!(copy.jump(false, 2, 2, read(&all)));
        assert_eq!(copy.row, 0);
    }

    #[test]
    fn a_search_that_finds_nothing_says_so_and_does_not_move() {
        // Moving somewhere arbitrary is worse than not moving: the reader
        // loses their place and has no way to tell that is what happened.
        let all = lines();
        let mut copy = Copy::entering(10, 4);
        copy.row = 3;
        copy.needle = "not in here".to_owned();
        assert!(!copy.jump(true, 10, 4, read(&all)));
        assert_eq!(copy.row, 3);
    }

    #[test]
    fn an_empty_search_is_dropped_rather_than_run() {
        // It matches every position on every line, which is a jump to wherever
        // the cursor already was and a needle that ruins the next `n`.
        let mut copy = Copy::entering(10, 4);
        copy.needle = "kept".to_owned();
        copy.search();
        assert_eq!(copy.submit(), None);
        assert_eq!(copy.needle, "kept", "the previous search was thrown away");
    }

    #[test]
    fn the_last_search_is_kept_so_it_can_be_repeated() {
        let mut copy = Copy::entering(10, 4);
        copy.search();
        copy.typing = Some("marker".to_owned());
        assert_eq!(copy.submit().as_deref(), Some("marker"));
        assert!(copy.typing.is_none(), "the field stayed open");
        assert_eq!(copy.needle, "marker");
    }
}
