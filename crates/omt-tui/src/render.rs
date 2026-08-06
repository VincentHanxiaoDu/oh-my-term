//! Turning a grid into bytes for the real terminal underneath.
//!
//! The whole job is to emit as little as possible. A renderer that redraws
//! everything on every frame works, looks fine on a laptop, and makes the
//! terminal unusable over ssh — so this diffs against what was last drawn and
//! writes only the runs that changed.

use std::io::{self, Write};

use omt_term::{Cell, ColorKind, Flags, Grid, Resolved, Underline};

/// What the screen currently shows, so the next frame can be a difference.
#[derive(Debug, Default)]
pub struct Screen {
    rows: Vec<Vec<Cell>>,
    cols: u16,
    cursor: Option<(u16, u16)>,
    visible: Option<bool>,
}

impl Screen {
    /// A screen that has never been drawn.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget everything, so the next frame is drawn in full.
    ///
    /// After a resize, or anything else that made the terminal's contents no
    /// longer what we believe: diffing against a stale model paints garbage and
    /// leaves it there.
    pub fn invalidate(&mut self) {
        self.rows.clear();
        self.cursor = None;
        self.visible = None;
    }

    /// Draw a grid, writing only what changed.
    ///
    /// # Errors
    /// Fails if the write does.
    pub fn draw(&mut self, out: &mut impl Write, grid: &Grid) -> io::Result<usize> {
        self.draw_at(out, grid, 0, 0)
    }

    /// Draw a grid with its top-left corner at an offset.
    ///
    /// For panes. The diff still works because the offset only changes where
    /// the cursor is parked before each run — what is compared is this screen's
    /// own memory of the same region, which is why one `Screen` belongs to one
    /// pane rather than to the whole terminal.
    ///
    /// # Errors
    /// Fails if the write does.
    pub fn draw_at(
        &mut self,
        out: &mut impl Write,
        grid: &Grid,
        at_col: u16,
        at_row: u16,
    ) -> io::Result<usize> {
        let size = grid.size();
        if self.cols != size.cols || self.rows.len() != size.rows as usize {
            self.rows = vec![Vec::new(); size.rows as usize];
            self.cols = size.cols;
            self.cursor = None;
            self.visible = None;
        }

        let mut buf: Vec<u8> = Vec::new();
        let mut written = 0usize;

        for row in 0..size.rows {
            let line = grid.row(row);
            let next: Vec<Cell> = line.densified(size.cols);
            let prev = &self.rows[row as usize];
            if *prev == next {
                continue;
            }

            // Find the changed span rather than repainting the row. A prompt
            // redraw touches a few cells; a full row is 80 cells of writes for
            // three of change.
            let first = (0..next.len()).find(|i| prev.get(*i) != Some(&next[*i]));
            let Some(first) = first else {
                continue;
            };
            let last = (0..next.len())
                .rev()
                .find(|i| prev.get(*i) != Some(&next[*i]))
                .unwrap_or(first);

            write!(
                buf,
                "\x1b[{};{}H",
                at_row + row + 1,
                at_col + first as u16 + 1
            )?;
            let mut style = Style::UNSET;
            for cell in &next[first..=last] {
                // Skipped *before* the style is considered. A spacer's flags
                // differ from its left half's, so applying style first would
                // emit a full SGR sequence between every wide character and the
                // next — which is both larger on the wire and visible as
                // flicker on a slow link.
                if cell.flags.contains(Flags::WIDE_SPACER) {
                    continue;
                }
                style.apply(&mut buf, cell)?;
                match cell.resolve() {
                    Resolved::Char('\0') => buf.push(b' '),
                    Resolved::Char(c) => {
                        let mut tmp = [0u8; 4];
                        buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
                    }
                    // A cluster the grapheme table holds. Until that is wired,
                    // a placeholder keeps every following column aligned.
                    Resolved::Grapheme(_) => buf.push(b'?'),
                }
            }
            write!(buf, "\x1b[0m")?;
            written += last - first + 1;
            self.rows[row as usize] = next;
        }

        let cursor = (grid.cursor.row, grid.cursor.col);
        if self.cursor != Some(cursor) || written > 0 {
            write!(
                buf,
                "\x1b[{};{}H",
                at_row + cursor.0 + 1,
                at_col + cursor.1 + 1
            )?;
            self.cursor = Some(cursor);
        }
        // Only when it changed. Re-asserting visibility every frame costs six
        // bytes on a screen where nothing happened, which is exactly the frame
        // that has to be free.
        if self.visible != Some(grid.cursor.visible) {
            if grid.cursor.visible {
                write!(buf, "\x1b[?25h")?;
            } else {
                write!(buf, "\x1b[?25l")?;
            }
            self.visible = Some(grid.cursor.visible);
        }

        if !buf.is_empty() {
            out.write_all(&buf)?;
            out.flush()?;
        }
        Ok(written)
    }
}

/// The SGR state currently in effect, so an unchanged style costs nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Style {
    fg: omt_term::Color,
    bg: omt_term::Color,
    flags: Flags,
}

impl Style {
    /// A value no cell can equal, so the first cell of every run emits its
    /// style rather than inheriting whatever the previous run left behind.
    const UNSET: Self = Self {
        fg: omt_term::Color::indexed(255),
        bg: omt_term::Color::indexed(254),
        flags: Flags::all(),
    };

    fn apply(&mut self, buf: &mut Vec<u8>, cell: &Cell) -> io::Result<()> {
        if self.fg == cell.fg && self.bg == cell.bg && self.flags == cell.flags {
            return Ok(());
        }
        write!(buf, "\x1b[0")?;
        if cell.flags.contains(Flags::BOLD) {
            write!(buf, ";1")?;
        }
        if cell.flags.contains(Flags::FAINT) {
            write!(buf, ";2")?;
        }
        if cell.flags.contains(Flags::ITALIC) {
            write!(buf, ";3")?;
        }
        if cell.flags.contains(Flags::INVERSE) {
            write!(buf, ";7")?;
        }
        if cell.flags.contains(Flags::STRIKETHROUGH) {
            write!(buf, ";9")?;
        }
        match cell.flags.underline() {
            Underline::None => {}
            Underline::Double => write!(buf, ";21")?,
            _ => write!(buf, ";4")?,
        }
        write_color(buf, cell.fg, true)?;
        write_color(buf, cell.bg, false)?;
        write!(buf, "m")?;

        self.fg = cell.fg;
        self.bg = cell.bg;
        self.flags = cell.flags;
        Ok(())
    }
}

fn write_color(buf: &mut Vec<u8>, color: omt_term::Color, foreground: bool) -> io::Result<()> {
    let base = if foreground { 30 } else { 40 };
    match color.kind() {
        // Nothing: the leading `0` already reset it, and emitting 39/49 as well
        // is two more bytes per run for no change.
        ColorKind::Default => Ok(()),
        ColorKind::Indexed(i) if i < 8 => write!(buf, ";{}", base + u16::from(i)),
        ColorKind::Indexed(i) if i < 16 => write!(buf, ";{}", base + 60 + u16::from(i) - 8),
        ColorKind::Indexed(i) => write!(buf, ";{};5;{i}", base + 8),
        ColorKind::Rgb(r, g, b) => write!(buf, ";{};2;{r};{g};{b}", base + 8),
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
    use omt_term::{GridSize, TermConfig, Terminal};

    fn terminal(cols: u16, rows: u16, input: &str) -> Terminal {
        let mut t = Terminal::new(TermConfig {
            size: GridSize::new(cols, rows),
            ..TermConfig::default()
        });
        t.advance(input.as_bytes());
        t
    }

    fn draw(screen: &mut Screen, t: &Terminal) -> (String, usize) {
        let mut out = Vec::new();
        let n = screen.draw(&mut out, t.grid()).expect("draw");
        (String::from_utf8_lossy(&out).into_owned(), n)
    }

    #[test]
    fn the_first_frame_draws_what_is_there() {
        let mut s = Screen::new();
        let t = terminal(20, 3, "hello");
        let (bytes, _) = draw(&mut s, &t);
        assert!(bytes.contains("hello"), "{bytes:?}");
    }

    #[test]
    fn an_unchanged_frame_writes_nothing_at_all() {
        // The property that makes this usable over ssh: a screen nobody touched
        // must cost zero bytes, not one full repaint per frame.
        let mut s = Screen::new();
        let t = terminal(20, 3, "hello");
        draw(&mut s, &t);
        let (bytes, n) = draw(&mut s, &t);
        assert_eq!(n, 0);
        assert!(bytes.is_empty(), "an idle frame wrote {bytes:?}");
    }

    #[test]
    fn only_the_changed_span_is_written() {
        // A prompt redraw touches a few cells. Repainting the row would be 80
        // cells of writes for three of change.
        let mut s = Screen::new();
        let mut t = terminal(80, 3, "aaaaaaaaaa");
        draw(&mut s, &t);

        t.advance(b"\x1b[1;5Hz");
        let (_, n) = draw(&mut s, &t);
        assert_eq!(n, 1, "one cell changed; {n} were written");
    }

    #[test]
    fn a_style_is_emitted_once_per_run_not_per_cell() {
        let mut s = Screen::new();
        let t = terminal(20, 2, "\x1b[31mredred\x1b[0m");
        let (bytes, _) = draw(&mut s, &t);
        assert_eq!(
            bytes.matches("\u{1b}[0;31m").count(),
            1,
            "the style repeated per cell: {bytes:?}"
        );
    }

    #[test]
    fn the_first_cell_of_a_frame_always_states_its_style() {
        // Inheriting whatever the previous run left behind is how a redraw
        // paints the wrong colour after a scroll.
        let mut s = Screen::new();
        let t = terminal(20, 2, "plain");
        let (bytes, _) = draw(&mut s, &t);
        assert!(bytes.contains("\u{1b}[0m"), "{bytes:?}");
    }

    #[test]
    fn invalidating_forces_a_full_redraw() {
        // After a resize the terminal's contents are no longer what we believe,
        // and diffing against a stale model paints garbage and leaves it.
        let mut s = Screen::new();
        let t = terminal(20, 3, "hello");
        draw(&mut s, &t);
        s.invalidate();
        let (bytes, n) = draw(&mut s, &t);
        assert!(n > 0);
        assert!(bytes.contains("hello"));
    }

    #[test]
    fn a_resize_redraws_rather_than_diffing_against_the_old_shape() {
        let mut s = Screen::new();
        let mut t = terminal(20, 3, "hello");
        draw(&mut s, &t);
        t.resize(GridSize::new(40, 5));
        let (_, n) = draw(&mut s, &t);
        assert!(n > 0, "a resize drew nothing");
    }

    #[test]
    fn a_wide_character_is_not_followed_by_a_space() {
        // Emitting one for the spacer half would overwrite the left half's
        // second column and the line would drift.
        let mut s = Screen::new();
        let t = terminal(20, 2, "漢字");
        let (bytes, _) = draw(&mut s, &t);
        assert!(
            bytes.contains("漢字"),
            "a style or a spacer landed between them: {bytes:?}"
        );
    }

    #[test]
    fn the_cursor_ends_where_the_grid_says() {
        let mut s = Screen::new();
        let t = terminal(20, 5, "\x1b[3;7Hx");
        let (bytes, _) = draw(&mut s, &t);
        assert!(bytes.ends_with("\u{1b}[?25h"), "{bytes:?}");
        assert!(bytes.contains("\u{1b}[3;8H"), "{bytes:?}");
    }

    #[test]
    fn a_hidden_cursor_is_hidden() {
        let mut s = Screen::new();
        let t = terminal(20, 3, "\x1b[?25lx");
        let (bytes, _) = draw(&mut s, &t);
        assert!(bytes.contains("\u{1b}[?25l"), "{bytes:?}");
    }

    #[test]
    fn truecolor_survives_to_the_wire() {
        let mut s = Screen::new();
        let t = terminal(20, 2, "\x1b[38;2;10;20;30mX");
        let (bytes, _) = draw(&mut s, &t);
        assert!(bytes.contains(";38;2;10;20;30"), "{bytes:?}");
    }

    #[test]
    fn a_pane_draws_where_it_was_put_and_not_at_the_origin() {
        // The whole point of panes: a second pane writing at column one would
        // paint over the first, which reads as corruption rather than as a
        // layout bug.
        let mut s = Screen::new();
        let t = terminal(20, 3, "hello");
        let mut out = Vec::new();
        s.draw_at(&mut out, t.grid(), 40, 5).expect("draw");
        let bytes = String::from_utf8_lossy(&out);
        assert!(bytes.contains("\u{1b}[6;41H"), "{bytes:?}");
        assert!(!bytes.contains("\u{1b}[1;1H"), "it drew at the origin");
    }

    #[test]
    fn a_panes_cursor_is_placed_inside_that_pane() {
        // Otherwise the cursor sits in whichever pane happens to be at the
        // origin, and typing appears to go to the wrong place.
        let mut s = Screen::new();
        let t = terminal(20, 5, "\x1b[2;3Hx");
        let mut out = Vec::new();
        s.draw_at(&mut out, t.grid(), 10, 10).expect("draw");
        let bytes = String::from_utf8_lossy(&out);
        assert!(bytes.contains("\u{1b}[12;14H"), "{bytes:?}");
    }

    #[test]
    fn an_offset_pane_still_writes_nothing_when_nothing_changed() {
        // The diff has to survive the offset, or every pane repaints every
        // frame and the whole thing is unusable over ssh.
        let mut s = Screen::new();
        let t = terminal(20, 3, "hello");
        let mut first = Vec::new();
        s.draw_at(&mut first, t.grid(), 40, 5).expect("draw");
        let mut second = Vec::new();
        let n = s.draw_at(&mut second, t.grid(), 40, 5).expect("draw");
        assert_eq!(n, 0);
        assert!(second.is_empty(), "an idle pane wrote {second:?}");
    }

    #[test]
    fn moving_the_cursor_alone_still_repositions_it() {
        let mut s = Screen::new();
        let mut t = terminal(20, 5, "x");
        draw(&mut s, &t);
        t.advance(b"\x1b[4;2H");
        let (bytes, _) = draw(&mut s, &t);
        assert!(bytes.contains("\u{1b}[4;2H"), "{bytes:?}");
    }
}
