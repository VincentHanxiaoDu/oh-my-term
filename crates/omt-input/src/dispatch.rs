//! Deciding whether a keypress is omt's, and passing on everything that is not.
//!
//! `docs/architecture/16-input-and-keymap.md` §2.
//!
//! The load-bearing decision: **passthrough writes the original bytes, never a
//! re-encoding of what omt decoded.** If omt re-encoded, every decode bug would
//! become a corruption bug inside somebody's editor, and any sequence omt does
//! not model would be destroyed on the way through. Keeping the bytes makes
//! passthrough lossless by construction — including for sequences omt does not
//! understand at all: a protocol added after this was written, a mouse report,
//! the body of a bracketed paste.
//!
//! The decoded key exists only to answer one question: is this bound? When the
//! answer is no, the decode is thrown away and the bytes go on untouched. That
//! ordering is what lets omt be wrong about a key without being harmful.

use crate::key::Chord;
use crate::keymap::{Action, Keymap, Mode};

/// What happens to some input bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// omt handles it.
    Handled {
        /// What omt does.
        action: Action,
    },
    /// The leader was pressed; the next key completes a chord.
    Pending,
    /// Not omt's. The original bytes go to the pty, unmodified.
    Passthrough {
        /// Exactly what arrived.
        bytes: Vec<u8>,
    },
    /// Pressed after the leader, and bound to nothing. Nothing happens.
    ///
    /// Distinct from passthrough on purpose. Forwarding it would have the
    /// program underneath act on a key the user meant for omt — the editor
    /// deleting a line because somebody mistyped a leader sequence. Nothing
    /// happening is the boring outcome, and boring is correct here.
    Ignored,
}

/// One keypress, with the bytes it arrived as.
///
/// The bytes are carried alongside the decode rather than reconstructed from
/// it. That is the whole design: a `Chord` omt failed to decode still has its
/// bytes, and they are what gets written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Press {
    /// What omt made of it, if anything.
    ///
    /// `None` for a sequence omt does not model. That is not an error — it is
    /// the common case for anything newer than omt's decoder, and it resolves
    /// to passthrough without a decision being made.
    pub chord: Option<Chord>,
    /// The bytes exactly as they arrived.
    pub bytes: Vec<u8>,
}

/// Where the resolver is in a multi-key sequence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Pending {
    /// Nothing is pending.
    #[default]
    None,
    /// The leader was pressed and omt is waiting for the second key.
    Leader,
}

/// Resolve one keypress.
///
/// `leader` is the chord that opens omt's own bindings. Everything else is
/// passthrough unless it completes a pending leader sequence — which is the
/// rule that keeps omt out of the way: without a leader press first, omt claims
/// nothing at all.
#[must_use]
pub fn resolve(
    press: &Press,
    keymap: &Keymap,
    modes: &[Mode],
    leader: &Chord,
    pending: Pending,
) -> (Resolution, Pending) {
    let Some(chord) = &press.chord else {
        // Undecodable, so certainly not bound. The bytes go on untouched, which
        // is the case this design exists for.
        return (
            Resolution::Passthrough {
                bytes: press.bytes.clone(),
            },
            pending,
        );
    };

    match pending {
        Pending::None => {
            if chord.canonical() == leader.canonical() {
                return (Resolution::Pending, Pending::Leader);
            }
            (
                Resolution::Passthrough {
                    bytes: press.bytes.clone(),
                },
                Pending::None,
            )
        }
        Pending::Leader => {
            // A second leader press means "send the leader through", which is
            // how somebody types the leader into the program underneath. The
            // bytes sent are the original ones, so the inner program receives
            // exactly what its user pressed.
            if chord.canonical() == leader.canonical() {
                return (
                    Resolution::Passthrough {
                        bytes: press.bytes.clone(),
                    },
                    Pending::None,
                );
            }
            match keymap.resolve(chord, modes) {
                // The keymap answers `Passthrough` for anything it has no
                // binding for, including an explicit unbind. After a leader
                // press that means "omt was asked for something it does not
                // have", which is not the same as "this key was never omt's".
                Action::Passthrough | Action::Unbound => (Resolution::Ignored, Pending::None),
                action => (Resolution::Handled { action }, Pending::None),
            }
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

    fn chord(spec: &str) -> Chord {
        Chord::parse(spec).expect("a chord")
    }

    fn press(spec: &str, bytes: &[u8]) -> Press {
        Press {
            chord: Chord::parse(spec).ok(),
            bytes: bytes.to_vec(),
        }
    }

    fn leader() -> Chord {
        chord("ctrl-b")
    }

    fn keymap() -> Keymap {
        crate::keymap::defaults()
    }

    #[test]
    fn an_ordinary_key_goes_straight_through() {
        // Without a leader press first, omt claims nothing at all.
        let p = press("a", b"a");
        let (out, pending) = resolve(&p, &keymap(), &[], &leader(), Pending::None);
        assert_eq!(out, Resolution::Passthrough { bytes: b"a".to_vec() });
        assert_eq!(pending, Pending::None);
    }

    #[test]
    fn the_bytes_that_go_through_are_the_bytes_that_arrived() {
        // The whole design. If omt re-encoded, every decode bug would become a
        // corruption bug inside somebody's editor.
        let arrived = b"\x1b[1;5C".to_vec();
        let p = Press {
            chord: Chord::parse("ctrl-right").ok(),
            bytes: arrived.clone(),
        };
        let (out, _) = resolve(&p, &keymap(), &[], &leader(), Pending::None);
        assert_eq!(out, Resolution::Passthrough { bytes: arrived });
    }

    #[test]
    fn a_sequence_omt_cannot_decode_still_reaches_the_program() {
        // The case this exists for: a protocol added after omt's decoder was
        // written, a mouse report, the body of a paste. Not an error.
        let unknown = b"\x1b[<0;24;12M".to_vec();
        let p = Press {
            chord: None,
            bytes: unknown.clone(),
        };
        let (out, _) = resolve(&p, &keymap(), &[], &leader(), Pending::None);
        assert_eq!(out, Resolution::Passthrough { bytes: unknown });
    }

    #[test]
    fn nothing_undecodable_is_ever_handled() {
        // omt may be wrong about a key without being harmful, and this is the
        // property that guarantees it.
        for bytes in [
            &b"\x1b[<0;24;12M"[..],
            &b"\x1b]52;c;AAAA\x07"[..],
            &b"\xf0\x9f\x92\xa9"[..],
            &b"\x1b[200~pasted\x1b[201~"[..],
        ] {
            let p = Press {
                chord: None,
                bytes: bytes.to_vec(),
            };
            for pending in [Pending::None, Pending::Leader] {
                let (out, _) = resolve(&p, &keymap(), &[], &leader(), pending);
                assert_eq!(
                    out,
                    Resolution::Passthrough {
                        bytes: bytes.to_vec()
                    },
                    "{bytes:?} was claimed"
                );
            }
        }
    }

    #[test]
    fn the_leader_opens_a_pending_sequence_rather_than_doing_anything() {
        let p = press("ctrl-b", b"\x02");
        let (out, pending) = resolve(&p, &keymap(), &[], &leader(), Pending::None);
        assert_eq!(out, Resolution::Pending);
        assert_eq!(pending, Pending::Leader);
    }

    #[test]
    fn pressing_the_leader_twice_sends_it_to_the_program() {
        // How somebody types the leader into whatever is underneath. The bytes
        // sent are the original ones, so the inner program receives exactly
        // what its user pressed.
        let p = press("ctrl-b", b"\x02");
        let (out, pending) = resolve(&p, &keymap(), &[], &leader(), Pending::Leader);
        assert_eq!(out, Resolution::Passthrough { bytes: b"\x02".to_vec() });
        assert_eq!(pending, Pending::None);
    }

    #[test]
    fn an_unbound_key_after_the_leader_is_swallowed_not_forwarded() {
        // Forwarding it would have the inner program act on a key the user
        // meant for omt. Nothing happening is the boring outcome; the editor
        // deleting a line is not.
        let p = press("z", b"z");
        let (out, pending) = resolve(&p, &keymap(), &[], &leader(), Pending::Leader);
        assert_eq!(out, Resolution::Ignored);
        assert_eq!(pending, Pending::None);
    }

    #[test]
    fn a_pending_sequence_always_ends_after_one_key() {
        // A resolver that could stay pending would leave omt eating input with
        // no visible reason, which is indistinguishable from a hang.
        for spec in ["z", "c", "ctrl-b", "?"] {
            let p = press(spec, spec.as_bytes());
            let (_, pending) = resolve(&p, &keymap(), &[], &leader(), Pending::Leader);
            assert_eq!(pending, Pending::None, "still pending after `{spec}`");
        }
    }

    #[test]
    fn a_bound_key_after_the_leader_is_omts() {
        let bindings = keymap();
        // The mode matters: a binding lives in one, and resolving with an
        // empty mode list finds nothing however well the key is bound. That is
        // the resolver working, and it cost a test failure to notice.
        let (mode, bound, _) = bindings
            .bindings()
            .into_iter()
            .find(|(_, _, action)| matches!(action, Action::Capability { .. }))
            .map(|(m, c, a)| (m, c.canonical(), a.clone()))
            .expect("the defaults bind a capability");
        let p = press(&bound, bound.as_bytes());
        let (out, pending) = resolve(&p, &bindings, &[mode], &leader(), Pending::Leader);
        assert!(matches!(out, Resolution::Handled { .. }), "{out:?}");
        assert_eq!(pending, Pending::None);
    }
}
