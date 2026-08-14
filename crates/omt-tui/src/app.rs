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
            crossterm::cursor::Hide,
            // The terminal wraps a paste in markers and delivers it as one
            // event instead of one per character. Without it a pasted
            // paragraph is hundreds of key events, and every layer above has
            // to be fast enough to keep up with a burst it should never have
            // been handed. It also lets the pane below know a burst is a
            // paste, which is what stops a pasted newline running a command.
            crossterm::event::EnableBracketedPaste
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
            crossterm::event::DisableBracketedPaste,
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
    /// Open the command palette.
    ///
    /// The palette is what makes the small binding table honest: every
    /// capability is reachable through it, so the ones without a chord are
    /// unbound rather than unavailable.
    Palette,
    /// Show the agent threads, and which of them are waiting on a human.
    Agents,
    /// Show the sessions, to move between them by name rather than by counting
    /// how many times to press next.
    Sessions,
    /// Nothing to do.
    Ignore,
}

/// The chord that leaves without killing anything.
///
/// A prefix rather than a bare key, because every bare key belongs to the
/// program underneath.
pub const PREFIX: KeyModifiers = KeyModifiers::CONTROL;

/// The default prefix key.
///
/// `Ctrl-B`, per [16 §3.2](../../../docs/architecture/16-input-and-keymap.md).
/// It is tmux's default and has been for two decades, which is the whole
/// argument: the largest group of incoming users relearns nothing.
///
/// This was `Ctrl-A` — screen's default — and that is the one alternative the
/// document explicitly considers and rejects, because in readline `Ctrl-A` is
/// `beginning-of-line`, a key people press constantly in a shell, and a shell
/// is what half of omt's panes contain. `omt-input`'s own keymap already
/// defaulted to `ctrl-b`; only the TUI disagreed, so the two halves of the
/// product shipped different leaders.
pub const PREFIX_KEY: char = 'b';

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
        // A paste goes through whole, wrapped in the markers the program
        // underneath is expecting. Forwarding the text bare would let a
        // pasted newline submit a half-typed command in a shell, and would
        // make a paste indistinguishable from very fast typing to an editor
        // that cares about the difference.
        Event::Paste(text) => (Input::Bytes(bracketed(text)), armed),
        // Focus and mouse are the program's to handle once those modes are
        // wired; forwarding them before then would send bytes no program
        // asked to receive.
        _ => (Input::Ignore, armed),
    }
}

/// Every key the leader namespace binds, and how it reads to a person.
///
/// One table rather than a `match` plus a hand-written hint string. The strip
/// shown after the leader is generated from this, so a binding that exists and
/// is not advertised — or advertised and does not exist — is not expressible.
/// The order is the order the strip lists them in: leaving, then making, then
/// moving, then finding.
pub const LEADER_KEYS: &[(char, &str)] = &[
    ('d', "detach"),
    ('c', "new"),
    ('s', "split"),
    ('o', "next"),
    ('x', "close"),
    ('w', "sessions"),
    ('a', "agents"),
    ('p', "palette"),
    ('?', "all keys"),
];

/// What one leader key does.
///
/// Kept beside [`LEADER_KEYS`] and checked against it by a test, because the
/// failure mode being designed out is a key that works and is never shown.
#[must_use]
pub fn leader_action(key: char) -> Option<Input> {
    Some(match key {
        'd' => Input::Detach,
        // The four tmux users already have in their fingers, spelled the same
        // way: making, splitting, cycling and closing.
        'c' | 's' => Input::Split,
        'o' | 'n' => Input::NextPane,
        'x' => Input::ClosePane,
        '?' => Input::Help,
        'p' => Input::Palette,
        'a' => Input::Agents,
        'w' => Input::Sessions,
        _ => return None,
    })
}

fn translate_key(key: &KeyEvent, armed: bool, mode: EncodeMode) -> (Input, bool) {
    if armed {
        // The prefix consumed the last key, so this one is a command.
        return match key.code {
            // The prefix twice sends a literal prefix, which is how a user
            // types the chord the multiplexer took. Checked before the table,
            // because `Ctrl-A` must not be read as the bare `a`.
            KeyCode::Char(c) if c == PREFIX_KEY && key.modifiers.contains(PREFIX) => (
                Input::Bytes(omt_input::encode(&Chord::ctrl(PREFIX_KEY), mode)),
                false,
            ),
            KeyCode::Char(c) if !key.modifiers.contains(PREFIX) => (
                // An unrecognized command does nothing rather than reaching the
                // program: a mistyped chord that silently ran something would
                // be worse than one that did not.
                leader_action(c).unwrap_or(Input::Ignore),
                false,
            ),
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

/// Wrap pasted text in the bracketed-paste markers.
///
/// `ESC [ 200 ~` … `ESC [ 201 ~`. A program that asked for bracketed paste
/// uses them to tell a paste from typing — which is how an editor knows not to
/// auto-indent it and how a shell knows a newline inside one is not `Enter`.
/// A program that did not ask still receives them, because omt is not the
/// layer that decides: the terminal it is running in sent them to omt for
/// exactly the same reason.
#[must_use]
pub fn bracketed(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 12);
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
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
            &key(KeyCode::Char(PREFIX_KEY), KeyModifiers::CONTROL),
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
            &key(KeyCode::Char(PREFIX_KEY), KeyModifiers::CONTROL),
            true,
            EncodeMode::default(),
        );
        // Derived from the prefix rather than written as a literal, so
        // changing the default leader cannot leave this test asserting the
        // old key's byte.
        assert_eq!(
            input,
            Input::Bytes(vec![(PREFIX_KEY as u8) & 0x1f]),
            "the literal prefix must be the prefix that is bound"
        );
    }

    #[test]
    fn a_paste_arrives_whole_and_wrapped() {
        // One event, not one per character. Reading a paste a character at a
        // time is what made a pasted paragraph appear letter by letter.
        let (input, armed) = translate(
            &Event::Paste("two words".to_owned()),
            false,
            EncodeMode::default(),
        );
        let Input::Bytes(bytes) = input else {
            panic!("a paste must reach the program, got {input:?}");
        };
        assert_eq!(bytes, b"\x1b[200~two words\x1b[201~");
        assert!(!armed, "a paste is not a command");
    }

    #[test]
    fn a_pasted_newline_is_inside_the_markers_rather_than_submitted() {
        // The markers are what tell a shell that a newline in here is text and
        // not `Enter`. Sending the text bare would run a half-typed command.
        let bytes = bracketed("first\nsecond");
        let text = String::from_utf8(bytes).expect("utf-8");
        assert!(text.starts_with("\x1b[200~"), "{text:?}");
        assert!(text.ends_with("\x1b[201~"), "{text:?}");
        assert!(
            text.trim_start_matches("\x1b[200~")
                .starts_with("first\nsecond"),
            "{text:?}"
        );
    }

    #[test]
    fn a_paste_while_the_prefix_is_armed_does_not_lose_the_prefix() {
        // Somebody who pressed the leader and then pasted has not cancelled
        // the leader, and swallowing it would make the next keystroke run a
        // command they did not mean.
        let (_, armed) = translate(&Event::Paste("x".to_owned()), true, EncodeMode::default());
        assert!(armed);
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
