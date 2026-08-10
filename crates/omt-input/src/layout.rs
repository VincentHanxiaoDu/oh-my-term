//! Bindings that survive a keyboard that is not American.
//!
//! `docs/architecture/16-input-and-keymap.md` §9.1.
//!
//! Binding on a *character* is wrong for anyone not on a US layout. `⟨leader⟩ |`
//! needs `AltGr` on a German keyboard and does not exist on a French AZERTY
//! without a dead-key dance. A default that requires a three-finger contortion
//! is a default those users rebind, and the ones who do not simply conclude the
//! feature is not for them.
//!
//! The rule: **a binding resolves against the base-layout key when the terminal
//! reports it, and against the produced character otherwise.** The kitty
//! protocol's alternate-key reporting supplies the base-layout code, so a
//! binding matches both the physical key that produces the character on the
//! user's own layout and the US-layout position.
//!
//! Under legacy encoding only the produced character is available. omt does not
//! pretend otherwise: it says which of the two it is using, because the
//! difference is invisible until a binding does not fire and the user has no
//! way to tell whether they mistyped or omt cannot see their layout.

use crate::key::Chord;

/// What the terminal was able to tell omt about a keypress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    /// The character the key produced on this layout.
    pub produced: String,
    /// The key at that physical position on the base layout, when known.
    ///
    /// `None` under legacy encoding, which is not a failure — it is most
    /// terminals most of the time, and the resolution below still works.
    pub base_layout: Option<String>,
}

/// How a binding was matched, so `omt keys explain` can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matched {
    /// Matched the character the key produced.
    Produced,
    /// Matched the physical key's base-layout identity.
    BaseLayout,
    /// Did not match.
    No,
}

/// Whether a reported key satisfies a bound chord.
///
/// Either identity matching is enough. A German user pressing the physical key
/// that is `\` on a US layout gets the binding, and so does one who went
/// through `AltGr` to produce the character itself — because both of them meant
/// the same thing, and refusing one of them is refusing it invisibly.
#[must_use]
pub fn matches(bound: &Chord, reported: &Reported) -> Matched {
    let wanted = bound.canonical();
    if let Ok(produced) = Chord::parse(&reported.produced) {
        if produced.canonical() == wanted {
            return Matched::Produced;
        }
    }
    if let Some(base) = &reported.base_layout {
        if let Ok(parsed) = Chord::parse(base) {
            if parsed.canonical() == wanted {
                return Matched::BaseLayout;
            }
        }
    }
    Matched::No
}

/// Characters that need `AltGr` on at least one common layout.
///
/// Not a complete list of every awkward key on every layout — it is the set omt
/// checks its own defaults against, which is a smaller and answerable question.
pub const AWKWARD: &[&str] = &["|", "\\", "@", "#", "~", "{", "}", "[", "]"];

/// Whether a chord would be awkward to press on a common non-US layout.
#[must_use]
pub fn is_awkward(chord: &Chord) -> bool {
    let canonical = chord.canonical();
    AWKWARD.iter().any(|c| canonical.ends_with(c))
}

/// The alias installed beside an awkward default.
///
/// Aliases rather than replacements: somebody on a US layout has muscle memory
/// for `|`, and taking it away to help a different user is a trade with no
/// winner. Both work.
#[must_use]
pub fn alias_for(chord: &Chord) -> Option<&'static str> {
    match chord.canonical().chars().last()? {
        '|' => Some("v"),
        '-' => Some("s"),
        _ => None,
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

    fn reported(produced: &str, base: Option<&str>) -> Reported {
        Reported {
            produced: produced.to_owned(),
            base_layout: base.map(str::to_owned),
        }
    }

    #[test]
    fn a_us_layout_matches_on_the_character_it_produced() {
        assert_eq!(
            matches(&chord("v"), &reported("v", Some("v"))),
            Matched::Produced
        );
    }

    #[test]
    fn a_non_us_layout_matches_on_the_physical_key() {
        // A German user pressing the physical key that is `w` on a US layout
        // gets the binding, without an AltGr dance.
        assert_eq!(
            matches(&chord("w"), &reported("z", Some("w"))),
            Matched::BaseLayout
        );
    }

    #[test]
    fn producing_the_character_the_hard_way_also_matches() {
        // Both users meant the same thing, and refusing one of them refuses it
        // invisibly.
        assert_eq!(
            matches(&chord("z"), &reported("z", Some("w"))),
            Matched::Produced
        );
    }

    #[test]
    fn legacy_encoding_still_matches_on_what_was_produced() {
        // No base layout is most terminals most of the time. Not a failure.
        assert_eq!(
            matches(&chord("v"), &reported("v", None)),
            Matched::Produced
        );
    }

    #[test]
    fn a_key_that_is_neither_does_not_match() {
        assert_eq!(matches(&chord("q"), &reported("z", Some("w"))), Matched::No);
    }

    #[test]
    fn how_it_matched_is_reported_so_a_missing_binding_is_explicable() {
        // The difference is invisible until a binding does not fire, and then
        // the user cannot tell whether they mistyped or omt cannot see their
        // layout.
        assert_ne!(
            matches(&chord("w"), &reported("z", Some("w"))),
            matches(&chord("z"), &reported("z", Some("w")))
        );
    }

    #[test]
    fn the_awkward_characters_are_recognised() {
        assert!(is_awkward(&chord("|")));
        assert!(is_awkward(&chord("@")));
        assert!(!is_awkward(&chord("v")));
        assert!(!is_awkward(&chord("ctrl-b")));
    }

    #[test]
    fn an_awkward_default_gets_an_alias_rather_than_a_replacement() {
        // Taking `|` away from somebody with muscle memory for it, to help a
        // different user, is a trade with no winner. Both work.
        assert_eq!(alias_for(&chord("|")), Some("v"));
        assert_eq!(alias_for(&chord("-")), Some("s"));
        assert_eq!(alias_for(&chord("v")), None);
    }

    #[test]
    fn omts_own_defaults_avoid_altgr_except_where_an_alias_exists() {
        // §9.1's consequence, checked rather than asserted in prose. This is
        // the test that fails the day somebody binds `⟨leader⟩ {`.
        let unaliased: Vec<String> = crate::keymap::defaults()
            .bindings()
            .into_iter()
            .map(|(_, chord, _)| chord)
            .filter(|c| is_awkward(c) && alias_for(c).is_none())
            .map(Chord::canonical)
            .collect();
        assert!(
            unaliased.is_empty(),
            "these need an AltGr on a common layout and have no alias: {unaliased:?}"
        );
    }
}
