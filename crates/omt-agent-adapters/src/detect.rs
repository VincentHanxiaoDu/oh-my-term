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
    /// The base name of `argv[0]`, where there is one.
    ///
    /// Separate from `exe_name` because they disagree, and the disagreement is
    /// the point: an agent that updates itself runs from a versioned path and
    /// is started under a stable name.
    #[must_use]
    pub fn argv0_name(&self) -> Option<&str> {
        self.argv
            .first()
            .map(|a| a.rsplit('/').next().unwrap_or(a.as_str()))
            .filter(|name| !name.is_empty())
    }

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

    // And the name it was *started* under, which for a self-updating agent is
    // the only place the name appears at all. Claude Code installs itself as
    // `.../claude/versions/2.1.233` behind a launcher on `PATH`, so the
    // executable's base name is a version number and `argv[0]` is the name.
    // Matching only the executable reported no agent for the commonest agent
    // there is, on every machine outside an agent session.
    if let Some(argv0) = obs.argv0_name()
        && fp.exe_names.contains(&argv0)
    {
        return Some(Detection {
            agent,
            tier: Tier::Process,
            evidence: format!("it was started as `{argv0}`"),
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

/// What an agent's terminal title says about it.
///
/// The strongest evidence available without cooperation. A title is set by the
/// program deliberately, through a channel every terminal has had for decades:
/// it needs no permission, it survives ssh, and it does not depend on how the
/// binary was launched, signed, or wrapped — which is the whole list of things
/// that make reading the process table unreliable.
///
/// Returns the agent and, where the title says so, what it is doing. A title
/// that names nothing yields nothing: guessing from an arbitrary string is how
/// a shell that puts the last command in its title becomes an agent.
#[must_use]
pub fn detect_from_title(adapters: &[&dyn AgentAdapter], title: &str) -> Option<TitleReading> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_lowercase();
    for adapter in adapters {
        let fingerprint = adapter.fingerprint();
        let Some(marks) = fingerprint.title.as_ref() else {
            continue;
        };
        let Some(name) = marks
            .names
            .iter()
            .find(|name| lowered.contains(&name.to_lowercase()))
        else {
            continue;
        };
        let first = trimmed.chars().next();
        let activity = first.and_then(|glyph| {
            if marks.working_prefix.iter().any(|r| r.contains(&glyph)) {
                Some(TitleActivity::Working)
            } else if marks.idle_prefix.contains(&glyph) {
                Some(TitleActivity::Idle)
            } else {
                None
            }
        });
        return Some(TitleReading {
            agent: adapter.kind(),
            activity,
            // The name that matched, never the glyph. A person reading why omt
            // thinks an agent is working does not want a codepoint.
            evidence: format!("its terminal title says `{name}`"),
        });
    }
    None
}

/// What an already-identified agent's title says it is doing.
///
/// Separate from `detect_from_title`, and the separation is the point: a title
/// may carry a state and not a name. Gemini's is `◇ Ready (dir)` — a state
/// glyph, a word, and the directory, with nothing that says which agent it is.
/// Requiring the name would throw away the state it *does* report.
///
/// So identity comes from wherever it can — a title that names the agent, the
/// process it was started as, an environment marker — and this asks the agent
/// omt settled on what its own glyph means.
#[must_use]
pub fn activity_from_title(
    marks: &crate::adapter::TitleMarks,
    title: &str,
) -> Option<TitleActivity> {
    let first = title.trim().chars().next()?;
    if marks.working_prefix.iter().any(|r| r.contains(&first)) {
        return Some(TitleActivity::Working);
    }
    marks
        .idle_prefix
        .contains(&first)
        .then_some(TitleActivity::Idle)
}

/// What a title told us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleReading {
    /// Which agent the title names.
    pub agent: AgentKind,
    /// What the title's leading glyph says it is doing, where it says anything.
    pub activity: Option<TitleActivity>,
    /// Why, in words, for `agent.explain`.
    pub evidence: String,
}

/// The state an agent's title glyph reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleActivity {
    /// A spinner frame: it is doing something.
    Working,
    /// Its resting mark: it is waiting for something to do.
    Idle,
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod title_tests {
    use super::*;

    fn adapters() -> crate::AdapterSet {
        crate::builtin()
    }

    #[test]
    fn a_resting_agent_names_itself_and_its_state() {
        // Observed from the shipped binary: `OSC 0 ; ✳ Claude Code`.
        let set = adapters();
        let read = detect_from_title(&set.all(), "\u{2733} Claude Code").expect("a reading");
        assert_eq!(read.agent, AgentKind::ClaudeCode);
        assert_eq!(read.activity, Some(TitleActivity::Idle));
    }

    #[test]
    fn the_spinner_is_the_one_the_shipped_binary_actually_draws() {
        // Captured from Claude Code while it worked: `◐ Claude Code`, then
        // `◑`, cycling. The rule said braille, taken from somebody else's
        // manifest and never checked against the thing it describes — so a
        // working agent read as idle, which is the report that found it.
        let set = adapters();
        for title in ["\u{25d0} Claude Code", "\u{25d1} Claude Code"] {
            let read = detect_from_title(&set.all(), title).expect("a reading");
            assert_eq!(
                read.activity,
                Some(TitleActivity::Working),
                "{title:?} read as {:?}",
                read.activity
            );
        }
    }

    #[test]
    fn the_state_is_read_even_when_the_title_stops_naming_the_agent() {
        // While it works, the title becomes the *question* — `◐ Count from 1
        // to 20 slowly` — and the agent's name is gone from it entirely. The
        // name is what identity matches on, so state has to be asked of the
        // agent already identified rather than of the title's text.
        let set = adapters();
        let all = set.all();
        assert!(
            detect_from_title(&all, "\u{25d0} Count from 1 to 20 slowly").is_none(),
            "a title with no agent name must not identify one"
        );
        let marks = all
            .iter()
            .find(|a| a.kind() == AgentKind::ClaudeCode)
            .and_then(|a| a.fingerprint().title)
            .expect("title marks");
        assert_eq!(
            activity_from_title(&marks, "\u{25d0} Count from 1 to 20 slowly"),
            Some(TitleActivity::Working)
        );
    }

    #[test]
    fn a_working_agent_is_a_spinner_frame_not_a_list_of_them() {
        // A spinner is an animation, and enumerating its frames is a list that
        // goes stale on the next frame they add. The whole braille block is
        // the range.
        let set = adapters();
        for frame in ['\u{2801}', '\u{2809}', '\u{28FF}'] {
            let title = format!("{frame} Claude Code");
            let read = detect_from_title(&set.all(), &title).expect("a reading");
            assert_eq!(read.activity, Some(TitleActivity::Working), "{title:?}");
        }
    }

    #[test]
    fn a_title_that_names_nothing_is_not_an_agent() {
        // A shell that puts the last command in its title would otherwise
        // become whichever agent that command resembled.
        let set = adapters();
        for title in ["~/src/omt", "vim README.md", "", "   ", "zsh"] {
            assert!(detect_from_title(&set.all(), title).is_none(), "{title:?}");
        }
    }

    #[test]
    fn the_agent_is_named_even_when_the_glyph_is_not_recognised() {
        // A new state mark is a state omt does not know, not an agent it does
        // not know. Reporting neither would lose the identity over a glyph.
        let set = adapters();
        let read = detect_from_title(&set.all(), "\u{2699} Claude Code").expect("a reading");
        assert_eq!(read.agent, AgentKind::ClaudeCode);
        assert_eq!(read.activity, None);
    }

    #[test]
    fn the_evidence_is_words_rather_than_a_codepoint() {
        // `agent.explain` puts this in front of a person.
        let set = adapters();
        let read = detect_from_title(&set.all(), "\u{2733} Claude Code").expect("a reading");
        assert!(read.evidence.contains("title"), "{}", read.evidence);
        assert!(!read.evidence.contains('\u{2733}'), "{}", read.evidence);
    }
}
