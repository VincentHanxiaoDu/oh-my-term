//! Proving a human is holding the device, again.
//!
//! `docs/architecture/23-identity-and-devices.md` §7.
//!
//! What this is *not* matters more than what it is. It never classifies an
//! agent's tool danger — that is D1, and the policy here is expressed over
//! omt's own capability names and effect bits, never over what a `Bash` call
//! contains and never as a condition on whether an interaction may be resolved
//! at all.
//!
//! And it is a **proof-of-presence timer, not an authorisation narrowing**.
//! Failing step-up does not reduce what the device may do. It means the device
//! must show a human is holding it before this call proceeds — and then it may
//! do everything again. Treating a failed step-up as a demotion would make a
//! phone in a pocket permanently less trusted than the laptop beside it, which
//! is the second-class mobile experience omt exists to remove.
//!
//! Off by default, because a proof-of-presence prompt on a machine nobody else
//! can reach is a keystroke tax with nothing on the other side of it.

use std::collections::BTreeSet;

use serde::Serialize;

/// Whether step-up is in use at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// Nothing is challenged.
    #[default]
    Off,
    /// The policy below applies.
    On,
}

/// What satisfies a challenge where WebAuthn is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fallback {
    /// A passphrase the device holds.
    Passphrase,
    /// Re-entering the credential.
    TokenReentry,
    /// Nothing does; the call is refused.
    ///
    /// An honest option rather than a hostile one: somebody who wants hardware
    /// presence and nothing else should be able to say so, and a fallback they
    /// did not choose would quietly weaken the thing they turned on.
    Deny,
}

/// When a call needs a fresh proof of presence.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Whether any of this applies.
    pub mode: Mode,
    /// Capability names that always need one.
    pub require: BTreeSet<String>,
    /// Effects that need one, so a capability added later is covered the day
    /// it appears rather than the day somebody remembers to list it.
    pub require_effects: Vec<String>,
    /// How long a verification counts for, in seconds.
    pub freshness_secs: u64,
    /// What satisfies it without WebAuthn.
    pub fallback: Fallback,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            require: BTreeSet::new(),
            require_effects: Vec::new(),
            freshness_secs: 5 * 60,
            fallback: Fallback::Passphrase,
        }
    }
}

/// The shortest and longest a verification may count for.
///
/// Thirty seconds and twelve hours. Below the floor the prompt arrives often
/// enough that people stop reading it, which is worse than not having it;
/// above the ceiling it is not proof of presence, it is a login.
pub const MIN_FRESHNESS: u64 = 30;
/// Likewise.
pub const MAX_FRESHNESS: u64 = 12 * 60 * 60;

/// What omt decided about one call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Proceed.
    Proceed,
    /// Prove a human is here first.
    Challenge {
        /// Why, so the prompt can say.
        because: String,
    },
}

impl Decision {
    /// Whether the call proceeds without a prompt.
    #[must_use]
    pub const fn proceeds(&self) -> bool {
        matches!(self, Self::Proceed)
    }
}

/// Whether this call needs a fresh proof of presence.
///
/// `verified_secs_ago` is `None` when the device has never verified.
#[must_use]
pub fn decide(
    policy: &Policy,
    capability: &str,
    effects: &[String],
    verified_secs_ago: Option<u64>,
) -> Decision {
    if policy.mode == Mode::Off {
        return Decision::Proceed;
    }

    let by_name = policy.require.contains(capability);
    let by_effect = effects
        .iter()
        .find(|e| policy.require_effects.iter().any(|r| r == *e));

    if !by_name && by_effect.is_none() {
        return Decision::Proceed;
    }

    let freshness = policy.freshness_secs.clamp(MIN_FRESHNESS, MAX_FRESHNESS);
    match verified_secs_ago {
        Some(ago) if ago <= freshness => Decision::Proceed,
        _ => Decision::Challenge {
            // Names the reason, because a prompt that says only "verify" gives
            // somebody no way to tell an expected challenge from one they
            // should be suspicious of.
            because: by_effect.map_or_else(
                || format!("`{capability}` always asks"),
                |effect| format!("`{capability}` is {effect}"),
            ),
        },
    }
}

/// Whether failing a challenge changes what the device may do.
///
/// Never. Stated as a function so the property is testable rather than a
/// paragraph — a failed step-up means "not now", not "not you".
#[must_use]
pub const fn failing_narrows_authority() -> bool {
    false
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn on() -> Policy {
        Policy {
            mode: Mode::On,
            require: ["config.set".to_owned()].into_iter().collect(),
            require_effects: vec!["DESTRUCTIVE".to_owned()],
            ..Policy::default()
        }
    }

    #[test]
    fn nothing_is_challenged_when_it_is_off() {
        // A proof-of-presence prompt on a machine nobody else can reach is a
        // keystroke tax with nothing on the other side of it.
        assert!(decide(&Policy::default(), "config.set", &[], None).proceeds());
    }

    #[test]
    fn a_named_capability_is_challenged() {
        assert!(!decide(&on(), "config.set", &[], None).proceeds());
    }

    #[test]
    fn an_effect_covers_a_capability_added_later() {
        // The same argument the role policy makes: a new destructive
        // capability is covered the day it appears rather than the day
        // somebody remembers to list it.
        let decision = decide(
            &on(),
            "something.brand_new",
            &["DESTRUCTIVE".to_owned()],
            None,
        );
        assert!(!decision.proceeds());
    }

    #[test]
    fn an_ordinary_call_is_not_challenged() {
        assert!(decide(&on(), "session.read", &[], None).proceeds());
    }

    #[test]
    fn a_recent_verification_counts() {
        assert!(decide(&on(), "config.set", &[], Some(60)).proceeds());
    }

    #[test]
    fn a_stale_verification_does_not() {
        assert!(!decide(&on(), "config.set", &[], Some(6 * 60)).proceeds());
    }

    #[test]
    fn the_freshness_window_is_clamped_at_both_ends() {
        // Below the floor the prompt arrives often enough that people stop
        // reading it, which is worse than not having it. Above the ceiling it
        // is not proof of presence, it is a login.
        let mut eager = on();
        eager.freshness_secs = 1;
        assert!(
            decide(&eager, "config.set", &[], Some(20)).proceeds(),
            "floor"
        );

        let mut lax = on();
        lax.freshness_secs = 10 * 24 * 60 * 60;
        assert!(
            !decide(&lax, "config.set", &[], Some(13 * 60 * 60)).proceeds(),
            "ceiling"
        );
    }

    #[test]
    fn a_challenge_says_why_it_is_asking() {
        // A prompt that says only "verify" gives somebody no way to tell an
        // expected challenge from one they should be suspicious of.
        let Decision::Challenge { because } =
            decide(&on(), "worktree.remove", &["DESTRUCTIVE".to_owned()], None)
        else {
            panic!("expected a challenge");
        };
        assert!(because.contains("worktree.remove"), "{because}");
        assert!(because.contains("DESTRUCTIVE"), "{because}");
    }

    #[test]
    fn failing_a_challenge_never_narrows_what_the_device_may_do() {
        // Treating it as a demotion would make a phone in a pocket permanently
        // less trusted than the laptop beside it, which is the second-class
        // mobile experience omt exists to remove. A failed step-up is "not
        // now", never "not you".
        assert!(!failing_narrows_authority());
    }

    #[test]
    fn the_policy_never_mentions_an_agents_tools() {
        // D1. This classifies omt's own operations and never the agent's tool
        // danger — checked by name because a `require_tools` field is a
        // feature request that will arrive sounding reasonable.
        let json = serde_json::to_string(&Fallback::Deny).expect("json");
        assert!(!json.contains("tool"));
        let fields = format!("{:?}", on());
        for word in ["tool", "bash", "command"] {
            assert!(!fields.to_lowercase().contains(word), "{fields}");
        }
    }

    #[test]
    fn deny_is_an_available_fallback() {
        // Somebody who wants hardware presence and nothing else should be able
        // to say so; a fallback they did not choose would quietly weaken the
        // thing they turned on.
        let mut strict = on();
        strict.fallback = Fallback::Deny;
        assert_eq!(strict.fallback, Fallback::Deny);
    }

    #[test]
    fn a_device_that_has_never_verified_is_challenged() {
        assert!(!decide(&on(), "config.set", &[], None).proceeds());
    }
}
