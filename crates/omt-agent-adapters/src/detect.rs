//! Recognizing which agent is running, and being honest about how well.
//!
//! The rule the whole ladder rests on: **the tier a detection reaches is
//! decided by the evidence, not by the agent**. Knowing it is Claude Code
//! because an environment variable said so is a different claim from guessing
//! it because a process is called `claude`, and the second must never license
//! anything the first does.

use omt_types::{AgentKind, Tier};

use crate::adapter::{AgentAdapter, Detection, Fingerprint};

/// What was observed about a process.
#[derive(Debug, Clone, Default)]
pub struct Observation {
    /// Its executable's base name.
    pub exe_name: Option<String>,
    /// Its argv.
    pub argv: Vec<String>,
    /// Its environment, where that could be read.
    ///
    /// Empty and "could not read" are deliberately the same thing here: on
    /// macOS another process's environment is simply not available, and a
    /// detector that treated absence as evidence of absence would report the
    /// wrong agent with confidence on half the supported platforms.
    pub env: Vec<(String, String)>,
}

impl Observation {
    /// The value of an environment variable, if it was observed.
    #[must_use]
    pub fn env_var(&self, key: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Identify the agent behind an observation.
///
/// Adapters are tried in order and the *best* match wins, not the first: an
/// argv pattern matching one adapter must not beat an environment marker
/// matching another, or a wrapper script's name would outrank the agent's own
/// statement of what it is.
#[must_use]
pub fn detect(adapters: &[&dyn AgentAdapter], obs: &Observation) -> Detection {
    let mut best: Option<Detection> = None;
    for adapter in adapters {
        if let Some(d) = match_one(adapter.kind(), &adapter.fingerprint(), obs)
            && best.as_ref().is_none_or(|b| d.tier > b.tier)
        {
            best = Some(d);
        }
    }
    best.unwrap_or(Detection {
        agent: AgentKind::Unknown,
        tier: Tier::Heuristic,
        evidence: "nothing about this process identified an agent".to_owned(),
        agent_session: None,
    })
}

fn match_one(agent: AgentKind, fp: &Fingerprint, obs: &Observation) -> Option<Detection> {
    let agent_session = fp.session_id_var.and_then(|v| {
        obs.env_var(v)
            .filter(|s| !s.is_empty())
            .map(std::borrow::ToOwned::to_owned)
    });

    // An environment marker was *put there* by the agent's own launcher or by
    // omt, so it is a statement rather than a resemblance.
    for marker in &fp.env_markers {
        if obs.env_var(marker).is_some() {
            return Some(Detection {
                agent,
                tier: Tier::Marker,
                evidence: format!("the environment variable `{marker}` is set"),
                agent_session,
            });
        }
    }

    // The executable's name is a fact about what is running, but a fact that
    // says nothing about what it *is*: anyone may name a script `claude`.
    if let Some(exe) = &obs.exe_name
        && fp.exe_names.iter().any(|n| n == exe)
    {
        return Some(Detection {
            agent,
            tier: Tier::Process,
            evidence: format!("the running executable is named `{exe}`"),
            agent_session,
        });
    }

    // Weakest: the agent is bundled behind a runtime, so the executable is
    // `node` or `bun` and only the arguments say anything at all.
    for pattern in &fp.argv_patterns {
        if let Some(hit) = obs.argv.iter().find(|a| a.contains(pattern)) {
            return Some(Detection {
                agent,
                tier: Tier::Process,
                evidence: format!("an argument contains `{pattern}`: {hit}"),
                agent_session,
            });
        }
    }

    None
}

/// Whether a detection at this tier may be trusted with structured content.
///
/// Restated here as a named function because the call sites read better for it,
/// and because this is the single check that keeps a locale change from
/// becoming a wrong "Allow" card.
#[must_use]
pub const fn may_emit_structured(tier: Tier) -> bool {
    tier.may_emit_structured_content()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::agents::{ClaudeCode, GenericAcp, HeuristicFloor};

    fn adapters() -> Vec<Box<dyn AgentAdapter>> {
        vec![
            Box::new(ClaudeCode),
            Box::new(GenericAcp::new(AgentKind::Opencode)),
            Box::new(HeuristicFloor::new(AgentKind::Aider)),
        ]
    }

    fn detect_with(obs: &Observation) -> Detection {
        let owned = adapters();
        let refs: Vec<&dyn AgentAdapter> = owned.iter().map(std::convert::AsRef::as_ref).collect();
        detect(&refs, obs)
    }

    #[test]
    fn an_environment_marker_identifies_the_agent_at_marker_tier() {
        let obs = Observation {
            env: vec![("CLAUDECODE".into(), "1".into())],
            ..Observation::default()
        };
        let d = detect_with(&obs);
        assert_eq!(d.agent, AgentKind::ClaudeCode);
        assert_eq!(d.tier, Tier::Marker);
    }

    #[test]
    fn an_executable_name_identifies_it_only_at_process_tier() {
        // Anyone may name a script `claude`. Knowing the name is not being
        // told, and the tier has to say so.
        let obs = Observation {
            exe_name: Some("claude".into()),
            ..Observation::default()
        };
        let d = detect_with(&obs);
        assert_eq!(d.agent, AgentKind::ClaudeCode);
        assert_eq!(d.tier, Tier::Process);
        assert!(
            !may_emit_structured(d.tier),
            "a guess must not license a card the user taps Allow on"
        );
    }

    #[test]
    fn a_marker_beats_a_name_belonging_to_a_different_agent() {
        // A wrapper script named `aider` that sets Claude Code's marker is
        // Claude Code. Taking the first match would report the wrapper.
        let obs = Observation {
            exe_name: Some("aider".into()),
            env: vec![("CLAUDECODE".into(), "1".into())],
            ..Observation::default()
        };
        let d = detect_with(&obs);
        assert_eq!(d.agent, AgentKind::ClaudeCode, "{}", d.evidence);
        assert_eq!(d.tier, Tier::Marker);
    }

    #[test]
    fn an_unrecognized_process_is_unknown_rather_than_a_guess() {
        // Naming some agent anyway would put a wrong label on a pane, which is
        // worse than an honest blank.
        let obs = Observation {
            exe_name: Some("vim".into()),
            ..Observation::default()
        };
        let d = detect_with(&obs);
        assert_eq!(d.agent, AgentKind::Unknown);
        assert_eq!(d.tier, Tier::Heuristic);
        assert!(!may_emit_structured(d.tier));
    }

    #[test]
    fn the_evidence_says_what_matched() {
        // `agent.explain` has to show a user *why*, not repeat the name.
        let obs = Observation {
            env: vec![("CLAUDECODE".into(), "1".into())],
            ..Observation::default()
        };
        assert!(
            detect_with(&obs).evidence.contains("CLAUDECODE"),
            "the evidence names the variable"
        );

        let obs = Observation {
            exe_name: Some("claude".into()),
            ..Observation::default()
        };
        assert!(detect_with(&obs).evidence.contains("claude"));
    }

    #[test]
    fn a_runtime_wrapped_bundle_is_found_through_its_arguments() {
        // The executable is `node` and says nothing; only argv identifies it.
        let obs = Observation {
            exe_name: Some("node".into()),
            argv: vec![
                "node".into(),
                "/opt/homebrew/lib/node_modules/opencode-ai/bin/opencode.js".into(),
            ],
            ..Observation::default()
        };
        let d = detect_with(&obs);
        assert_eq!(d.agent, AgentKind::Opencode);
        assert_eq!(d.tier, Tier::Process, "a guess, and labelled as one");
    }

    #[test]
    fn the_agents_own_session_id_is_picked_up_when_present() {
        // What makes a correlation direct rather than inferred.
        let obs = Observation {
            env: vec![
                ("CLAUDECODE".into(), "1".into()),
                ("CLAUDE_CODE_SESSION_ID".into(), "abc-123".into()),
            ],
            ..Observation::default()
        };
        assert_eq!(detect_with(&obs).agent_session.as_deref(), Some("abc-123"));
    }

    #[test]
    fn an_empty_session_id_is_not_a_session_id() {
        // An empty string would correlate every session with every other.
        let obs = Observation {
            env: vec![
                ("CLAUDECODE".into(), "1".into()),
                ("CLAUDE_CODE_SESSION_ID".into(), String::new()),
            ],
            ..Observation::default()
        };
        assert_eq!(detect_with(&obs).agent_session, None);
    }

    #[test]
    fn an_unreadable_environment_does_not_become_evidence_of_absence() {
        // On macOS another process's environment simply cannot be read. A
        // detector that concluded "no marker, therefore not Claude Code" would
        // be wrong on half the supported platforms.
        let obs = Observation {
            exe_name: Some("claude".into()),
            env: Vec::new(),
            ..Observation::default()
        };
        let d = detect_with(&obs);
        assert_eq!(d.agent, AgentKind::ClaudeCode);
        assert_eq!(
            d.tier,
            Tier::Process,
            "identified, but on weaker evidence — not rejected"
        );
    }
}
