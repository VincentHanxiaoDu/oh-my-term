//! Noticing that a pane is running an agent.
//!
//! The whole agent layer existed and nothing fed it from the local terminal.
//! `omt-agent-adapters` could identify eleven agents, `omt-agent` could merge
//! readings from six tiers, `Instance` could answer what a binding is doing —
//! and `omt` ran a shell without ever asking any of them a question. Starting
//! Claude Code in a pane and pressing `⟨leader⟩ a` showed "nothing matches",
//! because nothing had ever looked.
//!
//! Two sources, both of which the tier ladder already permits from here:
//!
//! - **Process** ([`Tier::Process`]) — the pane's foreground process, by name,
//!   argv and environment. It says *which* agent, and nothing about what it is
//!   doing.
//! - **Heuristic** ([`Tier::Heuristic`]) — whether bytes are arriving. It says
//!   *something is happening*, never what, which is the tier-0 rule and is
//!   enforced by the type: this module can only ever build an
//!   [`ActivityGuess`].
//!
//! Neither may emit structured content, so nothing here can produce a card.
//! That is the point: a card comes from a source that was told, and reading a
//! process table is not being told.

use omt_agent::SourceReading;
use omt_agent_adapters::{
    AgentAdapter, Detection, Observation, ScreenSignals, TitleActivity, detect, detect_from_title,
    guess_activity,
};
use omt_events::ActivityGuess;
use omt_types::{AgentKind, AgentState, Tier};

/// How often a pane's foreground process is inspected.
///
/// Every frame would mean a `sysctl` or a `/proc` read at 125 Hz per pane, to
/// answer a question whose answer changes when somebody runs a command. Half a
/// second is faster than a person can type a program's name and press enter,
/// and is invisible in a profile.
pub const PROBE_EVERY_MS: u64 = 500;

/// What a pane's agent looks like right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    /// Which agent, and on what evidence.
    pub detection: Detection,
    /// What it appears to be doing, at the floor's confidence.
    pub activity: ActivityGuess,
}

/// Identify what is running on a pty, from its foreground process.
///
/// `None` means nothing could be read — a pty whose foreground process has
/// gone, or a platform that will not report another process's environment.
/// Deliberately not "no agent": absence of evidence is reported as absence of
/// evidence, because on macOS another process's environment is simply not
/// readable and a prober that treated that as proof would confidently name the
/// wrong agent on half the supported platforms.
#[must_use]
pub fn identify(fd: std::os::fd::RawFd, adapters: &[&dyn AgentAdapter]) -> Option<Detection> {
    let pgid = omt_pty::foreground_process(fd).ok()?;
    let info = omt_pty::process_info(pgid).ok()?;
    let observation = Observation {
        exe_name: info.command_name().map(str::to_owned),
        argv: info.argv.clone(),
        // Empty when unreadable, which the detector already treats as "no
        // evidence" rather than "no marker".
        env: omt_pty::read_environ(pgid).unwrap_or_default(),
    };
    let found = detect(adapters, &observation);
    // `Unknown` is a real answer from `detect` — it means a process that is
    // not an agent, which is what a plain shell is, and reporting it would put
    // a row reading "unknown" in the list for every pane.
    (found.agent != AgentKind::Unknown).then_some(found)
}

/// What the pane's terminal title says is running in it.
///
/// Above the process probe on the ladder and read from a different place, so
/// the two can disagree and the stronger wins. A title is set by the program
/// deliberately: it needs no permission, it survives ssh, and it does not
/// depend on how the binary was launched or signed. The process table is none
/// of those things — reading a hardened binary's arguments is refused, and a
/// self-updating agent runs from a versioned path that does not carry its
/// name.
#[must_use]
pub fn identify_from_title(
    title: Option<&str>,
    adapters: &[&dyn AgentAdapter],
) -> Option<(Detection, Option<TitleActivity>)> {
    let read = detect_from_title(adapters, title?)?;
    Some((
        Detection {
            agent: read.agent,
            tier: Tier::Marker,
            evidence: read.evidence,
            agent_session: None,
        },
        read.activity,
    ))
}

/// The reading a title's state glyph produces.
///
/// Marker tier, and authoritative for what the agent is *doing*: the glyph is
/// the agent saying so, not omt inferring it from bytes arriving. That is the
/// difference between "something is on the pty" and "it says it is working",
/// and it is why this may suspend the floor while the floor may not suspend
/// it.
#[must_use]
pub fn title_reading(activity: TitleActivity, now_ms: u64) -> SourceReading {
    SourceReading {
        tier: Tier::Marker,
        state: match activity {
            TitleActivity::Working => AgentState::Working { detail: None },
            TitleActivity::Idle => AgentState::Idle,
        },
        at: now_ms,
        authoritative: true,
    }
}

/// The readings a detection and a screen produce, for the merge machine.
///
/// Two, not one, and separately tiered. The process probe knows *which* agent
/// with process-tier confidence and nothing about what it is doing; the screen
/// knows *something is happening* at the floor and nothing about who. Merging
/// them into a single reading would let one borrow the other's confidence,
/// which is the exact failure the tier ladder exists to prevent.
#[must_use]
pub fn readings(probed: &Probed, now_ms: u64) -> Vec<SourceReading> {
    vec![
        SourceReading {
            tier: probed.detection.tier,
            // A process probe can say an agent is *there*. Whether it is
            // working is not something a process table knows, so it reports
            // the weakest true thing.
            state: AgentState::Unknown,
            at: now_ms,
            // Never authoritative: it does not report a lifecycle, so it must
            // not suspend the heuristic that does the only reporting there is.
            authoritative: false,
        },
        SourceReading {
            tier: Tier::Heuristic,
            state: match probed.activity {
                // No detail: the floor watched bytes arrive. What the agent
                // is doing is not something it can know, and inventing a
                // string here would put a made-up sentence on a phone.
                ActivityGuess::Busy => AgentState::Working { detail: None },
                ActivityGuess::Idle => AgentState::Idle,
                ActivityGuess::NeedsAttention => AgentState::Blocked {
                    // `Unspecified` and no interaction, which is the honest
                    // pair: a bell means somebody is wanted, never what for,
                    // and never an answerable card.
                    reason: omt_types::BlockReason::Unspecified,
                    interaction: None,
                },
                ActivityGuess::Unknown => AgentState::Unknown,
            },
            at: now_ms,
            authoritative: false,
        },
    ]
}

/// The floor's guess about a pane, from what its terminal has been doing.
#[must_use]
pub fn activity(bytes_recently: bool, bell_recently: bool, quiet_for_secs: u64) -> ActivityGuess {
    guess_activity(&ScreenSignals {
        output_in_last_second: bytes_recently,
        bell_recently,
        quiet_for_secs,
    })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn probed(tier: Tier, activity: ActivityGuess) -> Probed {
        Probed {
            detection: Detection {
                agent: AgentKind::ClaudeCode,
                tier,
                evidence: "argv".to_owned(),
                agent_session: None,
            },
            activity,
        }
    }

    #[test]
    fn a_process_probe_never_claims_to_know_what_the_agent_is_doing() {
        // It read a process table. That says which program is running and
        // nothing whatever about whether it is waiting for a human.
        let out = readings(&probed(Tier::Process, ActivityGuess::Busy), 0);
        let process = out
            .iter()
            .find(|r| r.tier == Tier::Process)
            .expect("a process reading");
        assert_eq!(process.state, AgentState::Unknown);
    }

    #[test]
    fn neither_reading_is_authoritative() {
        // An authoritative source suspends the heuristic entirely. Neither of
        // these reports a lifecycle, so a claim of authority here would
        // silence the only source that is reporting anything at all.
        for r in readings(&probed(Tier::Process, ActivityGuess::Busy), 0) {
            assert!(!r.authoritative, "{r:?}");
        }
    }

    #[test]
    fn no_reading_from_here_may_emit_structured_content() {
        // The tier ladder's guard. A card must come from a source that was
        // told, and reading a process table is not being told.
        for r in readings(&probed(Tier::Process, ActivityGuess::NeedsAttention), 0) {
            assert!(
                !r.tier.may_emit_structured_content(),
                "{:?} could emit a card",
                r.tier
            );
        }
    }

    #[test]
    fn the_screen_reading_carries_the_activity_and_the_process_one_does_not() {
        let out = readings(&probed(Tier::Process, ActivityGuess::NeedsAttention), 7);
        let floor = out
            .iter()
            .find(|r| r.tier == Tier::Heuristic)
            .expect("a floor reading");
        assert!(
            matches!(
                floor.state,
                AgentState::Blocked {
                    reason: omt_types::BlockReason::Unspecified,
                    interaction: None
                }
            ),
            // The floor may say somebody is wanted. It may never say what for,
            // and it may never name a card to answer.
            "{:?}",
            floor.state
        );
        assert_eq!(floor.at, 7);
    }

    #[test]
    fn output_means_busy_and_silence_does_not_mean_blocked() {
        // The floor is allowed to say "something is happening" and to say
        // "nothing is". It is never allowed to conclude that silence means
        // somebody is being waited on.
        assert_eq!(activity(true, false, 0), ActivityGuess::Busy);
        // Whatever a long silence means, it is never "somebody is being waited
        // on" — that conclusion needs a source that was told.
        assert_ne!(activity(false, false, 30), ActivityGuess::NeedsAttention);
    }

    #[test]
    fn a_bell_is_the_strongest_thing_the_floor_has() {
        // The one signal a program sends deliberately to get a human's
        // attention.
        assert_eq!(activity(false, true, 2), ActivityGuess::NeedsAttention);
    }

    // The live-process counterpart of this — a pty with an agent on it,
    // identified through the whole loop — is
    // `test_an_agent_in_a_pane_is_found_and_named` in the Python suite. It
    // lives there rather than here because it needs a binary that is not
    // Apple-signed: a unit test written against `/bin/sleep` passed and failed
    // by turns, since a platform binary's environment is readable only
    // sometimes, and a flaky test that guards a real behaviour is worse than
    // no test because it trains people to re-run it.

    #[test]
    fn identifying_a_pty_with_no_process_reports_nothing_rather_than_guessing() {
        // Absence of evidence, reported as absence of evidence. A prober that
        // returned a default here would name an agent for every plain shell.
        let set = omt_agent_adapters::builtin();
        let refs: Vec<&dyn AgentAdapter> = set.all();
        // A file descriptor that is not a terminal has no foreground process.
        use std::os::fd::AsRawFd;
        let f = std::fs::File::open("/dev/null").expect("open");
        assert!(identify(f.as_raw_fd(), &refs).is_none());
    }
}
