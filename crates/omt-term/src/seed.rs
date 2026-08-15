//! A terminal's state, rendered back into the bytes that would produce it.
//!
//! What a client that has just arrived is given. It is deliberately *bytes*
//! rather than a structured snapshot: a client already runs an emulator, and
//! feeding it a seed uses the same parser as feeding it live output. A
//! structured snapshot would be a second way to reconstruct a screen, and a
//! second way is the one nobody exercises until the day it is wrong.
//!
//! It is not a transcript of what the session did. It is the shortest sequence
//! that leaves a fresh emulator of the same size looking like this one — which
//! is why it is emitted row by row from the current grid rather than replayed
//! from history.

use std::fmt::Write as _;

use crate::cell::{Cell, ColorKind, Flags, Resolved};
use crate::grid::Grid;
use crate::terminal::{Terminal, Which};

/// Render a terminal's visible state as escape sequences.
///
/// The screen, its colours, the cursor's position and visibility, and which
/// screen is active. Scrollback is *not* included: it is not on the screen, a
/// client asks for it through the history capabilities, and a seed that carried
/// a session's whole life would be megabytes at the moment somebody is waiting
/// to see a prompt.
#[must_use]
pub fn seed(terminal: &Terminal) -> Vec<u8> {
    let mut out = String::new();
    // Whichever screen the program is on. A full-screen program seeded onto the
    // primary screen would paint over the shell's scrollback, and leaving it
    // there is worse than the missing repaint: the user's history is gone.
    if terminal.active() == Which::Alternate {
        out.push_str("\x1b[?1049h");
    } else {
        out.push_str("\x1b[?1049l");
    }
    // Reset first: a fresh emulator is already clean, but a *reused* one is
    // whatever the last session left it as, and a seed that assumed otherwise
    // paints tomorrow's screen in yesterday's colours.
    out.push_str("\x1b[0m\x1b[H\x1b[2J");

    let grid = terminal.grid();
    write_rows(&mut out, grid);

    // The cursor last, so nothing after it moves it.
    let cursor = grid.cursor;
    let _ = write!(out, "\x1b[{};{}H", cursor.row + 1, cursor.col + 1);
    out.push_str(if cursor.visible {
        "\x1b[?25h"
    } else {
        "\x1b[?25l"
    });
    out.into_bytes()
}

/// Write every row, one cursor move per row.
///
/// Positioned per row rather than relying on wrapping: a row that ends exactly
/// at the right margin leaves the cursor in a pending-wrap state that differs
/// between emulators, and a seed that depended on it would be right here and
/// wrong somewhere else.
fn write_rows(out: &mut String, grid: &Grid) {
    let size = grid.size();
    let mut style = Style::default();
    for row in 0..size.rows {
        let line = grid.row(row);
        let cells = line.cells();
        // Trailing blanks in default colours are what a clear already left
        // behind. Emitting them costs the width of the screen per row on every
        // attach, over a link, for nothing.
        let last = cells
            .iter()
            .rposition(|c| !is_blank(c))
            .map_or(0, |i| i + 1);
        if last == 0 {
            continue;
        }
        let _ = write!(out, "\x1b[{};1H", row + 1);
        let mut skip = 0usize;
        for (col, cell) in cells.iter().enumerate().take(last) {
            if col < skip {
                continue;
            }
            if cell.flags.contains(Flags::WIDE_SPACER) {
                // The right half of a wide cluster is storage, not a
                // character; the emulator will allocate it again from the
                // character itself.
                continue;
            }
            let wanted = Style::of(cell);
            if wanted != style {
                out.push_str(&wanted.sgr_from(style));
                style = wanted;
            }
            match cell.resolve() {
                Resolved::Char('\0') => out.push(' '),
                Resolved::Char(ch) => out.push(ch),
                // A cluster omt cannot spell without the grapheme table it
                // lives in. A placeholder keeps the columns right, which is
                // what the rest of the screen depends on.
                Resolved::Grapheme(_) => out.push('\u{fffc}'),
            }
            skip = col + 1;
        }
    }
    if style != Style::default() {
        out.push_str("\x1b[0m");
    }
}

/// Whether a cell would be indistinguishable from one a clear left.
fn is_blank(cell: &Cell) -> bool {
    let plain = matches!(cell.resolve(), Resolved::Char(' ' | '\0'));
    plain
        && cell.fg.kind() == ColorKind::Default
        && cell.bg.kind() == ColorKind::Default
        && !cell.flags.intersects(
            Flags::BOLD
                | Flags::FAINT
                | Flags::ITALIC
                | Flags::BLINK
                | Flags::INVERSE
                | Flags::STRIKETHROUGH
                | Flags::UNDERLINE_MASK,
        )
}

/// Everything about a cell that is not its character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Style {
    fg: Option<ColorKind>,
    bg: Option<ColorKind>,
    flags: Flags,
}

impl Style {
    fn of(cell: &Cell) -> Self {
        Self {
            fg: Some(cell.fg.kind()),
            bg: Some(cell.bg.kind()),
            // Only what SGR can say. The storage bits — wide halves, grapheme
            // handles — are how omt keeps cells, not how a terminal is told.
            flags: cell.flags
                & (Flags::BOLD
                    | Flags::FAINT
                    | Flags::ITALIC
                    | Flags::BLINK
                    | Flags::INVERSE
                    | Flags::STRIKETHROUGH
                    | Flags::UNDERLINE_MASK),
        }
    }

    /// The sequence that turns `previous` into `self`.
    ///
    /// A full reset then everything, rather than the difference. The
    /// difference is smaller and it is where every terminal renderer has its
    /// oldest bug: an attribute nobody turned off, inherited by the next run,
    /// noticed months later as a line of the wrong colour.
    fn sgr_from(self, _previous: Self) -> String {
        let mut parts = vec!["0".to_owned()];
        if self.flags.contains(Flags::BOLD) {
            parts.push("1".to_owned());
        }
        if self.flags.contains(Flags::FAINT) {
            parts.push("2".to_owned());
        }
        if self.flags.contains(Flags::ITALIC) {
            parts.push("3".to_owned());
        }
        if self.flags.intersects(Flags::UNDERLINE_MASK) {
            parts.push("4".to_owned());
        }
        if self.flags.contains(Flags::BLINK) {
            parts.push("5".to_owned());
        }
        if self.flags.contains(Flags::INVERSE) {
            parts.push("7".to_owned());
        }
        if self.flags.contains(Flags::STRIKETHROUGH) {
            parts.push("9".to_owned());
        }
        if let Some(kind) = self.fg {
            parts.extend(colour(kind, true));
        }
        if let Some(kind) = self.bg {
            parts.extend(colour(kind, false));
        }
        format!("\x1b[{}m", parts.join(";"))
    }
}

/// One colour, as SGR parameters.
fn colour(kind: ColorKind, foreground: bool) -> Vec<String> {
    let base = if foreground { 30 } else { 40 };
    match kind {
        // Covered by the reset this always starts with.
        ColorKind::Default => Vec::new(),
        // The eight and the bright eight have their own short forms, which
        // every terminal has understood for forty years; the indexed form is
        // for the rest.
        ColorKind::Indexed(i @ 0..=7) => vec![(base + u16::from(i)).to_string()],
        ColorKind::Indexed(i @ 8..=15) => vec![(base + 60 + u16::from(i) - 8).to_string()],
        ColorKind::Indexed(i) => vec![format!("{};5;{i}", base + 8)],
        ColorKind::Rgb(r, g, b) => vec![format!("{};2;{r};{g};{b}", base + 8)],
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
    use crate::grid::GridSize;
    use crate::terminal::TermConfig;

    fn term(cols: u16, rows: u16) -> Terminal {
        Terminal::new(TermConfig {
            size: GridSize::new(cols, rows),
            ..TermConfig::default()
        })
    }

    /// Feed a seed to a fresh emulator of the same size.
    fn reseeded(original: &Terminal, cols: u16, rows: u16) -> Terminal {
        let mut fresh = term(cols, rows);
        fresh.advance(&seed(original));
        fresh
    }

    fn text_of(t: &Terminal) -> Vec<String> {
        (0..t.grid().size().rows).map(|r| t.row_text(r)).collect()
    }

    #[test]
    fn what_was_on_the_screen_is_on_the_screen_again() {
        // The whole contract: a client that has just arrived sees what the
        // session looks like, not what it did.
        let mut t = term(20, 4);
        t.advance(b"hello\r\nworld");
        let back = reseeded(&t, 20, 4);
        assert_eq!(text_of(&back), text_of(&t));
    }

    #[test]
    fn colours_and_attributes_survive() {
        // A seed that dropped them would repaint somebody's agent in the wrong
        // colours, which reads as omt corrupting the program's output.
        let mut t = term(20, 2);
        t.advance(b"\x1b[1;31mred bold\x1b[0m plain");
        let back = reseeded(&t, 20, 2);
        let (a, b) = (t.grid().row(0), back.grid().row(0));
        for col in 0..8 {
            assert_eq!(a.cells()[col].fg, b.cells()[col].fg, "column {col}");
            assert_eq!(
                a.cells()[col].flags.contains(Flags::BOLD),
                b.cells()[col].flags.contains(Flags::BOLD),
                "column {col}"
            );
        }
        assert_eq!(text_of(&back), text_of(&t));
    }

    #[test]
    fn the_cursor_comes_back_where_it_was() {
        // Somebody re-attaching is about to type. A cursor in the wrong place
        // is the first thing they see and the first thing they distrust.
        let mut t = term(20, 4);
        t.advance(b"one\r\ntwo\r\nthree");
        let back = reseeded(&t, 20, 4);
        assert_eq!(back.grid().cursor.row, t.grid().cursor.row);
        assert_eq!(back.grid().cursor.col, t.grid().cursor.col);
    }

    #[test]
    fn a_hidden_cursor_stays_hidden() {
        let mut t = term(10, 2);
        t.advance(b"\x1b[?25l");
        assert!(!reseeded(&t, 10, 2).grid().cursor.visible);
    }

    #[test]
    fn an_alternate_screen_is_seeded_as_one() {
        // Seeding a full-screen program onto the primary screen would paint
        // over the shell's scrollback, and that history does not come back.
        let mut t = term(10, 2);
        t.advance(b"\x1b[?1049hin here");
        let back = reseeded(&t, 10, 2);
        assert_eq!(back.active(), Which::Alternate);
        assert_eq!(text_of(&back), text_of(&t));
    }

    #[test]
    fn a_wide_character_still_takes_two_columns() {
        // The right half is storage, not a character. Emitted as one, it would
        // shift the rest of the row by a column per glyph.
        let mut t = term(10, 2);
        t.advance("你好ab".as_bytes());
        let back = reseeded(&t, 10, 2);
        assert_eq!(text_of(&back), text_of(&t));
        assert_eq!(back.grid().cursor.col, t.grid().cursor.col);
    }

    #[test]
    fn an_empty_screen_seeds_to_an_empty_screen() {
        let t = term(20, 3);
        let back = reseeded(&t, 20, 3);
        assert_eq!(text_of(&back), text_of(&t));
    }

    #[test]
    fn trailing_blanks_are_not_written_out() {
        // They are what the clear already left. Writing them costs the width
        // of the screen per row on every attach, over the link where it hurts.
        let mut t = term(80, 24);
        t.advance(b"short");
        let bytes = seed(&t);
        assert!(
            bytes.len() < 200,
            "a seed for one word was {} bytes",
            bytes.len()
        );
    }

    #[test]
    fn a_seed_starts_from_a_known_state() {
        // A reused emulator is whatever the last session left it as. A seed
        // that assumed a clean one paints today's screen in yesterday's
        // colours.
        let mut dirty = term(20, 2);
        dirty.advance(b"\x1b[41mleftover");
        let mut fresh = term(20, 2);
        fresh.advance(b"clean");
        dirty.advance(&seed(&fresh));
        assert_eq!(text_of(&dirty), text_of(&fresh));
        assert_eq!(
            dirty.grid().row(0).cells()[0].bg,
            fresh.grid().row(0).cells()[0].bg
        );
    }
}
