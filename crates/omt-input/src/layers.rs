//! The four layers a keypress passes through, and what omt may take from them.
//!
//! `docs/architecture/16-input-and-keymap.md` §1.
//!
//! ```text
//! L1  OS / window manager   Cmd+Tab, Cmd+Space, Super+…, Alt+Tab
//! L2  Outer terminal        tabs, splits, its own copy and paste
//! L3  omt                   sees only what L2 forwarded, as bytes
//! L4  The inner program     receives whatever omt passed through
//! ```
//!
//! omt lives at L3 and is squeezed from both sides. The asymmetry is what
//! matters: **omt cannot see L1 or L2 at all.** A chord the window manager
//! grabs never becomes bytes on the pty, so from omt's position it is
//! indistinguishable from a key nobody pressed. There is no runtime check to
//! write — the only defence is not documenting a binding that collides, which
//! makes this a list omt maintains and tests against rather than something it
//! can detect.
//!
//! That is the whole reason this module is data plus an audit rather than a
//! probe. A feature that cannot fail loudly at runtime has to fail at build
//! time instead.
//!
//! L4 is the opposite case: omt *can* see the inner program, and that is what
//! `conflicts` handles. These two modules are the same idea applied to layers
//! with different visibility.

use crate::key::Chord;
use crate::keymap::{Keymap, Mode};

/// Which layer owns a chord omt must not take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// The OS or window manager. omt never sees these at all.
    Os,
    /// The outer terminal emulator. It decides what becomes bytes.
    Terminal,
}

/// A chord omt must not bind, and who owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reserved {
    /// The chord, as written.
    pub chord: &'static str,
    /// Which layer takes it.
    pub layer: Layer,
    /// What it does there, so a refusal can explain itself.
    pub does: &'static str,
}

/// Chords omt must never document a binding for.
///
/// Deliberately conservative and platform-inclusive: a binding that works on
/// Linux and is swallowed on macOS is worse than one that works nowhere,
/// because it gets documented, learned, and then quietly fails for half the
/// people who read the documentation.
pub const RESERVED: &[Reserved] = &[
    Reserved {
        chord: "cmd-tab",
        layer: Layer::Os,
        does: "switches applications on macOS",
    },
    Reserved {
        chord: "cmd-space",
        layer: Layer::Os,
        does: "opens Spotlight on macOS",
    },
    Reserved {
        chord: "cmd-q",
        layer: Layer::Os,
        does: "quits the application",
    },
    Reserved {
        chord: "cmd-h",
        layer: Layer::Os,
        does: "hides the application on macOS",
    },
    Reserved {
        chord: "cmd-m",
        layer: Layer::Os,
        does: "minimises the window",
    },
    Reserved {
        chord: "alt-tab",
        layer: Layer::Os,
        does: "switches windows on Linux and Windows",
    },
    Reserved {
        chord: "ctrl-alt-delete",
        layer: Layer::Os,
        does: "is taken by the system on Windows",
    },
    Reserved {
        chord: "cmd-n",
        layer: Layer::Terminal,
        does: "opens a new terminal window",
    },
    Reserved {
        chord: "cmd-c",
        layer: Layer::Terminal,
        does: "is the terminal's own copy",
    },
    Reserved {
        chord: "cmd-v",
        layer: Layer::Terminal,
        does: "is the terminal's own paste",
    },
    Reserved {
        chord: "cmd-,",
        layer: Layer::Terminal,
        does: "opens the terminal's settings",
    },
];

/// Whether a chord belongs to a layer above omt.
#[must_use]
pub fn reserved_by(chord: &Chord) -> Option<&'static Reserved> {
    let wanted = chord.canonical();
    RESERVED
        .iter()
        .find(|r| Chord::parse(r.chord).is_ok_and(|parsed| parsed.canonical() == wanted))
}

/// Which of a keymap's bindings collide with a layer omt cannot see.
///
/// Every mode, unlike the L4 audit. A copy-mode binding is safe from the inner
/// program because that program is not reading keys — but the window manager
/// is always reading, and it does not care which of omt's surfaces has focus.
#[must_use]
pub fn audit(keymap: &Keymap) -> Vec<(Mode, String, &'static Reserved)> {
    let mut out: Vec<(Mode, String, &'static Reserved)> = keymap
        .bindings()
        .into_iter()
        .filter_map(|(mode, chord, _)| reserved_by(chord).map(|r| (mode, chord.canonical(), r)))
        .collect();
    out.sort_by(|a, b| a.2.layer.cmp(&b.2.layer).then(a.1.cmp(&b.1)));
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
    use crate::keymap::Action;

    fn chord(spec: &str) -> Chord {
        Chord::parse(spec).expect("a chord")
    }

    #[test]
    fn a_chord_the_window_manager_takes_is_recognised() {
        let found = reserved_by(&chord("cmd-tab")).expect("reserved");
        assert_eq!(found.layer, Layer::Os);
        assert!(found.does.contains("switches"));
    }

    #[test]
    fn a_chord_the_outer_terminal_takes_is_recognised_too() {
        // The layer that actually constrains omt: it decides which keys become
        // bytes at all.
        assert_eq!(
            reserved_by(&chord("cmd-c")).expect("reserved").layer,
            Layer::Terminal
        );
    }

    #[test]
    fn an_ordinary_chord_is_not_reserved() {
        assert!(reserved_by(&chord("ctrl-b")).is_none());
        assert!(reserved_by(&chord("cmd-k")).is_none());
    }

    #[test]
    fn omts_own_defaults_take_nothing_from_a_layer_it_cannot_see() {
        // There is no runtime check to write: a chord the window manager grabs
        // never becomes bytes, so from omt's position it is indistinguishable
        // from a key nobody pressed. A feature that cannot fail loudly at
        // runtime has to fail at build time instead, and this is that failure.
        let taken = audit(&crate::keymap::defaults());
        assert!(taken.is_empty(), "omt's defaults collide with: {taken:?}");
    }

    #[test]
    fn the_audit_does_fire_when_a_binding_collides() {
        // Otherwise the test above passes because the audit examines nothing.
        let mut km = Keymap::new();
        let _ = km.bind(
            Mode::Global,
            chord("cmd-q"),
            Action::Capability {
                name: "session.close".to_owned(),
            },
        );
        let found = audit(&km);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].2.layer, Layer::Os);
    }

    #[test]
    fn every_mode_is_audited_not_only_the_global_one() {
        // Unlike the inner-program audit. A copy-mode binding is safe from the
        // program in the pane because it is not reading keys — but the window
        // manager is always reading, and does not care which of omt's surfaces
        // has focus.
        let mut km = Keymap::new();
        let _ = km.bind(
            Mode::Copy,
            chord("cmd-space"),
            Action::Capability {
                name: "ui.close".to_owned(),
            },
        );
        assert_eq!(audit(&km).len(), 1);
    }

    #[test]
    fn every_reserved_entry_parses_and_explains_itself() {
        // An unparseable entry is silently skipped by `reserved_by`, so a typo
        // here would quietly stop protecting a chord.
        for entry in RESERVED {
            assert!(Chord::parse(entry.chord).is_ok(), "`{}`", entry.chord);
            assert!(!entry.does.is_empty(), "`{}`", entry.chord);
        }
    }

    #[test]
    fn the_list_covers_more_than_one_platform() {
        // A binding that works on Linux and is swallowed on macOS is worse
        // than one that works nowhere: it gets documented, learned, and then
        // quietly fails for half the people who read the documentation.
        let text: String = RESERVED.iter().map(|r| r.does).collect();
        assert!(text.contains("macOS"));
        assert!(text.contains("Linux"));
        assert!(text.contains("Windows"));
    }
}
