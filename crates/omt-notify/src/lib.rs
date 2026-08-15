//! Telling somebody an agent needs them, when they are looking at something
//! else.
//!
//! The premise of this product is that agents work while you do something
//! else. Everything omt draws — the bar, the panel, the lists — answers "who
//! needs me" *to somebody already looking at the screen*, which is the one
//! case where the answer did not need finding. A person in a browser, in an
//! editor, in another window, learns nothing.
//!
//! Two channels, both of which cost nothing when they are unavailable:
//!
//! * A notification the terminal emulator raises with the operating system.
//!   It is an escape sequence, so it works over ssh, inside tmux, from a
//!   machine with no desktop of its own — which is exactly where agents run.
//! * A sound, played by whatever the machine already has. omt ships no audio:
//!   a repository that carries media carries its licence too, and every system
//!   this runs on already has sounds somebody chose.
//!
//! **The text is untrusted.** A notification body is an agent's own question,
//! and it goes inside an escape sequence. Sanitizing is not tidiness — an
//! unescaped `ESC` in that position is one program writing control sequences
//! into another's terminal.

use std::io::{self, Write};

pub mod policy;
pub mod sound;

pub use policy::{Moment, When, worth_saying};
pub use sound::{Sound, play};

/// A terminal emulator that can raise a notification.
///
/// Named rather than probed, because there is no query for "can you notify"
/// and a sequence sent to an emulator that does not understand it is drawn as
/// text — in the middle of somebody's work. So the rule is: recognized or
/// silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// OSC 9, the widely-copied one.
    Osc9,
    /// OSC 99, with a title and a body as separate fields.
    Osc99,
}

/// What the environment says about the terminal on the other end.
///
/// Taken as a parameter rather than read from the process, so this is testable
/// without setting variables on a whole process — and so a daemon can answer
/// for a *client's* terminal rather than its own.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// `$TERM_PROGRAM`.
    pub term_program: Option<String>,
    /// `$TERM`.
    pub term: Option<String>,
    /// Whether `$KITTY_WINDOW_ID` is set.
    pub kitty_window: bool,
    /// Whether `$TMUX` is set — a sequence has to be wrapped to get through.
    pub inside_tmux: bool,
}

impl Environment {
    /// Read it from this process.
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            term_program: std::env::var("TERM_PROGRAM").ok(),
            term: std::env::var("TERM").ok(),
            kitty_window: std::env::var_os("KITTY_WINDOW_ID").is_some(),
            inside_tmux: std::env::var_os("TMUX").is_some(),
        }
    }

    /// Which backend this terminal has, if any.
    #[must_use]
    pub fn backend(&self) -> Option<Backend> {
        if let Some("ghostty" | "iTerm.app" | "WezTerm") = self.term_program.as_deref() {
            return Some(Backend::Osc9);
        }
        if self.kitty_window {
            return Some(Backend::Osc99);
        }
        match self.term.as_deref() {
            Some("xterm-ghostty") => Some(Backend::Osc9),
            Some("xterm-kitty") => Some(Backend::Osc99),
            Some(term) if term.contains("wezterm") => Some(Backend::Osc9),
            _ => None,
        }
    }
}

/// The bytes that raise one notification, or `None` when nothing here can.
///
/// Separated from writing them so the sequence is testable as a value. A test
/// that had to own a terminal to check an escape sequence is a test nobody
/// writes, and this is a place where a wrong byte is invisible until it is on
/// somebody's screen.
#[must_use]
pub fn sequence(env: &Environment, title: &str, body: Option<&str>) -> Option<Vec<u8>> {
    let backend = env.backend()?;
    let title = sanitize(title);
    let body = body.map(sanitize).filter(|b| !b.is_empty());
    let raw = match backend {
        Backend::Osc9 => {
            let text = body.map_or_else(|| title.clone(), |body| format!("{title}: {body}"));
            format!("\u{1b}]9;{text}\u{1b}\\").into_bytes()
        }
        Backend::Osc99 => body.map_or_else(
            || format!("\u{1b}]99;;{title}\u{1b}\\").into_bytes(),
            |body| {
                format!("\u{1b}]99;i=1:d=0;{title}\u{1b}\\\u{1b}]99;i=1:p=body;{body}\u{1b}\\")
                    .into_bytes()
            },
        ),
    };
    Some(if env.inside_tmux {
        through_tmux(&raw)
    } else {
        raw
    })
}

/// Raise one, on a writer.
///
/// Reports whether anything was written, so a caller can say "your terminal
/// cannot do this" rather than leaving somebody to wonder why nothing
/// happened.
///
/// # Errors
/// Fails only if the writer does.
pub fn raise(
    out: &mut impl Write,
    env: &Environment,
    title: &str,
    body: Option<&str>,
) -> io::Result<bool> {
    let Some(bytes) = sequence(env, title, body) else {
        return Ok(false);
    };
    out.write_all(&bytes)?;
    out.flush()?;
    Ok(true)
}

/// Strip what must never reach a terminal inside a notification.
///
/// The text is an agent's own words. `ESC`, `BEL` and `ST` would end the
/// sequence early and leave the rest to be interpreted as commands, which is
/// one program writing control sequences into another's terminal. Newlines and
/// tabs become spaces because a notification is one line wherever it is shown.
fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '\u{1b}' && *c != '\u{7}' && *c != '\u{9c}')
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            other => other,
        })
        .collect()
}

/// Wrap a sequence so it survives a multiplexer.
///
/// A multiplexer owns the outer terminal and interprets what a program writes;
/// an unwrapped sequence stops at it. The wrapper doubles every `ESC`, which
/// is how the passthrough is specified and not an escaping choice omt makes.
fn through_tmux(sequence: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(sequence.len() + 16);
    out.extend_from_slice(b"\x1bPtmux;");
    for &byte in sequence {
        if byte == 0x1b {
            out.push(0x1b);
        }
        out.push(byte);
    }
    out.extend_from_slice(b"\x1b\\");
    out
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn env(program: Option<&str>, term: Option<&str>) -> Environment {
        Environment {
            term_program: program.map(str::to_owned),
            term: term.map(str::to_owned),
            kitty_window: false,
            inside_tmux: false,
        }
    }

    #[test]
    fn a_terminal_nobody_recognizes_gets_nothing_at_all() {
        // There is no query for "can you notify". A sequence an emulator does
        // not understand is *drawn as text*, in the middle of somebody's work
        // — so the rule is recognized or silent.
        assert!(sequence(&env(None, Some("dumb")), "hi", None).is_none());
        let mut out = Vec::new();
        assert!(!raise(&mut out, &env(None, None), "hi", None).expect("write"));
        assert!(out.is_empty());
    }

    #[test]
    fn an_escape_in_the_text_never_reaches_the_terminal() {
        // The body is an agent's own question. An unescaped ESC there ends the
        // sequence early and the rest is interpreted as commands — one program
        // writing control sequences into another's terminal.
        let hostile = "done\u{1b}]0;stolen title\u{7}";
        let bytes = sequence(&env(Some("ghostty"), None), hostile, None).expect("a sequence");
        let text = String::from_utf8(bytes).expect("utf8");
        assert_eq!(text.matches('\u{1b}').count(), 2, "{text:?}");
        assert!(!text.contains("\u{7}"));
    }

    #[test]
    fn a_body_with_newlines_stays_one_line() {
        let bytes =
            sequence(&env(Some("iTerm.app"), None), "asked", Some("a\nb\tc")).expect("sequence");
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.contains("a b c"), "{text:?}");
    }

    #[test]
    fn the_structured_backend_keeps_the_title_and_body_apart() {
        // The point of the richer sequence: a title somebody can read at a
        // glance and a body they read if it is theirs.
        let env = Environment {
            kitty_window: true,
            ..Environment::default()
        };
        let bytes = sequence(&env, "claude is waiting", Some("app · 2")).expect("sequence");
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.contains("]99;i=1:d=0;claude is waiting"), "{text:?}");
        assert!(text.contains("]99;i=1:p=body;app · 2"), "{text:?}");
    }

    #[test]
    fn inside_a_multiplexer_the_sequence_is_wrapped_to_get_out() {
        // Unwrapped, it stops at the multiplexer, which owns the outer
        // terminal — and the notification is silently never raised.
        let env = Environment {
            term_program: Some("ghostty".to_owned()),
            inside_tmux: true,
            ..Environment::default()
        };
        let bytes = sequence(&env, "hi", None).expect("sequence");
        assert!(bytes.starts_with(b"\x1bPtmux;"), "{bytes:?}");
        assert!(bytes.ends_with(b"\x1b\\"));
        // Every inner ESC doubled, as the passthrough specifies.
        assert!(bytes.windows(2).any(|w| w == b"\x1b\x1b"));
    }

    #[test]
    fn every_named_terminal_is_recognized_by_more_than_one_signal() {
        // `$TERM_PROGRAM` is lost across ssh and `$TERM` is lost inside some
        // multiplexers. Recognizing only one of them means the notification
        // works at a desk and stops at exactly the distance it exists for.
        assert_eq!(env(Some("ghostty"), None).backend(), Some(Backend::Osc9));
        assert_eq!(
            env(None, Some("xterm-ghostty")).backend(),
            Some(Backend::Osc9)
        );
        assert_eq!(
            env(None, Some("xterm-kitty")).backend(),
            Some(Backend::Osc99)
        );
        assert_eq!(
            env(None, Some("wezterm-256color")).backend(),
            Some(Backend::Osc9)
        );
    }
}
