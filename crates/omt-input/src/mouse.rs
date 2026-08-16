//! Encoding a mouse event for the program that asked for it.
//!
//! A report that reaches a program in the wrong shape is worse than one that
//! never arrives, because the program decodes it as something else. Three
//! things are easy to drop here, and each of them silently changes what the
//! user did:
//!
//! * **The modifiers.** Ctrl+click and Alt+drag are distinct gestures to the
//!   program receiving them. Encoding them as a plain click delivers a
//!   different gesture than the one that was made.
//! * **The buttons past the first two.** Middle-click pastes in most programs
//!   that read the mouse at all, and a model with no middle button cannot say
//!   it happened.
//! * **A coordinate that does not fit.** The legacy encoding has one byte per
//!   axis. Wrapping it reports a click somewhere else on the screen, which is
//!   worse than reporting nothing.

/// Which button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The first button.
    Left,
    /// The second.
    Middle,
    /// The third.
    Right,
    /// The wheel, in a direction.
    Wheel(Wheel),
    /// Nothing held, which is what motion without a button reports.
    None,
}

/// Which way the wheel turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    /// Away from the user.
    Up,
    /// Towards.
    Down,
    /// A horizontal wheel or a sideways trackpad gesture.
    Left,
    /// The other way.
    Right,
}

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A button went down.
    Press,
    /// It came up.
    Release,
    /// The pointer moved with a button held.
    Drag,
    /// The pointer moved with nothing held.
    Motion,
}

/// Which modifiers were held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    /// Shift.
    pub shift: bool,
    /// Alt, or Meta.
    pub alt: bool,
    /// Control.
    pub ctrl: bool,
}

/// One mouse event, in a pane's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    /// What happened.
    pub kind: Kind,
    /// Which button it was about.
    pub button: Button,
    /// What was held.
    pub mods: Mods,
    /// Zero-based column within the pane.
    pub col: u16,
    /// Zero-based row within the pane.
    pub row: u16,
}

/// Which encoding the program asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// SGR (1006). The only one that works past column 223.
    Sgr,
    /// The original one, one byte per field.
    Legacy,
}

/// The largest coordinate the legacy encoding can carry.
///
/// One byte, offset by 32, and 255 is the last value that survives the trip.
const LEGACY_MAX: u16 = 255 - 32;

/// The button byte, before the encoding-specific offset.
fn button_bits(event: &MouseEvent) -> u8 {
    let base = match event.button {
        Button::Left => 0,
        Button::Middle => 1,
        Button::Right => 2,
        // Motion with nothing held is reported as "no button", which is the
        // low two bits set — the same value a release uses in the legacy
        // encoding, and distinguished there by the motion bit.
        Button::None => 3,
        // The wheel lives in a high range of its own, so it is never confused
        // with a button being held down.
        Button::Wheel(Wheel::Up) => 64,
        Button::Wheel(Wheel::Down) => 65,
        Button::Wheel(Wheel::Left) => 66,
        Button::Wheel(Wheel::Right) => 67,
    };
    let motion = match event.kind {
        Kind::Drag | Kind::Motion => 32,
        Kind::Press | Kind::Release => 0,
    };
    // Kept, deliberately. A gesture with Ctrl held is a different gesture, and
    // delivering it as a plain click changes what the user did without saying
    // so.
    let mods = u8::from(event.mods.shift) * 4
        + u8::from(event.mods.alt) * 8
        + u8::from(event.mods.ctrl) * 16;
    base + motion + mods
}

/// Encode one event for a program.
///
/// Returns nothing when the event cannot be said in this encoding — which,
/// with the legacy one, means a coordinate too large for a byte. Reporting a
/// wrapped coordinate would be a click somewhere else on the screen.
#[must_use]
pub fn encode(event: &MouseEvent, encoding: Encoding) -> Option<Vec<u8>> {
    // On the wire these are one-based; everything inside omt is zero-based,
    // and the conversion belongs here rather than at every call site.
    let (col, row) = (event.col.saturating_add(1), event.row.saturating_add(1));
    let bits = button_bits(event);
    match encoding {
        Encoding::Sgr => {
            // The release is the only thing the final letter carries, which is
            // why SGR can report *which* button came up and the legacy
            // encoding cannot.
            let last = if event.kind == Kind::Release {
                'm'
            } else {
                'M'
            };
            Some(format!("\x1b[<{bits};{col};{row}{last}").into_bytes())
        }
        Encoding::Legacy => {
            if col > LEGACY_MAX || row > LEGACY_MAX {
                return None;
            }
            // A release is button 3 here: the encoding has no room to say
            // which one, and inventing an answer would tell the program a
            // button it never saw pressed has come up.
            let bits = if event.kind == Kind::Release {
                3 + (bits & !3)
            } else {
                bits
            };
            Some(vec![
                0x1b,
                b'[',
                b'M',
                32u8.saturating_add(bits),
                32u8.saturating_add(col.min(LEGACY_MAX) as u8),
                32u8.saturating_add(row.min(LEGACY_MAX) as u8),
            ])
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

    fn event(kind: Kind, button: Button, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            button,
            mods: Mods::default(),
            col,
            row,
        }
    }

    fn sgr(event: &MouseEvent) -> String {
        String::from_utf8(encode(event, Encoding::Sgr).expect("sgr always encodes"))
            .expect("valid utf8")
    }

    #[test]
    fn a_press_and_its_release_are_told_apart_by_the_final_letter() {
        // The whole reason SGR exists in a world that already had the other
        // one: a release that says which button came up.
        let press = event(Kind::Press, Button::Left, 0, 0);
        assert_eq!(sgr(&press), "\x1b[<0;1;1M");
        let release = event(Kind::Release, Button::Left, 0, 0);
        assert_eq!(sgr(&release), "\x1b[<0;1;1m");
    }

    #[test]
    fn each_button_is_its_own() {
        // Middle-click pastes in most programs that read the mouse at all, so
        // a model that cannot say it happened loses a gesture people use.
        assert_eq!(sgr(&event(Kind::Press, Button::Left, 0, 0)), "\x1b[<0;1;1M");
        assert_eq!(
            sgr(&event(Kind::Press, Button::Middle, 0, 0)),
            "\x1b[<1;1;1M"
        );
        assert_eq!(
            sgr(&event(Kind::Press, Button::Right, 0, 0)),
            "\x1b[<2;1;1M"
        );
    }

    #[test]
    fn what_was_held_travels_with_the_event() {
        // Ctrl+click is a different gesture to the program receiving it.
        let mut e = event(Kind::Press, Button::Left, 0, 0);
        e.mods.ctrl = true;
        assert_eq!(sgr(&e), "\x1b[<16;1;1M");
        e.mods.alt = true;
        assert_eq!(sgr(&e), "\x1b[<24;1;1M");
    }

    #[test]
    fn a_drag_says_it_is_a_drag() {
        let e = event(Kind::Drag, Button::Left, 4, 2);
        assert_eq!(sgr(&e), "\x1b[<32;5;3M");
    }

    #[test]
    fn the_wheel_is_not_a_button_being_held() {
        assert_eq!(
            sgr(&event(Kind::Press, Button::Wheel(Wheel::Up), 0, 0)),
            "\x1b[<64;1;1M"
        );
        assert_eq!(
            sgr(&event(Kind::Press, Button::Wheel(Wheel::Down), 0, 0)),
            "\x1b[<65;1;1M"
        );
    }

    #[test]
    fn sgr_carries_a_column_the_old_encoding_cannot() {
        // The reason SGR is the one omt asks its own terminal for. A wide
        // window past column 223 is ordinary, not exotic.
        let e = event(Kind::Press, Button::Left, 300, 10);
        assert_eq!(sgr(&e), "\x1b[<0;301;11M");
        assert!(
            encode(&e, Encoding::Legacy).is_none(),
            "a wrapped coordinate is a click somewhere else on the screen, \
             which is worse than one that did not happen"
        );
    }

    #[test]
    fn the_old_encoding_says_what_it_can() {
        let bytes =
            encode(&event(Kind::Press, Button::Left, 0, 0), Encoding::Legacy).expect("in range");
        assert_eq!(bytes, b"\x1b[M\x20\x21\x21");
    }

    #[test]
    fn the_old_encoding_cannot_say_which_button_came_up() {
        // It has no room to. Naming one would tell the program a button it
        // never saw pressed has been released.
        let bytes = encode(
            &event(Kind::Release, Button::Middle, 0, 0),
            Encoding::Legacy,
        )
        .expect("in range");
        assert_eq!(bytes[3], 32 + 3);
    }
}
