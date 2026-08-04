//! Resolving a keypress to an action, or letting it through untouched.
//!
//! The default is **passthrough**. omt is a terminal before it is anything
//! else, and a key that omt has no opinion about must reach the program the
//! user is actually talking to, byte for byte.

use std::collections::BTreeMap;

use crate::key::Chord;

/// What a keypress does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Invoke a capability.
    Capability {
        /// Its name.
        name: String,
    },
    /// Send the bytes to the program, as if omt were not here.
    Passthrough,
    /// Explicitly unbound: resolution stops and falls through to passthrough.
    ///
    /// Distinct from "no binding" so a user can remove a default without
    /// knowing what layer put it there.
    Unbound,
}

/// Which keymap layer a binding lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    /// Always active.
    Global,
    /// Active while a modal overlay is open.
    Overlay,
    /// Active in copy mode.
    Copy,
    /// A vim-style normal mode, for those who want it.
    VimNormal,
}

/// Keys omt refuses to bind globally, and why.
///
/// Not a preference. A configuration that made `Ctrl+C` unreachable produces a
/// terminal in which a runaway process cannot be stopped — and the user finds
/// out during the emergency, which is the worst possible moment to discover a
/// keybinding problem.
pub const RESERVED_GLOBAL: &[(&str, &str)] = &[
    (
        "ctrl-c",
        "it must reach the inner program: it is how every CLI is interrupted",
    ),
    (
        "ctrl-d",
        "it must reach the inner program: it is end-of-input to every shell and REPL",
    ),
    (
        "ctrl-z",
        "it must reach the inner program: it is how a job is suspended",
    ),
    (
        "ctrl-\\",
        "it must reach the inner program: it is the quit signal of last resort",
    ),
];

/// The diagnostic code for refusing a reserved binding.
pub const RESERVED_BINDING: &str = "OMT-C410";

/// Why a binding was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("[{code}] omt refuses to bind `{chord}` globally: {reason}")]
pub struct BindingRefused {
    /// The stable, greppable code.
    pub code: &'static str,
    /// The chord that was refused.
    pub chord: String,
    /// Why, in words a user can act on.
    pub reason: &'static str,
}

/// A set of bindings, resolved most-specific-first.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    bindings: BTreeMap<(Mode, Chord), Action>,
}

impl Keymap {
    /// An empty keymap: everything passes through.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a chord in a mode.
    ///
    /// # Errors
    /// Refuses a reserved chord in the global layer, naming why. A mode-local
    /// binding is allowed: while a palette is open, `Ctrl+C` closing the
    /// palette is right, and the alternative — an overlay that leaks `Ctrl+C`
    /// to a running build — is a data-loss bug.
    pub fn bind(&mut self, mode: Mode, chord: Chord, action: Action) -> Result<(), BindingRefused> {
        if mode == Mode::Global && action != Action::Passthrough {
            let canonical = chord.canonical();
            if let Some((_, reason)) = RESERVED_GLOBAL.iter().find(|(c, _)| *c == canonical) {
                return Err(BindingRefused {
                    code: RESERVED_BINDING,
                    chord: canonical,
                    reason,
                });
            }
        }
        self.bindings.insert((mode, chord), action);
        Ok(())
    }

    /// What a chord does, given which modes are active.
    ///
    /// Modes are tried most specific first, and anything without a binding
    /// passes through. Passthrough as the default rather than as a fallback
    /// case is what makes omt lossless for programs it knows nothing about.
    #[must_use]
    pub fn resolve(&self, chord: &Chord, active: &[Mode]) -> Action {
        for mode in active {
            match self.bindings.get(&(*mode, chord.clone())) {
                // An explicit unbind stops resolution here rather than
                // continuing to a lower layer — which is what makes it possible
                // to remove a default without knowing where it came from.
                Some(Action::Unbound) => return Action::Passthrough,
                Some(action) => return action.clone(),
                None => {}
            }
        }
        Action::Passthrough
    }

    /// Every binding, in a stable order.
    #[must_use]
    pub fn bindings(&self) -> Vec<(Mode, &Chord, &Action)> {
        self.bindings.iter().map(|((m, c), a)| (*m, c, a)).collect()
    }

    /// Chords bound to more than one action across modes.
    ///
    /// Not an error — that is what modes are for — but a settings UI has to be
    /// able to show it, since a binding shadowed by a mode the user forgot
    /// about is otherwise indistinguishable from one that does not work.
    #[must_use]
    pub fn shadowed(&self) -> Vec<&Chord> {
        let mut seen: BTreeMap<&Chord, usize> = BTreeMap::new();
        for (_, chord) in self.bindings.keys() {
            *seen.entry(chord).or_default() += 1;
        }
        seen.into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(c, _)| c)
            .collect()
    }
}

/// The default keymap.
///
/// Deliberately small. Every binding here is a key omt took from the program
/// underneath, and each one has to earn that.
#[must_use]
pub fn defaults() -> Keymap {
    let mut km = Keymap::new();
    let mut bind = |mode: Mode, spec: &str, action: &str| {
        if let Ok(chord) = Chord::parse(spec) {
            // The defaults are checked by the same rule as a user's config, so
            // a reserved key cannot sneak in through the built-ins.
            let _ = km.bind(
                mode,
                chord,
                Action::Capability {
                    name: action.to_owned(),
                },
            );
        }
    };

    bind(Mode::Global, "cmd-k", "palette.open");
    bind(Mode::Global, "ctrl-shift-p", "palette.open");
    bind(Mode::Global, "cmd-t", "session.create");
    bind(Mode::Global, "cmd-d", "pane.split_right");
    bind(Mode::Global, "cmd-shift-d", "pane.split_down");
    bind(Mode::Global, "cmd-w", "pane.close");
    bind(Mode::Global, "cmd-f", "search.open");
    bind(Mode::Global, "cmd-alt-left", "pane.focus_left");
    bind(Mode::Global, "cmd-alt-right", "pane.focus_right");
    bind(Mode::Global, "cmd-alt-up", "pane.focus_up");
    bind(Mode::Global, "cmd-alt-down", "pane.focus_down");

    // Inside an overlay these are right, and only inside one: an overlay that
    // leaked Ctrl+C to a running build would lose the user's work.
    bind(Mode::Overlay, "ctrl-c", "ui.close");
    bind(Mode::Overlay, "esc", "ui.close");
    bind(Mode::Overlay, "enter", "ui.accept");

    bind(Mode::Copy, "esc", "ui.close");
    bind(Mode::Copy, "y", "copy_mode.yank");

    km
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::key::{Key, Modifiers};

    fn capability(name: &str) -> Action {
        Action::Capability {
            name: name.to_owned(),
        }
    }

    #[test]
    fn an_unbound_key_passes_through() {
        // omt is a terminal first. A key it has no opinion about must reach the
        // program the user is talking to, byte for byte.
        let km = defaults();
        assert_eq!(
            km.resolve(&Chord::plain(Key::Char('a')), &[Mode::Global]),
            Action::Passthrough
        );
    }

    #[test]
    fn ctrl_c_cannot_be_bound_globally() {
        // A config that made it unreachable produces a terminal where a runaway
        // process cannot be stopped, and the user finds out mid-emergency.
        let mut km = Keymap::new();
        let err = km
            .bind(Mode::Global, Chord::ctrl('c'), capability("session.close"))
            .expect_err("must refuse");
        assert_eq!(err.code, RESERVED_BINDING);
        assert!(err.reason.contains("interrupted"), "{}", err.reason);
    }

    #[test]
    fn the_other_signal_keys_are_reserved_too() {
        let mut km = Keymap::new();
        for spec in ["ctrl-d", "ctrl-z", "ctrl-\\"] {
            let chord = Chord::parse(spec).expect(spec);
            assert!(
                km.bind(Mode::Global, chord, capability("x")).is_err(),
                "{spec} should be reserved"
            );
        }
    }

    #[test]
    fn ctrl_c_reaches_the_program_by_default() {
        let km = defaults();
        assert_eq!(
            km.resolve(&Chord::ctrl('c'), &[Mode::Global]),
            Action::Passthrough
        );
    }

    #[test]
    fn ctrl_c_may_be_bound_inside_an_overlay() {
        // While a palette is open it should close the palette. The alternative
        // — an overlay that leaks Ctrl+C to a running build — is a data-loss
        // bug, not a purity win.
        let km = defaults();
        assert_eq!(
            km.resolve(&Chord::ctrl('c'), &[Mode::Overlay, Mode::Global]),
            capability("ui.close")
        );
    }

    #[test]
    fn a_more_specific_mode_wins() {
        let km = defaults();
        assert_eq!(
            km.resolve(&Chord::plain(Key::Escape), &[Mode::Overlay, Mode::Global]),
            capability("ui.close")
        );
        assert_eq!(
            km.resolve(&Chord::plain(Key::Escape), &[Mode::Global]),
            Action::Passthrough,
            "and outside the overlay it belongs to the program"
        );
    }

    #[test]
    fn an_explicit_unbind_removes_a_default_without_falling_through() {
        // So a user can drop a binding without knowing which layer put it
        // there.
        let mut km = defaults();
        let chord = Chord::parse("cmd-k").expect("parse");
        assert_eq!(
            km.resolve(&chord, &[Mode::Global]),
            capability("palette.open")
        );
        km.bind(Mode::Global, chord.clone(), Action::Unbound)
            .expect("unbind");
        assert_eq!(km.resolve(&chord, &[Mode::Global]), Action::Passthrough);
    }

    #[test]
    fn rebinding_replaces_rather_than_accumulating() {
        let mut km = defaults();
        let chord = Chord::parse("cmd-k").expect("parse");
        km.bind(Mode::Global, chord.clone(), capability("mine"))
            .expect("rebind");
        assert_eq!(km.resolve(&chord, &[Mode::Global]), capability("mine"));
    }

    #[test]
    fn two_spellings_of_one_chord_are_the_same_binding() {
        // Otherwise a rebind silently shadows nothing and the user's key still
        // does the old thing.
        let mut km = Keymap::new();
        km.bind(
            Mode::Global,
            Chord::parse("ctrl-shift-p").expect("a"),
            capability("first"),
        )
        .expect("bind");
        km.bind(
            Mode::Global,
            Chord::parse("shift-ctrl-p").expect("b"),
            capability("second"),
        )
        .expect("rebind");
        assert_eq!(km.bindings().len(), 1);
        assert_eq!(
            km.resolve(&Chord::parse("ctrl-shift-p").expect("c"), &[Mode::Global]),
            capability("second")
        );
    }

    #[test]
    fn the_defaults_contain_no_reserved_global_binding() {
        // The built-ins are checked by the same rule as a user's config, so a
        // reserved key cannot sneak in through them.
        let km = defaults();
        for (spec, _) in RESERVED_GLOBAL {
            let chord = Chord::parse(spec).expect(spec);
            assert_eq!(
                km.resolve(&chord, &[Mode::Global]),
                Action::Passthrough,
                "{spec} was bound globally by default"
            );
        }
    }

    #[test]
    fn a_shadowed_chord_is_reportable() {
        // Not an error — that is what modes are for — but a binding shadowed by
        // a mode the user forgot about is otherwise indistinguishable from one
        // that does not work.
        let km = defaults();
        let shadowed: Vec<String> = km.shadowed().iter().map(|c| c.canonical()).collect();
        assert!(shadowed.contains(&"esc".to_owned()), "{shadowed:?}");
    }

    #[test]
    fn resolution_with_no_active_modes_passes_everything_through() {
        let km = defaults();
        assert_eq!(
            km.resolve(&Chord::parse("cmd-k").expect("parse"), &[]),
            Action::Passthrough
        );
    }

    #[test]
    fn a_binding_may_be_passthrough_explicitly_even_for_a_reserved_key() {
        // Writing down what already happens must not be an error.
        let mut km = Keymap::new();
        km.bind(Mode::Global, Chord::ctrl('c'), Action::Passthrough)
            .expect("stating the default is allowed");
    }

    #[test]
    fn every_default_binding_uses_a_modifier_the_program_will_not_miss() {
        // A plain letter bound globally would make that letter untypable.
        for (mode, chord, _) in defaults().bindings() {
            if mode == Mode::Global {
                assert!(
                    chord.mods != Modifiers::NONE,
                    "{} is bound globally with no modifier",
                    chord.canonical()
                );
            }
        }
    }
}
