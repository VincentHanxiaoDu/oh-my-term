//! Keys, chords, and turning them back into the bytes a program expects.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A key, independent of how it was encoded on the wire.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    /// A printable character.
    Char(char),
    /// Enter.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// An arrow.
    Up,
    /// An arrow.
    Down,
    /// An arrow.
    Left,
    /// An arrow.
    Right,
    /// A function key.
    F(u8),
}

/// Which modifiers were held.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct Modifiers {
    /// Control.
    pub ctrl: bool,
    /// Alt / Option.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// Command / Super.
    pub meta: bool,
}

impl Modifiers {
    /// No modifiers.
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    /// Control alone.
    pub const CTRL: Self = Self {
        ctrl: true,
        ..Self::NONE
    };

    /// Whether none are held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.meta
    }
}

/// One keypress.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Chord {
    /// Which key.
    pub key: Key,
    /// Which modifiers.
    pub mods: Modifiers,
}

impl Chord {
    /// A chord with no modifiers.
    #[must_use]
    pub const fn plain(key: Key) -> Self {
        Self {
            key,
            mods: Modifiers::NONE,
        }
    }

    /// A control chord.
    #[must_use]
    pub const fn ctrl(c: char) -> Self {
        Self {
            key: Key::Char(c),
            mods: Modifiers::CTRL,
        }
    }

    /// Parse the config spelling, e.g. `ctrl-shift-p` or `cmd-k`.
    ///
    /// # Errors
    /// Returns the offending token when something in the chord is not a
    /// modifier or a key this build knows.
    pub fn parse(spec: &str) -> Result<Self, ChordParseError> {
        let mut mods = Modifiers::NONE;
        let lower = spec.to_lowercase();
        let parts: Vec<&str> = lower.split('-').collect();
        let Some((last, prefixes)) = parts.split_last() else {
            return Err(ChordParseError::Empty);
        };

        for p in prefixes {
            match *p {
                "ctrl" | "control" | "c" => mods.ctrl = true,
                "alt" | "option" | "opt" | "meta" | "m" => mods.alt = true,
                "shift" | "s" => mods.shift = true,
                // `cmd` and `super` are the same modifier under two names,
                // because a config written on one platform should still load on
                // the other rather than silently binding nothing.
                "cmd" | "command" | "super" | "win" => mods.meta = true,
                other => {
                    return Err(ChordParseError::UnknownModifier {
                        token: other.to_owned(),
                    });
                }
            }
        }

        let key = match *last {
            "enter" | "return" | "cr" => Key::Enter,
            "tab" => Key::Tab,
            "backspace" | "bs" => Key::Backspace,
            "esc" | "escape" => Key::Escape,
            "del" | "delete" => Key::Delete,
            "insert" | "ins" => Key::Insert,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" | "pgup" => Key::PageUp,
            "pagedown" | "pgdn" => Key::PageDown,
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "space" => Key::Char(' '),
            f if f.starts_with('f') && f.len() > 1 => {
                let n: u8 = f[1..].parse().map_err(|_| ChordParseError::UnknownKey {
                    token: f.to_owned(),
                })?;
                Key::F(n)
            }
            single if single.chars().count() == 1 => {
                Key::Char(single.chars().next().unwrap_or('?'))
            }
            other => {
                return Err(ChordParseError::UnknownKey {
                    token: other.to_owned(),
                });
            }
        };

        Ok(Self { key, mods })
    }

    /// The canonical spelling, which is what a config is written back as.
    #[must_use]
    pub fn canonical(&self) -> String {
        let mut s = String::new();
        // A fixed order, so two spellings of one chord compare equal in a
        // config file as well as in memory.
        if self.mods.ctrl {
            s.push_str("ctrl-");
        }
        if self.mods.alt {
            s.push_str("alt-");
        }
        if self.mods.shift {
            s.push_str("shift-");
        }
        if self.mods.meta {
            s.push_str("cmd-");
        }
        s.push_str(&key_name(&self.key));
        s
    }
}

fn key_name(key: &Key) -> String {
    match key {
        Key::Char(' ') => "space".to_owned(),
        Key::Char(c) => c.to_lowercase().to_string(),
        Key::Enter => "enter".to_owned(),
        Key::Tab => "tab".to_owned(),
        Key::Backspace => "backspace".to_owned(),
        Key::Escape => "esc".to_owned(),
        Key::Delete => "delete".to_owned(),
        Key::Insert => "insert".to_owned(),
        Key::Home => "home".to_owned(),
        Key::End => "end".to_owned(),
        Key::PageUp => "pageup".to_owned(),
        Key::PageDown => "pagedown".to_owned(),
        Key::Up => "up".to_owned(),
        Key::Down => "down".to_owned(),
        Key::Left => "left".to_owned(),
        Key::Right => "right".to_owned(),
        Key::F(n) => format!("f{n}"),
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

/// Why a chord could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChordParseError {
    /// The spec was empty.
    #[error("a chord cannot be empty")]
    Empty,
    /// A prefix that is not a modifier.
    #[error("`{token}` is not a modifier")]
    UnknownModifier {
        /// What was written.
        token: String,
    },
    /// A key name nothing knows.
    #[error("`{token}` is not a key")]
    UnknownKey {
        /// What was written.
        token: String,
    },
}

/// How the terminal is currently encoding keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EncodeMode {
    /// DECCKM: cursor keys send `SS3` rather than `CSI`.
    pub application_cursor: bool,
}

/// Encode a chord as the bytes a program on a pty expects.
///
/// This is the inverse of what the terminal parses, and the two have to agree:
/// a key encoded one way and decoded another is a keystroke that arrives as
/// something the user did not type.
#[must_use]
pub fn encode(chord: &Chord, mode: EncodeMode) -> Vec<u8> {
    let m = chord.mods;
    match &chord.key {
        Key::Char(c) if m.ctrl => {
            // Control maps a letter into the C0 range. This is how Ctrl-C has
            // meant interrupt since before any of this existed, and it is why
            // it is position-independent: 0x03 means the same thing whatever is
            // on screen.
            let upper = c.to_ascii_uppercase();
            let byte = match upper {
                '@'..='_' => (upper as u8) - 0x40,
                '?' => 0x7f,
                ' ' => 0,
                _ => return vec![*c as u8],
            };
            if m.alt { vec![0x1b, byte] } else { vec![byte] }
        }
        Key::Char(c) => {
            let mut out = Vec::new();
            if m.alt {
                // Alt is a prefixed escape, which is the convention every
                // terminal and every readline implementation already agrees on.
                out.push(0x1b);
            }
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            out
        }
        Key::Enter => with_alt(m, b"\r"),
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => with_alt(m, b"\t"),
        // The one everybody gets wrong: backspace is DEL, not BS. Sending 0x08
        // makes a shell's erase key stop working on half of all systems.
        Key::Backspace => with_alt(m, b"\x7f"),
        Key::Escape => b"\x1b".to_vec(),
        Key::Up | Key::Down | Key::Right | Key::Left | Key::Home | Key::End => {
            let final_byte = match chord.key {
                Key::Up => b'A',
                Key::Down => b'B',
                Key::Right => b'C',
                Key::Left => b'D',
                Key::Home => b'H',
                _ => b'F',
            };
            if m.is_empty() && mode.application_cursor {
                vec![0x1b, b'O', final_byte]
            } else if m.is_empty() {
                vec![0x1b, b'[', final_byte]
            } else {
                // The modified form carries a parameter; without it a program
                // cannot tell Shift-Left from Left.
                format!("\x1b[1;{}{}", modifier_param(m), final_byte as char).into_bytes()
            }
        }
        Key::Delete => tilde(3, m),
        Key::Insert => tilde(2, m),
        Key::PageUp => tilde(5, m),
        Key::PageDown => tilde(6, m),
        Key::F(n) => function_key(*n, m),
    }
}

fn with_alt(m: Modifiers, bytes: &[u8]) -> Vec<u8> {
    if m.alt {
        let mut v = vec![0x1b];
        v.extend_from_slice(bytes);
        v
    } else {
        bytes.to_vec()
    }
}

/// The xterm modifier parameter: a bitfield plus one.
const fn modifier_param(m: Modifiers) -> u8 {
    let shift = if m.shift { 1 } else { 0 };
    let alt = if m.alt { 2 } else { 0 };
    let ctrl = if m.ctrl { 4 } else { 0 };
    1 + shift + alt + ctrl
}

fn tilde(n: u8, m: Modifiers) -> Vec<u8> {
    if m.is_empty() {
        format!("\x1b[{n}~").into_bytes()
    } else {
        format!("\x1b[{n};{}~", modifier_param(m)).into_bytes()
    }
}

fn function_key(n: u8, m: Modifiers) -> Vec<u8> {
    // F1–F4 use SS3 and the rest use the tilde form. Not a choice — it is what
    // terminfo says, and a program reading these will not accept the other.
    let base: Vec<u8> = match n {
        1..=4 => vec![0x1b, b'O', b'P' + (n - 1)],
        5 => b"\x1b[15~".to_vec(),
        6..=9 => format!("\x1b[{}~", 17 + (n - 6)).into_bytes(),
        10 => b"\x1b[21~".to_vec(),
        11 | 12 => format!("\x1b[{}~", 23 + (n - 11)).into_bytes(),
        _ => return Vec::new(),
    };
    if m.is_empty() {
        return base;
    }
    let param = modifier_param(m);
    match n {
        1..=4 => format!("\x1b[1;{}{}", param, (b'P' + (n - 1)) as char).into_bytes(),
        _ => {
            let text = String::from_utf8_lossy(&base).into_owned();
            text.replace('~', &format!(";{param}~")).into_bytes()
        }
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

    #[test]
    fn ctrl_c_is_the_interrupt_byte() {
        // 0x03 whatever else is happening, which is exactly why it is safe to
        // send without inspecting the screen.
        assert_eq!(encode(&Chord::ctrl('c'), EncodeMode::default()), vec![0x03]);
        assert_eq!(encode(&Chord::ctrl('C'), EncodeMode::default()), vec![0x03]);
    }

    #[test]
    fn ctrl_d_and_ctrl_z_land_where_a_shell_expects_them() {
        assert_eq!(encode(&Chord::ctrl('d'), EncodeMode::default()), vec![0x04]);
        assert_eq!(encode(&Chord::ctrl('z'), EncodeMode::default()), vec![0x1a]);
    }

    #[test]
    fn backspace_is_del_not_bs() {
        // The one everybody gets wrong. Sending 0x08 makes the erase key stop
        // working on half of all systems.
        assert_eq!(
            encode(&Chord::plain(Key::Backspace), EncodeMode::default()),
            vec![0x7f]
        );
    }

    #[test]
    fn alt_is_a_prefixed_escape() {
        let chord = Chord {
            key: Key::Char('b'),
            mods: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        };
        assert_eq!(encode(&chord, EncodeMode::default()), vec![0x1b, b'b']);
    }

    #[test]
    fn arrows_change_shape_in_application_cursor_mode() {
        // A full-screen program sets DECCKM and then expects SS3. Sending CSI
        // anyway is why arrow keys stop working inside some TUIs.
        let up = Chord::plain(Key::Up);
        assert_eq!(encode(&up, EncodeMode::default()), b"\x1b[A".to_vec());
        assert_eq!(
            encode(
                &up,
                EncodeMode {
                    application_cursor: true
                }
            ),
            b"\x1bOA".to_vec()
        );
    }

    #[test]
    fn a_modified_arrow_carries_its_modifier() {
        // Without the parameter a program cannot tell Shift-Left from Left.
        let chord = Chord {
            key: Key::Left,
            mods: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        };
        assert_eq!(encode(&chord, EncodeMode::default()), b"\x1b[1;2D".to_vec());

        let ctrl_right = Chord {
            key: Key::Right,
            mods: Modifiers::CTRL,
        };
        assert_eq!(
            encode(&ctrl_right, EncodeMode::default()),
            b"\x1b[1;5C".to_vec()
        );
    }

    #[test]
    fn a_modified_arrow_ignores_application_cursor_mode() {
        // There is no SS3 form that carries a modifier, so the CSI form is the
        // only correct encoding even when DECCKM is on.
        let chord = Chord {
            key: Key::Up,
            mods: Modifiers::CTRL,
        };
        assert_eq!(
            encode(
                &chord,
                EncodeMode {
                    application_cursor: true
                }
            ),
            b"\x1b[1;5A".to_vec()
        );
    }

    #[test]
    fn shift_tab_is_a_back_tab_rather_than_a_modified_tab() {
        assert_eq!(
            encode(
                &Chord {
                    key: Key::Tab,
                    mods: Modifiers {
                        shift: true,
                        ..Modifiers::NONE
                    }
                },
                EncodeMode::default()
            ),
            b"\x1b[Z".to_vec()
        );
    }

    #[test]
    fn the_editing_keys_use_the_tilde_form() {
        assert_eq!(
            encode(&Chord::plain(Key::Delete), EncodeMode::default()),
            b"\x1b[3~".to_vec()
        );
        assert_eq!(
            encode(&Chord::plain(Key::PageUp), EncodeMode::default()),
            b"\x1b[5~".to_vec()
        );
    }

    #[test]
    fn function_keys_use_the_two_forms_terminfo_specifies() {
        // Not a choice: a program reading these will not accept the other.
        assert_eq!(
            encode(&Chord::plain(Key::F(1)), EncodeMode::default()),
            b"\x1bOP".to_vec()
        );
        assert_eq!(
            encode(&Chord::plain(Key::F(5)), EncodeMode::default()),
            b"\x1b[15~".to_vec()
        );
    }

    #[test]
    fn a_plain_character_is_its_utf8() {
        assert_eq!(
            encode(&Chord::plain(Key::Char('é')), EncodeMode::default()),
            "é".as_bytes().to_vec()
        );
    }

    #[test]
    fn chords_parse_from_the_config_spelling() {
        assert_eq!(Chord::parse("ctrl-c").expect("parse"), Chord::ctrl('c'));
        assert_eq!(
            Chord::parse("shift-tab").expect("parse"),
            Chord {
                key: Key::Tab,
                mods: Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                }
            }
        );
        assert_eq!(Chord::parse("f12").expect("parse").key, Key::F(12));
    }

    #[test]
    fn cmd_and_super_are_the_same_modifier() {
        // A config written on a Mac should still load on Linux rather than
        // silently binding nothing.
        assert_eq!(
            Chord::parse("cmd-k").expect("cmd"),
            Chord::parse("super-k").expect("super")
        );
    }

    #[test]
    fn parsing_is_case_insensitive() {
        assert_eq!(
            Chord::parse("Ctrl-Shift-P").expect("parse"),
            Chord::parse("ctrl-shift-p").expect("parse")
        );
    }

    #[test]
    fn a_nonsense_modifier_is_refused_with_the_token_named() {
        // So the diagnostic can point at the word rather than the whole line.
        let err = Chord::parse("hyper-k").expect_err("must refuse");
        assert!(
            matches!(err, ChordParseError::UnknownModifier { ref token } if token == "hyper"),
            "{err:?}"
        );
    }

    #[test]
    fn a_nonsense_key_is_refused() {
        let err = Chord::parse("ctrl-nonsense").expect_err("must refuse");
        assert!(matches!(err, ChordParseError::UnknownKey { .. }), "{err:?}");
    }

    #[test]
    fn the_canonical_spelling_is_order_independent() {
        // Two spellings of one chord have to compare equal in a config file as
        // well as in memory, or a rebind silently shadows nothing.
        let a = Chord::parse("shift-ctrl-p").expect("a");
        let b = Chord::parse("ctrl-shift-p").expect("b");
        assert_eq!(a, b);
        assert_eq!(a.canonical(), b.canonical());
        assert_eq!(a.canonical(), "ctrl-shift-p");
    }

    #[test]
    fn a_canonical_spelling_parses_back_to_itself() {
        for spec in ["ctrl-c", "alt-left", "cmd-k", "f5", "shift-tab", "space"] {
            let chord = Chord::parse(spec).expect(spec);
            let round = Chord::parse(&chord.canonical()).expect("round trip");
            assert_eq!(chord, round, "{spec}");
        }
    }
}
