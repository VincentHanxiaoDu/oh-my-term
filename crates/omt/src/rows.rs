//! Turning what the instance knows into rows somebody can read.
//!
//! Split out of the frame loop, which was carrying the loop, the observation
//! pass, the layout tree and these builders in one file over the length limit.
//! These are the part with no timing in them: given an instance and the panes
//! on screen, what should the list say — which is a question about wording and
//! ordering, and is testable without a terminal.

use omt_daemon::Instance;

use crate::run::LocalPane;

/// The rows behind the agents list.
///
/// Built from what the probe saw in each pane, not only from thread rosters.
/// A roster is filled by hooks and protocols; a pty-only session has neither,
/// which is why this said "nothing matches" with an agent plainly running in
/// the pane.
pub(crate) fn agent_rows(
    instance: &mut Instance,
    panes: &[LocalPane],
    now: u64,
) -> Vec<omt_tui::Choice> {
    // Each row with what it wants, so the sort is over states rather than over
    // whether a row's text happens to contain a word. Two agents can be called
    // the same thing, so this is positional and never keyed by label.
    let mut rows: Vec<(u8, omt_tui::Choice)> = Vec::new();
    for pane in panes {
        let title = instance
            .session(pane.session)
            .map_or_else(|| "shell".to_owned(), |s| s.title.clone());
        if let Some(probed) = &pane.agent {
            let state = instance
                .agent_state(pane.binding, now)
                .unwrap_or(omt_types::AgentState::Unknown);
            rows.push((
                omt_tui::attention(&state, pane.seen),
                omt_tui::Choice {
                    // The agent's name in the column the eye lands on. This was
                    // the session title — "shell" — with the agent buried mid-line
                    // after it, so a list opened *to find out whether omt knows
                    // about Claude Code* answered with the word "shell". Somebody
                    // reading that concludes it does not, and they are reading it
                    // correctly: the first column is the answer.
                    label: agent_name(probed.detection.agent).to_owned(),
                    // Then where it is and what it is doing, and last the
                    // evidence — somebody looking at a wrong detection needs to
                    // see what was matched, not be told a name again.
                    detail: format!(
                        "{} · {} · {}",
                        title,
                        omt_tui::state_word(&state, pane.seen),
                        probed.detection.evidence
                    ),
                    chord: None,
                    // What this row wants, as a shape. A colour is the first
                    // thing a terminal theme takes away.
                    mark: Some(omt_tui::state_mark(&state, pane.seen)),
                },
            ));
        }
        let Some(roster) = instance.threads(pane.session) else {
            continue;
        };
        for thread in roster.threads() {
            rows.push((
                omt_tui::attention(&thread.state, true),
                omt_tui::Choice {
                    label: title.clone(),
                    detail: format!("{:?}", thread.state).to_lowercase(),
                    chord: None,
                    mark: Some(omt_tui::state_mark(&thread.state, true)),
                },
            ));
        }
    }
    if rows.is_empty() {
        // An empty list and "omt found nothing" are different answers, and the
        // second is the one that says whether to go looking for a bug.
        //
        // Worded as a report of what omt did, not as a verdict on what is
        // there. "No agent in any pane" is a sentence somebody reads while an
        // agent's own interface is on the screen behind it, and it makes omt
        // look like it is lying rather than like it has not found something
        // yet — which is what it was read as, twice.
        rows.push((
            u8::MAX,
            omt_tui::Choice {
                label: "omt has not identified an agent in any pane".to_owned(),
                detail: format!(
                    "it looks every {}ms at what each pane is running; \
                 a pane started moments ago may not have been seen yet",
                    crate::probe::PROBE_EVERY_MS
                ),
                chord: None,
                // No mark: this row is not a thing with a state, it is omt
                // reporting on itself.
                mark: None,
            },
        ));
    }
    // By what each row wants, not by whether its text happens to contain a
    // word. Waiting first, then a result nobody has seen, then what is busy —
    // see `omt_tui::attention` for why "done" outranks "working".
    rows.sort_by_key(|(rank, _)| *rank);
    rows.into_iter().map(|(_, choice)| choice).collect()
}

/// An agent's name in as few columns as it can be told apart in.
///
/// Cell B of the status bar is measured in characters and drops from the left
/// when it runs out, so "Claude Code" there costs a session name somebody else
/// needed. The short form is what the agent's own CLI is called, which is also
/// what somebody typed to start it.
pub(crate) fn agent_short_name(kind: omt_types::AgentKind) -> String {
    match kind {
        omt_types::AgentKind::ClaudeCode => "claude",
        omt_types::AgentKind::Codex => "codex",
        omt_types::AgentKind::Copilot => "copilot",
        omt_types::AgentKind::Cursor => "cursor",
        omt_types::AgentKind::Opencode => "opencode",
        omt_types::AgentKind::GeminiCli => "gemini",
        omt_types::AgentKind::QwenCode => "qwen",
        omt_types::AgentKind::Goose => "goose",
        omt_types::AgentKind::Aider => "aider",
        omt_types::AgentKind::Amp => "amp",
        omt_types::AgentKind::Crush => "crush",
        // A third party's adapter can register an agent this build has never
        // heard of. It gets a name rather than a panic.
        _ => "agent",
    }
    .to_owned()
}

/// An agent's name, as a person reads it.
pub(crate) fn agent_name(kind: omt_types::AgentKind) -> String {
    // The debug spelling with the camel humps split, rather than a table that
    // has to be extended every time an agent is added and silently prints
    // `QwenCode` when it is not.
    let raw = format!("{kind:?}");
    let mut out = String::new();
    for (i, ch) in raw.char_indices() {
        if i > 0 && ch.is_uppercase() {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// The rows behind the sessions list.
///
/// Built from the instance rather than from the panes, so a session running in
/// another workspace — or one whose pane was closed while it kept running — is
/// still findable. Whether a row is on screen is said in the row instead of
/// deciding whether the row exists at all: "it is running, you just cannot see
/// it" is the answer somebody is looking for, and omitting it looks like the
/// session is gone.
pub(crate) fn session_rows(instance: &Instance, panes: &[LocalPane]) -> Vec<omt_tui::Choice> {
    let mut rows: Vec<(u8, omt_tui::Choice)> = Vec::new();
    for session in instance.sessions() {
        let workspace = instance
            .workspaces()
            .into_iter()
            .find(|w| w.id == session.workspace)
            .map_or_else(|| "?".to_owned(), |w| w.name.clone());
        let mut parts = vec![workspace, format!("{:?}", session.state).to_lowercase()];
        if let Some(roster) = instance.threads(session.id) {
            let blocked = roster.awaiting_human().len();
            if blocked > 0 {
                parts.push(format!("{blocked} waiting on you"));
            } else if !roster.is_empty() {
                parts.push(format!("{} running", roster.threads().len()));
            }
        }
        if !panes.iter().any(|p| p.session == session.id) {
            parts.push("not on screen".to_owned());
        }
        // What the session wants, from what its agent is doing where there is
        // one, and from its own lifecycle where there is not. A shape rather
        // than a colour: a colour is the first thing a terminal theme takes
        // away.
        let wants = instance
            .threads(session.id)
            .filter(|roster| !roster.awaiting_human().is_empty())
            .map_or_else(
                || match session.state {
                    omt_session::SessionState::Orphaned
                    | omt_session::SessionState::Exited { .. } => (
                        u8::MAX,
                        omt_tui::state_mark(&omt_types::AgentState::Unknown, true),
                    ),
                    _ => (3, omt_tui::state_mark(&omt_types::AgentState::Idle, true)),
                },
                |_| {
                    (
                        0,
                        omt_tui::state_mark(
                            &omt_types::AgentState::Blocked {
                                reason: omt_types::BlockReason::Unspecified,
                                interaction: None,
                            },
                            true,
                        ),
                    )
                },
            );
        rows.push((
            wants.0,
            omt_tui::Choice {
                label: session.title.clone(),
                detail: parts.join(" · "),
                chord: None,
                mark: Some(wants.1),
            },
        ));
    }
    // By what each row wants, not by whether its text happens to contain a
    // word: anything waiting on a human first, because that is why this list
    // gets opened.
    rows.sort_by_key(|(rank, _)| *rank);
    let rows: Vec<omt_tui::Choice> = rows.into_iter().map(|(_, choice)| choice).collect();
    rows
}
