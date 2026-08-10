//! How a paste can reach this machine, and which way is actually available.
//!
//! `docs/architecture/09-ssh-and-media.md` §5.1. There is no standard by which
//! a terminal emulator hands a pasted image to the foreground application.
//! Bracketed paste carries text. That is the whole problem, and every tier here
//! is a way around it.
//!
//! The rule that makes this worth having: **a tier is never claimed without a
//! positive handshake.** Not "kitty is probably running, so assume OSC 5522" —
//! probably means a paste that silently does nothing, and the user has no way
//! to tell whether they mis-copied or omt mis-guessed. Each tier is either
//! confirmed or it is not reached, and the achieved tier is recorded so every
//! surface can say exactly *why* paste behaves the way it does.
//!
//! Probing is downward: the best mechanism first, falling to the one that
//! always works. Falling *up* would mean a machine that could do the good thing
//! settling for the QR code because the cheap probe answered first.

use serde::{Deserialize, Serialize};

/// How a paste gets here, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// omt on both ends, over its own control channel.
    Native,
    /// A socket forwarded back to an omt instance with a real clipboard.
    ReverseSocket,
    /// omt's own OSC channel plus a companion process on the near side.
    OscBridge,
    /// The terminal's own upload protocol.
    TerminalNative,
    /// A phone, a browser upload, a QR handoff. Always available, always worse.
    OutOfBand,
}

impl Tier {
    /// What to tell somebody about why paste works the way it does.
    #[must_use]
    pub const fn explain(self) -> &'static str {
        match self {
            Self::Native => "omt is running on both ends, so a paste crosses directly.",
            Self::ReverseSocket => {
                "Paste reaches your laptop's clipboard through a forwarded socket."
            }
            Self::OscBridge => {
                "Paste goes through omt's escape-sequence channel and a companion on your laptop."
            }
            Self::TerminalNative => "Your terminal's own upload protocol is carrying the paste.",
            Self::OutOfBand => {
                "No in-band channel is available here. Send the image from your phone or the web client."
            }
        }
    }
}

/// What a probe found. Every field is something observed, never assumed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Probe {
    /// omt answered on the far side of the connection.
    pub omt_on_both_ends: bool,
    /// A forwarded socket exists and an omt instance answered on it.
    ///
    /// Both halves matter. A socket path that exists but answers nothing is the
    /// exact shape of a stale forward from a previous session, and treating its
    /// presence as success gives a paste that vanishes.
    pub reverse_socket_answered: bool,
    /// A companion process on the near side acknowledged omt's OSC handshake.
    pub osc_companion_answered: bool,
    /// The terminal replied to a query about its own upload protocol.
    pub terminal_upload_confirmed: bool,
}

/// Which tier this connection actually achieved, and why.
///
/// Out-of-band is the floor rather than a failure: a phone can always send an
/// image, so there is no state in which paste is impossible — only states in
/// which it is less convenient than somebody hoped.
#[must_use]
pub fn achieved(probe: &Probe) -> Tier {
    if probe.omt_on_both_ends {
        Tier::Native
    } else if probe.reverse_socket_answered {
        Tier::ReverseSocket
    } else if probe.osc_companion_answered {
        Tier::OscBridge
    } else if probe.terminal_upload_confirmed {
        Tier::TerminalNative
    } else {
        Tier::OutOfBand
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
    fn nothing_confirmed_falls_all_the_way_to_out_of_band() {
        // Never "probably kitty, so assume its protocol". Probably means a
        // paste that silently does nothing, and the user cannot tell whether
        // they mis-copied or omt mis-guessed.
        assert_eq!(achieved(&Probe::default()), Tier::OutOfBand);
    }

    #[test]
    fn there_is_no_state_in_which_paste_is_impossible() {
        // A phone can always send an image. Out-of-band is the floor, not a
        // failure.
        assert_eq!(achieved(&Probe::default()), Tier::OutOfBand);
        assert!(!Tier::OutOfBand.explain().is_empty());
    }

    #[test]
    fn the_best_confirmed_mechanism_wins() {
        // Probing downward: a machine that could do the good thing must not
        // settle for the QR code because a cheaper probe answered first.
        let all = Probe {
            omt_on_both_ends: true,
            reverse_socket_answered: true,
            osc_companion_answered: true,
            terminal_upload_confirmed: true,
        };
        assert_eq!(achieved(&all), Tier::Native);
    }

    #[test]
    fn each_tier_is_reached_only_when_its_own_probe_answered() {
        let cases = [
            (
                Probe {
                    reverse_socket_answered: true,
                    ..Probe::default()
                },
                Tier::ReverseSocket,
            ),
            (
                Probe {
                    osc_companion_answered: true,
                    ..Probe::default()
                },
                Tier::OscBridge,
            ),
            (
                Probe {
                    terminal_upload_confirmed: true,
                    ..Probe::default()
                },
                Tier::TerminalNative,
            ),
        ];
        for (probe, expected) in cases {
            assert_eq!(achieved(&probe), expected, "{probe:?}");
        }
    }

    #[test]
    fn a_socket_that_exists_but_answers_nothing_is_not_a_tier() {
        // The exact shape of a stale forward from a previous session. Treating
        // its presence as success gives a paste that vanishes, which is the
        // failure this whole ladder exists to make impossible.
        let stale = Probe {
            reverse_socket_answered: false,
            ..Probe::default()
        };
        assert_eq!(achieved(&stale), Tier::OutOfBand);
    }

    #[test]
    fn every_tier_can_say_why_paste_behaves_as_it_does() {
        for tier in [
            Tier::Native,
            Tier::ReverseSocket,
            Tier::OscBridge,
            Tier::TerminalNative,
            Tier::OutOfBand,
        ] {
            let said = tier.explain();
            assert!(!said.is_empty(), "{tier:?}");
            assert!(said.ends_with('.'), "{tier:?}: {said}");
        }
    }

    #[test]
    fn the_ordering_is_best_first_so_comparisons_mean_what_they_read() {
        assert!(Tier::Native < Tier::ReverseSocket);
        assert!(Tier::ReverseSocket < Tier::OscBridge);
        assert!(Tier::OscBridge < Tier::TerminalNative);
        assert!(Tier::TerminalNative < Tier::OutOfBand);
    }
}
