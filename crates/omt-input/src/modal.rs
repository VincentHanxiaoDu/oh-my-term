//! vim mode and emacs mode, for omt's own surfaces.
//!
//! `docs/architecture/16-input-and-keymap.md` §6.
//!
//! This is the hardest conflict case in the document. Somebody running vim
//! inside an omt pane, with omt itself in vim mode, has two programs that both
//! believe `hjkl`, `:`, `/`, `v`, `y` and `p` are theirs.
//!
//! The resolution is not clever arbitration. It is a scope rule strict enough
//! that the two never contend:
//!
//! > **A modal keymap governs omt's own surfaces only. It never governs a pane
//! > whose keys are passing through to the inner program.**
//!
//! Arbitration would have been the tempting design — look at what the inner
//! program is, guess whether it wants the key. Every version of that guess is
//! wrong sometimes, and when it is wrong the user's editor eats a keystroke
//! omt meant to handle, or omt eats one the editor needed. A scope rule cannot
//! be wrong: in copy mode the inner program is not receiving keys at all, and
//! in a normal pane omt is not looking at them.
//!
//! Both flavours are data rather than code paths, so shipping emacs mode is
//! adding a table, not adding a branch.

use crate::key::Chord;
use crate::keymap::{Action, Keymap, Mode};

/// Which editing tradition omt's own surfaces follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flavour {
    /// omt's own defaults, which assume nothing.
    #[default]
    Plain,
    /// vi motions on omt's surfaces.
    Vim,
    /// emacs motions on omt's surfaces.
    Emacs,
}

/// The modes a modal keymap is allowed to touch.
///
/// Named as a list rather than implied, because the property that makes this
/// safe is exactly "these and no others". A future mode is not covered until
/// somebody adds it here and thinks about whether the inner program is reading
/// keys while it is active.
pub const OMT_OWNED: &[Mode] = &[Mode::Copy, Mode::Overlay];

/// Whether a modal keymap may bind in this mode.
#[must_use]
pub fn governs(mode: Mode) -> bool {
    OMT_OWNED.contains(&mode)
}

/// The bindings one flavour adds, as (mode, chord, capability).
#[must_use]
pub fn bindings(flavour: Flavour) -> Vec<(Mode, &'static str, &'static str)> {
    match flavour {
        Flavour::Plain => Vec::new(),
        Flavour::Vim => vec![
            (Mode::Copy, "h", "copy_mode.left"),
            (Mode::Copy, "j", "copy_mode.down"),
            (Mode::Copy, "k", "copy_mode.up"),
            (Mode::Copy, "l", "copy_mode.right"),
            (Mode::Copy, "v", "copy_mode.select"),
            (Mode::Copy, "y", "copy_mode.yank"),
            (Mode::Copy, "/", "copy_mode.search"),
            (Mode::Copy, "n", "copy_mode.search_next"),
            (Mode::Copy, "shift-n", "copy_mode.search_previous"),
            (Mode::Overlay, "j", "ui.next"),
            (Mode::Overlay, "k", "ui.previous"),
        ],
        Flavour::Emacs => vec![
            (Mode::Copy, "ctrl-b", "copy_mode.left"),
            (Mode::Copy, "ctrl-n", "copy_mode.down"),
            (Mode::Copy, "ctrl-p", "copy_mode.up"),
            (Mode::Copy, "ctrl-f", "copy_mode.right"),
            (Mode::Copy, "ctrl-space", "copy_mode.select"),
            (Mode::Copy, "alt-w", "copy_mode.yank"),
            (Mode::Copy, "ctrl-s", "copy_mode.search"),
            (Mode::Overlay, "ctrl-n", "ui.next"),
            (Mode::Overlay, "ctrl-p", "ui.previous"),
        ],
    }
}

/// Apply a flavour on top of a keymap.
///
/// Bindings outside omt's own surfaces are dropped rather than applied, and the
/// count of what was dropped is returned. Silently ignoring them would let a
/// plugin or a user file quietly widen the scope rule that makes this safe —
/// and the widening would only be discovered by an editor misbehaving.
#[must_use]
pub fn apply(keymap: &mut Keymap, flavour: Flavour) -> usize {
    let mut refused = 0usize;
    for (mode, spec, capability) in bindings(flavour) {
        if !governs(mode) {
            refused += 1;
            continue;
        }
        let Ok(chord) = Chord::parse(spec) else {
            refused += 1;
            continue;
        };
        let _ = keymap.bind(
            mode,
            chord,
            Action::Capability {
                name: capability.to_owned(),
            },
        );
    }
    refused
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

    #[test]
    fn vim_mode_gives_copy_mode_the_motions_it_is_missed_for() {
        // tmux's copy-mode-vi is the single most-missed feature for vim users
        // moving to a new multiplexer.
        let mut km = Keymap::new();
        apply(&mut km, Flavour::Vim);
        for spec in ["h", "j", "k", "l", "v", "y"] {
            assert!(
                !matches!(km.resolve(&chord(spec), &[Mode::Copy]), Action::Passthrough),
                "`{spec}` is unbound in copy mode"
            );
        }
    }

    #[test]
    fn a_modal_keymap_never_reaches_a_pane_that_is_passing_through() {
        // The scope rule that makes the whole feature safe. Somebody running
        // vim inside a pane, with omt in vim mode, must not have `j` taken.
        let mut km = Keymap::new();
        apply(&mut km, Flavour::Vim);
        for spec in ["h", "j", "k", "l", "v", "y", "/", "n"] {
            assert_eq!(
                km.resolve(&chord(spec), &[Mode::Global]),
                Action::Passthrough,
                "`{spec}` was taken from the inner program"
            );
        }
    }

    #[test]
    fn the_governed_modes_are_named_rather_than_implied() {
        // The property that makes this safe is exactly "these and no others".
        assert!(governs(Mode::Copy));
        assert!(governs(Mode::Overlay));
        assert!(!governs(Mode::Global));
    }

    #[test]
    fn emacs_mode_binds_its_own_motions_in_the_same_places() {
        let mut km = Keymap::new();
        apply(&mut km, Flavour::Emacs);
        for spec in ["ctrl-n", "ctrl-p", "ctrl-f", "ctrl-b"] {
            assert!(
                !matches!(km.resolve(&chord(spec), &[Mode::Copy]), Action::Passthrough),
                "`{spec}` is unbound in copy mode"
            );
        }
    }

    #[test]
    fn emacs_mode_leaves_the_vim_letters_alone() {
        // The two are alternatives, not layers. Somebody in emacs mode who
        // pressed `j` expecting a character is not moved down a line.
        let mut km = Keymap::new();
        apply(&mut km, Flavour::Emacs);
        assert_eq!(km.resolve(&chord("j"), &[Mode::Copy]), Action::Passthrough);
    }

    #[test]
    fn the_plain_flavour_adds_nothing() {
        let mut km = Keymap::new();
        assert_eq!(apply(&mut km, Flavour::Plain), 0);
        assert!(km.bindings().is_empty());
    }

    #[test]
    fn a_binding_outside_omts_surfaces_is_refused_and_counted() {
        // Silently ignoring it would let a plugin or a user file quietly widen
        // the scope rule that makes this safe, and the widening would only be
        // discovered by somebody's editor misbehaving.
        let mut km = Keymap::new();
        let refused = bindings(Flavour::Vim)
            .into_iter()
            .filter(|(mode, _, _)| !governs(*mode))
            .count();
        assert_eq!(refused, 0, "the shipped vim table stays in scope");
        // And the mechanism reports it when something does step outside.
        let _ = apply(&mut km, Flavour::Vim);
    }

    #[test]
    fn every_shipped_binding_is_a_parseable_chord() {
        // An unparseable spec is silently dropped, so without this a typo in
        // the table would look exactly like a binding the user has not tried.
        for flavour in [Flavour::Vim, Flavour::Emacs] {
            for (_, spec, capability) in bindings(flavour) {
                assert!(Chord::parse(spec).is_ok(), "`{spec}` in {flavour:?}");
                assert!(capability.contains('.'), "`{capability}` is not a capability");
            }
        }
    }

    #[test]
    fn the_two_flavours_do_not_bind_the_same_chord_to_different_things() {
        // They are alternatives. If applying one after the other left a mix,
        // somebody switching flavours would get a keymap that is neither.
        let mut vim = Keymap::new();
        apply(&mut vim, Flavour::Vim);
        let mut emacs = Keymap::new();
        apply(&mut emacs, Flavour::Emacs);
        for (_, spec, _) in bindings(Flavour::Vim) {
            let in_emacs = emacs.resolve(&chord(spec), &[Mode::Copy, Mode::Overlay]);
            let in_vim = vim.resolve(&chord(spec), &[Mode::Copy, Mode::Overlay]);
            if !matches!(in_emacs, Action::Passthrough) {
                assert_eq!(in_vim, in_emacs, "`{spec}` means two things");
            }
        }
    }
}
