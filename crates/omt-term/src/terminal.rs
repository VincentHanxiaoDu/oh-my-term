//! The terminal: bytes in, state and host actions out.

use vte::{Params, Perform};

use crate::action::{ActionQueue, Backpressure, HostAction, TermMode, Warning, WindowOp};
use crate::cell::{Cell, Color, Flags, Underline};
use crate::grid::{EraseExtent, Grid, GridSize};
use crate::line::{HyperlinkId, Line};
use crate::scrollback::{Scrollback, ScrollbackLimits};

/// How the terminal is configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermConfig {
    /// The starting size.
    pub size: GridSize,
    /// Scrollback caps.
    pub scrollback: ScrollbackLimits,
    /// Most host actions queued per `advance`.
    pub max_host_actions: usize,
    /// What to answer when a program asks what colour the text is, as
    /// `rgb:rrrr/gggg/bbbb`.
    ///
    /// Programs ask this to decide whether they are on a light or a dark
    /// terminal, and an agent CLI is one of the programs that does. A terminal
    /// that stays silent leaves them to guess, and the guess is wrong about
    /// half the time — which looks like the *agent* has a bug.
    pub foreground: Option<String>,
    /// The same for the background, which is the one almost everything reads.
    pub background: Option<String>,
    /// How many command blocks to keep, when the caller has an opinion.
    ///
    /// `None` is the built-in limit. A cap belongs to whoever configured it,
    /// and `omt-term` is below the layer that reads configuration — so it is
    /// passed in, which is the only direction the layering allows.
    pub max_blocks: Option<usize>,
    /// Whether OSC 133 marks are honoured. On unless somebody said otherwise.
    pub block_marks: bool,
}

impl Default for TermConfig {
    fn default() -> Self {
        Self {
            size: GridSize::new(80, 24),
            scrollback: ScrollbackLimits::default(),
            max_host_actions: 256,
            // No claim by default. omt only knows these when the terminal it
            // is itself running in has answered, and inventing a colour is
            // worse than silence: a program told the background is black on a
            // white terminal renders unreadably and has been *told* to.
            foreground: None,
            background: None,
            max_blocks: None,
            block_marks: true,
        }
    }
}

/// What marks an `OSC 777 ; notify` as a structured agent event rather than
/// something to show a person.
///
/// A URL-shaped marker rather than a number: numbers in this space are
/// allocated by whoever gets there first, and a collision would have omt
/// reading somebody else's notification as an agent's report.
pub const AGENT_EVENT_MARKER: &str = "omt://agent-event";

/// Which version of that channel this build reads.
///
/// Advertised in the environment of every pty omt spawns. An agent that has to
/// guess whether its host understands a sequence either stays silent or writes
/// JSON onto somebody's screen, and there is no reply on this channel to ask
/// with — so the answer is put where it can be read before the first byte.
pub const AGENT_EVENT_PROTOCOL: u32 = 1;

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
    /// Takes APC out of the stream before the parser can swallow it.
    graphics: crate::graphics::Scanner,
    state: State,
    /// Command blocks, positioned in absolute rows.
    ///
    /// Here rather than in `State` because a block's position depends on how
    /// much has scrolled off, and the parser has no idea.
    blocks: crate::block::BlockTracker,
    /// Where the command being typed starts on the screen, while one is.
    command_at: Option<(u64, u16)>,
    /// What changed since a renderer last looked.
    ///
    /// Here rather than in `State` for the same reason: it is derived from the
    /// screen after the parser has finished with it, by comparing line
    /// generations, which is a scan of `rows` integers rather than a
    /// comparison of cells.
    damage: crate::damage::Tracker,
}

/// Everything the parser mutates.
///
/// Split from [`Terminal`] because `vte` borrows the performer mutably while
/// the parser drives it, and the parser cannot borrow itself.
pub(crate) struct State {
    primary: Grid,
    alternate: Grid,
    active: Which,
    scrollback: Scrollback,
    actions: ActionQueue,
    modes: Modes,
    pub(crate) hyperlinks: Vec<String>,
    pub(crate) open_link: Option<(HyperlinkId, u16, u16)>,
    tab_stops: Vec<bool>,
    pub(crate) title: Option<String>,
    /// Set when the action queue refused an undroppable action, so the caller
    /// is told to drain before feeding more bytes.
    stalled: Option<HostAction>,
    /// Shell-integration transitions the terminal has not positioned yet.
    pub(crate) pending_blocks: Vec<crate::action::BlockEvent>,
    /// Set when a printable character was written since the last check.
    ///
    /// Only printables: an escape sequence that moves the cursor is not a
    /// program producing output, and treating it as one would open a block
    /// every time something repainted its status line.
    wrote_output: bool,
    /// What to answer a program that asks what colour things are.
    pub(crate) colours: (Option<String>, Option<String>),
    /// Images a program drew, against the absolute row each starts on.
    ///
    /// Kept rather than rendered: drawing one means asking the terminal omt is
    /// itself drawn on, clipped to the pane's rectangle. Knowing an image is
    /// there and how much room it takes is what everything else is built on —
    /// and before this existed, an image was nothing at all.
    pub(crate) images: Vec<(u64, crate::graphics::Image)>,
}

/// How much image payload one terminal holds, across every image in it.
///
/// Held so an image can be handed back to a terminal that can draw it. Past
/// this the oldest payloads are dropped and their cards remain, because the
/// alternative is a session that grows without limit for as long as somebody
/// keeps printing screenshots.
pub const MAX_HELD_IMAGE_BYTES: usize = 32 << 20; // 32 MiB

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

    /// Whether the program has grabbed the mouse at all.
    ///
    /// For everything that only needs to know *whether* the mouse belongs to
    /// the program. What class of event it asked for is a different question,
    /// and asking this one instead of that one is how a click-only program
    /// ends up being sent motion.
    #[must_use]
    pub fn mouse_tracking(&self) -> bool {
        self.contains(TermMode::MouseClicks)
            || self.contains(TermMode::MouseButtonMotion)
            || self.contains(TermMode::MouseAnyMotion)
    }

    /// Whether motion with a button held is wanted.
    #[must_use]
    pub fn mouse_drag(&self) -> bool {
        self.contains(TermMode::MouseButtonMotion) || self.contains(TermMode::MouseAnyMotion)
    }

    /// Every mode that is on.
    pub fn iter(&self) -> impl Iterator<Item = TermMode> + '_ {
        self.set.iter().copied()
    }
}

impl Terminal {
    /// Tell it what colours the terminal on the other end actually uses.
    ///
    /// Separate from construction because omt learns this later than it
    /// creates the emulator: the answer comes from the terminal omt is itself
    /// drawing on, or from configuration, and a session can be created before
    /// either is known.
    pub fn tell_colours(&mut self, foreground: Option<String>, background: Option<String>) {
        self.state.colours = (foreground, background);
    }

    /// A fresh terminal.
    #[must_use]
    pub fn new(config: TermConfig) -> Self {
        let size = config.size;
        Self {
            parser: vte::Parser::new(),
            graphics: crate::graphics::Scanner::default(),
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
                colours: (config.foreground.clone(), config.background.clone()),
                images: Vec::new(),
            },
            blocks: crate::block::BlockTracker::configured(config.max_blocks, config.block_marks),
            command_at: None,
            damage: crate::damage::Tracker::new(size.rows),
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
            // Before the parser, because `vte` consumes an APC string in a
            // state that makes no callback: a graphics sequence reaching it is
            // a sequence nothing can ever see.
            match self.graphics.step(*b) {
                crate::graphics::Step::Took => continue,
                crate::graphics::Step::Complete(complete) => {
                    self.state.place_image(&complete);
                    continue;
                }
                crate::graphics::Step::Pass(passed) => {
                    self.parser.advance(&mut self.state, &passed);
                    // Blocks are tracked here rather than in the parser
                    // because a block is positioned in *absolute* rows, and
                    // only this level knows how much has scrolled off. Peeked
                    // rather than drained: the host still receives every
                    // action it would have.
                    self.track_blocks();
                }
            }
        }
        self.note_damage();
        bytes.len()
    }

    /// What changed since the last time this was asked.
    ///
    /// Taking it clears it: two renderers sharing one terminal each need their
    /// own record, and handing the same one out twice would have the second
    /// redraw nothing.
    pub fn take_damage(&mut self) -> crate::damage::Damage {
        self.damage.take()
    }

    /// Fold the current screen's line generations into the damage record.
    fn note_damage(&mut self) {
        let generations: Vec<crate::line::Generation> = self
            .grid()
            .rows()
            .iter()
            .map(crate::line::Line::generation)
            .collect();
        self.damage.observe(&generations);
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

    /// One line of history, by its absolute row.
    ///
    /// Absolute because that is the coordinate a block records: a command whose
    /// output has scrolled off the screen still names the rows it wrote, and a
    /// screen-relative index would point somewhere else by the time anybody
    /// asked.
    #[must_use]
    pub fn line_at(&self, absolute: u64) -> Option<&crate::line::Line> {
        let scrolled = self.state.scrollback.len() as u64;
        if absolute < scrolled {
            return self.state.scrollback.line(absolute);
        }
        let row = u16::try_from(absolute - scrolled).ok()?;
        (row < self.grid().size().rows).then(|| self.grid().row(row))
    }

    /// The images a program has drawn, each against the absolute row it
    /// starts on.
    #[must_use]
    pub fn images(&self) -> &[(u64, crate::graphics::Image)] {
        &self.state.images
    }

    /// How many lines have ever been written, history plus screen.
    #[must_use]
    pub fn total_rows(&self) -> u64 {
        self.state.scrollback.len() as u64 + u64::from(self.grid().size().rows)
    }

    /// Feed any new shell-integration transitions to the block tracker.
    ///
    /// Through a small queue of its own rather than by inspecting the action
    /// queue: the host drains that one, and two consumers of a queue with one
    /// cursor is how one of them silently stops seeing things.
    fn track_blocks(&mut self) {
        let row = self.state.scrollback.len() as u64 + u64::from(self.grid().cursor.row);

        // The command line, read off the screen while the shell says it is
        // being typed. Nothing did this: `command_text` existed, one test
        // called it, and every block in production carried an empty command —
        // so the block view showed output with no command above it and the
        // history had nothing to record.
        //
        // Read rather than accumulated from keystrokes, and *replaced* rather
        // than appended: a line is edited before it is submitted, and
        // appending would record the backspaces along with the corrections.
        if self.blocks.collecting()
            && let Some((mark_row, mark_col)) = self.command_at
            && mark_row == row
        {
            let text = self.state.grid().row(self.grid().cursor.row).text();
            let from = usize::from(mark_col);
            let to = usize::from(self.grid().cursor.col);
            let line: String = text
                .chars()
                .skip(from)
                .take(to.saturating_sub(from))
                .collect();
            self.blocks.set_command_text(line.trim_end());
        }

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
            // Where on the screen the command begins, so the line can be read
            // back from it. Taken at the mark rather than guessed from the
            // prompt's width, which varies with everything a prompt can show.
            match event {
                crate::action::BlockEvent::CommandStart => {
                    self.command_at = Some((row, self.grid().cursor.col));
                }
                crate::action::BlockEvent::OutputStart
                | crate::action::BlockEvent::CommandEnd { .. } => self.command_at = None,
                crate::action::BlockEvent::PromptStart => {}
            }
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
        // The shape changed, so a renderer's cached geometry is about a screen
        // that no longer exists. Reported as structural rather than as "every
        // row changed", which would have it draw new content into the old
        // geometry.
        self.damage.structural(size.rows);
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
    pub(crate) fn grid(&self) -> &Grid {
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

    pub(crate) fn emit(&mut self, action: HostAction) {
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

    /// Give an image the rows it needs, and remember where it went.
    ///
    /// An image that occupies nothing is one the next line writes over, which
    /// on screen is indistinguishable from an image that never arrived. So it
    /// takes real rows, moved through exactly as a program printing that many
    /// lines would move through them — which is also what makes it scroll like
    /// everything else, because the rows *are* everything else.
    pub(crate) fn place_image(&mut self, complete: &crate::graphics::Complete) {
        let Some(image) = complete.image.clone() else {
            return;
        };
        // Absolute, so the placement still means something after the rows it
        // sits on have scrolled out of the screen and into the history.
        let started = self.scrollback.len() as u64 + u64::from(self.grid().cursor.row);
        self.images.push((started, image.clone()));
        crate::graphics::evict(&mut self.images, MAX_HELD_IMAGE_BYTES);
        if complete.keep_cursor {
            // The program asked to stay where it is: it is drawing something
            // it means to write over itself.
            return;
        }
        // Carriage return first, for the same reason a printed line ends with
        // one: what comes after an image starts at the left, not indented by
        // wherever the sequence happened to leave the cursor.
        self.grid_mut().cursor.col = 0;
        // Written into the cells, not painted over them. What is in the grid
        // scrolls, copies, seeds and reaches the web client without any of
        // those knowing an image exists; an overlay would have to be
        // reimplemented on every surface to match.
        let cols = self.grid().size().cols;
        for line in crate::graphics::card(&image, cols, image.rows) {
            for ch in line.chars() {
                self.print(ch);
            }
            self.grid_mut().cursor.col = 0;
            self.linefeed();
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
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        // The whole body lives in `osc.rs`: it is the widest surface in the
        // emulator and the file it was in went over the length this repository
        // allows. The trait stays here, where the parser is driven.
        State::osc_dispatch(self, params, bell_terminated);
    }

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
            // IL and DL act from the cursor row, not from the top of the
            // scrolling region. Scrolling the whole region instead moves the
            // rows above the cursor, so an editor deleting a line mid-file
            // redraws the screen one row off.
            'L' => {
                let n = arg(params, 0, 1);
                self.grid_mut().insert_lines(n);
            }
            'M' => {
                let n = arg(params, 0, 1);
                self.grid_mut().delete_lines(n);
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
                let was = std::mem::take(&mut self.modes);
                self.primary.hard_reset();
                self.alternate.hard_reset();
                self.active = Which::Primary;
                self.tab_stops = default_tab_stops(self.primary.size().cols);
                // Every mode a reset turned off is announced. A client tracking
                // modes by event -- which is the only way a remote one can --
                // would otherwise still believe the terminal was on the
                // alternate screen, in bracketed paste, or reporting mouse.
                for mode in was.iter() {
                    self.emit(HostAction::ModeChanged {
                        mode,
                        enabled: false,
                    });
                }
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
            1000 => self.set_mode(TermMode::MouseClicks, enabled),
            1002 => self.set_mode(TermMode::MouseButtonMotion, enabled),
            1003 => self.set_mode(TermMode::MouseAnyMotion, enabled),
            1007 => self.set_mode(TermMode::AlternateScroll, enabled),
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

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod alt_screen_tests {
    use super::*;

    fn term() -> Terminal {
        Terminal::new(TermConfig {
            size: GridSize::new(20, 6),
            ..TermConfig::default()
        })
    }

    #[test]
    fn the_primary_screens_contents_survive_the_round_trip() {
        // What 1049 promises and the reason the two grids are separate: what
        // was on screen before a full-screen program ran is still there after
        // it exits.
        let mut t = term();
        t.advance(b"keep me\r\n");
        t.advance(b"\x1b[?1049h");
        t.advance(b"scribble");
        t.advance(b"\x1b[?1049l");
        assert!(
            t.grid().row(0).text().starts_with("keep me"),
            "primary contents were lost: {:?}",
            t.grid().row(0).text()
        );
    }

    #[test]
    fn the_alternate_screen_starts_blank_every_time() {
        // Cleared on entry, so a second program does not inherit the first
        // one's screen.
        let mut t = term();
        t.advance(b"\x1b[?1049h");
        t.advance(b"first program");
        t.advance(b"\x1b[?1049l");
        t.advance(b"\x1b[?1049h");
        assert!(
            t.grid().row(0).text().trim().is_empty(),
            "the alternate screen kept the previous program's output: {:?}",
            t.grid().row(0).text()
        );
    }

    #[test]
    fn the_cursor_a_program_left_on_the_alternate_screen_is_not_the_one_that_comes_back() {
        // Each screen carries its own cursor, which is what makes 1049's
        // "restore" free here: nothing the alternate screen's program does can
        // move the primary's. Written down because the obvious alternative —
        // one shared cursor plus an explicit save/restore — is what most
        // emulators do, and somebody refactoring towards it would break this
        // without noticing.
        let mut t = term();
        t.advance(b"one\r\ntwo\r\n");
        let before = (t.grid().cursor.row, t.grid().cursor.col);
        t.advance(b"\x1b[?1049h");
        t.advance(b"\x1b[6;1Hbottom of the alt screen");
        assert_ne!(
            (t.grid().cursor.row, t.grid().cursor.col),
            before,
            "the alternate screen is sharing the primary's cursor"
        );
        t.advance(b"\x1b[?1049l");
        assert_eq!((t.grid().cursor.row, t.grid().cursor.col), before);
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
    use crate::action::{BlockEvent, ClipboardSelection};
    use crate::osc::{decode_base64, parse_x_color};

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
        // this rule exists for.
        let mut t = Terminal::new(TermConfig::default());
        t.advance(b"$ ls\r\nfile\r\n$ ");
        let blocks = t.blocks();
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert!(!blocks[0].attributed, "a command was invented for it");
        assert_eq!(
            blocks[0].command, "",
            "a command was guessed from the screen"
        );
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

    #[test]
    fn writing_to_the_screen_damages_the_row_it_wrote_to() {
        // The whole point: a renderer redraws that row and not the screen.
        let mut t = term(80, 24);
        t.advance(b"hello");
        let damage = t.take_damage();
        let crate::damage::DamageKind::Rows(rows) = damage.kind else {
            panic!("rows, got {damage:?}");
        };
        assert_eq!(rows.iter().map(|d| d.row).collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn an_untouched_screen_reports_nothing_to_draw() {
        let mut t = term(80, 24);
        t.advance(b"hello");
        t.take_damage();
        assert!(t.take_damage().is_empty(), "and it is not reported twice");
    }

    #[test]
    fn writing_on_two_rows_damages_both_and_no_others() {
        let mut t = term(80, 24);
        t.advance(b"one\r\ntwo");
        let crate::damage::DamageKind::Rows(rows) = t.take_damage().kind else {
            panic!("rows");
        };
        assert_eq!(rows.iter().map(|d| d.row).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn a_resize_is_structural_so_a_renderer_discards_its_geometry() {
        // Told "every row changed" instead, it would draw the new content into
        // the old geometry.
        let mut t = term(80, 24);
        t.advance(b"hello");
        t.take_damage();
        t.resize(GridSize { cols: 40, rows: 12 });
        let damage = t.take_damage();
        assert!(damage.is_structural(), "{damage:?}");
    }

    #[test]
    fn the_epoch_moves_across_a_resize_and_not_otherwise() {
        // A renderer holding damage from a previous epoch cannot apply it: the
        // rows it names are not the rows that exist now.
        let mut t = term(80, 24);
        t.advance(b"hello");
        let before = t.take_damage().epoch;
        t.advance(b" again");
        assert_eq!(t.take_damage().epoch, before);
        t.resize(GridSize { cols: 40, rows: 12 });
        assert!(t.take_damage().epoch > before);
    }

    #[test]
    fn a_screenful_of_output_degrades_to_a_redraw_rather_than_a_list() {
        // A record larger than the screen it describes is not worth sending.
        let mut t = term(80, 24);
        for i in 0..200 {
            t.advance(format!("line {i}\r\n").as_bytes());
        }
        let kind = t.take_damage().kind;
        assert!(
            matches!(
                kind,
                crate::damage::DamageKind::Full | crate::damage::DamageKind::Rows(_)
            ),
            "{kind:?}"
        );
    }
    #[test]
    fn a_program_asking_what_colour_the_background_is_gets_an_answer() {
        // Agent CLIs ask this to decide whether they are on a light or a dark
        // terminal. A terminal that stays silent leaves them to guess, and the
        // guess is wrong about half the time — which looks like the *agent*
        // has the bug.
        let mut t = Terminal::new(TermConfig {
            size: GridSize::new(10, 2),
            background: Some("rgb:1111/2222/3333".to_owned()),
            foreground: Some("rgb:eeee/eeee/eeee".to_owned()),
            ..TermConfig::default()
        });
        feed(&mut t, "\x1b]11;?\x1b\\");
        let replied = t.drain_actions().into_iter().find_map(|a| match a {
            HostAction::Reply(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
            _ => None,
        });
        assert_eq!(replied.as_deref(), Some("\x1b]11;rgb:1111/2222/3333\x1b\\"));
    }

    #[test]
    fn a_terminal_that_does_not_know_its_colours_says_nothing() {
        // Inventing one is worse than silence: a program told the background
        // is black on a white terminal renders unreadably, and has been *told*
        // to do it.
        let mut t = term(10, 2);
        feed(&mut t, "\x1b]11;?\x1b\\");
        assert!(
            !t.drain_actions()
                .iter()
                .any(|a| matches!(a, HostAction::Reply(_)))
        );
    }

    #[test]
    fn setting_a_colour_is_not_a_question_and_is_not_answered() {
        // `OSC 11 ; <colour>` sets; only `?` asks. Answering a set would send
        // bytes to a program that is not reading a reply.
        let mut t = Terminal::new(TermConfig {
            size: GridSize::new(10, 2),
            background: Some("rgb:0000/0000/0000".to_owned()),
            ..TermConfig::default()
        });
        feed(&mut t, "\x1b]11;rgb:ffff/ffff/ffff\x1b\\");
        assert!(
            !t.drain_actions()
                .iter()
                .any(|a| matches!(a, HostAction::Reply(_)))
        );
    }
    #[test]
    fn a_program_can_ask_for_a_notification() {
        // A long build in a pane nobody is watching. omt is the terminal it is
        // talking to, so omt is what has to pass it on rather than draw it.
        let mut t = term(10, 2);
        feed(&mut t, "\x1b]9;build finished\x07");
        assert!(t.drain_actions().iter().any(|a| matches!(
            a,
            HostAction::Notify { title: None, body } if body == "build finished"
        )));
    }

    #[test]
    fn a_notification_with_a_title_keeps_them_apart() {
        let mut t = term(10, 2);
        feed(&mut t, "\x1b]777;notify;tests;42 passed\x07");
        assert!(t.drain_actions().iter().any(|a| matches!(
            a,
            HostAction::Notify { title: Some(title), body } if title == "tests" && body == "42 passed"
        )));
    }

    #[test]
    fn an_agent_can_report_over_the_pty_when_it_has_no_hook() {
        // The same shape a hook reports, arriving on the pty instead of a
        // socket. Weaker evidence, and the tier says so — but legible, where
        // before it was a notification with JSON in it shown to a person.
        let mut t = term(10, 2);
        feed(
            &mut t,
            "\x1b]777;notify;omt://agent-event;{\"event\":\"PreToolUse\";\"tool_name\":\"Bash\"}\x07",
        );
        let reported = t.drain_actions().into_iter().find_map(|a| match a {
            HostAction::AgentEvent(json) => Some(json),
            _ => None,
        });
        // Rejoined across the semicolons the parser splits on, which JSON is
        // full of — split naively, every such report arrives truncated at the
        // first field.
        assert_eq!(
            reported.as_deref(),
            Some("{\"event\":\"PreToolUse\";\"tool_name\":\"Bash\"}")
        );
    }

    #[test]
    fn an_empty_notification_is_not_raised() {
        // A sequence with nothing in it is a program clearing a notification
        // or getting it wrong; either way there is nothing to say.
        let mut t = term(10, 2);
        feed(&mut t, "\x1b]9;\x07");
        assert!(
            !t.drain_actions()
                .iter()
                .any(|a| matches!(a, HostAction::Notify { .. }))
        );
    }
    #[test]
    fn a_block_carries_the_command_that_was_typed() {
        // `command_text` existed and one test called it; nothing in production
        // did, so every block carried an empty command — the block view showed
        // output with no command above it, and history had nothing to record.
        let mut t = term(40, 4);
        feed(
            &mut t,
            "\x1b]133;A\x07$ \x1b]133;B\x07cargo test\x1b]133;C\x07",
        );
        feed(&mut t, "ok\r\n\x1b]133;D;0\x07");
        assert_eq!(t.blocks()[0].command, "cargo test");
    }

    #[test]
    fn a_corrected_command_is_recorded_as_the_line_that_ran() {
        // The line is read off the screen as it changes rather than
        // accumulated from keystrokes: appending would record the mistake, the
        // backspaces and the correction as one command.
        let mut t = term(40, 4);
        feed(&mut t, "\x1b]133;A\x07$ \x1b]133;B\x07lx");
        // A backspace and the rest of the word, as a shell redraws it.
        feed(&mut t, "\x08s -la\x1b]133;C\x07");
        feed(&mut t, "\x1b]133;D;0\x07");
        assert_eq!(t.blocks()[0].command, "ls -la");
    }

    #[test]
    fn a_prompt_with_no_command_after_it_records_none() {
        // Pressing Enter on an empty line is not a command, and a history full
        // of blank entries is worse than a short one.
        let mut t = term(40, 4);
        feed(&mut t, "\x1b]133;A\x07$ \x1b]133;B\x07\x1b]133;C\x07");
        feed(&mut t, "\x1b]133;D;0\x07");
        assert_eq!(t.blocks()[0].command, "");
    }

    #[test]
    fn the_three_tracking_modes_are_three_different_requests() {
        // Collapsed into one flag, a program that asked for clicks gets sent
        // motion — input it never asked for, arriving in the middle of
        // whatever it was parsing.
        let mut t = term(40, 4);
        feed(&mut t, "\x1b[?1000h");
        assert!(t.modes().contains(TermMode::MouseClicks));
        assert!(t.modes().mouse_tracking(), "the mouse is grabbed");
        assert!(!t.modes().mouse_drag(), "but motion was not asked for");

        let mut t = term(40, 4);
        feed(&mut t, "\x1b[?1002h");
        assert!(t.modes().mouse_drag());
        assert!(!t.modes().contains(TermMode::MouseClicks));
    }

    #[test]
    fn alternate_scroll_is_asked_not_assumed() {
        // A full-screen program that did not set 1007 and is sent arrow keys
        // is an editor whose cursor moves when somebody scrolls.
        let mut t = term(40, 4);
        feed(&mut t, "\x1b[?1049h");
        assert!(!t.modes().contains(TermMode::AlternateScroll));
        feed(&mut t, "\x1b[?1007h");
        assert!(t.modes().contains(TermMode::AlternateScroll));
        feed(&mut t, "\x1b[?1007l");
        assert!(!t.modes().contains(TermMode::AlternateScroll));
    }
}
