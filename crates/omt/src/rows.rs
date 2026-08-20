//! Turning what the instance knows into rows somebody can read.
//!
//! Split out of the frame loop, which was carrying the loop, the observation
//! pass, the layout tree and these builders in one file over the length limit.
//! These are the part with no timing in them: given an instance and the panes
//! on screen, what should the list say — which is a question about wording and
//! ordering, and is testable without a terminal.

use omt_daemon::Instance;

use crate::panes::LocalPane;

/// One session, as every list in the terminal needs it.
///
/// The view model both modes produce. It exists because the row builders used
/// to take an `Instance` — which a terminal watching an instance it does not
/// own does not have — and because the alternative, a second set of builders
/// for the attached case, is two lists that have to agree and will not.
///
/// What only a local probe can know is optional rather than absent: the wire
/// carries a state's name, not the evidence a probe matched, and a row that
/// claimed evidence it did not have would be worse than one that says less.
#[derive(Debug, Clone)]
pub(crate) struct Watched {
    /// Which session.
    pub(crate) id: omt_types::SessionId,
    /// What it is called.
    pub(crate) title: String,
    /// Which workspace it belongs to, by name.
    pub(crate) workspace: String,
    /// What the agent in it is doing.
    pub(crate) state: omt_types::AgentState,
    /// Where the *session* is in its own life, which is a different question.
    pub(crate) lifecycle: omt_session::SessionState,
    /// Which agent, short enough for a column.
    pub(crate) agent: Option<String>,
    /// The agent's name as a person reads it, where omt identified one.
    pub(crate) agent_full: Option<String>,
    /// What a probe matched, where a probe was the one that found it.
    pub(crate) evidence: Option<String>,
    /// How many agent threads are in it.
    pub(crate) threads: usize,
    /// How many of those are waiting for a human.
    pub(crate) blocked: usize,
    /// Whether anybody has looked since its agent stopped working.
    pub(crate) seen: bool,
    /// Whether it has a pane on this screen.
    pub(crate) on_screen: bool,
}

/// What this terminal is watching, when it owns the sessions itself.
pub(crate) fn watched_local(
    instance: &mut Instance,
    panes: &[LocalPane],
    now: u64,
) -> Vec<Watched> {
    let ids: Vec<_> = instance.sessions().iter().map(|s| s.id).collect();
    let mut out = Vec::new();
    for id in ids {
        let binding = instance.binding_for(id);
        let state = instance
            .agent_state(binding, now)
            .unwrap_or(omt_types::AgentState::Unknown);
        let (threads, blocked) = instance.threads(id).map_or((0, 0), |roster| {
            (roster.threads().len(), roster.awaiting_human().len())
        });
        let detected = instance.observed_agent(id).cloned();
        let Some(session) = instance.session(id) else {
            continue;
        };
        let workspace_id = session.workspace;
        let title = session.title.clone();
        let lifecycle = session.state;
        let workspace = instance
            .workspaces()
            .into_iter()
            .find(|w| w.id == workspace_id)
            .map_or_else(|| "?".to_owned(), |w| w.name.clone());
        let pane = panes.iter().find(|p| p.session == id);
        out.push(Watched {
            id,
            title,
            workspace,
            state,
            lifecycle,
            agent: detected.as_ref().map(|d| agent_short_name(d.agent)),
            agent_full: detected.as_ref().map(|d| agent_name(d.agent)),
            evidence: detected.map(|d| d.evidence),
            threads,
            blocked,
            seen: pane.is_none_or(|p| p.seen),
            on_screen: pane.is_some(),
        });
    }
    out
}

/// The same view, when the sessions are somewhere else.
///
/// What the instance reports plus what this terminal knows about its own
/// panes. Two fields are deliberately empty rather than guessed: the evidence
/// a probe matched, which no probe here ran, and the agent's full name, which
/// the roster gives as its short form — a row that invented either would be
/// making up the part somebody reads when a detection looks wrong.
#[allow(
    dead_code,
    reason = "the attached mode's producer: tested against the local one so \
              the two cannot say different things about the same session, and \
              reached when the terminal attaches rather than owns"
)]
pub(crate) fn watched_remote(
    reported: &[crate::backing::Reported],
    panes: &[LocalPane],
    workspace: &str,
) -> Vec<Watched> {
    reported
        .iter()
        .map(|session| {
            let pane = panes.iter().find(|p| p.session == session.id);
            Watched {
                id: session.id,
                title: session.title.clone(),
                workspace: workspace.to_owned(),
                // The name the state travelled under, turned back into a state
                // by the same table the words come from. A word this build has
                // never heard of reads as unknown, never as idle.
                state: session
                    .agent_state
                    .as_deref()
                    .map_or(omt_types::AgentState::Unknown, omt_tui::state_from_word),
                // The instance says what it is doing, not where it is in its
                // own life; a session that is reported at all is one it still
                // holds.
                lifecycle: omt_session::SessionState::Running,
                agent: session.agent.clone(),
                agent_full: session.agent.clone(),
                evidence: None,
                threads: session.threads,
                blocked: session.blocked,
                seen: pane.is_none_or(|p| p.seen),
                on_screen: pane.is_some(),
            }
        })
        .collect()
}

/// The rows behind the agents list.
///
/// Built from what the probe saw in each pane, not only from thread rosters.
/// A roster is filled by hooks and protocols; a pty-only session has neither,
/// which is why this said "nothing matches" with an agent plainly running in
/// the pane.
pub(crate) fn agent_rows(watched: &[Watched]) -> Vec<omt_tui::Choice> {
    // Each row with what it wants, so the sort is over states rather than over
    // whether a row's text happens to contain a word. Two agents can be called
    // the same thing, so this is positional and never keyed by label.
    let mut rows: Vec<(u8, omt_tui::Choice)> = Vec::new();
    for session in watched {
        if let Some(name) = &session.agent_full {
            // The agent's name in the column the eye lands on. This was the
            // session title — "shell" — with the agent buried mid-line after
            // it, so a list opened *to find out whether omt knows about an
            // agent* answered with the word "shell". Somebody reading that
            // concludes it does not, and they are reading it correctly: the
            // first column is the answer.
            let mut detail = vec![
                session.title.clone(),
                omt_tui::state_word(&session.state, session.seen).to_owned(),
            ];
            // Last, and only when there is one: somebody looking at a wrong
            // detection needs to see what was matched, not be told a name
            // again. A terminal watching an instance has no probe of its own,
            // and inventing evidence would be worse than saying less.
            if let Some(evidence) = &session.evidence {
                detail.push(evidence.clone());
            }
            rows.push((
                omt_tui::attention(&session.state, session.seen),
                omt_tui::Choice {
                    label: name.clone(),
                    detail: detail.join(" · "),
                    chord: None,
                    // What this row wants, as a shape. A colour is the first
                    // thing a terminal theme takes away.
                    mark: Some(omt_tui::state_mark(&session.state, session.seen)),
                },
            ));
        }
        // A thread that is waiting is its own row: a subagent can need a
        // person while the agent that spawned it is still working, and a list
        // that showed only the parent would hide the one that is waiting.
        for _ in 0..session.blocked {
            let waiting = omt_types::AgentState::Blocked {
                reason: omt_types::BlockReason::Unspecified,
                interaction: None,
            };
            rows.push((
                omt_tui::attention(&waiting, true),
                omt_tui::Choice {
                    label: session.title.clone(),
                    detail: "waiting on you".to_owned(),
                    chord: None,
                    mark: Some(omt_tui::state_mark(&waiting, true)),
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
pub(crate) fn session_rows(watched: &[Watched]) -> Vec<omt_tui::Choice> {
    let mut rows: Vec<(u8, omt_tui::Choice)> = Vec::new();
    for session in watched {
        let mut parts = vec![
            session.workspace.clone(),
            format!("{:?}", session.lifecycle).to_lowercase(),
        ];
        if session.blocked > 0 {
            parts.push(format!("{} waiting on you", session.blocked));
        } else if session.threads > 0 {
            parts.push(format!("{} running", session.threads));
        }
        if !session.on_screen {
            parts.push("not on screen".to_owned());
        }
        // What the session wants, from what its agent is doing where there is
        // one, and from its own lifecycle where there is not. A shape rather
        // than a colour: a colour is the first thing a terminal theme takes
        // away.
        let wants = if session.blocked > 0 {
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
        } else {
            match session.lifecycle {
                omt_session::SessionState::Orphaned | omt_session::SessionState::Exited { .. } => (
                    u8::MAX,
                    omt_tui::state_mark(&omt_types::AgentState::Unknown, true),
                ),
                _ => (3, omt_tui::state_mark(&omt_types::AgentState::Idle, true)),
            }
        };
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
    rows.into_iter().map(|(_, choice)| choice).collect()
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
    watched: &[Watched],
    branch: Option<&str>,
) -> Vec<omt_tui::PanelSession> {
    watched
        .iter()
        .map(|session| omt_tui::PanelSession {
            session: session.id.to_wire(),
            title: session.title.clone(),
            agent: session.agent.clone(),
            state: session.state.clone(),
            workspace: session.workspace.clone(),
            // The branch, from the cache the status bar already fills. Asking
            // git per session per frame is what that cache exists to prevent,
            // and a panel that made omt shell out sixty times a second would
            // be a panel somebody turns off.
            branch: branch.map(str::to_owned),
            seen: session.seen,
            on_screen: session.on_screen,
        })
        .collect()
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

/// The conversation, from what the instance sent rather than from omt's own
/// types.
///
/// Deliberately structural rather than typed: the wire shape is the
/// capability's, a client one version older or newer will meet a kind it does
/// not know, and refusing to draw the rest of somebody's conversation over one
/// unfamiliar entry would be worse than saying what it can.
fn remote_conversation_rows(entries: &[serde_json::Value], cols: u16) -> Vec<omt_tui::Choice> {
    let said: Vec<omt_tui::Said> = entries
        .iter()
        .map(|entry| {
            let kind = entry["kind"].as_str().unwrap_or_default();
            let who = match kind {
                "user" => {
                    if entry["synthetic"].as_bool().unwrap_or(false) {
                        omt_tui::Who::Onward
                    } else {
                        omt_tui::Who::You
                    }
                }
                "assistant" | "plan" => omt_tui::Who::Agent,
                "reasoning" => omt_tui::Who::Thinking,
                "tool_call" | "tool_result" => omt_tui::Who::Tool,
                "file_changed" => omt_tui::Who::File,
                _ => omt_tui::Who::Other,
            };
            let text = entry["text"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| entry["name"].as_str().map(str::to_owned))
                .or_else(|| entry["path"].as_str().map(str::to_owned))
                .unwrap_or_else(|| kind.to_owned());
            omt_tui::Said { who, text }
        })
        .collect();
    if said.is_empty() {
        return conversation_rows(&[], &[], cols);
    }
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
    attached: &mut Option<crate::backing::Attached>,
    state: &crate::state::State,
    session: omt_types::SessionId,
    cols: u16,
) -> Vec<omt_tui::Choice> {
    // Attached, the transcript lives where the hooks report — the instance —
    // so it is asked for rather than read. A terminal that read its own empty
    // copy would say "nothing said yet" about an agent mid-conversation, which
    // is the most confident way to be wrong.
    if let Some(joined) = attached.as_mut() {
        let entries = crate::backing::transcript(joined, session).unwrap_or_default();
        return remote_conversation_rows(&entries, cols);
    }
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

/// The rows behind the agent's own command list.
///
/// omt's palette lists what *omt* can do; this lists what the agent in this
/// pane says it can do, which is a different question and has a different
/// answer per pane. The names come from the agent — never from a table omt
/// keeps — because a list offering commands the agent in front of somebody
/// does not have is worse than no list.
pub(crate) fn command_rows(commands: &[(String, String)]) -> Vec<omt_tui::Choice> {
    if commands.is_empty() {
        // Why, not blankness. An agent reports its commands over its hooks or
        // its protocol; one identified only from its screen has never told omt
        // anything of the sort, and that is a fact about the connection rather
        // than about the agent.
        return vec![omt_tui::Choice {
            label: "this agent has not said what it offers".to_owned(),
            detail: "its commands arrive with its hooks or its protocol; \
                     an agent recognised only from its screen reports none"
                .to_owned(),
            chord: None,
            mark: None,
        }];
    }
    commands
        .iter()
        .map(|(name, description)| omt_tui::Choice {
            // With the slash, because that is what gets typed and what
            // somebody is looking for.
            label: format!("/{name}"),
            detail: description.clone(),
            chord: None,
            mark: None,
        })
        .collect()
}

/// The rows behind what has been run here before.
///
/// Ordered by the history's own score, which weighs how often a command was
/// run *in this workspace* — the reason a suggestion is there is the reason it
/// is near the top, and the count is shown so somebody can see it.
pub(crate) fn recall_rows(suggestions: &[(String, u32)]) -> Vec<omt_tui::Choice> {
    if suggestions.is_empty() {
        return vec![omt_tui::Choice {
            label: "nothing has been recorded here yet".to_owned(),
            detail: "commands are written down when a shell says one finished, \
                     which needs its shell integration installed"
                .to_owned(),
            chord: None,
            mark: None,
        }];
    }
    suggestions
        .iter()
        .map(|(command, uses)| omt_tui::Choice {
            label: command.clone(),
            detail: match uses {
                0 => "from another workspace".to_owned(),
                1 => "once here".to_owned(),
                many => format!("{many} times here"),
            },
            chord: None,
            mark: None,
        })
        .collect()
}

/// What has been run in this workspace, best first.
pub(crate) fn recall_of(
    attached: &mut Option<crate::backing::Attached>,
    state: &crate::state::State,
    workspace: omt_types::WorkspaceId,
) -> Vec<(String, u32)> {
    if let Some(joined) = attached.as_mut() {
        return crate::backing::recall(joined, workspace).unwrap_or_default();
    }
    state.recall().map_or_else(
        |_| Vec::new(),
        |history| {
            history
                .suggest("", workspace, 20)
                .into_iter()
                .map(|s| (s.command, s.uses_here))
                .collect()
        },
    )
}

/// What the agent in a session offers, from wherever the session lives.
pub(crate) fn offers_of(
    attached: &mut Option<crate::backing::Attached>,
    state: &crate::state::State,
    session: omt_types::SessionId,
) -> Vec<(String, String)> {
    if let Some(joined) = attached.as_mut() {
        return crate::backing::offers(joined, session).unwrap_or_default();
    }
    state.transcripts().map_or_else(
        |_| Vec::new(),
        |transcripts| {
            transcripts
                .get(session)
                .map_or_else(Vec::new, |transcript| {
                    transcript
                        .commands()
                        .iter()
                        .map(|command| {
                            (
                                command.name.clone(),
                                command.description.clone().unwrap_or_default(),
                            )
                        })
                        .collect()
                })
        },
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod view_tests {
    use super::*;

    #[test]
    fn a_command_row_is_typed_the_way_it_is_read() {
        // The label is what gets typed into the agent's own interface, so it
        // carries the slash somebody is looking for and nothing they are not.
        let rows = command_rows(&[("compact".to_owned(), "shorten the context".to_owned())]);
        assert_eq!(rows[0].label, "/compact");
        assert_eq!(rows[0].detail, "shorten the context");
    }

    #[test]
    fn an_agent_that_has_reported_nothing_says_why() {
        // Blankness reads as a broken list. The reason is a fact about the
        // connection — hooks or a protocol — rather than about the agent.
        let rows = command_rows(&[]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].label.contains("has not said"), "{rows:?}");
        assert!(
            !rows[0].label.starts_with('/'),
            "the row that explains an absence must not look typeable: {rows:?}"
        );
    }

    #[test]
    fn a_command_with_no_description_still_lists() {
        // Agents report the name and often nothing else. A list that dropped
        // those would hide most of what an agent offers.
        let rows = command_rows(&[("clear".to_owned(), String::new())]);
        assert_eq!(rows[0].label, "/clear");
        assert!(rows[0].detail.is_empty());
    }

    fn reported(state: Option<&str>, blocked: usize) -> crate::backing::Reported {
        crate::backing::Reported {
            id: omt_types::SessionId::new(),
            title: "app".to_owned(),
            agent: Some("claude".to_owned()),
            agent_state: state.map(str::to_owned),
            threads: 1,
            blocked,
            live: true,
        }
    }

    #[test]
    fn a_remote_state_becomes_the_same_state_the_local_one_would() {
        // The two modes have to produce rows that say the same thing, or the
        // same session reads differently depending on which terminal is
        // looking at it.
        let watched = watched_remote(&[reported(Some("blocked"), 1)], &[], "app");
        assert!(matches!(
            watched[0].state,
            omt_types::AgentState::Blocked { .. }
        ));
        let rows = agent_rows(&watched);
        assert!(
            rows.iter().any(|r| r.detail.contains("waiting on you")),
            "{rows:?}"
        );
    }

    #[test]
    fn a_state_the_instance_did_not_report_is_unknown_rather_than_idle() {
        // "Nothing to do here" is the one wrong answer for a session omt has
        // not been told about yet.
        let watched = watched_remote(&[reported(None, 0)], &[], "app");
        assert!(matches!(watched[0].state, omt_types::AgentState::Unknown));
    }

    #[test]
    fn a_remote_row_claims_no_evidence_it_does_not_have() {
        // The evidence column is what somebody reads when a detection looks
        // wrong. A terminal watching an instance ran no probe, so it says
        // less rather than inventing the part that matters most.
        let watched = watched_remote(&[reported(Some("working"), 0)], &[], "app");
        assert!(watched[0].evidence.is_none());
        let rows = agent_rows(&watched);
        assert_eq!(rows[0].detail.matches('·').count(), 1, "{rows:?}");
    }

    #[test]
    fn a_session_with_no_pane_says_so_rather_than_being_left_out() {
        // "It is running, you just cannot see it" is the answer somebody opens
        // this list for.
        let watched = watched_remote(&[reported(Some("idle"), 0)], &[], "app");
        let rows = session_rows(&watched);
        assert!(
            rows.iter().any(|r| r.detail.contains("not on screen")),
            "{rows:?}"
        );
    }
}
