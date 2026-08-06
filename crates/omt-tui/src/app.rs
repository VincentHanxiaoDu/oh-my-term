//! The loop: read the terminal, drive the session, draw what changed.
//!
//! Raw mode is entered and left through a guard rather than by matching calls.
//! A panic between the two would otherwise leave the user's shell without echo
//! and without a working Ctrl-C — a terminal they have to close the window to
//! escape, which is the worst possible way for a bug to present.

use std::io::{self, Write};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use omt_input::{Chord, EncodeMode, Key, Modifiers};

/// Restores the terminal however the program leaves.
///
/// Held for the life of the loop. `Drop` runs on a panic, on `?`, and on a
/// clean exit alike, which is the only way to be sure the user's shell comes
/// back usable.
pub struct RawGuard;

impl RawGuard {
    /// Enter raw mode and switch to the alternate screen.
    ///
    /// # Errors
    /// Fails if the terminal refuses, which is what happens when stdout is not
    /// one.
    pub fn enter() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        crossterm::execute!(
            out,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::cursor::Hide
        )?;
        out.flush()?;
        Ok(Self)
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = crossterm::execute!(
            out,
            crossterm::cursor::Show,
            crossterm::terminal::LeaveAlternateScreen
        );
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = out.flush();
    }
}

/// What the loop should do about an input event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Bytes to write to the session.
    Bytes(Vec<u8>),
    /// The window changed size.
    Resize {
        /// New width.
        cols: u16,
        /// New height.
        rows: u16,
    },
    /// Detach and leave the session running.
    Detach,
    /// Split: put a new session beside this one.
    Split,
    /// Move focus to the next pane.
    NextPane,
    /// Close the focused pane, leaving its session running.
    ClosePane,
    /// Show every binding.
    Help,
    /// Nothing to do.
    Ignore,
}

/// The chord that leaves without killing anything.
///
/// A prefix rather than a bare key, because every bare key belongs to the
/// program underneath. `Ctrl-A` then `d` is what every multiplexer user already
/// has in their fingers.
pub const PREFIX: KeyModifiers = KeyModifiers::CONTROL;
/// The prefix key.
pub const PREFIX_KEY: char = 'a';

/// Translate one terminal event, given whether the prefix was just pressed.
///
/// Returns the input and whether the prefix is now armed.
#[must_use]
pub fn translate(event: &Event, armed: bool, mode: EncodeMode) -> (Input, bool) {
    match event {
        Event::Resize(cols, rows) => (
            Input::Resize {
                cols: *cols,
                rows: *rows,
            },
            false,
        ),
        Event::Key(key) => translate_key(key, armed, mode),
        // Focus, mouse and paste are the program's to handle once those modes
        // are wired; forwarding them before then would send bytes no program
        // asked to receive.
        _ => (Input::Ignore, armed),
    }
}

fn translate_key(key: &KeyEvent, armed: bool, mode: EncodeMode) -> (Input, bool) {
    if armed {
        // The prefix consumed the last key, so this one is a command.
        return match key.code {
            KeyCode::Char('d') => (Input::Detach, false),
            // The three tmux users already have in their fingers, spelled the
            // same way: splitting, cycling and closing.
            KeyCode::Char('s' | 'c') => (Input::Split, false),
            KeyCode::Char('o' | 'n') => (Input::NextPane, false),
            KeyCode::Char('x') => (Input::ClosePane, false),
            KeyCode::Char('?') => (Input::Help, false),
            // The prefix twice sends a literal prefix, which is how a user
            // types the chord the multiplexer took.
            KeyCode::Char(c) if c == PREFIX_KEY && key.modifiers.contains(PREFIX) => (
                Input::Bytes(omt_input::encode(&Chord::ctrl(PREFIX_KEY), mode)),
                false,
            ),
            // An unrecognized command does nothing rather than reaching the
            // program: a mistyped chord that silently ran something would be
            // worse than one that did not.
            _ => (Input::Ignore, false),
        };
    }

    if key.modifiers.contains(PREFIX) && matches!(key.code, KeyCode::Char(c) if c == PREFIX_KEY) {
        return (Input::Ignore, true);
    }

    match to_chord(key) {
        Some(chord) => (Input::Bytes(omt_input::encode(&chord, mode)), false),
        None => (Input::Ignore, false),
    }
}

/// Map a crossterm key onto omt's own vocabulary.
///
/// Going through `omt-input` rather than crossterm's own encoding is what keeps
/// the TUI and every remote client sending identical bytes for a key.
#[must_use]
pub fn to_chord(key: &KeyEvent) -> Option<Chord> {
    let mods = Modifiers {
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
        meta: key.modifiers.contains(KeyModifiers::SUPER),
    };
    let code = match key.code {
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Esc => Key::Escape,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::F(n) => Key::F(n),
        _ => return None,
    };
    let mods = if matches!(key.code, KeyCode::BackTab) {
        Modifiers {
            shift: true,
            ..mods
        }
    } else {
        mods
    };
    Some(Chord { key: code, mods })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {

    #[test]
    fn the_prefix_commands_are_the_ones_people_already_know() {
        // Spelled the same way tmux spells them, because the muscle memory is
        // the whole reason a prefix works at all.
        let mode = EncodeMode::default();
        for (key, expected) in [
            ('d', Input::Detach),
            ('s', Input::Split),
            ('o', Input::NextPane),
            ('x', Input::ClosePane),
            ('?', Input::Help),
        ] {
            let event = Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
            let (input, armed) = translate(&event, true, mode);
            assert_eq!(input, expected, "prefix {key}");
            assert!(!armed, "the prefix stayed armed after {key}");
        }
    }

    #[test]
    fn an_unknown_prefix_command_does_nothing_at_all() {
        // A mistyped chord that silently ran something is worse than one that
        // did not, and it must not reach the program either.
        let event = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        let (input, armed) = translate(&event, true, EncodeMode::default());
        assert_eq!(input, Input::Ignore);
        assert!(!armed);
    }
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn plain(c: char) -> Event {
        key(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn an_ordinary_key_becomes_the_bytes_a_program_expects() {
        let (input, armed) = translate(&plain('a'), false, EncodeMode::default());
        assert_eq!(input, Input::Bytes(b"a".to_vec()));
        assert!(!armed);
    }

    #[test]
    fn ctrl_c_reaches_the_program() {
        // The key omt may never take. If the multiplexer swallowed it, a
        // runaway process could not be stopped.
        let (input, _) = translate(
            &key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            false,
            EncodeMode::default(),
        );
        assert_eq!(input, Input::Bytes(vec![0x03]));
    }

    #[test]
    fn the_prefix_arms_rather_than_being_sent() {
        let (input, armed) = translate(
            &key(KeyCode::Char('a'), KeyModifiers::CONTROL),
            false,
            EncodeMode::default(),
        );
        assert_eq!(input, Input::Ignore);
        assert!(armed);
    }

    #[test]
    fn the_prefix_then_d_detaches() {
        let (input, armed) = translate(&plain('d'), true, EncodeMode::default());
        assert_eq!(input, Input::Detach);
        assert!(!armed, "and the prefix is spent");
    }

    #[test]
    fn the_prefix_twice_sends_a_literal_one() {
        // How a user types the chord the multiplexer took from them.
        let (input, _) = translate(
            &key(KeyCode::Char('a'), KeyModifiers::CONTROL),
            true,
            EncodeMode::default(),
        );
        assert_eq!(input, Input::Bytes(vec![0x01]));
    }

    #[test]
    fn an_unrecognized_command_does_nothing_rather_than_reaching_the_program() {
        // A mistyped chord that silently ran something is worse than one that
        // did not.
        let (input, armed) = translate(&plain('z'), true, EncodeMode::default());
        assert_eq!(input, Input::Ignore);
        assert!(!armed);
    }

    #[test]
    fn a_resize_is_reported_and_clears_the_prefix() {
        let (input, armed) = translate(&Event::Resize(100, 40), true, EncodeMode::default());
        assert_eq!(
            input,
            Input::Resize {
                cols: 100,
                rows: 40
            }
        );
        assert!(!armed, "a window change is not half a chord");
    }

    #[test]
    fn arrows_follow_the_mode_the_program_set() {
        // Going through omt-input rather than crossterm's own encoding is what
        // keeps the TUI and every remote client sending identical bytes.
        let up = key(KeyCode::Up, KeyModifiers::NONE);
        let (normal, _) = translate(&up, false, EncodeMode::default());
        let (app, _) = translate(
            &up,
            false,
            EncodeMode {
                application_cursor: true,
            },
        );
        assert_eq!(normal, Input::Bytes(b"\x1b[A".to_vec()));
        assert_eq!(app, Input::Bytes(b"\x1bOA".to_vec()));
    }

    #[test]
    fn backspace_is_del() {
        let (input, _) = translate(
            &key(KeyCode::Backspace, KeyModifiers::NONE),
            false,
            EncodeMode::default(),
        );
        assert_eq!(input, Input::Bytes(vec![0x7f]));
    }

    #[test]
    fn shift_tab_is_a_back_tab() {
        let (input, _) = translate(
            &key(KeyCode::BackTab, KeyModifiers::NONE),
            false,
            EncodeMode::default(),
        );
        assert_eq!(input, Input::Bytes(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn an_event_nothing_handles_is_ignored_rather_than_forwarded() {
        // Sending bytes no program asked to receive is how a paste turns into a
        // command.
        let (input, armed) = translate(&Event::FocusGained, true, EncodeMode::default());
        assert_eq!(input, Input::Ignore);
        assert!(armed, "and it does not spend a pending prefix");
    }
}
