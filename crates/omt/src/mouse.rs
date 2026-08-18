//! Whose event a pointer event is.
//!
//! The rule omt's documents have carried since before any of this existed:
//! **the program in the pane owns the click.** vim, helix, htop, lazygit and
//! essentially every agent's own interface ask for the mouse, and a
//! multiplexer that swallows their clicks so its own features can run has
//! broken all of them. No host feature is worth that.
//!
//! What omt keeps is the part no program asked for: events outside any pane's
//! grid, and events held with Shift — which every terminal omt runs *inside*
//! already treats as "this one is for the multiplexer", which is precisely why
//! applications do not expect to receive them.

use omt_input::mouse::{Button, Encoding, Kind, MouseEvent, Wheel};
use omt_term::TermMode;

/// What the pane's program has asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Asked {
    /// Any of 1000/1002/1003.
    pub tracking: bool,
    /// 1002 or 1003: motion while a button is held.
    pub drag: bool,
    /// 1006, and so how a report should be spelled.
    pub sgr: bool,
    /// The alternate screen is up.
    pub alternate: bool,
    /// 1007: the wheel should become arrow keys.
    pub alternate_scroll: bool,
}

impl Asked {
    /// Read it off a terminal's modes.
    #[must_use]
    pub fn of(terminal: &omt_term::Terminal) -> Self {
        let modes = terminal.modes();
        Self {
            tracking: modes.mouse_tracking(),
            drag: modes.mouse_drag(),
            sgr: modes.contains(TermMode::SgrMouse),
            alternate: modes.contains(TermMode::AlternateScreen),
            alternate_scroll: modes.contains(TermMode::AlternateScroll),
        }
    }
}

/// What omt should do with one pointer event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Bytes for the program in the pane.
    Forward(Vec<u8>),
    /// omt's own: begin, extend or end a selection.
    Select(Kind),
    /// omt's own: move through its scrollback, by this many lines.
    ///
    /// Negative is towards the top, which is where the wheel is pushed to see
    /// what has already gone past.
    Scroll(i16),
    /// Nothing — an event nobody asked for, and not one omt has a use for.
    Ignore,
}

/// How many lines one notch of the wheel moves omt's own scrollback.
pub const WHEEL_LINES: i16 = 3;

/// Decide where one event goes.
///
/// `inside` says whether the pointer is over the pane's grid at all; chrome is
/// omt's by construction, and a program cannot be sent a click on a status bar
/// it does not know exists.
#[must_use]
pub fn route(event: &MouseEvent, asked: Asked, inside: bool) -> Route {
    if !inside {
        return omt_own(event, asked);
    }
    // The convention, one level down. Every terminal omt runs inside lets
    // Shift select through a program that has grabbed the mouse, which is
    // exactly why applications do not expect a shifted click — and so why it
    // is available to take.
    if event.mods.shift {
        return omt_own(event, asked);
    }
    if let Button::Wheel(direction) = event.button {
        return wheel(event, asked, direction);
    }
    if !asked.tracking {
        return omt_own(event, asked);
    }
    // A program that asked for presses and releases is not asking for the
    // pointer's whole path. Sending it anyway is input it did not request,
    // arriving in the middle of whatever it was parsing.
    if matches!(event.kind, Kind::Drag | Kind::Motion) && !asked.drag {
        return Route::Ignore;
    }
    // Motion with nothing held is only ever wanted by 1003, and `drag` alone
    // does not imply it.
    if event.kind == Kind::Motion && !asked.drag {
        return Route::Ignore;
    }
    forward(event, asked)
}

/// Encode for the program, in the spelling it asked for.
fn forward(event: &MouseEvent, asked: Asked) -> Route {
    let encoding = if asked.sgr {
        Encoding::Sgr
    } else {
        Encoding::Legacy
    };
    // `None` means the legacy encoding cannot say where this happened. A
    // wrapped coordinate would be a click somewhere else on the screen.
    omt_input::mouse::encode(event, encoding).map_or(Route::Ignore, Route::Forward)
}

/// The wheel, which has three possible destinations and one common mistake.
fn wheel(event: &MouseEvent, asked: Asked, direction: Wheel) -> Route {
    if asked.tracking {
        return forward(event, asked);
    }
    // Asked, never assumed. A full-screen program that did not set 1007 and is
    // sent arrow keys is an editor whose cursor moves when somebody scrolls.
    if asked.alternate && asked.alternate_scroll {
        let arrow: &[u8] = match direction {
            Wheel::Up => b"\x1b[A",
            Wheel::Down => b"\x1b[B",
            // A pager has no sideways. Sending left and right arrows would
            // move a cursor rather than scroll anything.
            Wheel::Left | Wheel::Right => return Route::Ignore,
        };
        return Route::Forward(arrow.repeat(WHEEL_LINES as usize));
    }
    if asked.alternate {
        // The alternate screen has no scrollback of its own to move through,
        // so there is nothing for omt to do either.
        return Route::Ignore;
    }
    match direction {
        Wheel::Up => Route::Scroll(-WHEEL_LINES),
        Wheel::Down => Route::Scroll(WHEEL_LINES),
        Wheel::Left | Wheel::Right => Route::Ignore,
    }
}

/// The events omt acts on itself.
fn omt_own(event: &MouseEvent, asked: Asked) -> Route {
    match (event.kind, event.button) {
        (_, Button::Wheel(direction)) => {
            // Even taken back from a program, a wheel over the alternate
            // screen has no scrollback to move through.
            if asked.alternate {
                Route::Ignore
            } else {
                match direction {
                    Wheel::Up => Route::Scroll(-WHEEL_LINES),
                    Wheel::Down => Route::Scroll(WHEEL_LINES),
                    Wheel::Left | Wheel::Right => Route::Ignore,
                }
            }
        }
        // Only the first button selects. A right-click that started a
        // selection would make the menu key destroy what it was opened over.
        (kind @ (Kind::Press | Kind::Drag | Kind::Release), Button::Left) => Route::Select(kind),
        _ => Route::Ignore,
    }
}

/// Turn the outer terminal's event into omt's, in the pane's own coordinates.
///
/// Returns nothing for the events omt has no use for — motion with no button
/// held, when nothing has asked to see it — so the caller does not have to
/// carry a variant it will only discard.
#[must_use]
pub fn from_crossterm(
    event: &crossterm::event::MouseEvent,
    rect: omt_tui::Rect,
) -> Option<(MouseEvent, bool)> {
    use crossterm::event::MouseEventKind;
    let (kind, button) = match event.kind {
        MouseEventKind::Down(b) => (Kind::Press, of_button(b)),
        MouseEventKind::Up(b) => (Kind::Release, of_button(b)),
        MouseEventKind::Drag(b) => (Kind::Drag, of_button(b)),
        MouseEventKind::Moved => (Kind::Motion, Button::None),
        MouseEventKind::ScrollUp => (Kind::Press, Button::Wheel(Wheel::Up)),
        MouseEventKind::ScrollDown => (Kind::Press, Button::Wheel(Wheel::Down)),
        MouseEventKind::ScrollLeft => (Kind::Press, Button::Wheel(Wheel::Left)),
        MouseEventKind::ScrollRight => (Kind::Press, Button::Wheel(Wheel::Right)),
    };
    let mods = event.modifiers;
    let inside = event.column >= rect.col
        && event.row >= rect.row
        && event.column < rect.col.saturating_add(rect.cols)
        && event.row < rect.row.saturating_add(rect.rows);
    Some((
        MouseEvent {
            kind,
            button,
            mods: omt_input::mouse::Mods {
                shift: mods.contains(crossterm::event::KeyModifiers::SHIFT),
                alt: mods.contains(crossterm::event::KeyModifiers::ALT),
                ctrl: mods.contains(crossterm::event::KeyModifiers::CONTROL),
            },
            // Pane-relative. A program is told where in *its* grid the pointer
            // is; omt's own frame is not something it knows exists.
            col: event.column.saturating_sub(rect.col),
            row: event.row.saturating_sub(rect.row),
        },
        inside,
    ))
}

fn of_button(button: crossterm::event::MouseButton) -> Button {
    match button {
        crossterm::event::MouseButton::Left => Button::Left,
        crossterm::event::MouseButton::Middle => Button::Middle,
        crossterm::event::MouseButton::Right => Button::Right,
    }
}

/// Take one pointer event, wherever it ends up going.
///
/// Here rather than in the frame loop because the deciding already lives here,
/// and a routing rule split across two files is one that gets changed in one
/// of them.
///
/// Returns whether the screen needs redrawing.
///
/// # Errors
/// Fails only if writing to the session does — the same failure as any other
/// keystroke, and for the same reason.
/// Everything the mouse handler needs, which exceeds the wordy-argument limit.
///
/// A struct rather than nine unlabelled parameters: the selection path
/// reaches the configuration through it, and the seventh word is where
/// somebody swaps two values.
pub(crate) struct Pointer<'a> {
    /// The event itself.
    pub(crate) pointer: &'a crossterm::event::MouseEvent,
    /// Where the pane is on the screen.
    pub(crate) rect: omt_tui::Rect,
    /// The instance the pane belongs to.
    pub(crate) instance: &'a mut omt_daemon::Instance,
    /// The attached client, when there is one.
    pub(crate) attached: &'a mut Option<crate::backing::Attached>,
    /// Every pane on screen.
    pub(crate) panes: &'a mut [crate::panes::LocalPane],
    /// Which one is focused.
    pub(crate) focus: usize,
    /// The in-flight selection, if there is one.
    pub(crate) copying: &'a mut Option<omt_tui::CopyMode>,
    /// The configuration.
    pub(crate) state: &'a crate::state::State,
}

pub(crate) fn handle(ctx: Pointer<'_>) -> anyhow::Result<bool> {
    let Pointer {
        pointer,
        rect,
        instance,
        attached,
        panes,
        focus,
        copying,
        state,
    } = ctx;
    use anyhow::Context as _;

    let Some((moused, inside)) = from_crossterm(pointer, rect) else {
        return Ok(false);
    };
    let session = panes.get(focus).map(|pane| pane.session);
    let asked = {
        let seam = crate::backing::backing(instance, attached);
        session
            .and_then(|id| seam.terminal(id))
            .map(Asked::of)
            .unwrap_or_default()
    };
    match route(&moused, asked, inside) {
        Route::Forward(bytes) => {
            if let Some(id) = session {
                if attached.is_some() {
                    crate::backing::backing(instance, attached).write(id, &bytes)?;
                } else if let Some(runtime) = instance.runtime_mut(id) {
                    let pane = &mut panes[focus];
                    runtime
                        .write_input(
                            &mut pane.writer,
                            pane.epoch,
                            crate::run::monotonic_ms(),
                            &bytes,
                        )
                        .context("writing a mouse report")?;
                }
            }
            Ok(false)
        }
        Route::Select(kind) => {
            let total = crate::terminal_bits::history_depth(instance, attached, session);
            crate::terminal_bits::selecting(
                copying,
                kind,
                &moused,
                rect,
                total,
                &mut |row| crate::terminal_bits::line_of(instance, attached, session, row),
                state,
            );
            Ok(true)
        }
        Route::Scroll(lines) => {
            let total = crate::terminal_bits::history_depth(instance, attached, session);
            crate::terminal_bits::scrolling(copying, lines, rect, total);
            Ok(true)
        }
        Route::Ignore => Ok(false),
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
    use omt_input::mouse::Mods;

    fn at(kind: Kind, button: Button) -> MouseEvent {
        MouseEvent {
            kind,
            button,
            mods: Mods::default(),
            col: 3,
            row: 4,
        }
    }

    fn grabbed() -> Asked {
        Asked {
            tracking: true,
            sgr: true,
            ..Asked::default()
        }
    }

    #[test]
    fn a_program_that_grabbed_the_mouse_gets_the_click() {
        // The whole rule. A multiplexer that swallows this has broken vim,
        // and no feature of omt's is worth that.
        let route = route(&at(Kind::Press, Button::Left), grabbed(), true);
        assert!(
            matches!(route, Route::Forward(ref bytes) if bytes == b"\x1b[<0;4;5M"),
            "{route:?}"
        );
    }

    #[test]
    fn shift_takes_it_back() {
        // Every terminal omt runs inside implements this, which is why
        // applications do not expect a shifted click.
        let mut event = at(Kind::Press, Button::Left);
        event.mods.shift = true;
        assert_eq!(route(&event, grabbed(), true), Route::Select(Kind::Press));
    }

    #[test]
    fn a_click_only_program_is_not_sent_the_pointers_whole_path() {
        // 1000 asked for presses and releases. Motion is input it never
        // requested, arriving mid-parse.
        let clicks = Asked {
            drag: false,
            ..grabbed()
        };
        assert_eq!(
            route(&at(Kind::Drag, Button::Left), clicks, true),
            Route::Ignore
        );
        assert!(matches!(
            route(&at(Kind::Press, Button::Left), clicks, true),
            Route::Forward(_)
        ));
    }

    #[test]
    fn a_program_that_asked_for_drags_gets_them() {
        let dragging = Asked {
            drag: true,
            ..grabbed()
        };
        assert!(matches!(
            route(&at(Kind::Drag, Button::Left), dragging, true),
            Route::Forward(_)
        ));
    }

    #[test]
    fn where_nothing_grabbed_the_mouse_a_drag_selects() {
        let free = Asked::default();
        assert_eq!(
            route(&at(Kind::Press, Button::Left), free, true),
            Route::Select(Kind::Press)
        );
        assert_eq!(
            route(&at(Kind::Drag, Button::Left), free, true),
            Route::Select(Kind::Drag)
        );
        assert_eq!(
            route(&at(Kind::Release, Button::Left), free, true),
            Route::Select(Kind::Release)
        );
    }

    #[test]
    fn a_click_on_omts_own_chrome_is_never_the_programs() {
        // A program cannot be sent a click on a status bar it does not know
        // exists.
        assert_eq!(
            route(&at(Kind::Press, Button::Left), grabbed(), false),
            Route::Select(Kind::Press)
        );
    }

    #[test]
    fn the_wheel_reaches_a_program_that_asked_for_the_mouse() {
        let route = route(&at(Kind::Press, Button::Wheel(Wheel::Up)), grabbed(), true);
        assert!(matches!(route, Route::Forward(ref b) if b.starts_with(b"\x1b[<64")));
    }

    #[test]
    fn a_pager_that_asked_for_alternate_scroll_gets_arrow_keys() {
        let pager = Asked {
            alternate: true,
            alternate_scroll: true,
            ..Asked::default()
        };
        assert_eq!(
            route(&at(Kind::Press, Button::Wheel(Wheel::Down)), pager, true),
            Route::Forward(b"\x1b[B".repeat(WHEEL_LINES as usize))
        );
    }

    #[test]
    fn a_full_screen_program_that_asked_for_neither_is_sent_nothing() {
        // The mistake worth not making: translating for every alternate-screen
        // program sends arrow keys to an editor, whose cursor then moves when
        // somebody scrolls.
        let editor = Asked {
            alternate: true,
            alternate_scroll: false,
            ..Asked::default()
        };
        assert_eq!(
            route(&at(Kind::Press, Button::Wheel(Wheel::Up)), editor, true),
            Route::Ignore
        );
    }

    #[test]
    fn over_a_shell_the_wheel_moves_omts_own_scrollback() {
        let shell = Asked::default();
        assert_eq!(
            route(&at(Kind::Press, Button::Wheel(Wheel::Up)), shell, true),
            Route::Scroll(-WHEEL_LINES)
        );
        assert_eq!(
            route(&at(Kind::Press, Button::Wheel(Wheel::Down)), shell, true),
            Route::Scroll(WHEEL_LINES)
        );
    }

    #[test]
    fn a_coordinate_the_old_encoding_cannot_carry_is_not_wrapped() {
        // Wrapping reports a click somewhere else on the screen, which is
        // worse than reporting none.
        let legacy = Asked {
            sgr: false,
            ..grabbed()
        };
        let mut event = at(Kind::Press, Button::Left);
        event.col = 400;
        assert_eq!(route(&event, legacy, true), Route::Ignore);
    }

    #[test]
    fn only_the_first_button_selects() {
        // A right-click that started a selection would have the menu key
        // destroy what it was opened over.
        assert_eq!(
            route(&at(Kind::Press, Button::Right), Asked::default(), true),
            Route::Ignore
        );
    }
}
