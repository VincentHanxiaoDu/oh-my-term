//! The terminal: bytes in, state and host actions out.

use vte::{Params, Perform};

use crate::action::{
    ActionQueue, Backpressure, BlockEvent, ClipboardSelection, ColorSlot, HostAction,
    HyperlinkEvent, TermMode, Warning, WindowOp,
};
use crate::cell::{Cell, Color, Flags, Underline};
use crate::grid::{EraseExtent, Grid, GridSize};
use crate::line::{HyperlinkId, Line};
use crate::scrollback::{Scrollback, ScrollbackLimits};

/// How the terminal is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermConfig {
    /// The starting size.
    pub size: GridSize,
    /// Scrollback caps.
    pub scrollback: ScrollbackLimits,
    /// Most host actions queued per `advance`.
    pub max_host_actions: usize,
}

impl Default for TermConfig {
    fn default() -> Self {
        Self {
            size: GridSize::new(80, 24),
            scrollback: ScrollbackLimits::default(),
            max_host_actions: 256,
        }
    }
}

/// Which screen is being drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// The scrolling one.
    Primary,
    /// The one full-screen programs draw on, which has no scrollback.
    Alternate,
}

/// The whole emulator.
pub struct Terminal {
    parser: vte::Parser,
    state: State,
    /// Command blocks, positioned in absolute rows.
    ///
    /// Here rather than in `State` because a block's position depends on how
    /// much has scrolled off, and the parser has no idea.
    blocks: crate::block::BlockTracker,
}

/// Everything the parser mutates.
///
/// Split from [`Terminal`] because `vte` borrows the performer mutably while
/// the parser drives it, and the parser cannot borrow itself.
struct State {
    primary: Grid,
    alternate: Grid,
    active: Which,
    scrollback: Scrollback,
    actions: ActionQueue,
    modes: Modes,
    hyperlinks: Vec<String>,
    open_link: Option<(HyperlinkId, u16, u16)>,
    tab_stops: Vec<bool>,
    title: Option<String>,
    /// Set when the action queue refused an undroppable action, so the caller
    /// is told to drain before feeding more bytes.
    stalled: Option<HostAction>,
    /// Shell-integration transitions the terminal has not positioned yet.
    pending_blocks: Vec<crate::action::BlockEvent>,
    /// Set when a printable character was written since the last check.
    ///
    /// Only printables: an escape sequence that moves the cursor is not a
    /// program producing output, and treating it as one would open a block
    /// every time something repainted its status line.
    wrote_output: bool,
}

/// The DEC private modes that are on.
#[derive(Debug, Clone, Default)]
pub struct Modes {
    set: std::collections::BTreeSet<TermMode>,
}

impl Modes {
    /// Whether a mode is on.
    #[must_use]
    pub fn contains(&self, m: TermMode) -> bool {
        self.set.contains(&m)
    }
}

impl Terminal {
    /// A fresh terminal.
    #[must_use]
    pub fn new(config: TermConfig) -> Self {
        let size = config.size;
        Self {
            parser: vte::Parser::new(),
            state: State {
                primary: Grid::new(size),
                alternate: Grid::new(size),
                active: Which::Primary,
                scrollback: Scrollback::new(config.scrollback),
                actions: ActionQueue::new(config.max_host_actions),
                modes: Modes::default(),
                hyperlinks: Vec::new(),
                open_link: None,
                tab_stops: default_tab_stops(size.cols),
                title: None,
                stalled: None,
                pending_blocks: Vec::new(),
                wrote_output: false,
            },
            blocks: crate::block::BlockTracker::new(),
        }
    }

    /// Feed bytes.
    ///
    /// Returns how many were consumed. That is fewer than were offered only
    /// when an undroppable host action could not be queued; the caller drains
    /// and re-enters with the rest, which is backpressure rather than loss.
    pub fn advance(&mut self, bytes: &[u8]) -> usize {
        for (i, b) in bytes.iter().enumerate() {
            if self.state.stalled.is_some() {
                return i;
            }
            self.parser.advance(&mut self.state, &[*b]);
            // Blocks are tracked here rather than in the parser because a block
            // is positioned in *absolute* rows, and only this level knows how
            // much has scrolled off. Peeked rather than drained: the host still
            // receives every action it would have.
            self.track_blocks();
        }
        bytes.len()
    }

    /// The command blocks this session has produced.
    ///
    /// Empty for a shell that emits no OSC 133, which is the honest answer: a
    /// tracker that guessed at command boundaries from prompt shapes would be
    /// wrong on every shell whose prompt it did not recognise.
    #[must_use]
    pub fn blocks(&self) -> &[crate::block::Block] {
        self.blocks.blocks()
    }

    /// The block still running, if one is.
    #[must_use]
    pub fn active_block(&self) -> Option<&crate::block::Block> {
        self.blocks.active()
    }

    /// The most recent command that failed.
    #[must_use]
    pub fn last_failed_block(&self) -> Option<&crate::block::Block> {
        self.blocks.last_failure()
    }

    /// The row the cursor is on, counting from the first line ever written.
    ///
    /// Absolute rather than screen-relative, because a block outlives the
    /// screen it started on: a command whose output has scrolled past would
    /// otherwise appear to start wherever the screen happens to be now.
    #[must_use]
    pub fn absolute_row(&self) -> u64 {
        self.state.scrollback.len() as u64 + u64::from(self.grid().cursor.row)
    }

    /// Feed any new shell-integration transitions to the block tracker.
    ///
    /// Through a small queue of its own rather than by inspecting the action
    /// queue: the host drains that one, and two consumers of a queue with one
    /// cursor is how one of them silently stops seeing things.
    fn track_blocks(&mut self) {
        let row = self.state.scrollback.len() as u64 + u64::from(self.grid().cursor.row);

        // Output with nothing open opens a block. This is what makes a shell
        // with no OSC 133 — `ssh` to a host that has none, a container, an
        // ancient login shell — produce something rather than an empty session
        // that looks broken.
        if self.state.wrote_output {
            self.state.wrote_output = false;
            self.blocks.output_at(row);
        }

        if self.state.pending_blocks.is_empty() {
            return;
        }
        for event in std::mem::take(&mut self.state.pending_blocks) {
            self.blocks.apply(event, row);
        }
    }

    /// Take everything the host must do or be told.
    #[must_use]
    pub fn drain_actions(&mut self) -> Vec<HostAction> {
        let mut out = self.state.actions.drain();
        if let Some(stalled) = self.state.stalled.take() {
            // The action that did not fit goes at the end, in the order it
            // happened, rather than being dropped.
            out.push(stalled);
        }
        out
    }

    /// The grid being drawn on.
    #[must_use]
    pub fn grid(&self) -> &Grid {
        self.state.grid()
    }

    /// Which screen that is.
    #[must_use]
    pub const fn active(&self) -> Which {
        self.state.active
    }

    /// The scrollback.
    #[must_use]
    pub const fn scrollback(&self) -> &Scrollback {
        &self.state.scrollback
    }

    /// The scrollback, mutably — resolving a position memoizes.
    pub const fn scrollback_mut(&mut self) -> &mut Scrollback {
        &mut self.state.scrollback
    }

    /// Which modes are on.
    #[must_use]
    pub const fn modes(&self) -> &Modes {
        &self.state.modes
    }

    /// The window title, if a program set one.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.state.title.as_deref()
    }

    /// A hyperlink's target by id.
    #[must_use]
    pub fn hyperlink(&self, id: HyperlinkId) -> Option<&str> {
        self.state.hyperlinks.get(id.0 as usize).map(String::as_str)
    }

    /// One visible row as plain text.
    ///
    /// The interface `omt-agent` reads and nothing else: styling stripped,
    /// spacers and padding resolved, so a card's `1.` prefix can be checked for
    /// before a numeric accelerator is used.
    #[must_use]
    pub fn row_text(&self, row: u16) -> String {
        self.state.grid().row(row).text()
    }

    /// The whole visible screen as lines of text.
    #[must_use]
    pub fn screen_text(&self) -> Vec<String> {
        self.state.grid().rows().iter().map(Line::text).collect()
    }

    /// Resize, reflowing the primary screen's content.
    ///
    /// The alternate screen is cleared and resized without reflow: a
    /// full-screen program redraws on `SIGWINCH`, and re-wrapping the frame it
    /// happened to have drawn produces garbage. Every serious terminal does
    /// this, and it is not a shortcut.
    pub fn resize(&mut self, size: GridSize) -> ResizeReport {
        let st = &mut self.state;
        let old = st.primary.size();

        // Everything the primary screen holds becomes logical lines, is
        // re-wrapped at the new width, and comes back — rather than the grid
        // trying to join and re-split its own rows.
        let used = used_height(&st.primary);
        let rows: Vec<Line> = st.primary.rows()[..used as usize].to_vec();
        st.scrollback.push_rows(&rows);

        let mut fresh = Grid::new(size);
        fresh.cursor = st.primary.cursor;
        let logical = st.scrollback.take_last(size.rows as usize);
        let mut wrapped: Vec<Line> = Vec::new();
        for l in &logical {
            wrapped.extend(crate::scrollback::wrap_line(l, size.cols));
        }
        // Anything that no longer fits goes back where it came from, so
        // shrinking a window scrolls content up rather than destroying it.
        let overflow = wrapped.len().saturating_sub(size.rows as usize);
        if overflow > 0 {
            let kept: Vec<Line> = wrapped.drain(..overflow).collect();
            st.scrollback.push_rows(&kept);
        }
        let placed = wrapped.len();
        for (i, l) in wrapped.into_iter().enumerate() {
            *fresh.row_mut(u16::try_from(i).unwrap_or(u16::MAX)) = l;
        }
        fresh.cursor.row = placed.saturating_sub(1).min(size.rows as usize - 1) as u16;
        fresh.cursor.col = fresh.cursor.col.min(size.cols - 1);
        st.primary = fresh;

        st.alternate = Grid::new(size);
        st.tab_stops = default_tab_stops(size.cols);

        ResizeReport {
            from: old,
            to: size,
            reflowed: size.cols != old.cols,
        }
    }
}

/// What a resize did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeReport {
    /// The old size.
    pub from: GridSize,
    /// The new one.
    pub to: GridSize,
    /// Whether content was re-wrapped, which only a width change causes.
    pub reflowed: bool,
}

/// The last non-empty row, plus one.
fn used_height(g: &Grid) -> u16 {
    let mut used = 0;
    for (i, row) in g.rows().iter().enumerate() {
        if !row.cells().is_empty() {
            used = u16::try_from(i).unwrap_or(u16::MAX) + 1;
        }
    }
    used.max(g.cursor.row + 1)
}

fn default_tab_stops(cols: u16) -> Vec<bool> {
    (0..cols).map(|c| c > 0 && c % 8 == 0).collect()
}

impl State {
    fn grid(&self) -> &Grid {
        match self.active {
            Which::Primary => &self.primary,
            Which::Alternate => &self.alternate,
        }
    }

    fn grid_mut(&mut self) -> &mut Grid {
        match self.active {
            Which::Primary => &mut self.primary,
            Which::Alternate => &mut self.alternate,
        }
    }

    fn emit(&mut self, action: HostAction) {
        if let Backpressure::Stop(a) = self.actions.push(action) {
            self.stalled = Some(a);
        }
    }

    /// File a row that left the top of the screen.
    ///
    /// Only the primary screen has anywhere to file it. The alternate screen
    /// scrolling is a program redrawing its own frame, and storing those rows
    /// would fill scrollback with the intermediate states of a text editor.
    fn retire(&mut self, rows: Vec<Line>) {
        if self.active == Which::Primary && !rows.is_empty() {
            self.scrollback.push_rows(&rows);
        }
    }

    fn linefeed(&mut self) {
        let g = self.grid_mut();
        if g.cursor.row == g.margins.bottom {
            let off = g.scroll_up(1);
            self.retire(off);
        } else {
            g.move_by(1, 0);
        }
    }

    fn set_mode(&mut self, mode: TermMode, enabled: bool) {
        if enabled {
            self.modes.set.insert(mode);
        } else {
            self.modes.set.remove(&mode);
        }
        self.emit(HostAction::ModeChanged { mode, enabled });
    }

    fn switch_screen(&mut self, to: Which) {
        if self.active == to {
            return;
        }
        if to == Which::Alternate {
            self.alternate = Grid::new(self.primary.size());
            self.alternate.cursor.fg = self.primary.cursor.fg;
            self.alternate.cursor.bg = self.primary.cursor.bg;
        }
        self.active = to;
        self.set_mode(TermMode::AlternateScreen, to == Which::Alternate);
    }

    fn apply_sgr(&mut self, params: &Params) {
        let mut iter = params.iter();
        if params.is_empty() {
            self.reset_sgr();
            return;
        }
        while let Some(p) = iter.next() {
            let Some(&n) = p.first() else { continue };
            match n {
                0 => self.reset_sgr(),
                1 => self.cursor_flags(Flags::BOLD, true),
                2 => self.cursor_flags(Flags::FAINT, true),
                3 => self.cursor_flags(Flags::ITALIC, true),
                4 => {
                    // The sub-parameter form 4:3 selects a style; a bare 4 is
                    // a single underline.
                    let style = p.get(1).copied().unwrap_or(1);
                    let u = Underline::from_style(u8::try_from(style).unwrap_or(1));
                    let g = self.grid_mut();
                    g.cursor.flags = g.cursor.flags.with_underline(u);
                }
                5 | 6 => self.cursor_flags(Flags::BLINK, true),
                7 => self.cursor_flags(Flags::INVERSE, true),
                8 => self.cursor_flags(Flags::INVISIBLE, true),
                9 => self.cursor_flags(Flags::STRIKETHROUGH, true),
                21 => {
                    let g = self.grid_mut();
                    g.cursor.flags = g.cursor.flags.with_underline(Underline::Double);
                }
                22 => self.cursor_flags(Flags::BOLD | Flags::FAINT, false),
                23 => self.cursor_flags(Flags::ITALIC, false),
                24 => {
                    let g = self.grid_mut();
                    g.cursor.flags = g.cursor.flags.with_underline(Underline::None);
                }
                25 => self.cursor_flags(Flags::BLINK, false),
                27 => self.cursor_flags(Flags::INVERSE, false),
                28 => self.cursor_flags(Flags::INVISIBLE, false),
                29 => self.cursor_flags(Flags::STRIKETHROUGH, false),
                30..=37 => self.grid_mut().cursor.fg = Color::indexed((n - 30) as u8),
                38 => {
                    if let Some(c) = parse_extended(p, &mut iter) {
                        self.grid_mut().cursor.fg = c;
                    }
                }
                39 => self.grid_mut().cursor.fg = Color::DEFAULT,
                40..=47 => self.grid_mut().cursor.bg = Color::indexed((n - 40) as u8),
                48 => {
                    if let Some(c) = parse_extended(p, &mut iter) {
                        self.grid_mut().cursor.bg = c;
                    }
                }
                49 => self.grid_mut().cursor.bg = Color::DEFAULT,
                90..=97 => self.grid_mut().cursor.fg = Color::indexed((n - 90 + 8) as u8),
                100..=107 => self.grid_mut().cursor.bg = Color::indexed((n - 100 + 8) as u8),
                _ => {}
            }
        }
    }

    fn reset_sgr(&mut self) {
        let g = self.grid_mut();
        g.cursor.fg = Color::DEFAULT;
        g.cursor.bg = Color::DEFAULT;
        g.cursor.flags = Flags::empty();
    }

    fn cursor_flags(&mut self, f: Flags, on: bool) {
        let g = self.grid_mut();
        if on {
            g.cursor.flags.insert(f);
        } else {
            g.cursor.flags.remove(f);
        }
    }
}

/// Parse the 38/48 extended-colour forms, in both the sub-parameter and the
/// legacy semicolon spelling.
fn parse_extended(p: &[u16], iter: &mut vte::ParamsIter<'_>) -> Option<Color> {
    let mut take = |i: usize| -> Option<u16> {
        p.get(i).copied().or_else(|| {
            // The semicolon form: the values are separate parameters rather
            // than sub-parameters of this one. Both spellings are in the wild.
            iter.next().and_then(|q| q.first().copied())
        })
    };
    match take(1)? {
        2 => {
            let r = take(2)?;
            let g = take(3)?;
            let b = take(4)?;
            Some(Color::rgb(
                u8::try_from(r).unwrap_or(255),
                u8::try_from(g).unwrap_or(255),
                u8::try_from(b).unwrap_or(255),
            ))
        }
        5 => Some(Color::indexed(u8::try_from(take(2)?).unwrap_or(0))),
        _ => None,
    }
}

fn arg(params: &Params, i: usize, default: u16) -> u16 {
    params
        .iter()
        .nth(i)
        .and_then(|p| p.first().copied())
        .filter(|v| *v != 0)
        .unwrap_or(default)
}

fn arg0(params: &Params, i: usize) -> u16 {
    params
        .iter()
        .nth(i)
        .and_then(|p| p.first().copied())
        .unwrap_or(0)
}

impl Perform for State {
    fn print(&mut self, c: char) {
        // A program produced something. Only printables count: a cursor move
        // is not output, and treating it as one would open a block every time
        // a status line repainted itself.
        self.wrote_output = true;
        let width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if width == 0 {
            // A combining mark belongs to the character before it. Dropping it
            // is wrong, but so is giving it a cell of its own; interning is a
            // future change, and until then the base character stands.
            return;
        }
        let g = self.grid_mut();
        let cell = Cell::new(c, g.cursor.fg, g.cursor.bg, g.cursor.flags);
        // A line long enough to wrap past the bottom scrolls exactly as a
        // newline does, and what leaves the top has to be filed the same way.
        let scrolled = g.put(cell, u16::try_from(width).unwrap_or(1));
        self.retire(scrolled);
        if let Some((id, row, start)) = self.open_link {
            let g = self.grid_mut();
            let col = g.cursor.col;
            if g.cursor.row == row && col > start {
                g.row_mut(row).set_hyperlink(start..col, id);
            }
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => self.emit(HostAction::Bell),
            0x08 => {
                let g = self.grid_mut();
                g.move_by(0, -1);
            }
            0x09 => {
                let g = self.grid_mut();
                let from = g.cursor.col;
                let cols = g.size().cols;
                let next = (from + 1..cols)
                    .find(|c| self.tab_stops.get(*c as usize).copied().unwrap_or(false))
                    .unwrap_or(cols - 1);
                let g = self.grid_mut();
                let template = Cell::new(' ', g.cursor.fg, g.cursor.bg, g.cursor.flags);
                for c in from..next {
                    g.row_mut(g.cursor.row).set(c, template);
                }
                let row = g.cursor.row;
                // Recorded as a run so a copy restores the tab rather than the
                // spaces that were drawn for it.
                g.row_mut(row).mark_tab_run(from, next);
                g.goto(row, next);
            }
            0x0a..=0x0c => self.linefeed(),
            0x0d => {
                let g = self.grid_mut();
                let row = g.cursor.row;
                g.goto(row, 0);
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, c: char) {
        let private = intermediates.first() == Some(&b'?');
        match c {
            'A' => self.grid_mut().move_by(-i32::from(arg(params, 0, 1)), 0),
            'B' | 'e' => self.grid_mut().move_by(i32::from(arg(params, 0, 1)), 0),
            'C' | 'a' => self.grid_mut().move_by(0, i32::from(arg(params, 0, 1))),
            'D' => self.grid_mut().move_by(0, -i32::from(arg(params, 0, 1))),
            'E' => {
                let n = i32::from(arg(params, 0, 1));
                let g = self.grid_mut();
                g.move_by(n, 0);
                let row = g.cursor.row;
                g.goto(row, 0);
            }
            'F' => {
                let n = i32::from(arg(params, 0, 1));
                let g = self.grid_mut();
                g.move_by(-n, 0);
                let row = g.cursor.row;
                g.goto(row, 0);
            }
            'G' | '`' => {
                let col = arg(params, 0, 1) - 1;
                let g = self.grid_mut();
                let row = g.cursor.row;
                g.goto(row, col);
            }
            'H' | 'f' => {
                let row = arg(params, 0, 1) - 1;
                let col = arg(params, 1, 1) - 1;
                self.grid_mut().goto(row, col);
            }
            'J' => {
                let extent = match arg0(params, 0) {
                    1 => EraseExtent::ToStart,
                    2 | 3 => EraseExtent::All,
                    _ => EraseExtent::ToEnd,
                };
                let old = self.grid_mut().erase_screen(extent);
                // `clear` should push the screen into scrollback, not destroy
                // it — the user scrolls up expecting to find it.
                self.retire(old);
            }
            'K' => {
                let extent = match arg0(params, 0) {
                    1 => EraseExtent::ToStart,
                    2 => EraseExtent::All,
                    _ => EraseExtent::ToEnd,
                };
                self.grid_mut().erase_line(extent);
            }
            'L' => {
                let n = arg(params, 0, 1);
                self.grid_mut().scroll_down(n);
            }
            'M' => {
                let n = arg(params, 0, 1);
                let off = self.grid_mut().scroll_up(n);
                self.retire(off);
            }
            'P' => self.grid_mut().delete_cells(arg(params, 0, 1)),
            '@' => self.grid_mut().insert_cells(arg(params, 0, 1)),
            'X' => {
                // ECH erases in place without moving anything.
                let n = arg(params, 0, 1);
                let g = self.grid_mut();
                let template = g.cursor.erase_cell();
                let (row, col) = (g.cursor.row, g.cursor.col);
                let cols = g.size().cols;
                for c in col..(col + n).min(cols) {
                    g.row_mut(row).set(c, template);
                }
            }
            'S' => {
                let n = arg(params, 0, 1);
                let off = self.grid_mut().scroll_up(n);
                self.retire(off);
            }
            'T' => {
                let n = arg(params, 0, 1);
                self.grid_mut().scroll_down(n);
            }
            'd' => {
                let row = arg(params, 0, 1) - 1;
                let g = self.grid_mut();
                let col = g.cursor.col;
                g.goto(row, col);
            }
            'g' => {
                let cols = self.grid().size().cols;
                match arg0(params, 0) {
                    3 => self.tab_stops = vec![false; cols as usize],
                    _ => {
                        let col = self.grid().cursor.col as usize;
                        if let Some(s) = self.tab_stops.get_mut(col) {
                            *s = false;
                        }
                    }
                }
            }
            'h' | 'l' => {
                let enabled = c == 'h';
                for p in params.iter() {
                    let Some(&n) = p.first() else { continue };
                    self.dec_mode(n, enabled, private);
                }
            }
            'm' => self.apply_sgr(params),
            'n' => {
                if arg0(params, 0) == 6 {
                    let g = self.grid();
                    let reply =
                        format!("\x1b[{};{}R", g.cursor.row + 1, g.cursor.col + 1).into_bytes();
                    self.emit(HostAction::Reply(reply));
                } else if arg0(params, 0) == 5 {
                    self.emit(HostAction::Reply(b"\x1b[0n".to_vec()));
                }
            }
            'r' => {
                let rows = self.grid().size().rows;
                let top = arg(params, 0, 1) - 1;
                let bottom = arg(params, 1, rows) - 1;
                let g = self.grid_mut();
                if top < bottom && bottom < rows {
                    g.margins.top = top;
                    g.margins.bottom = bottom;
                }
                g.goto(0, 0);
            }
            's' => self.grid_mut().save_cursor(),
            'u' => self.grid_mut().restore_cursor(),
            'c' => {
                // Primary DA: a VT220 with the features this actually has.
                self.emit(HostAction::Reply(b"\x1b[?62;22c".to_vec()));
            }
            't' => {
                let op = match arg0(params, 0) {
                    8 => WindowOp::RequestResize {
                        rows: arg0(params, 1),
                        cols: arg0(params, 2),
                    },
                    13 => WindowOp::ReportPosition,
                    18 => WindowOp::ReportTextAreaSize,
                    other => WindowOp::Other(other),
                };
                self.emit(HostAction::WindowOp(op));
            }
            _ => self.emit(HostAction::Warn(Warning::Unsupported {
                what: format!("CSI {c}"),
            })),
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates.first(), byte) {
            (None, b'D') => self.linefeed(),
            (None, b'E') => {
                self.linefeed();
                let g = self.grid_mut();
                let row = g.cursor.row;
                g.goto(row, 0);
            }
            (None, b'M') => {
                let g = self.grid_mut();
                if g.cursor.row == g.margins.top {
                    g.scroll_down(1);
                } else {
                    g.move_by(-1, 0);
                }
            }
            (None, b'H') => {
                let col = self.grid().cursor.col as usize;
                if let Some(s) = self.tab_stops.get_mut(col) {
                    *s = true;
                }
            }
            (None, b'7') => self.grid_mut().save_cursor(),
            (None, b'8') => self.grid_mut().restore_cursor(),
            (None, b'c') => {
                // RIS. The scrollback survives: a program resetting the
                // terminal has not asked to destroy the user's history.
                self.primary.hard_reset();
                self.alternate.hard_reset();
                self.active = Which::Primary;
                self.modes = Modes::default();
                self.tab_stops = default_tab_stops(self.primary.size().cols);
            }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        let Some(&code) = params.first() else { return };
        let text = |i: usize| -> String {
            params
                .get(i)
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default()
        };
        match code {
            b"0" | b"2" => {
                let t = text(1);
                self.title = Some(t.clone());
                self.emit(HostAction::SetTitle(t));
            }
            b"1" => self.emit(HostAction::SetIconName(text(1))),
            b"7" => self.emit(HostAction::SetCwd { url: text(1) }),
            b"8" => {
                // OSC 8 ; params ; uri
                let uri = text(2);
                if uri.is_empty() {
                    self.open_link = None;
                    self.emit(HostAction::Hyperlink(HyperlinkEvent::Closed));
                } else {
                    let id = HyperlinkId(u32::try_from(self.hyperlinks.len()).unwrap_or(0));
                    self.hyperlinks.push(uri.clone());
                    let g = self.grid();
                    self.open_link = Some((id, g.cursor.row, g.cursor.col));
                    let explicit = text(1);
                    self.emit(HostAction::Hyperlink(HyperlinkEvent::Opened {
                        id: explicit
                            .strip_prefix("id=")
                            .map(std::borrow::ToOwned::to_owned),
                        uri,
                    }));
                }
            }
            b"52" => {
                let selection = if text(1).starts_with('p') {
                    ClipboardSelection::Primary
                } else {
                    ClipboardSelection::Clipboard
                };
                let payload = text(2);
                if payload == "?" {
                    self.emit(HostAction::ClipboardRead { selection });
                } else {
                    // Decoded here, honoured by policy elsewhere: a program
                    // must not be able to set the clipboard unobserved.
                    let data = decode_base64(payload.as_bytes());
                    self.emit(HostAction::ClipboardWrite { selection, data });
                }
            }
            b"133" => {
                let event = match text(1).as_bytes().first() {
                    Some(b'A') => Some(BlockEvent::PromptStart),
                    Some(b'B') => Some(BlockEvent::CommandStart),
                    Some(b'C') => Some(BlockEvent::OutputStart),
                    Some(b'D') => Some(BlockEvent::CommandEnd {
                        exit_code: params
                            .get(2)
                            .and_then(|b| std::str::from_utf8(b).ok())
                            .and_then(|s| s.parse().ok()),
                    }),
                    _ => None,
                };
                if let Some(e) = event {
                    self.pending_blocks.push(e);
                    self.emit(HostAction::Block(e));
                }
            }
            b"1337" => {
                let body = text(1);
                if let Some(rest) = body.strip_prefix("SetUserVar=")
                    && let Some((key, value)) = rest.split_once('=')
                {
                    self.emit(HostAction::SetUserVar {
                        key: key.to_owned(),
                        value: String::from_utf8_lossy(&decode_base64(value.as_bytes()))
                            .into_owned(),
                    });
                }
            }
            b"4" => {
                if let Ok(i) = text(1).parse::<u8>() {
                    self.emit(HostAction::SetColor {
                        slot: ColorSlot::Indexed(i),
                        color: parse_x_color(&text(2)),
                    });
                }
            }
            b"10" | b"11" | b"12" => {
                let slot = match code {
                    b"10" => ColorSlot::Foreground,
                    b"11" => ColorSlot::Background,
                    _ => ColorSlot::Cursor,
                };
                self.emit(HostAction::SetColor {
                    slot,
                    color: parse_x_color(&text(1)),
                });
            }
            _ => {}
        }
    }
}

impl State {
    fn dec_mode(&mut self, n: u16, enabled: bool, private: bool) {
        if !private {
            // The only ANSI mode that matters here.
            if n == 4 {
                // IRM: insert mode. Modelled as a cursor property rather than
                // a queued action, since nothing outside the core cares.
            }
            return;
        }
        match n {
            1 => self.set_mode(TermMode::ApplicationCursor, enabled),
            3 => {
                // DECCOLM: the program is changing the width itself, so the
                // host must push the new size to the PTY rather than the other
                // way round.
                let rows = self.grid().size().rows;
                let cols = if enabled { 132 } else { 80 };
                let size = GridSize::new(cols, rows);
                self.primary = Grid::new(size);
                self.alternate = Grid::new(size);
                self.emit(HostAction::NotifyResize { cols, rows });
            }
            6 => self.grid_mut().origin_mode = enabled,
            7 => self.grid_mut().autowrap = enabled,
            25 => self.grid_mut().cursor.visible = enabled,
            1000 | 1002 | 1003 => self.set_mode(TermMode::MouseTracking, enabled),
            1004 => self.set_mode(TermMode::FocusReporting, enabled),
            1006 => self.set_mode(TermMode::SgrMouse, enabled),
            1049 => {
                self.switch_screen(if enabled {
                    Which::Alternate
                } else {
                    Which::Primary
                });
            }
            2004 => self.set_mode(TermMode::BracketedPaste, enabled),
            2026 => self.set_mode(TermMode::SynchronizedOutput, enabled),
            _ => {}
        }
    }
}

/// Decode the base64 an OSC 52 carries.
///
/// Written out rather than pulled in: it is twenty lines, and a dependency in
/// the terminal core is a dependency in everything.
fn decode_base64(input: &[u8]) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut acc = 0u32;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for b in input {
        if *b == b'=' {
            break;
        }
        let Some(v) = TABLE.iter().position(|t| t == b) else {
            continue;
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out
}

/// Parse the `rgb:RRRR/GGGG/BBBB` and `#RRGGBB` spellings X uses.
fn parse_x_color(s: &str) -> Option<Color> {
    if let Some(rest) = s.strip_prefix("rgb:") {
        let mut parts = rest.split('/');
        let mut take = || -> Option<u8> {
            let p = parts.next()?;
            let v = u32::from_str_radix(p, 16).ok()?;
            // Components may be 1 to 4 hex digits wide; scale to eight bits.
            let scaled = match p.len() {
                1 => v * 17,
                2 => v,
                3 => v >> 4,
                _ => v >> 8,
            };
            u8::try_from(scaled).ok()
        };
        return Some(Color::rgb(take()?, take()?, take()?));
    }
    if let Some(hex) = s.strip_prefix('#')
        && hex.len() == 6
    {
        let v = u32::from_str_radix(hex, 16).ok()?;
        return Some(Color::rgb(
            ((v >> 16) & 0xff) as u8,
            ((v >> 8) & 0xff) as u8,
            (v & 0xff) as u8,
        ));
    }
    None
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {

    #[test]
    fn real_shell_integration_bytes_produce_a_block() {
        // End to end from the wire: OSC 133 A/B/C/D through the parser, out as
        // a block with its command and its exit code. Everything else about
        // the tracker is tested against transitions; this is the part that
        // proves the transitions arrive at all.
        let mut t = Terminal::new(TermConfig::default());
        t.advance(b"\x1b]133;A\x07$ \x1b]133;B\x07cargo test\x1b]133;C\x07");
        t.advance(b"ok\r\n");
        t.advance(b"\x1b]133;D;0\x07");

        let blocks = t.blocks();
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert_eq!(blocks[0].outcome, crate::block::Outcome::Exited { code: 0 });
        assert!(t.active_block().is_none(), "the block never closed");
    }

    #[test]
    fn a_failing_command_is_visible_as_one() {
        let mut t = Terminal::new(TermConfig::default());
        t.advance(b"\x1b]133;A\x07\x1b]133;C\x07");
        t.advance(b"\x1b]133;D;101\x07");
        assert!(t.blocks()[0].outcome.is_failure());
    }

    #[test]
    fn a_shell_without_marks_still_produces_a_block_from_its_output() {
        // What `ssh` to a host with no shell integration looks like. It used to
        // produce nothing, so the session appeared empty — the exact case
        // another terminal's EarlyOutput exists for.
        let mut t = Terminal::new(TermConfig::default());
        t.advance(b"$ ls\r\nfile\r\n$ ");
        let blocks = t.blocks();
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert!(!blocks[0].attributed, "a command was invented for it");
        assert_eq!(blocks[0].command, "", "a command was guessed from the screen");
    }

    #[test]
    fn a_cursor_move_alone_does_not_open_a_block() {
        // A status line repainting itself is not a command running, and a
        // block per repaint would bury every real one.
        let mut t = Terminal::new(TermConfig::default());
        t.advance(b"\x1b[5;1H\x1b[2K");
        assert!(t.blocks().is_empty(), "{:?}", t.blocks());
    }

    #[test]
    fn a_marked_session_does_not_also_get_early_blocks() {
        // Otherwise every command would produce two: the marked one and an
        // unattributed twin.
        let mut t = Terminal::new(TermConfig::default());
        t.advance(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07");
        t.advance(b"file\r\n");
        t.advance(b"\x1b]133;D;0\x07");
        assert_eq!(t.blocks().len(), 1, "{:?}", t.blocks());
        assert!(t.blocks()[0].attributed);
    }

    #[test]
    fn a_running_command_is_reported_as_running() {
        let mut t = Terminal::new(TermConfig::default());
        t.advance(b"\x1b]133;A\x07\x1b]133;C\x07");
        assert!(t.active_block().is_some(), "nothing was running");
    }

    use super::*;

    fn term(cols: u16, rows: u16) -> Terminal {
        Terminal::new(TermConfig {
            size: GridSize::new(cols, rows),
            ..TermConfig::default()
        })
    }

    fn feed(t: &mut Terminal, s: &str) {
        t.advance(s.as_bytes());
    }

    #[test]
    fn plain_text_lands_on_the_screen() {
        let mut t = term(20, 3);
        feed(&mut t, "hello");
        assert_eq!(t.row_text(0), "hello");
    }

    #[test]
    fn a_newline_moves_down_and_a_return_moves_to_the_left() {
        let mut t = term(20, 3);
        feed(&mut t, "one\r\ntwo");
        assert_eq!(t.row_text(0), "one");
        assert_eq!(t.row_text(1), "two");
    }

    #[test]
    fn a_bare_newline_does_not_return_to_column_zero() {
        // The distinction every "why is my output staircasing" bug is about.
        let mut t = term(20, 3);
        feed(&mut t, "one\ntwo");
        assert_eq!(t.row_text(1), "   two");
    }

    #[test]
    fn scrolling_off_the_top_lands_in_scrollback() {
        let mut t = term(20, 2);
        feed(&mut t, "one\r\ntwo\r\nthree");
        assert_eq!(t.row_text(0), "two");
        assert_eq!(t.row_text(1), "three");
        assert_eq!(
            t.scrollback().line(0).expect("scrolled off").text(),
            "one",
            "and is still findable"
        );
    }

    #[test]
    fn a_wrapping_line_that_scrolls_off_is_still_filed() {
        // Wrapping past the bottom scrolls just like a newline; content lost
        // this way is content the user watched go by and cannot scroll back
        // to.
        let mut t = term(4, 2);
        feed(&mut t, "aaaabbbbccccdddd");
        assert!(
            t.scrollback().lines().any(|l| l.text().contains("aaaa")),
            "the first row reached scrollback"
        );
    }

    #[test]
    fn a_wrapped_line_is_stored_as_one_logical_line() {
        // And it is stored as content, not as the rows it happened to occupy,
        // so a later resize re-wraps it rather than preserving the old breaks.
        let mut t = term(4, 2);
        feed(&mut t, "aaaabbbbccccdddd");
        let first = t.scrollback().line(0).expect("filed").text();
        assert!(first.len() > 4, "joined rather than split: {first:?}");
    }

    #[test]
    fn cursor_positioning_is_one_based_on_the_wire() {
        let mut t = term(20, 5);
        feed(&mut t, "\x1b[3;5Hx");
        assert_eq!(t.row_text(2), "    x", "row 3 column 5 is index 2, 4");
    }

    #[test]
    fn a_zero_argument_means_one() {
        let mut t = term(20, 5);
        feed(&mut t, "\x1b[0;0Hx");
        assert_eq!(t.row_text(0), "x");
    }

    #[test]
    fn colours_apply_to_what_follows_and_reset_stops_them() {
        let mut t = term(20, 2);
        feed(&mut t, "\x1b[31mred\x1b[0mplain");
        assert_eq!(t.grid().row(0).cell(0).fg, Color::indexed(1));
        assert_eq!(t.grid().row(0).cell(3).fg, Color::DEFAULT);
    }

    #[test]
    fn truecolor_is_parsed_in_both_spellings() {
        // Both the sub-parameter and the semicolon form are in the wild, and a
        // terminal that only handles one renders half the internet grey.
        let mut t = term(20, 2);
        feed(&mut t, "\x1b[38;2;10;20;30mX");
        assert_eq!(t.grid().row(0).cell(0).fg, Color::rgb(10, 20, 30));

        let mut t = term(20, 2);
        feed(&mut t, "\x1b[38:2::10:20:30mX");
        let fg = t.grid().row(0).cell(0).fg;
        assert!(
            matches!(fg.kind(), crate::cell::ColorKind::Rgb(..)),
            "{fg:?}"
        );
    }

    #[test]
    fn a_256_colour_index_is_parsed() {
        let mut t = term(20, 2);
        feed(&mut t, "\x1b[38;5;200mX");
        assert_eq!(t.grid().row(0).cell(0).fg, Color::indexed(200));
    }

    #[test]
    fn underline_styles_come_through_the_sub_parameter_form() {
        let mut t = term(20, 2);
        feed(&mut t, "\x1b[4:3mX");
        assert_eq!(t.grid().row(0).cell(0).flags.underline(), Underline::Curly);
        feed(&mut t, "\x1b[24mY");
        assert_eq!(t.grid().row(0).cell(1).flags.underline(), Underline::None);
    }

    #[test]
    fn erasing_the_line_respects_the_current_background() {
        let mut t = term(20, 2);
        feed(&mut t, "text\x1b[41m\x1b[2K");
        assert_eq!(t.grid().row(0).cell(10).bg, Color::indexed(1));
    }

    #[test]
    fn clearing_the_screen_pushes_it_into_scrollback() {
        // The user scrolls up after `clear` expecting to find what was there.
        let mut t = term(20, 3);
        feed(&mut t, "important\r\n\x1b[2J");
        assert_eq!(t.row_text(0), "");
        assert!(
            t.scrollback().lines().any(|l| l.text() == "important"),
            "the cleared screen is still in history"
        );
    }

    #[test]
    fn the_alternate_screen_has_no_scrollback() {
        // A text editor's intermediate frames must not fill the user's
        // history.
        let mut t = term(10, 2);
        feed(&mut t, "\x1b[?1049h");
        assert_eq!(t.active(), Which::Alternate);
        let before = t.scrollback().len();
        feed(&mut t, "a\r\nb\r\nc\r\nd\r\ne");
        assert_eq!(
            t.scrollback().len(),
            before,
            "nothing the alternate screen scrolled was filed"
        );
        feed(&mut t, "\x1b[?1049l");
        assert_eq!(t.active(), Which::Primary);
    }

    #[test]
    fn switching_to_the_alternate_screen_is_announced() {
        // The mode the agent-detection downgrade keys on.
        let mut t = term(10, 2);
        feed(&mut t, "\x1b[?1049h");
        let actions = t.drain_actions();
        assert!(
            actions.iter().any(|a| matches!(
                a,
                HostAction::ModeChanged {
                    mode: TermMode::AlternateScreen,
                    enabled: true
                }
            )),
            "{actions:?}"
        );
        assert!(t.modes().contains(TermMode::AlternateScreen));
    }

    #[test]
    fn returning_from_the_alternate_screen_restores_the_primary_content() {
        let mut t = term(20, 3);
        feed(&mut t, "shell output");
        feed(&mut t, "\x1b[?1049h");
        feed(&mut t, "editor frame");
        feed(&mut t, "\x1b[?1049l");
        assert_eq!(
            t.row_text(0),
            "shell output",
            "the primary screen was untouched"
        );
    }

    #[test]
    fn bracketed_paste_is_reported_because_input_encoding_depends_on_it() {
        let mut t = term(10, 2);
        feed(&mut t, "\x1b[?2004h");
        assert!(t.modes().contains(TermMode::BracketedPaste));
        feed(&mut t, "\x1b[?2004l");
        assert!(!t.modes().contains(TermMode::BracketedPaste));
    }

    #[test]
    fn a_cursor_report_is_answered_with_bytes_the_host_writes_back() {
        let mut t = term(20, 5);
        feed(&mut t, "\x1b[3;5H\x1b[6n");
        let actions = t.drain_actions();
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, HostAction::Reply(b) if b == b"\x1b[3;5R")),
            "{actions:?}"
        );
    }

    #[test]
    fn the_title_is_reported_and_remembered() {
        let mut t = term(10, 2);
        feed(&mut t, "\x1b]0;my title\x07");
        assert_eq!(t.title(), Some("my title"));
        assert!(
            t.drain_actions()
                .iter()
                .any(|a| matches!(a, HostAction::SetTitle(s) if s == "my title"))
        );
    }

    #[test]
    fn osc_seven_reports_where_the_shell_is() {
        let mut t = term(10, 2);
        feed(&mut t, "\x1b]7;file://host/home/me\x07");
        assert!(
            t.drain_actions()
                .iter()
                .any(|a| matches!(a, HostAction::SetCwd { url } if url == "file://host/home/me"))
        );
    }

    #[test]
    fn shell_integration_marks_become_block_events() {
        // The flagship path for the block model: the shell says where the
        // prompt and the output begin, so nothing has to be guessed.
        let mut t = term(20, 3);
        feed(&mut t, "\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07");
        feed(&mut t, "out\r\n\x1b]133;D;0\x07");
        let actions = t.drain_actions();
        let blocks: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                HostAction::Block(b) => Some(*b),
                _ => None,
            })
            .collect();
        assert_eq!(
            blocks,
            [
                BlockEvent::PromptStart,
                BlockEvent::CommandStart,
                BlockEvent::OutputStart,
                BlockEvent::CommandEnd { exit_code: Some(0) },
            ]
        );
    }

    #[test]
    fn a_nonzero_exit_code_survives() {
        let mut t = term(20, 3);
        feed(&mut t, "\x1b]133;D;127\x07");
        assert!(t.drain_actions().iter().any(|a| matches!(
            a,
            HostAction::Block(BlockEvent::CommandEnd {
                exit_code: Some(127)
            })
        )));
    }

    #[test]
    fn a_hyperlink_covers_the_text_written_inside_it() {
        let mut t = term(20, 2);
        feed(
            &mut t,
            "\x1b]8;;https://example.com\x07link\x1b]8;;\x07 after",
        );
        let id = t
            .grid()
            .row(0)
            .extras()
            .expect("extras")
            .hyperlink_at(2)
            .expect("a link on the text");
        assert_eq!(t.hyperlink(id), Some("https://example.com"));
        assert!(
            t.grid()
                .row(0)
                .extras()
                .expect("extras")
                .hyperlink_at(10)
                .is_none(),
            "and not the text after it"
        );
    }

    #[test]
    fn a_clipboard_write_is_decoded_but_not_performed() {
        // The core must never touch the clipboard itself; policy lives above.
        let mut t = term(10, 2);
        feed(&mut t, "\x1b]52;c;aGVsbG8=\x07");
        let actions = t.drain_actions();
        assert!(
            actions.iter().any(|a| matches!(
                a,
                HostAction::ClipboardWrite { selection: ClipboardSelection::Clipboard, data }
                    if data == b"hello"
            )),
            "{actions:?}"
        );
    }

    #[test]
    fn a_clipboard_read_is_a_request_not_an_answer() {
        let mut t = term(10, 2);
        feed(&mut t, "\x1b]52;c;?\x07");
        assert!(
            t.drain_actions()
                .iter()
                .any(|a| matches!(a, HostAction::ClipboardRead { .. }))
        );
    }

    #[test]
    fn a_window_resize_request_is_reported_never_honoured() {
        // The size belongs to the user's layout, not to whatever is running.
        let mut t = term(20, 5);
        feed(&mut t, "\x1b[8;40;100t");
        let actions = t.drain_actions();
        assert!(actions.iter().any(|a| matches!(
            a,
            HostAction::WindowOp(WindowOp::RequestResize {
                rows: 40,
                cols: 100
            })
        )));
        assert_eq!(t.grid().size(), GridSize::new(20, 5), "and nothing moved");
    }

    #[test]
    fn a_scrolling_region_clips_the_scroll() {
        let mut t = term(10, 4);
        feed(&mut t, "a\r\nb\r\nc\r\nd");
        feed(&mut t, "\x1b[2;3r");
        feed(&mut t, "\x1b[3;1H\ne");
        assert_eq!(t.row_text(0), "a", "outside the region");
        assert_eq!(t.row_text(3), "d");
    }

    #[test]
    fn tabs_move_to_the_next_stop_and_are_recoverable() {
        let mut t = term(20, 2);
        feed(&mut t, "a\tb");
        assert_eq!(t.grid().cursor.col, 9);
        assert_eq!(t.row_text(0), "a\tb", "a copy gets the tab back");
    }

    #[test]
    fn backspace_moves_without_erasing() {
        let mut t = term(10, 2);
        feed(&mut t, "abc\x08X");
        assert_eq!(t.row_text(0), "abX");
    }

    #[test]
    fn deleting_and_inserting_characters_shift_the_line() {
        let mut t = term(10, 2);
        feed(&mut t, "abcdef\x1b[1;2H\x1b[2P");
        assert_eq!(t.row_text(0), "adef");
        feed(&mut t, "\x1b[1;2H\x1b[2@");
        assert_eq!(t.row_text(0), "a  def");
    }

    #[test]
    fn a_reset_clears_the_screen_but_not_the_history() {
        // A program resetting the terminal has not asked to destroy what the
        // user did before it ran.
        let mut t = term(10, 2);
        feed(&mut t, "one\r\ntwo\r\nthree");
        let history = t.scrollback().len();
        assert!(history > 0);
        feed(&mut t, "\x1bc");
        assert_eq!(t.row_text(0), "");
        assert_eq!(t.scrollback().len(), history, "history survived");
    }

    #[test]
    fn an_unsupported_sequence_warns_rather_than_corrupting() {
        let mut t = term(10, 2);
        feed(&mut t, "\x1b[99Z");
        let actions = t.drain_actions();
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, HostAction::Warn(Warning::Unsupported { .. }))),
            "{actions:?}"
        );
        feed(&mut t, "ok");
        assert_eq!(t.row_text(0), "ok", "and the parser kept its place");
    }

    #[test]
    fn a_split_escape_sequence_still_parses() {
        // Bytes arrive in whatever chunks the PTY felt like; a sequence split
        // across two reads must not be printed as text.
        let mut t = term(20, 3);
        t.advance(b"\x1b[3");
        t.advance(b";5Hx");
        assert_eq!(t.row_text(2), "    x");
        assert_eq!(t.row_text(0), "", "no escape bytes leaked onto the screen");
    }

    #[test]
    fn wide_characters_take_two_columns() {
        let mut t = term(10, 2);
        feed(&mut t, "漢字");
        assert_eq!(t.grid().cursor.col, 4);
        assert_eq!(t.row_text(0), "漢字");
    }

    #[test]
    fn resizing_narrower_reflows_the_content() {
        // The whole point of storing logical lines: the text is preserved, the
        // geometry is not.
        let mut t = term(20, 4);
        feed(&mut t, "the quick brown fox jumps over");
        let report = t.resize(GridSize::new(10, 4));
        assert!(report.reflowed);
        let joined: String = t.screen_text().join("");
        assert!(
            joined.contains("quick") && joined.contains("jumps"),
            "{joined:?}"
        );
    }

    #[test]
    fn resizing_taller_does_not_lose_content() {
        let mut t = term(20, 3);
        feed(&mut t, "one\r\ntwo\r\nthree");
        t.resize(GridSize::new(20, 6));
        let text = t.screen_text().join("|");
        assert!(text.contains("three"), "{text}");
    }

    #[test]
    fn a_resize_that_only_changes_height_does_not_reflow() {
        let mut t = term(20, 3);
        feed(&mut t, "text");
        assert!(!t.resize(GridSize::new(20, 8)).reflowed);
    }

    #[test]
    fn an_x_colour_is_parsed_in_both_spellings() {
        assert_eq!(parse_x_color("#ff8000"), Some(Color::rgb(255, 128, 0)));
        assert_eq!(
            parse_x_color("rgb:ffff/8080/0000"),
            Some(Color::rgb(255, 128, 0))
        );
        assert_eq!(parse_x_color("nonsense"), None);
    }

    #[test]
    fn base64_decodes_with_and_without_padding() {
        assert_eq!(decode_base64(b"aGVsbG8="), b"hello");
        assert_eq!(decode_base64(b"aGk="), b"hi");
        assert_eq!(decode_base64(b""), b"");
    }

    #[test]
    fn a_control_sequence_after_the_last_column_does_not_wrap() {
        // The deferred-wrap case, end to end through the parser.
        let mut t = term(4, 3);
        feed(&mut t, "abcd\x1b[1;1HX");
        assert_eq!(t.row_text(0), "Xbcd");
        assert_eq!(t.row_text(1), "");
    }

    #[test]
    fn a_program_changing_the_width_tells_the_host_to_push_it() {
        // DECCOLM: the core cannot resize the PTY, so it must say so.
        let mut t = term(80, 24);
        feed(&mut t, "\x1b[?3h");
        let actions = t.drain_actions();
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, HostAction::NotifyResize { cols: 132, .. })),
            "{actions:?}"
        );
    }
}
