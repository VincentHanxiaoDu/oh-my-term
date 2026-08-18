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
    /// Whether the last search ran backwards.
    ///
    /// Kept so `n` repeats the search that was actually made. A repeat key
    /// that always went forwards would make a backward search a one-shot.
    pub backwards: bool,
    /// Whether the selection takes whole lines.
    ///
    /// A flag rather than a second kind of selection: `span` already returns
    /// an ordered pair, and widening that pair when the text is taken leaves
    /// every consumer that understands a span working unchanged.
    pub linewise: bool,
}

/// Which way a word motion goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// To the start of the next word.
    Next,
    /// To the start of the previous one.
    Back,
    /// To the end of this one.
    End,
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
    /// The first character that is not a space.
    FirstNonBlank,
    /// Half a screen up, which keeps the reader's place in view in a way a
    /// whole page does not.
    HalfPageUp,
    /// Half a screen down.
    HalfPageDown,
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
            backwards: false,
            linewise: false,
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
        mut read: impl FnMut(u64) -> Option<String>,
    ) -> bool {
        if self.needle.is_empty() {
            return false;
        }
        // Smart case: a query typed in lower case matches either, and a
        // capital in it means that capital. Somebody looking for `error` wants
        // every spelling of it; somebody who typed `ERROR` typed the capitals
        // on purpose, and matching anything else walks them through every line
        // they did not mean.
        let exact = self.needle.chars().any(char::is_uppercase);
        let needle = if exact {
            self.needle.clone()
        } else {
            self.needle.to_lowercase()
        };
        // `FnMut` all the way down: reading a line can be a call to another
        // process, and a signature that promised otherwise would push the lie
        // outward until somebody wrapped a socket in a lock to satisfy it.
        let hit = if forwards {
            (self.row + 1..total_rows).find(|row| matches(*row, &needle, exact, &mut read))
        } else {
            (0..self.row)
                .rev()
                .find(|row| matches(*row, &needle, exact, &mut read))
        };
        let Some(row) = hit else {
            return false;
        };
        self.row = row;
        self.col = read(row)
            .and_then(|text| {
                let hay = if exact { text } else { text.to_lowercase() };
                hay.find(&needle)
                    .map(|byte| u16::try_from(hay[..byte].chars().count()).unwrap_or(0))
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
            Move::HalfPageUp => self.row = self.row.saturating_sub(page.div_ceil(2)),
            Move::HalfPageDown => self.row = (self.row + page.div_ceil(2)).min(last),
            // Needs the line's text, which `go` does not have. Handled by
            // `first_non_blank`, and here so the enum stays one vocabulary.
            Move::FirstNonBlank => {}
        }
        self.follow(rows, total_rows);
    }

    /// Move by word.
    ///
    /// Takes a reader for the same reason everything else here does: reading a
    /// line can be a call to another process, and a signature that promised
    /// otherwise would push the lie outward until somebody wrapped a socket in
    /// a lock to satisfy it.
    ///
    /// A word motion **stops at the end of its line** rather than wrapping. In
    /// a scrollback the next line is usually unrelated output rather than a
    /// continuation, so wrapping moves the reader somewhere they were not
    /// looking.
    pub fn word(&mut self, towards: Word, read: impl FnMut(u64) -> Option<String>) {
        self.word_between(towards, "", read);
    }

    /// The same, with the characters that end a word.
    ///
    /// `terminal.word_separators` is in the settings reference and in the
    /// importer that maps other terminals' configurations onto omt's, and it
    /// reached nothing: a word motion always stopped at whitespace, so
    /// `/usr/local/bin` was one word however anybody had configured it.
    /// Empty keeps the old rule, which is what a caller with no configuration
    /// wants.
    pub fn word_between(
        &mut self,
        towards: Word,
        separators: &str,
        mut read: impl FnMut(u64) -> Option<String>,
    ) {
        let Some(text) = read(self.row) else { return };
        let chars: Vec<char> = text.chars().collect();
        let at = usize::from(self.col);
        let is_word = |c: char| !c.is_whitespace() && !separators.contains(c);
        let landed = match towards {
            Word::Next => {
                let mut i = at;
                // Off the current word, then over the space between.
                while i < chars.len() && is_word(chars[i]) {
                    i += 1;
                }
                while i < chars.len() && !is_word(chars[i]) {
                    i += 1;
                }
                i
            }
            Word::Back => {
                let mut i = at;
                while i > 0 && !is_word(chars[i - 1]) {
                    i -= 1;
                }
                while i > 0 && is_word(chars[i - 1]) {
                    i -= 1;
                }
                i
            }
            Word::End => {
                let mut i = at.saturating_add(1);
                while i < chars.len() && !is_word(chars[i]) {
                    i += 1;
                }
                while i + 1 < chars.len() && is_word(chars[i + 1]) {
                    i += 1;
                }
                i.min(chars.len().saturating_sub(1))
            }
        };
        self.col = u16::try_from(landed.min(chars.len().saturating_sub(1))).unwrap_or(self.col);
    }

    /// To the first character on the line that is not a space.
    pub fn first_non_blank(&mut self, mut read: impl FnMut(u64) -> Option<String>) {
        let Some(text) = read(self.row) else { return };
        let at = text.chars().position(|c| !c.is_whitespace()).unwrap_or(0);
        self.col = u16::try_from(at).unwrap_or(0);
    }

    /// To the previous or next prompt.
    ///
    /// Takes the rows rather than the blocks: copy mode has no business
    /// knowing what a block is, and the rows are the only part it can act on —
    /// they are absolute, which is the coordinate it already moves in.
    ///
    /// Which rows count is the **caller's** decision, because that is where
    /// the block model lives. A block omt never saw a command for has no
    /// prompt, and jumping to a row omt guessed at would land somewhere
    /// arbitrary while looking deliberate.
    ///
    /// Returns whether it moved, so a caller can say nothing happened rather
    /// than leave somebody wondering whether the key exists.
    pub fn to_prompt(
        &mut self,
        forwards: bool,
        prompts: &[u64],
        rows: u16,
        total_rows: u64,
    ) -> bool {
        let found = if forwards {
            prompts.iter().copied().find(|row| *row > self.row)
        } else {
            prompts.iter().copied().rfind(|row| *row < self.row)
        };
        let Some(row) = found else { return false };
        self.row = row;
        self.col = 0;
        self.follow(rows, total_rows);
        true
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
        if self.linewise {
            // Whole rows, so the highlight says what will actually be copied.
            // A line-wise selection drawn as a span would tell somebody they
            // are taking less than they are.
            return row >= start.0 && row <= end.0;
        }
        (row, col) >= start && (row, col) <= end
    }
}

/// Whether one row contains the needle.
///
/// `exact` comes from the query having a capital in it: without one, both
/// sides are lower-cased, because somebody searching their own scrollback is
/// looking for a word and not for a spelling of it.
fn matches(
    row: u64,
    needle: &str,
    exact: bool,
    read: &mut impl FnMut(u64) -> Option<String>,
) -> bool {
    read(row).is_some_and(|text| {
        if exact {
            text.contains(needle)
        } else {
            text.to_lowercase().contains(needle)
        }
    })
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
pub fn selected_text(copy: &Copy, mut read: impl FnMut(u64) -> Option<String>) -> String {
    let Some(((from_row, from_col), (to_row, to_col))) = copy.span() else {
        return String::new();
    };
    // Line-wise widens the span to whole rows here, at the one point the text
    // is taken, so nothing else has to learn a second shape of selection.
    let (from_col, to_col) = if copy.linewise {
        (0, u16::MAX)
    } else {
        (from_col, to_col)
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
        // `to_col` is `u16::MAX` for a line-wise selection, so this is the
        // whole row.
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

    #[test]
    fn a_word_motion_reaches_an_argument_without_walking_to_it() {
        // Reaching one argument in a long command line character by character
        // is not reading, it is typing.
        let line = |_row: u64| Some("cargo test --workspace".to_owned());
        let mut copy = Copy::entering(1, 10);
        copy.col = 0;
        copy.word(Word::Next, line);
        assert_eq!(copy.col, 6, "the start of `test`");
        copy.word(Word::Next, line);
        assert_eq!(copy.col, 11, "the start of `--workspace`");
        copy.word(Word::Back, line);
        assert_eq!(copy.col, 6);
        copy.word(Word::End, line);
        assert_eq!(copy.col, 9, "the end of `test`");
    }

    #[test]
    fn word_separators_end_a_word_where_they_are_configured_to() {
        // `terminal.word_separators` reached nothing, so `/usr/local/bin` was
        // one word however anybody had configured it — including for anyone
        // who imported the setting from the terminal they came from.
        let line = |_row: u64| Some("/usr/local/bin end".to_owned());
        let mut plain = Copy::entering(0, 24);
        plain.word(Word::Next, line);
        assert_eq!(plain.col, 15, "whitespace alone leaves the path whole");

        let mut split = Copy::entering(0, 24);
        split.word_between(Word::Next, "/", line);
        assert_eq!(split.col, 1, "a separator ends the word where it is");
    }

    #[test]
    fn a_word_motion_stops_at_the_end_of_its_line() {
        // In a scrollback the next line is usually unrelated output rather
        // than a continuation, so wrapping moves the reader somewhere they
        // were not looking.
        let line = |_row: u64| Some("one two".to_owned());
        let mut copy = Copy::entering(10, 5);
        copy.col = 4;
        copy.word(Word::Next, line);
        assert_eq!(copy.col, 6, "the last word, not the next line");
        let before = copy.row;
        copy.word(Word::Next, line);
        assert_eq!(copy.row, before);
    }

    #[test]
    fn the_first_thing_on_the_line_is_not_the_first_column() {
        let mut copy = Copy::entering(1, 5);
        copy.col = 9;
        copy.first_non_blank(|_| Some("    indented".to_owned()));
        assert_eq!(copy.col, 4);
    }

    #[test]
    fn half_a_page_keeps_the_readers_place_in_view() {
        let mut copy = Copy::entering(100, 10);
        copy.row = 50;
        copy.go(Move::HalfPageUp, 100, 10, 80);
        assert_eq!(copy.row, 45);
        copy.go(Move::HalfPageDown, 100, 10, 80);
        assert_eq!(copy.row, 50);
    }

    #[test]
    fn a_line_wise_selection_takes_whole_rows() {
        // What somebody copies out of a scrollback is a command or a line of
        // output far more often than it is a span within one.
        let lines = ["first line", "second line", "third line"];
        let read = |row: u64| lines.get(row as usize).map(|s| (*s).to_owned());
        let mut copy = Copy::entering(3, 3);
        copy.row = 0;
        copy.col = 4;
        copy.linewise = true;
        copy.mark();
        copy.row = 1;
        copy.col = 2;
        assert_eq!(selected_text(&copy, read), "first line\nsecond line");
        // And the highlight says the same thing the copy will.
        assert!(copy.selected(0, 0) && copy.selected(1, 9));
    }

    #[test]
    fn a_line_wise_selection_of_one_row_is_that_row() {
        let read = |_row: u64| Some("the whole line".to_owned());
        let mut copy = Copy::entering(1, 3);
        copy.col = 5;
        copy.linewise = true;
        copy.mark();
        assert_eq!(selected_text(&copy, read), "the whole line");
    }

    #[test]
    fn a_search_without_capitals_matches_either_case() {
        let lines = ["nothing here", "an ERROR happened"];
        let read = |row: u64| lines.get(row as usize).map(|s| (*s).to_owned());
        let mut copy = Copy::entering(2, 2);
        copy.row = 0;
        copy.needle = "error".to_owned();
        assert!(copy.jump(true, 2, 2, read));
        assert_eq!(copy.row, 1);
    }

    #[test]
    fn a_search_with_a_capital_means_that_capital() {
        // Somebody who typed the capitals typed them on purpose, and matching
        // anything else walks them through every line they did not mean.
        let lines = ["an error happened", "an ERROR happened"];
        let read = |row: u64| lines.get(row as usize).map(|s| (*s).to_owned());
        let mut copy = Copy::entering(2, 2);
        copy.row = 0;
        copy.needle = "ERROR".to_owned();
        assert!(copy.jump(true, 2, 2, read));
        assert_eq!(copy.row, 1, "the lower-case line is not a match");
    }

    #[test]
    fn jumping_between_prompts_is_what_a_scrollback_is_for() {
        // And it is the one thing here that needs the shell to have said where
        // its prompts are, which is why nothing without a block model can do
        // it at all.
        let prompts = [3u64, 40, 91];
        let mut copy = Copy::entering(120, 10);
        copy.row = 50;
        assert!(copy.to_prompt(false, &prompts, 10, 120));
        assert_eq!(copy.row, 40, "the prompt of the command above");
        assert!(copy.to_prompt(true, &prompts, 10, 120));
        assert_eq!(copy.row, 91);
    }

    #[test]
    fn there_is_nothing_before_the_first_prompt() {
        let prompts = [3u64];
        let mut copy = Copy::entering(120, 10);
        copy.row = 3;
        assert!(!copy.to_prompt(false, &prompts, 10, 120));
        assert_eq!(copy.row, 3, "it did not move somewhere arbitrary");
    }

    #[test]
    fn a_shell_with_no_integration_has_no_prompts_to_jump_to() {
        let mut copy = Copy::entering(120, 10);
        copy.row = 50;
        assert!(!copy.to_prompt(false, &[], 10, 120));
        assert!(!copy.to_prompt(true, &[], 10, 120));
        assert_eq!(copy.row, 50);
    }
}
