//! Colour for omt's own chrome.
//!
//! The chrome was drawn in reverse video — `\x1b[7m` — because it is the one
//! highlight every terminal has. That is the right answer for a *selection*,
//! which has to be visible in a theme nobody chose. It is the wrong answer for
//! a status bar and a panel: reverse video across a whole row is a solid slab
//! of the foreground colour, which on a dark theme is a white bar with the
//! terminal's content squeezed under it.
//!
//! A theme has existed in this repository the whole time, with a background, a
//! foreground and sixteen colours, and none of the chrome ever read it. So
//! this is the same shape of gap as the rest: a vocabulary that was there and
//! a consumer that was not.
//!
//! **Everything degrades.** A terminal that cannot do more than reverse video
//! gets what it always got, and nothing here is load-bearing for meaning:
//! state is still shape and words, never colour, because colour is the first
//! thing a terminal theme takes away.

use omt_theme::{Rgb, Theme};

/// How much colour the terminal on the other end can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Depth {
    /// Twenty-four bit.
    True,
    /// The two hundred and fifty-six colour cube.
    Indexed,
    /// Nothing to be trusted beyond bold and reverse.
    #[default]
    Plain,
}

impl Depth {
    /// What this terminal admits to.
    ///
    /// Read from the environment, never queried: a query's reply arrives in
    /// the same stream as somebody's typing, and an answer mistaken for
    /// keystrokes puts escape text into their shell.
    #[must_use]
    pub fn detect(look: impl Fn(&str) -> Option<String>) -> Self {
        // Somebody who set this asked for no colour at all, and the convention
        // is that the value does not matter.
        if look("NO_COLOR").is_some() {
            return Self::Plain;
        }
        let term = look("TERM").unwrap_or_default();
        if term.is_empty() || term == "dumb" {
            return Self::Plain;
        }
        let colorterm = look("COLORTERM").unwrap_or_default();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return Self::True;
        }
        if term.contains("256") || term.contains("kitty") || term.contains("direct") {
            return Self::Indexed;
        }
        // A terminal that says only `xterm` may well do more, but this is the
        // guess that cannot look broken: eight colours are safe everywhere.
        Self::Indexed
    }

    /// From the process's own environment.
    #[must_use]
    pub fn of_environment() -> Self {
        Self::detect(|key| std::env::var(key).ok())
    }
}

/// The colours omt draws its own furniture with.
///
/// Derived from the theme rather than invented, so a terminal somebody themed
/// and a browser showing the same session do not disagree about what omt looks
/// like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    /// Behind a bar or a panel: the background, lifted just enough to read as
    /// a surface rather than a hole.
    pub surface: Rgb,
    /// Behind the row somebody is on.
    pub picked: Rgb,
    /// Lines and edges.
    pub border: Rgb,
    /// Text that is present but not the point.
    pub muted: Rgb,
    /// Text.
    pub text: Rgb,
    /// The one colour that draws the eye, used once per surface at most.
    pub accent: Rgb,
}

impl Chrome {
    /// Work the furniture out from a theme.
    #[must_use]
    pub fn of(theme: &Theme) -> Self {
        let bg = theme.background;
        let fg = theme.foreground;
        Self {
            // Small steps. A surface that jumps far from the background reads
            // as a separate window rather than a part of this one, which is
            // what a full-width reverse-video bar did.
            surface: mix(bg, fg, 0.10),
            picked: mix(bg, theme.palette.normal[4], 0.28),
            border: mix(bg, fg, 0.22),
            muted: mix(bg, fg, 0.55),
            text: fg,
            accent: theme.palette.normal[4],
        }
    }

    /// The furniture omt ships with.
    #[must_use]
    pub fn builtin() -> Self {
        Self::of(&Theme::builtin_dark())
    }
}

/// Move one colour a fraction of the way towards another.
#[must_use]
pub fn mix(from: Rgb, to: Rgb, amount: f64) -> Rgb {
    let at = amount.clamp(0.0, 1.0);
    let one = |a: u8, b: u8| {
        let value = f64::from(a) + (f64::from(b) - f64::from(a)) * at;
        // Rounded rather than truncated: truncation drifts every channel down,
        // which over several mixes turns a grey into a darker grey.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to a byte's range before the cast"
        )]
        {
            value.round().clamp(0.0, 255.0) as u8
        }
    };
    Rgb(one(from.0, to.0), one(from.1, to.1), one(from.2, to.2))
}

/// Set the foreground.
#[must_use]
pub fn fg(colour: Rgb, depth: Depth) -> String {
    match depth {
        Depth::True => format!("\x1b[38;2;{};{};{}m", colour.0, colour.1, colour.2),
        Depth::Indexed => format!("\x1b[38;5;{}m", indexed(colour)),
        Depth::Plain => String::new(),
    }
}

/// Set the background.
#[must_use]
pub fn bg(colour: Rgb, depth: Depth) -> String {
    match depth {
        Depth::True => format!("\x1b[48;2;{};{};{}m", colour.0, colour.1, colour.2),
        Depth::Indexed => format!("\x1b[48;5;{}m", indexed(colour)),
        Depth::Plain => String::new(),
    }
}

/// Everything back to normal.
pub const RESET: &str = "\x1b[0m";

/// Reverse video, which is what a terminal with no colour gets.
pub const REVERSE: &str = "\x1b[7m";

/// Dim, which is the only "less than this" a plain terminal has.
pub const DIM: &str = "\x1b[2m";

/// The nearest colour in the 256-colour cube.
///
/// The grey ramp is checked as well as the cube: omt's own surfaces are greys
/// a shade apart, and the cube's greys are coarse enough that two of them
/// would land on the same square and the surface would vanish.
#[must_use]
pub fn indexed(colour: Rgb) -> u8 {
    let (r, g, b) = (colour.0, colour.1, colour.2);
    // The 24-step grey ramp, which is finer than the cube for near-greys.
    let grey_like = r.abs_diff(g) < 10 && g.abs_diff(b) < 10 && r.abs_diff(b) < 10;
    if grey_like {
        let level = f64::from(u16::from(r) + u16::from(g) + u16::from(b)) / 3.0;
        if level < 8.0 {
            return 16;
        }
        if level > 238.0 {
            return 231;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "bounded to 0..=23 by the arithmetic above"
        )]
        let step = (((level - 8.0) / 10.0).round().clamp(0.0, 23.0)) as u8;
        return 232 + step;
    }
    let axis = |v: u8| -> u16 {
        // The cube's steps are 0, 95, 135, 175, 215, 255 — not even, so the
        // nearest one is chosen rather than computed.
        const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let mut best = 0u16;
        let mut distance = u16::MAX;
        for (i, step) in STEPS.iter().enumerate() {
            let d = u16::from(v.abs_diff(*step));
            if d < distance {
                distance = d;
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "i is bounded by STEPS' length"
                )]
                {
                    best = i as u16;
                }
            }
        }
        best
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "16 + 5*36 + 5*6 + 5 = 231, which is a byte"
    )]
    {
        (16 + 36 * axis(r) + 6 * axis(g) + axis(b)) as u8
    }
}

/// Paint one line of a panel.
///
/// The whole line is given a surface, so the panel reads as one object sitting
/// over the pane rather than as text with gaps in it. The row somebody is on
/// gets the accent behind it — one thing on the surface draws the eye, and it
/// is the thing Enter will act on.
#[must_use]
pub fn overlay_line(row: crate::overlay::Row, text: &str, chrome: Chrome, depth: Depth) -> String {
    use crate::overlay::Row;
    if depth == Depth::Plain {
        // What it always was. A terminal with no colour still needs the panel
        // to stand off the pane behind it, and reverse video is the one
        // highlight every terminal has.
        return format!("{REVERSE}{text}{RESET}");
    }
    let (back, front) = match row {
        Row::Edge => (chrome.surface, chrome.border),
        Row::Filter | Row::Note => (chrome.surface, chrome.muted),
        Row::Choice => (chrome.surface, chrome.text),
        Row::Picked => (chrome.picked, chrome.text),
    };
    // The panel is centred, so each line arrives with the margin in front of
    // it. Painting that margin turns a floating panel into a full-width band
    // with a box drawn inside it — the surface has to start where the box
    // does, or the shape being drawn is not the shape being seen.
    let margin: String = text.chars().take_while(|c| *c == ' ').collect();
    let body: String = text.chars().skip(margin.chars().count()).collect();
    format!(
        "{margin}{}{}{body}{RESET}",
        bg(back, depth),
        fg(front, depth)
    )
}

/// Paint the status bar.
///
/// One surface, muted text, and the accent spent on the one word that says
/// which omt this is — rather than a full-width slab of the foreground colour,
/// which is what reverse video across a whole row amounts to.
#[must_use]
pub fn status_line(text: &str, chrome: Chrome, depth: Depth) -> String {
    if depth == Depth::Plain {
        return format!("{REVERSE}{text}{RESET}");
    }
    let surface = bg(chrome.surface, depth);
    let mut out = format!("{surface}{}", fg(chrome.muted, depth));
    // The leading name, in the accent. Split on the first space so the rest of
    // the bar keeps whatever spacing the renderer gave it.
    if let Some((name, rest)) = text.split_once(' ') {
        out.push_str(&fg(chrome.accent, depth));
        out.push_str(name);
        out.push_str(&fg(chrome.muted, depth));
        out.push(' ');
        out.push_str(rest);
    } else {
        out.push_str(text);
    }
    out.push_str(RESET);
    out
}

/// Paint a line that says what a mode is and how to leave it.
#[must_use]
pub fn hint_line(text: &str, chrome: Chrome, depth: Depth) -> String {
    if depth == Depth::Plain {
        return format!("{REVERSE}{text}{RESET}");
    }
    format!(
        "{}{}{text}{RESET}",
        bg(chrome.surface, depth),
        fg(chrome.accent, depth)
    )
}

/// The vertical rule between two panes.
#[must_use]
pub fn separator(chrome: Chrome, depth: Depth) -> String {
    if depth == Depth::Plain {
        return format!("{DIM}\u{2502}{RESET}");
    }
    format!("{}\u{2502}{RESET}", fg(chrome.border, depth))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn nobody_who_asked_for_no_colour_gets_any() {
        // The convention is that the value does not matter, only that it is
        // set — and somebody who set it meant it.
        assert_eq!(
            Depth::detect(env(&[("NO_COLOR", ""), ("COLORTERM", "truecolor")])),
            Depth::Plain
        );
    }

    #[test]
    fn a_terminal_that_says_truecolor_is_believed() {
        assert_eq!(
            Depth::detect(env(&[
                ("TERM", "xterm-256color"),
                ("COLORTERM", "truecolor")
            ])),
            Depth::True
        );
    }

    #[test]
    fn a_terminal_with_no_term_at_all_gets_nothing() {
        // Which is what a pipe looks like, and a pipe with escape codes in it
        // is a log nobody can read.
        assert_eq!(Depth::detect(env(&[])), Depth::Plain);
        assert_eq!(Depth::detect(env(&[("TERM", "dumb")])), Depth::Plain);
    }

    #[test]
    fn the_ordinary_terminal_gets_the_cube() {
        // Apple's Terminal is this one, and it is what omt is being run in.
        assert_eq!(
            Depth::detect(env(&[("TERM", "xterm-256color")])),
            Depth::Indexed
        );
    }

    #[test]
    fn a_surface_is_a_step_from_the_background_not_a_leap() {
        // The defect this replaces: reverse video across a whole row is a slab
        // of the foreground colour, which on a dark theme is a white bar.
        let chrome = Chrome::builtin();
        let bg = Theme::builtin_dark().background;
        let lift = i32::from(chrome.surface.0) - i32::from(bg.0);
        assert!(
            (4..40).contains(&lift),
            "the surface is {lift} from the background, which is either invisible or a window"
        );
    }

    #[test]
    fn every_furniture_colour_can_be_read_against_its_own_surface() {
        // A muted colour that cannot be read is not muted, it is missing.
        let chrome = Chrome::builtin();
        for (name, colour) in [
            ("text", chrome.text),
            ("muted", chrome.muted),
            ("accent", chrome.accent),
        ] {
            let ratio = colour.contrast(chrome.surface);
            assert!(ratio > 2.5, "{name} against its surface is {ratio:.2}");
        }
    }

    #[test]
    fn mixing_moves_the_whole_way_and_no_further() {
        let black = Rgb(0, 0, 0);
        let white = Rgb(255, 255, 255);
        assert_eq!(mix(black, white, 0.0), black);
        assert_eq!(mix(black, white, 1.0), white);
        assert_eq!(mix(black, white, 0.5), Rgb(128, 128, 128));
        // Out of range is clamped rather than wrapped, because a colour that
        // wrapped would be the opposite of what was asked for.
        assert_eq!(mix(black, white, 2.0), white);
        assert_eq!(mix(black, white, -1.0), black);
    }

    #[test]
    fn a_plain_terminal_is_sent_no_colour_at_all() {
        assert!(fg(Rgb(1, 2, 3), Depth::Plain).is_empty());
        assert!(bg(Rgb(1, 2, 3), Depth::Plain).is_empty());
    }

    #[test]
    fn colour_is_spelled_the_way_each_depth_expects() {
        assert_eq!(fg(Rgb(10, 20, 30), Depth::True), "\x1b[38;2;10;20;30m");
        assert!(bg(Rgb(10, 20, 30), Depth::Indexed).starts_with("\x1b[48;5;"));
    }

    #[test]
    fn near_greys_use_the_grey_ramp_so_two_surfaces_stay_apart() {
        // The cube's greys are coarse; omt's surfaces are a shade apart, and
        // on the cube they would land on the same square and the surface would
        // disappear.
        let chrome = Chrome::builtin();
        let bg_index = indexed(Theme::builtin_dark().background);
        let surface_index = indexed(chrome.surface);
        assert_ne!(
            bg_index, surface_index,
            "the surface and the background became the same colour"
        );
        assert!((232..=255).contains(&surface_index), "{surface_index}");
    }

    #[test]
    fn a_colour_that_is_not_grey_lands_in_the_cube() {
        let blue = indexed(Rgb(0x59, 0xa5, 0xff));
        assert!((16..232).contains(&blue), "{blue}");
    }

    #[test]
    fn a_panel_reads_as_one_object_and_the_picked_row_stands_out() {
        let chrome = Chrome::builtin();
        let picked = overlay_line(crate::overlay::Row::Picked, " x ", chrome, Depth::True);
        let plain = overlay_line(crate::overlay::Row::Choice, " x ", chrome, Depth::True);
        assert!(
            picked.contains("48;2;"),
            "the picked row has its own surface"
        );
        assert_ne!(picked, plain, "the row somebody is on looks like it");
        assert!(
            plain.contains("48;2;"),
            "every row shares the panel's surface"
        );
    }

    #[test]
    fn a_terminal_with_no_colour_gets_exactly_what_it_always_got() {
        // The furniture is decoration; nothing here carries meaning that a
        // colourless terminal would lose.
        let chrome = Chrome::builtin();
        for line in [
            overlay_line(crate::overlay::Row::Picked, "x", chrome, Depth::Plain),
            status_line("omt 1:shell", chrome, Depth::Plain),
            hint_line("Esc leave", chrome, Depth::Plain),
        ] {
            assert!(line.starts_with(REVERSE), "{line:?}");
        }
        assert!(separator(chrome, Depth::Plain).starts_with(DIM));
    }

    #[test]
    fn the_status_bar_spends_its_accent_on_one_word() {
        // One thing per surface draws the eye. Two is a bar nobody reads.
        let bar = status_line("omt 1:shell main", Chrome::builtin(), Depth::True);
        assert_eq!(bar.matches("38;2;89;165;255").count(), 1, "{bar:?}");
    }

    #[test]
    fn a_bar_with_one_word_is_still_painted() {
        let bar = status_line("omt", Chrome::builtin(), Depth::True);
        assert!(bar.contains("48;2;"), "{bar:?}");
        assert!(bar.ends_with(RESET));
    }
}
