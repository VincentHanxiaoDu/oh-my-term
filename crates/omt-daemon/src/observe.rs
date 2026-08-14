//! Looking at what a session is running.
//!
//! Moved here from the TUI's loop, where it could only ever see the sessions
//! that process happened to be drawing. A session created through the daemon —
//! every session a phone or the web client opens — went unobserved, so one
//! surface could report agents and the other structurally could not.
//!
//! Everything here is a reading, never a conclusion. What a reading *means* is
//! the merge machine's decision, and the tier on each one is what keeps a
//! guess from overruling a statement.

use omt_agent::SourceReading;
use omt_agent_adapters::{
    AgentAdapter, Detection, Observation, ScreenState, TitleActivity, detect, detect_from_title,
};
use omt_types::{AgentKind, AgentState, Tier};

/// What is running in a pty, and what it says it is doing.
///
/// The title first. An agent that sets one is telling its host what it is,
/// through a channel that needs no permission, survives ssh, and does not
/// depend on how the binary was launched or signed. The process table is none
/// of those: a hardened binary refuses its arguments, and a self-updating
/// agent runs from a versioned path whose name is a version number.
#[must_use]
pub fn identify(
    fd: std::os::fd::RawFd,
    title: Option<&str>,
    adapters: &[&dyn AgentAdapter],
) -> Option<(Detection, Option<TitleActivity>)> {
    if let Some(title) = title
        && let Some(read) = detect_from_title(adapters, title)
    {
        return Some((
            Detection {
                agent: read.agent,
                tier: Tier::Marker,
                evidence: read.evidence,
                agent_session: None,
            },
            read.activity,
        ));
    }

    let pgid = omt_pty::foreground_process(fd).ok()?;
    let info = omt_pty::process_info(pgid).ok()?;
    let observation = Observation {
        exe_name: info.command_name().map(str::to_owned),
        argv: info.argv.clone(),
        // Empty when unreadable, which the detector treats as "no evidence"
        // rather than "no marker".
        env: omt_pty::read_environ(pgid).unwrap_or_default(),
    };
    let found = detect(adapters, &observation);
    // `Unknown` is a real answer: it means a process that is not an agent,
    // which is what a plain shell is.
    if found.agent == AgentKind::Unknown {
        return None;
    }
    // Identity from the process, and the state from the title anyway. A title
    // may carry a state and no name — Gemini's is `◇ Ready (dir)`, which says
    // how it is and which directory and nothing about which agent — so
    // requiring the name would throw away the state it does report.
    let said = title.and_then(|title| {
        adapters
            .iter()
            .find(|a| a.kind() == found.agent)
            .and_then(|a| a.fingerprint().title)
            .and_then(|marks| omt_agent_adapters::activity_from_title(&marks, title))
    });
    Some((found, said))
}

/// The readings a detection produces.
#[must_use]
pub fn readings(
    detection: &Detection,
    said: Option<TitleActivity>,
    now_ms: u64,
) -> Vec<SourceReading> {
    let mut out = vec![SourceReading {
        tier: detection.tier,
        // Identity is not a lifecycle. What a process table or a title *name*
        // is which agent, and the weakest true thing about what it is doing is
        // that nothing here knows.
        state: AgentState::Unknown,
        at: now_ms,
        authoritative: false,
    }];
    if let Some(activity) = said {
        out.push(SourceReading {
            tier: Tier::Marker,
            state: match activity {
                TitleActivity::Working => AgentState::Working { detail: None },
                TitleActivity::Idle => AgentState::Idle,
            },
            at: now_ms,
            // The agent said so. That may suspend a guess made from bytes
            // arriving; a guess may not suspend it.
            authoritative: true,
        });
    }
    out
}

/// The reading a screen rule produces.
///
/// `Hold` produces none, which is how an agent showing its own history stops
/// the present being read out of the past.
#[must_use]
pub fn screen_reading(state: ScreenState, now_ms: u64) -> Option<SourceReading> {
    let state = match state {
        ScreenState::Working => AgentState::Working { detail: None },
        ScreenState::Blocked => AgentState::Blocked {
            // Unspecified and no interaction: a screen rule knows somebody is
            // wanted and never what for, and inventing an answerable card out
            // of matched text is how a locale change becomes a wrong "Allow".
            reason: omt_types::BlockReason::Unspecified,
            interaction: None,
        },
        ScreenState::Idle => AgentState::Idle,
        ScreenState::Hold => return None,
    };
    Some(SourceReading {
        tier: Tier::Transcript,
        state,
        at: now_ms,
        authoritative: true,
    })
}

/// The visible screen, as text.
///
/// Bounded to what is on screen: scrollback is where an answered question
/// lives, and a rule that reached into it would report an agent as waiting for
/// something it was told an hour ago.
#[must_use]
pub fn screen_text(grid: &omt_term::Grid) -> String {
    let mut out = String::new();
    for row in grid.rows() {
        out.push_str(row.text().trim_end());
        out.push('\n');
    }
    out
}
