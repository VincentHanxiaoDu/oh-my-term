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

/// Whether the agent panel is on.
///
/// Off unless somebody asked for it, per 19 §2.1b. Read from the environment
/// rather than from the config file for now: the setting belongs in
/// `appearance`, and putting it there is a schema change with a generated
/// artifact behind it — worth doing on its own rather than folded into the
/// panel it configures.
#[must_use]
pub(crate) fn panel_enabled() -> bool {
    if let Ok(explicit) = std::env::var("OMT_AGENT_PANEL") {
        return explicit != "0" && !explicit.is_empty();
    }
    panel_choice_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|kept| kept.trim() == "1")
}

/// What the panel shows, from the instance.
///
/// The rows come from `omt_tui::entries`, which is the one computation both
/// surfaces use. This only gathers what it needs — which agent, what state,
/// where — and hands it over.
pub(crate) fn panel_sessions(
    instance: &mut Instance,
    panes: &[LocalPane],
    now: u64,
    branch: Option<&str>,
) -> Vec<omt_tui::PanelSession> {
    let branch = branch.map(str::to_owned);
    let ids: Vec<_> = instance.sessions().iter().map(|s| s.id).collect();
    let mut out = Vec::new();
    for id in ids {
        let Some(session) = instance.session(id) else {
            continue;
        };
        let title = session.title.clone();
        let workspace = instance
            .workspaces()
            .into_iter()
            .find(|w| w.id == session.workspace)
            .map_or_else(|| "?".to_owned(), |w| w.name.clone());
        let pane = panes.iter().find(|p| p.session == id);
        let binding = instance.binding_for(id);
        let state = instance
            .agent_state(binding, now)
            .unwrap_or(omt_types::AgentState::Unknown);
        let agent = instance
            .observed_agent(id)
            .map(|found| agent_short_name(found.agent));
        out.push(omt_tui::PanelSession {
            session: id.to_wire(),
            title,
            agent,
            state,
            workspace,
            // The branch, from the cache the status bar already fills. Asking
            // git per session per frame is what that cache exists to prevent,
            // and a panel that made omt shell out sixty times a second would
            // be a panel somebody turns off.
            branch: branch.clone(),
            seen: pane.is_none_or(|p| p.seen),
            on_screen: pane.is_some(),
        });
    }
    out
}

/// Remember whether the panel is on.
///
/// A panel somebody has to turn on every session is one they stop using
/// (19 §2.1b). Written beside the snapshot rather than into the config file:
/// this is a per-machine view preference, like which pane is focused, and
/// putting it in the file somebody edits by hand would have omt rewriting
/// their comments.
pub(crate) fn remember_panel(on: bool) {
    let Some(path) = panel_choice_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A failure here costs the setting on the next start and nothing else,
    // which is not worth interrupting somebody's session for.
    let _ = std::fs::write(&path, if on { "1" } else { "0" });
}

/// Where that choice is kept.
fn panel_choice_path() -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })
        .ok()?;
    Some(base.join("omt").join("agent-panel"))
}

/// The rows behind the agent conversation.
///
/// The terminal grid shows what an agent *painted* — a box it redraws and
/// scrolls away. This is what it said and did, which is the thing a phone has
/// been able to read since the web client shipped and a keyboard could not.
///
/// Empty is a real answer with words on it: a shell has no conversation, and a
/// list that came up blank would read as omt having lost one.
pub(crate) fn conversation_rows(
    entries: &[crate::transcript::Entry],
    asking: &[omt_events::Interaction],
    cols: u16,
) -> Vec<omt_tui::Choice> {
    if entries.is_empty() && asking.is_empty() {
        return vec![omt_tui::Choice {
            label: "nothing said yet".to_owned(),
            detail: "this pane has no agent transcript — a shell has no conversation".to_owned(),
            chord: None,
            mark: None,
        }];
    }
    let mut said: Vec<omt_tui::Said> = entries.iter().map(said_of).collect();
    // Last, because it is the thing to deal with and the eye lands at the
    // bottom of a conversation. Read from the ledger rather than the record:
    // an open card is state, and a transcript that carried a copy would
    // disagree with the card list the moment somebody answered.
    said.extend(asking.iter().filter_map(question_of));
    omt_tui::conversation_lines(&said, cols)
        .into_iter()
        .map(|line| omt_tui::Choice {
            label: line,
            detail: String::new(),
            chord: None,
            mark: None,
        })
        .collect()
}

/// An open question, as a line somebody reads.
///
/// Only the kinds that are questions. A permission card and a plan review are
/// their own surfaces with their own words, and flattening them into one line
/// of prose would ask somebody to approve a command they cannot see.
fn question_of(interaction: &omt_events::Interaction) -> Option<omt_tui::Said> {
    let omt_events::InteractionKind::Choice { questions } = &interaction.kind else {
        return None;
    };
    let text = questions
        .iter()
        .map(|q| {
            let options = q
                .options
                .iter()
                .map(|o| o.label.clone())
                .collect::<Vec<_>>()
                .join(" / ");
            format!("{} [{options}]", q.question)
        })
        .collect::<Vec<_>>()
        .join("  ");
    Some(omt_tui::Said {
        who: omt_tui::Who::Asking,
        text,
    })
}

/// One transcript entry, as a line somebody reads.
fn said_of(entry: &crate::transcript::Entry) -> omt_tui::Said {
    use crate::transcript::Entry;
    let (who, text) = match entry {
        Entry::User { text, synthetic } => (
            // A turn omt sent on somebody's behalf is marked as one. Shown as
            // a person's turn, it is omt putting words in their mouth.
            if *synthetic {
                omt_tui::Who::Onward
            } else {
                omt_tui::Who::You
            },
            text.clone(),
        ),
        Entry::Assistant { text } => (omt_tui::Who::Agent, text.clone()),
        Entry::Reasoning { text } => (omt_tui::Who::Thinking, text.clone()),
        Entry::ToolCall { name, input, .. } => {
            (omt_tui::Who::Tool, format!("{name} {}", one_line(input)))
        }
        Entry::ToolResult { error, .. } => (
            omt_tui::Who::Tool,
            error
                .clone()
                .map_or_else(|| "done".to_owned(), |e| format!("failed: {e}")),
        ),
        Entry::Plan { steps } => (
            omt_tui::Who::Agent,
            format!(
                "plan: {}",
                steps
                    .iter()
                    .map(|s| s.title.clone())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        ),
        Entry::FileChanged { path, change, .. } => (omt_tui::Who::File, format!("{change} {path}")),
        Entry::Other { category, text } => (omt_tui::Who::Other, format!("{category}: {text}")),
    };
    omt_tui::Said { who, text }
}

/// A tool's arguments on one line.
///
/// Summarized rather than pretty-printed: a `Write` call carries a whole file,
/// and a conversation view that unrolled it would be a file viewer with a
/// conversation lost somewhere inside it.
fn one_line(input: &serde_json::Value) -> String {
    let text = match input {
        serde_json::Value::Object(fields) => fields
            .iter()
            .map(|(k, v)| match v {
                serde_json::Value::String(s) => format!("{k}={s}"),
                other => format!("{k}={other}"),
            })
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    };
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .collect();
    // Cut on a character boundary, and said so: a line ending in `…` is
    // visibly shortened, where a silent cut reads as the whole argument.
    if flat.chars().count() > 120 {
        flat.chars().take(119).collect::<String>() + "\u{2026}"
    } else {
        flat
    }
}

/// The focused pane's conversation, as rows.
///
/// Reads the transcript through the same lock every other surface uses, and
/// answers with words rather than an empty list when there is nothing — a
/// blank overlay reads as a conversation omt lost.
pub(crate) fn conversation_of(
    instance: &Instance,
    state: &crate::state::State,
    session: omt_types::SessionId,
    cols: u16,
) -> Vec<omt_tui::Choice> {
    // Read from the instance the caller already holds, never locked again
    // here: the frame loop holds that guard for the whole iteration, and a
    // second acquisition on the same thread is a deadlock rather than a wait —
    // the terminal simply stops, with no error anywhere to say why.
    let asking: Vec<omt_events::Interaction> = instance
        .ledger
        .open_interactions()
        .into_iter()
        .filter(|i| i.session == session)
        .cloned()
        .collect();
    let Ok(transcripts) = state.transcripts() else {
        return conversation_rows(&[], &asking, cols);
    };
    let entries = transcripts
        .get(session)
        .map(|t| t.entries().to_vec())
        .unwrap_or_default();
    conversation_rows(&entries, &asking, cols)
}
