//! Finding things in a session's history.
//!
//! `docs/architecture/04-terminal-core.md` §8.1.
//!
//! **Search runs over logical lines**, so a match that spans a soft wrap is
//! found naturally. That is not a special case that was handled — it is a
//! special case that does not arise, because what is stored is content rather
//! than rows, and it is the clearest single win of that decision. A terminal
//! storing wrapped rows needs a "soft boundary extender" to find `error:` when
//! the `err` sat at the end of one row and `or:` at the start of the next.
//!
//! **Results are positions, not rows and columns**, so they survive a resize.
//! A highlight that moved to the wrong text when somebody widened their window
//! would be worse than no highlight.
//!
//! **A search is resumable and bounded.** Searching a hundred thousand lines
//! must never block the caller for more than one budget's worth of work — the
//! caller is a UI thread, and a search that takes a second is a second in which
//! the terminal does not respond to typing.

use crate::line::Line;
use crate::scrollback::Position;

/// How to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// Exactly.
    Sensitive,
    /// Folding ASCII case.
    Insensitive,
}

/// Which way to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Toward newer lines.
    Forward,
    /// Toward older ones.
    Backward,
}

/// What to look for.
#[derive(Debug, Clone)]
pub struct Query {
    /// The text.
    pub pattern: String,
    /// How to compare it.
    pub case: Case,
    /// Which way.
    pub direction: Direction,
    /// Whether to continue from the other end when it runs out.
    pub wrap: bool,
}

impl Query {
    /// A forward, case-insensitive search — what a person means by "find".
    #[must_use]
    pub fn literal(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_owned(),
            case: Case::Insensitive,
            direction: Direction::Forward,
            wrap: true,
        }
    }
}

/// Where a search has got to, so it can be continued.
#[derive(Debug, Clone)]
pub struct Cursor {
    /// The next logical line to look at.
    pub at: u64,
    /// How many lines one call may examine.
    pub budget_lines: u32,
    /// Whether it has already been round once.
    ///
    /// Held so a wrapping search stops rather than going round forever when
    /// there is nothing to find.
    pub seen_wrap: bool,
}

impl Cursor {
    /// A cursor starting at a line, with the default budget.
    #[must_use]
    pub const fn at(line: u64) -> Self {
        Self {
            at: line,
            budget_lines: 5_000,
            seen_wrap: false,
        }
    }
}

/// A hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Where it starts.
    pub start: Position,
    /// Where it ends.
    pub end: Position,
    /// The line it was on, for showing in a result list.
    pub preview: String,
}

/// What one call produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A hit.
    Found(Match),
    /// The budget ran out before anything was found.
    ///
    /// Distinct from `Exhausted`: the caller should call again, and a UI should
    /// keep showing "searching" rather than "no results" — which is the wrong
    /// answer given to somebody who would have got a hit one line later.
    Budget,
    /// There is nothing more to look at.
    Exhausted,
}

/// The text of a line, for matching.
fn text_of(line: &Line) -> String {
    let mut out = String::new();
    for cell in line.cells() {
        if cell.flags.contains(crate::cell::Flags::WIDE_SPACER) {
            continue;
        }
        match cell.resolve() {
            crate::cell::Resolved::Char('\0') => out.push(' '),
            crate::cell::Resolved::Char(c) => out.push(c),
            crate::cell::Resolved::Grapheme(_) => out.push('?'),
        }
    }
    out
}

/// Whether a line could possibly match, cheaply.
///
/// A byte-level check before building a `String` for the line: most lines in a
/// scrollback do not contain the pattern, and allocating one string per line to
/// find that out is the whole cost of a search.
fn might_contain(line: &Line, first: u8, case: Case) -> bool {
    let lower = first.to_ascii_lowercase();
    let upper = first.to_ascii_uppercase();
    line.cells().iter().any(|cell| {
        let crate::cell::Resolved::Char(c) = cell.resolve() else {
            // A grapheme could be anything; let the slow path decide.
            return true;
        };
        let mut buffer = [0u8; 4];
        let bytes = c.encode_utf8(&mut buffer).as_bytes();
        match case {
            Case::Sensitive => bytes.contains(&first),
            Case::Insensitive => bytes.contains(&lower) || bytes.contains(&upper),
        }
    })
}

fn find_in(text: &str, pattern: &str, case: Case) -> Option<usize> {
    match case {
        Case::Sensitive => text.find(pattern),
        Case::Insensitive => text.to_lowercase().find(&pattern.to_lowercase()),
    }
}

/// Look for the next match, spending at most one budget.
///
/// `lines` is indexed from `first_line`, which is the absolute index of its
/// first element — so positions come back absolute and survive eviction of
/// everything before them.
pub fn step(lines: &[Line], first_line: u64, query: &Query, cursor: &mut Cursor) -> Outcome {
    if query.pattern.is_empty() {
        // Every position matches an empty pattern, which is not an answer.
        return Outcome::Exhausted;
    }
    let Some(first_byte) = query.pattern.as_bytes().first().copied() else {
        return Outcome::Exhausted;
    };
    let last = first_line + lines.len() as u64;
    let mut examined = 0u32;

    loop {
        if examined >= cursor.budget_lines {
            // The caller gets its thread back and calls again. A UI that
            // reported "no results" here would be answering a question it has
            // not finished asking.
            return Outcome::Budget;
        }
        if cursor.at < first_line || cursor.at >= last {
            if query.wrap && !cursor.seen_wrap {
                cursor.seen_wrap = true;
                cursor.at = match query.direction {
                    Direction::Forward => first_line,
                    Direction::Backward => last.saturating_sub(1),
                };
                continue;
            }
            return Outcome::Exhausted;
        }

        let index = (cursor.at - first_line) as usize;
        let line = &lines[index];
        examined += 1;

        let hit = if might_contain(line, first_byte, query.case) {
            let text = text_of(line);
            find_in(&text, &query.pattern, query.case).map(|offset| {
                // A byte offset into the rendered text is a cell offset only
                // while everything is one byte wide, so it is counted in
                // characters rather than assumed.
                let cells = text[..offset].chars().count() as u32;
                Match {
                    start: Position::new(cursor.at, cells),
                    end: Position::new(
                        cursor.at,
                        cells + query.pattern.chars().count() as u32,
                    ),
                    preview: text.trim_end().to_owned(),
                }
            })
        } else {
            None
        };

        // Advanced before returning, so calling again continues rather than
        // finding the same hit forever.
        match query.direction {
            Direction::Forward => cursor.at += 1,
            Direction::Backward => {
                if cursor.at == 0 {
                    cursor.at = u64::MAX;
                } else {
                    cursor.at -= 1;
                }
            }
        }

        if let Some(hit) = hit {
            return Outcome::Found(hit);
        }
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
    use crate::cell::{Cell, Color, Flags};

    fn line(text: &str) -> Line {
        let cells: Vec<Cell> = text
            .chars()
            .map(|c| Cell::new(c, Color::DEFAULT, Color::DEFAULT, Flags::empty()))
            .collect();
        Line::from_cells(cells)
    }

    fn corpus() -> Vec<Line> {
        vec![
            line("the first line"),
            line("error: something went wrong"),
            line("the third line"),
            line("ERROR: shouting"),
        ]
    }

    #[test]
    fn a_literal_is_found_and_reported_as_a_position() {
        // Positions rather than rows and columns, so the hit survives a resize.
        let lines = corpus();
        let mut cursor = Cursor::at(0);
        let Outcome::Found(hit) = step(&lines, 0, &Query::literal("something"), &mut cursor) else {
            panic!("found");
        };
        assert_eq!(hit.start.line, 1);
        assert_eq!(hit.start.offset, 7);
        assert_eq!(hit.end.offset, 7 + 9);
    }

    #[test]
    fn case_folding_finds_the_shouted_one_too() {
        let lines = corpus();
        let mut cursor = Cursor::at(3);
        let Outcome::Found(hit) = step(&lines, 0, &Query::literal("error"), &mut cursor) else {
            panic!("found");
        };
        assert_eq!(hit.start.line, 3);
    }

    #[test]
    fn a_case_sensitive_search_does_not() {
        let lines = corpus();
        let mut query = Query::literal("ERROR");
        query.case = Case::Sensitive;
        query.wrap = false;
        let mut cursor = Cursor::at(0);
        let Outcome::Found(hit) = step(&lines, 0, &query, &mut cursor) else {
            panic!("found");
        };
        assert_eq!(hit.start.line, 3, "not the lowercase one on line 1");
    }

    #[test]
    fn a_match_spanning_a_soft_wrap_is_found_because_lines_are_logical() {
        // The clearest win of storing content rather than rows: a terminal that
        // stored wrapped rows would need a soft-boundary special case to find
        // this at all.
        let mut long = line("the error: appears after a wrap point in one logical line");
        long.set_wrap(crate::line::Wrap::Soft);
        let lines = vec![long];
        let mut cursor = Cursor::at(0);
        assert!(matches!(
            step(&lines, 0, &Query::literal("appears after"), &mut cursor),
            Outcome::Found(_)
        ));
    }

    #[test]
    fn searching_continues_rather_than_finding_the_same_hit_forever() {
        let lines = corpus();
        let mut query = Query::literal("line");
        query.wrap = false;
        let mut cursor = Cursor::at(0);
        let mut found = Vec::new();
        while let Outcome::Found(hit) = step(&lines, 0, &query, &mut cursor) {
            found.push(hit.start.line);
        }
        assert_eq!(found, vec![0, 2]);
    }

    #[test]
    fn a_backward_search_walks_the_other_way() {
        let lines = corpus();
        let mut query = Query::literal("line");
        query.direction = Direction::Backward;
        query.wrap = false;
        let mut cursor = Cursor::at(3);
        let Outcome::Found(hit) = step(&lines, 0, &query, &mut cursor) else {
            panic!("found");
        };
        assert_eq!(hit.start.line, 2);
    }

    #[test]
    fn running_out_of_budget_is_not_the_same_as_finding_nothing() {
        // A UI reporting "no results" here would be answering a question it has
        // not finished asking — and the hit might be on the next line.
        let lines = corpus();
        let mut query = Query::literal("shouting");
        query.wrap = false;
        let mut cursor = Cursor::at(0);
        cursor.budget_lines = 2;
        assert_eq!(step(&lines, 0, &query, &mut cursor), Outcome::Budget);
        // Called again, it continues from where it stopped and finds it.
        assert!(matches!(
            step(&lines, 0, &query, &mut cursor),
            Outcome::Found(_)
        ));
    }

    #[test]
    fn a_wrapping_search_goes_round_once_and_then_stops() {
        // Otherwise a pattern that is not there spins forever.
        let lines = corpus();
        let mut cursor = Cursor::at(2);
        assert_eq!(
            step(&lines, 0, &Query::literal("nowhere"), &mut cursor),
            Outcome::Exhausted
        );
    }

    #[test]
    fn a_wrapping_search_finds_a_hit_behind_where_it_started() {
        let lines = corpus();
        let mut cursor = Cursor::at(2);
        let Outcome::Found(hit) = step(&lines, 0, &Query::literal("first"), &mut cursor) else {
            panic!("found");
        };
        assert_eq!(hit.start.line, 0, "it wrapped round to reach it");
    }

    #[test]
    fn positions_are_absolute_so_eviction_does_not_move_them() {
        // The scrollback drops its oldest chunk and everything still points at
        // the same content.
        let lines = corpus();
        let mut cursor = Cursor::at(1_000);
        let Outcome::Found(hit) = step(&lines, 1_000, &Query::literal("error"), &mut cursor) else {
            panic!("found");
        };
        assert_eq!(hit.start.line, 1_001);
    }

    #[test]
    fn an_empty_pattern_matches_nothing_rather_than_everything() {
        let lines = corpus();
        let mut cursor = Cursor::at(0);
        assert_eq!(
            step(&lines, 0, &Query::literal(""), &mut cursor),
            Outcome::Exhausted
        );
    }

    #[test]
    fn the_preview_is_the_line_a_result_list_shows() {
        let lines = corpus();
        let mut cursor = Cursor::at(0);
        let Outcome::Found(hit) = step(&lines, 0, &Query::literal("something"), &mut cursor) else {
            panic!("found");
        };
        assert_eq!(hit.preview, "error: something went wrong");
    }
}
