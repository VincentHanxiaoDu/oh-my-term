//! The heuristic floor — for agents that tell us nothing.
//!
//! Aider, Amp and Crush in TUI mode expose no hooks, no protocol and no
//! transcript omt can read. What a phone gets for one of these is
//! `busy | idle | needs you` plus a letterboxed grid, and **the UI says so**
//! rather than leaving the user to work it out from an empty transcript.
//!
//! The temptation this module exists to refuse is screen scraping into
//! structure. It would work, demo well, and be wrong the first time a user
//! changed locale or the agent shipped a new spinner — and being wrong here
//! means a card someone taps "Allow" on. So there is deliberately no code path
//! from screen text to anything but [`ActivityGuess`].

use std::ffi::OsString;

use omt_events::{ActivityGuess, AgentPayload};
use omt_types::{AgentKind, Tier};

use crate::adapter::{AdapterError, AgentAdapter, Fingerprint, Interrupt, SpawnCtx};

/// An agent omt can only watch.
#[derive(Debug, Clone)]
pub struct HeuristicFloor {
    kind: AgentKind,
}

impl HeuristicFloor {
    /// A floor adapter for one agent.
    #[must_use]
    pub const fn new(kind: AgentKind) -> Self {
        Self { kind }
    }

    /// The agents that land here.
    pub const COVERS: &'static [AgentKind] = &[
        AgentKind::Aider,
        AgentKind::Amp,
        AgentKind::Crush,
        // And anything omt has never heard of. This is the difference between
        // "omt supports these eleven agents" and "omt supports agents": a CLI
        // released next month gets a pane that shows whether it is working and
        // a way to stop it, and says plainly that it is guessing — instead of
        // getting no adapter at all and therefore nothing.
        AgentKind::Unknown,
    ];
}

impl AgentAdapter for HeuristicFloor {
    fn kind(&self) -> AgentKind {
        self.kind
    }

    fn fingerprint(&self) -> Fingerprint {
        match self.kind {
            AgentKind::Aider => Fingerprint {
                exe_names: vec!["aider"],
                argv_patterns: vec!["aider"],
                ..Fingerprint::default()
            },
            AgentKind::Amp => Fingerprint {
                exe_names: vec!["amp"],
                argv_patterns: vec!["@sourcegraph/amp"],
                ..Fingerprint::default()
            },
            AgentKind::Crush => Fingerprint {
                exe_names: vec!["crush"],
                argv_patterns: vec!["charmbracelet/crush"],
                ..Fingerprint::default()
            },
            _ => Fingerprint::default(),
        }
    }

    fn spawn_env(&self, ctx: &SpawnCtx) -> Vec<(OsString, OsString)> {
        // Injected even though nothing here will read them: if the user later
        // installs an integration, or runs a wrapper that does, the
        // correlation is already in place rather than needing a restart.
        vec![
            (
                OsString::from("OMT_INSTANCE"),
                OsString::from(&ctx.instance),
            ),
            (OsString::from("OMT_SESSION"), OsString::from(&ctx.session)),
            (OsString::from("OMT_SOCK"), OsString::from(&ctx.socket)),
        ]
    }

    fn best_tier(&self) -> Tier {
        Tier::Heuristic
    }

    fn interrupt(&self) -> Interrupt {
        // The entire remote control surface for one of these. A control
        // character is position-independent by construction, so it is safe
        // without inferring anything about screen state — which is exactly why
        // it is the only synthetic input the floor is allowed.
        Interrupt::ControlC
    }

    fn static_commands(&self) -> Vec<&'static str> {
        match self.kind {
            // Aider's commands are fixed and documented, and it cannot be
            // asked. Everywhere an agent *can* be asked, it is.
            AgentKind::Aider => vec![
                "add", "drop", "undo", "diff", "commit", "run", "test", "clear", "tokens", "model",
            ],
            _ => Vec::new(),
        }
    }

    fn normalize(
        &self,
        event: &str,
        _payload: &serde_json::Value,
    ) -> Result<Vec<AgentPayload>, AdapterError> {
        // There is no structured source to normalize. Anything arriving here
        // came from somewhere it should not have.
        Err(AdapterError::UnknownEvent {
            agent: self.kind,
            event: event.to_owned(),
        })
    }
}

/// What a screen heuristic is allowed to conclude.
///
/// The signals are deliberately the ones that survive a locale change: whether
/// bytes are arriving, and whether the cursor sits after something that looks
/// like a prompt. Neither reads a word.
#[must_use]
pub fn guess_activity(signals: &ScreenSignals) -> ActivityGuess {
    if signals.output_in_last_second {
        return ActivityGuess::Busy;
    }
    if signals.bell_recently {
        // A bell is the one thing a terminal program sends *deliberately* to
        // get a human's attention, so it is the strongest floor-level signal
        // there is — and it still only means "attention", never "approve
        // this".
        return ActivityGuess::NeedsAttention;
    }
    if signals.quiet_for_secs >= 2 {
        return ActivityGuess::Idle;
    }
    ActivityGuess::Unknown
}

/// What a heuristic source may look at.
///
/// Everything here is language-independent on purpose. There is no field for
/// screen text, because a field for screen text is how the temptation gets in.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScreenSignals {
    /// Bytes arrived from the pty within the last second.
    pub output_in_last_second: bool,
    /// A bell rang recently.
    pub bell_recently: bool,
    /// How long the pty has been quiet.
    pub quiet_for_secs: u64,
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
    fn the_floor_admits_it_is_the_floor() {
        // Claiming anything higher would turn on a transcript view that can
        // never be populated.
        let a = HeuristicFloor::new(AgentKind::Aider);
        assert_eq!(a.best_tier(), Tier::Heuristic);
        assert!(
            !a.best_tier().may_emit_structured_content(),
            "no card from here can ever say Allow"
        );
    }

    #[test]
    fn there_is_no_normalizer_because_there_is_no_structured_source() {
        let err = HeuristicFloor::new(AgentKind::Amp)
            .normalize("anything", &serde_json::Value::Null)
            .expect_err("nothing may be normalized at this tier");
        assert!(matches!(err, AdapterError::UnknownEvent { .. }));
    }

    #[test]
    fn interrupt_is_control_c_because_it_is_position_independent() {
        // The floor's only control, and safe precisely because it needs no
        // inference about what is on screen.
        let a = HeuristicFloor::new(AgentKind::Crush);
        assert_eq!(a.interrupt(), Interrupt::ControlC);
        assert_eq!(a.interrupt().bytes(), Some(&b"\x03"[..]));
    }

    #[test]
    fn arriving_output_means_busy() {
        assert_eq!(
            guess_activity(&ScreenSignals {
                output_in_last_second: true,
                ..ScreenSignals::default()
            }),
            ActivityGuess::Busy
        );
    }

    #[test]
    fn a_bell_means_attention_and_nothing_stronger() {
        // The strongest signal available down here, and it still does not mean
        // "a question with these options is on screen".
        assert_eq!(
            guess_activity(&ScreenSignals {
                bell_recently: true,
                quiet_for_secs: 5,
                ..ScreenSignals::default()
            }),
            ActivityGuess::NeedsAttention
        );
    }

    #[test]
    fn silence_means_idle_only_after_it_has_lasted() {
        // A gap between two writes is not the end of a turn.
        assert_eq!(
            guess_activity(&ScreenSignals {
                quiet_for_secs: 0,
                ..ScreenSignals::default()
            }),
            ActivityGuess::Unknown
        );
        assert_eq!(
            guess_activity(&ScreenSignals {
                quiet_for_secs: 5,
                ..ScreenSignals::default()
            }),
            ActivityGuess::Idle
        );
    }

    #[test]
    fn every_guess_is_unstructured() {
        // The invariant the whole tier ladder rests on: there is no code path
        // from a screen heuristic to a tool call.
        for signals in [
            ScreenSignals {
                output_in_last_second: true,
                ..ScreenSignals::default()
            },
            ScreenSignals {
                bell_recently: true,
                ..ScreenSignals::default()
            },
            ScreenSignals {
                quiet_for_secs: 9,
                ..ScreenSignals::default()
            },
            ScreenSignals::default(),
        ] {
            let payload = AgentPayload::Activity {
                state: guess_activity(&signals),
            };
            assert!(
                !payload.is_structured(),
                "a heuristic produced something structured: {payload:?}"
            );
        }
    }

    #[test]
    fn a_static_command_list_exists_only_where_the_agent_cannot_be_asked() {
        // Everywhere else, asking is the rule — a maintained list goes stale.
        assert!(
            !HeuristicFloor::new(AgentKind::Aider)
                .static_commands()
                .is_empty()
        );
        assert!(
            HeuristicFloor::new(AgentKind::Crush)
                .static_commands()
                .is_empty()
        );
    }

    #[test]
    fn correlation_variables_are_injected_even_though_nothing_reads_them_yet() {
        // So installing an integration later does not need a restart.
        let env = HeuristicFloor::new(AgentKind::Aider).spawn_env(&SpawnCtx {
            session: "s-9".into(),
            ..SpawnCtx::default()
        });
        assert!(
            env.iter().any(|(k, v)| k == "OMT_SESSION" && v == "s-9"),
            "{env:?}"
        );
    }
}
