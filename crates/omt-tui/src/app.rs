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
            crossterm::event::EnableBracketedPaste,
            // Without this omt never sees a pointer at all, and a drag across
            // a pane is the *outer* terminal selecting a rectangle of omt's
            // whole frame — every pane's chrome included. Asking for reports
            // is what makes a selection mean the pane it was made in.
            crossterm::event::EnableMouseCapture
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
            crossterm::event::DisableMouseCapture,
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
    /// Leave, and let the sessions carry on without this terminal.
    ///
    /// What it means depends on who owns them, and omt says which: attached to
    /// an instance it ends nothing, and on the fallback path — where an
    /// instance could not be started — it ends every pty this process holds,
    /// and asks first.
    Detach,
    /// Split: put a new session beside this one, side by side.
    Split,
    /// Split the other way: put a new session below this one.
    ///
    /// Two keys rather than one that guesses, because which way somebody wants
    /// a screen cut depends on what they are about to put in it, and a wrong
    /// guess costs a close and a re-split.
    SplitDown,
    /// Show or hide the agent panel.
    Panel,
    /// Read what has scrolled away, and take a copy of it.
    CopyMode,
    /// What has been run here before, across sessions.
    ///
    /// Not the shell's own history — that is one shell's, and reachable with
    /// the shell's own keys. This is every session's, which is the part no
    /// shell can offer.
    Recall,
    /// What the agent in this pane says it offers.
    ///
    /// The agent's own commands, not omt's. omt has no composer to complete
    /// into — you type into the agent's interface — so this lists what it
    /// reported and types the one you pick.
    Commands,
    /// Read the focused agent's conversation.
    ///
    /// What it *said and did*, not what it painted. The grid is a box an agent
    /// redraws and scrolls away; the transcript is the record, and it was
    /// readable from a phone months before it was readable from a keyboard.
    Conversation,
    /// Fill the screen with the focused pane, or put it back.
    Zoom,
    /// Move focus to the pane it was in before this one.
    LastPane,
    /// Move the focused pane in a direction, rather than the focus.
    Swap(crate::tree::Towards),
    /// Focus the next agent that needs a person.
    NextWaiting,
    /// Focus a pane by the number the status bar gives it.
    ///
    /// One-based, because the bar reads `1:zsh 2:claude` and a key that
    /// disagreed with what is written on the screen would be worse than no
    /// key at all.
    Goto(usize),
    /// Move focus in a direction.
    ///
    /// Geometric, not "the next one in a list": with more than two panes,
    /// cycling is a guessing game and the arrow keys are what everybody
    /// already knows.
    Focus(crate::tree::Towards),
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
    // Short labels, and the shortness is load-bearing: this is one row.
    //
    // **The order is a priority order**, and it is the whole design of this
    // table. There are more actions than a row can hold — that was true at ten
    // and is truer at fourteen — so the strip drops from the end and says so
    // with a marker pointing at the full list. What must survive a narrow
    // terminal is what somebody is most likely to have opened the strip to
    // find, which is why `waiting` and `agents` are above `panel` and
    // `palette` rather than in the order they were added.
    // "detach" again, and now it is true: the sessions live in an instance
    // this terminal is watching, so leaving is leaving. It read "quit" for
    // exactly as long as the key ended every pty it was holding.
    ('d', "detach"),
    ('c', "new"),
    ('s', "s/S split"),
    ('h', "hjkl/HJKL"),
    ('z', "zoom"),
    ('!', "waiting"),
    ('a', "agents"),
    ('t', "transcript"),
    ('[', "copy"),
    ('/', "commands"),
    ('r', "recall"),
    ('w', "sessions"),
    ('g', "panel"),
    ('x', "close"),
    ('p', "palette"),
    ('1', "1-9 pane"),
    // Bound and deliberately not listed: `o` cycles panes, which is what
    // `hjkl` does less precisely; `j`, `k` and `l` are the other three
    // directions and `HJKL` moves the pane the same way; `;` returns to the
    // last pane; and `?` opens the full list, which is a strange thing to
    // advertise *on* the list of keys — the overflow marker points at it when
    // there is more to see.
];

/// What one leader key does.
///
/// Kept beside [`LEADER_KEYS`] and checked against it by a test, because the
/// failure mode being designed out is a key that works and is never shown.
#[must_use]
pub fn leader_action(key: char) -> Option<Input> {
    Some(match key {
        'd' => Input::Detach,
        'S' => Input::SplitDown,
        'g' => Input::Panel,
        'z' => Input::Zoom,
        ';' => Input::LastPane,
        'H' => Input::Swap(crate::tree::Towards::Left),
        'J' => Input::Swap(crate::tree::Towards::Down),
        'K' => Input::Swap(crate::tree::Towards::Up),
        'L' => Input::Swap(crate::tree::Towards::Right),
        '!' => Input::NextWaiting,
        // The numbers the status bar already writes. Everybody's fingers do
        // this before they read any documentation.
        digit @ '1'..='9' => Input::Goto(digit.to_digit(10).unwrap_or(1) as usize),
        'h' => Input::Focus(crate::tree::Towards::Left),
        'j' => Input::Focus(crate::tree::Towards::Down),
        'k' => Input::Focus(crate::tree::Towards::Up),
        'l' => Input::Focus(crate::tree::Towards::Right),
        // The four tmux users already have in their fingers, spelled the same
        // way: making, splitting, cycling and closing.
        'c' | 's' => Input::Split,
        'o' | 'n' => Input::NextPane,
        'x' => Input::ClosePane,
        '?' => Input::Help,
        'p' => Input::Palette,
        'a' => Input::Agents,
        't' => Input::Conversation,
        // The bracket tmux uses, so fingers that already know it are right.
        '[' => Input::CopyMode,
        // The character the commands themselves start with.
        '/' => Input::Commands,
        'r' => Input::Recall,
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
        let (input, armed) = translate(&plain('~'), true, EncodeMode::default());
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

/// Wraps one frame so the terminal shows all of it or none of it.
///
/// DEC 2026, synchronized output. A frame is dozens of writes — the panes, the
/// separators, the status bar, an overlay, the cursor — and a terminal is free
/// to present the screen between any two of them. What that looks like is a
/// status bar from this frame above a pane from the last one, for one refresh,
/// every frame: tearing, and the reason a terminal program looks like it is
/// struggling even when it is fast.
///
/// A terminal that does not know the sequence ignores it, which is why this is
/// unconditional rather than behind a capability query: the query costs a
/// round trip and the fallback is the behaviour without it.
///
/// The frame is drawn into a buffer and written once, which has a second
/// consequence that matters more than the tearing: **a frame that drew nothing
/// writes nothing.** Wrapping the writes as they happen would emit the
/// begin/end pair on every tick of an idle loop — sixteen bytes, sixty times a
/// second, into a terminal where nothing is happening — which is the defect
/// this was added next to, not a new one to introduce.
#[derive(Debug)]
pub struct Frame<'a, W: io::Write> {
    out: &'a mut W,
    body: Vec<u8>,
}

impl<'a, W: io::Write> Frame<'a, W> {
    /// Begin a frame.
    pub fn begin(out: &'a mut W) -> Self {
        Self {
            out,
            // Roughly one screen of cells with escapes, so an ordinary frame
            // never reallocates.
            body: Vec::with_capacity(8192),
        }
    }

    /// The buffer this frame is drawn into.
    pub fn writer(&mut self) -> &mut Vec<u8> {
        &mut self.body
    }

    /// Present it, if there is anything to present.
    ///
    /// # Errors
    /// Fails if the write or the flush fails.
    pub fn present(self) -> io::Result<()> {
        if self.body.is_empty() {
            return Ok(());
        }
        self.out.write_all(b"\x1b[?2026h")?;
        self.out.write_all(&self.body)?;
        self.out.write_all(b"\x1b[?2026l")?;
        self.out.flush()
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod frame_tests {
    use super::Frame;

    #[test]
    fn a_frame_is_wrapped_in_the_synchronised_output_pair() {
        let mut out: Vec<u8> = Vec::new();
        let mut frame = Frame::begin(&mut out);
        frame.writer().extend_from_slice(b"drawing");
        frame.present().expect("present");
        assert_eq!(out, b"\x1b[?2026hdrawing\x1b[?2026l");
    }

    #[test]
    fn a_frame_that_drew_nothing_writes_nothing() {
        // The idle loop runs this every tick. Emitting the pair anyway would
        // be sixteen bytes a tick into a terminal where nothing is happening,
        // which is the defect this sits next to rather than a new one.
        let mut out: Vec<u8> = Vec::new();
        let frame = Frame::begin(&mut out);
        frame.present().expect("present");
        assert!(out.is_empty(), "an empty frame wrote {out:?}");
    }

    #[test]
    fn the_pair_brackets_everything_the_frame_wrote() {
        // The whole point: a terminal may present the screen between any two
        // writes, and a frame is dozens of them.
        let mut out: Vec<u8> = Vec::new();
        let mut frame = Frame::begin(&mut out);
        for part in ["pane", "separator", "status", "cursor"] {
            frame.writer().extend_from_slice(part.as_bytes());
        }
        frame.present().expect("present");
        let text = String::from_utf8(out).expect("utf8");
        let begin = text.find("\u{1b}[?2026h").expect("begin marker");
        let end = text.find("\u{1b}[?2026l").expect("end marker");
        assert!(begin < text.find("pane").expect("pane"));
        assert!(end > text.find("cursor").expect("cursor"));
    }
}
