//! Who owns a click.
//!
//! `docs/architecture/18-semantic-open.md` §5.1. This is the part that decides
//! whether semantic open is a real feature or a broken editor.
//!
//! When the program in the pane has enabled mouse reporting, **every button
//! press is that program's input**. vim, helix, htop, lazygit and essentially
//! every agent TUI do this, so it is the common case rather than an edge case,
//! and the design starts from it. A multiplexer that swallows clicks so its own
//! link handler can run has broken vim, and no link feature is worth that.
//!
//! The load-bearing row is Shift+click while reporting is on. That Shift
//! bypasses application mouse reporting is long-standing xterm behaviour,
//! implemented by every terminal omt cares about, and applications
//! overwhelmingly do not expect shifted clicks *because* the convention exists.
//! So omt does the same thing one level down: with reporting on it consumes
//! Shift+click itself and forwards everything else untouched.
//!
//! Alt+click is forwarded even though omt would like it, because many
//! applications already use it. Wanting a binding is not a reason to take one
//! somebody else is using.

use serde::Serialize;

/// Which modifiers were held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Cmd on macOS, Super elsewhere.
    pub super_key: bool,
    /// Alt / Option.
    pub alt: bool,
}

/// What happens to a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    /// Passed to the program in the pane, untouched.
    Application,
    /// omt activates whatever is under the pointer.
    Activate,
    /// omt starts or replaces a selection.
    Selection,
    /// omt extends the current selection.
    ExtendSelection,
    /// omt starts a rectangular selection.
    BlockSelection,
}

/// Decide who owns a click.
///
/// The whole table from §5.1, written as one function so the two halves cannot
/// drift: the rules for reporting-on and reporting-off are the same decision
/// asked under different conditions, and splitting them is how a modifier ends
/// up handled in one branch and forgotten in the other.
#[must_use]
pub fn owner(mouse_reporting: bool, modifiers: Modifiers) -> Owner {
    if mouse_reporting {
        // Shift first: it is the one omt claims, and claiming it is what makes
        // activation possible at all inside an application that grabbed the
        // mouse.
        if modifiers.shift {
            return Owner::Activate;
        }
        if modifiers.super_key {
            // Only if the outer terminal delivers it at all — omt cannot
            // guarantee that, so this is offered rather than promised.
            return Owner::Activate;
        }
        // Everything else, including Alt, goes to the application. Many apps
        // already use Alt+click, and wanting a binding is not a reason to take
        // one somebody else is using.
        return Owner::Application;
    }

    if modifiers.super_key {
        Owner::Activate
    } else if modifiers.shift {
        Owner::ExtendSelection
    } else if modifiers.alt {
        Owner::BlockSelection
    } else {
        Owner::Selection
    }
}

/// Whether omt consumed the click, or the program under it did.
#[must_use]
pub fn consumed(owner: Owner) -> bool {
    owner != Owner::Application
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::default()
        }
    }

    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Modifiers::default()
        }
    }

    fn super_key() -> Modifiers {
        Modifiers {
            super_key: true,
            ..Modifiers::default()
        }
    }

    #[test]
    fn a_plain_click_inside_vim_belongs_to_vim() {
        // The rule the whole module exists to protect. A multiplexer that
        // swallows this has broken every editor, and no link feature is worth
        // that.
        assert_eq!(owner(true, Modifiers::default()), Owner::Application);
    }

    #[test]
    fn shift_click_is_the_one_omt_claims_while_reporting_is_on() {
        // The load-bearing row. Applications overwhelmingly do not expect
        // shifted clicks, precisely because the xterm convention exists.
        assert_eq!(owner(true, shift()), Owner::Activate);
    }

    #[test]
    fn alt_click_is_forwarded_even_though_omt_would_like_it() {
        // Many applications already use it. Wanting a binding is not a reason
        // to take one somebody else is using.
        assert_eq!(owner(true, alt()), Owner::Application);
    }

    #[test]
    fn with_reporting_off_a_plain_click_selects() {
        assert_eq!(owner(false, Modifiers::default()), Owner::Selection);
        assert_eq!(owner(false, shift()), Owner::ExtendSelection);
        assert_eq!(owner(false, alt()), Owner::BlockSelection);
    }

    #[test]
    fn super_click_activates_either_way() {
        // The one modifier that means the same thing in both states, so
        // somebody who learns it does not have to also learn when it applies.
        assert_eq!(owner(false, super_key()), Owner::Activate);
        assert_eq!(owner(true, super_key()), Owner::Activate);
    }

    #[test]
    fn shift_means_different_things_by_state_and_that_is_deliberate() {
        // With reporting off there is a selection to extend; with it on there
        // is not, and Shift is the only key applications reliably ignore.
        assert_eq!(owner(false, shift()), Owner::ExtendSelection);
        assert_eq!(owner(true, shift()), Owner::Activate);
    }

    #[test]
    fn every_combination_has_an_answer() {
        // A modifier handled in one branch and forgotten in the other is
        // exactly the drift that writing this as one function prevents.
        for reporting in [true, false] {
            for shift in [true, false] {
                for super_key in [true, false] {
                    for alt in [true, false] {
                        let m = Modifiers {
                            shift,
                            super_key,
                            alt,
                        };
                        let _ = owner(reporting, m);
                    }
                }
            }
        }
    }

    #[test]
    fn nothing_omt_claims_is_also_forwarded() {
        // Both would mean a click that selects in omt and clicks in vim.
        for m in [shift(), super_key()] {
            assert!(consumed(owner(true, m)));
        }
        assert!(!consumed(owner(true, Modifiers::default())));
        assert!(!consumed(owner(true, alt())));
    }

    #[test]
    fn with_reporting_on_omt_claims_exactly_two_combinations() {
        // Counted, because the cost of each one is a binding taken away from
        // the program in the pane.
        let claimed = [
            Modifiers::default(),
            shift(),
            alt(),
            super_key(),
            Modifiers {
                shift: true,
                alt: true,
                super_key: false,
            },
        ]
        .into_iter()
        .filter(|m| consumed(owner(true, *m)))
        .count();
        assert_eq!(claimed, 3, "shift, super, and shift+alt");
    }
}
