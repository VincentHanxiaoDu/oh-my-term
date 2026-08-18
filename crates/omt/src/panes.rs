//! The panes the local terminal is drawing.
//!
//! Split out of `run.rs` when it crossed the hard file-length limit. The split
//! is along the seam the gate exposed rather than a convenient line count: a
//! pane owns a session, a screen and a writer token, and the loop that draws
//! frames owns everything else.

use anyhow::{Context, Result};
use omt_daemon::{Instance, SessionRuntime};
use omt_pty::{PtyConfig, PtySize};
use omt_session::{SessionKind, SessionMode};
use omt_tui::Screen;

use omt_session::{WriterPolicy, WriterState};

use crate::run::monotonic_ms;
use crate::terminal_bits::host_colours;

/// One pane the local TUI is drawing.
///
/// Each carries its own `Screen`, because the damage tracking is per region:
/// one screen for the whole terminal would diff a pane's output against its
/// neighbour's and repaint both.
pub(crate) struct LocalPane {
    pub(crate) session: omt_types::SessionId,
    /// Its identity in the workspace's view, so closing it here closes the same
    /// pane a remote client sees.
    pub(crate) pane: Option<omt_types::PaneId>,
    pub(crate) screen: Screen,
    pub(crate) writer: WriterState,
    pub(crate) epoch: omt_session::Epoch,
    /// This pane's agent binding. Minted per pane rather than per detection:
    /// a binding is "the agent occupying this pane", and re-minting it when
    /// the detection changed would throw away the history the merge machine
    /// decides from.
    pub(crate) binding: omt_types::BindingId,
    /// What was last seen running here, and when it was last looked for.
    pub(crate) agent: Option<crate::probe::Probed>,
    pub(crate) probed_at: u64,
    /// When this pane last produced anything, which is the entirety of what
    /// the heuristic floor has to go on.
    pub(crate) last_output_ms: u64,
    /// Which leaf of the layout tree this pane is.
    ///
    /// The tree holds the arrangement and knows nothing about sessions; this
    /// is the join. Kept on the pane rather than in a map because a pane and
    /// its place are created and destroyed together, and two structures that
    /// have to be kept in step are two structures that will not be.
    pub(crate) tile: omt_tui::PaneId,
    /// Whether anybody has looked at this pane since its agent stopped
    /// working.
    ///
    /// Not on the wire and not in `AgentState`, because it is not something
    /// the agent knows: an agent is idle either way, and whether *you* have
    /// seen it is a fact about you. It is what separates "it finished while
    /// you were elsewhere" from "it is sitting there and you have been
    /// watching it", which is the distinction this whole product is for.
    pub(crate) seen: bool,
    /// What this pane's agent was doing when it was last looked at.
    ///
    /// Kept so a *change* can be told from a report. Every source that reads a
    /// screen re-answers on a timer, so a notification fired on the state
    /// rather than on the transition would fire every few hundred
    /// milliseconds — which is how a tool gets muted in its first hour.
    pub(crate) said: Option<omt_types::AgentState>,
}

/// Everything a new pane needs, which is more than seven words.
///
/// A struct rather than a longer parameter list: the shape is the argument
/// list of one call site repeated three times, and the seventh unlabelled
/// argument is where somebody swaps two strings.
#[derive(Clone, Copy)]
pub(crate) struct Opening<'a> {
    pub(crate) workspace: omt_types::WorkspaceId,
    pub(crate) program: &'a str,
    pub(crate) cwd: &'a str,
    pub(crate) size: PtySize,
    pub(crate) tile: omt_tui::PaneId,
    pub(crate) socket: &'a str,
    pub(crate) limits: omt_daemon::SessionLimits,
}

impl LocalPane {
    /// A pane for a session this terminal is watching rather than running.
    ///
    /// No pty, no writer token held here — the token is the client's and lives
    /// with the connection, because the thing that owns the session is the
    /// thing that must arbitrate who types into it.
    pub(crate) fn watching(session: omt_types::SessionId, tile: omt_tui::PaneId) -> Self {
        Self {
            session,
            // The instance holds the workspace's panes. A client inventing one
            // would put a pane in its own head that no other surface can see.
            pane: None,
            screen: Screen::new(),
            writer: WriterState::new(WriterPolicy::default()),
            epoch: omt_session::Epoch::default(),
            binding: omt_types::BindingId::new(),
            // What is running in it comes from the instance, which is the one
            // with the pty to look at.
            agent: None,
            probed_at: 0,
            last_output_ms: 0,
            tile,
            seen: true,
            said: None,
        }
    }

    /// Start a session and take its writer token.
    pub(crate) fn open(instance: &mut Instance, opening: &Opening<'_>) -> Result<Self> {
        let Opening {
            workspace,
            program,
            cwd,
            size,
            tile,
            socket,
            limits,
        } = *opening;
        let session = instance
            .create_session(workspace, SessionKind::Shell, SessionMode::Pty)
            .context("creating a session")?;
        let runtime = SessionRuntime::spawn(
            session,
            &PtyConfig {
                program: program.into(),
                args: Vec::new(),
                cwd: Some(cwd.into()),
                size,
                // What a hook needs to know before it has read a byte:
                // which session it belongs to and where to report. Without
                // the socket a hook gives up on its first line — which is
                // why every agent run inside omt was reporting nothing.
                env: vec![
                    ("OMT_SESSION".to_owned(), session.to_wire()),
                    ("OMT_SOCK".to_owned(), socket.to_owned()),
                    // Said out loud, because an agent that has to guess whether
                    // its host understands a sequence will either stay silent
                    // or write JSON onto somebody's screen. The version is the
                    // whole negotiation: a number an agent can compare, with no
                    // round trip on a channel that has no replies.
                    (
                        "OMT_AGENT_EVENTS".to_owned(),
                        omt_term::AGENT_EVENT_PROTOCOL.to_string(),
                    ),
                ],
                ..PtyConfig::default()
            },
            limits,
        )
        .context("starting the shell")?;
        instance.attach(runtime).context("attaching")?;
        // What colour the terminal on the other end is. Programs in this pane
        // ask, and an agent CLI is one of the programs that asks: silence
        // leaves it to guess whether it is on a light or a dark terminal, and
        // the guess is wrong about half the time.
        let (foreground, background) = host_colours();
        if let Some(rt) = instance.runtime_mut(session) {
            rt.tell_colours(foreground, background);
        }
        let pane = instance.add_pane(workspace, session);

        // The local terminal holds the token like any other actor rather than
        // writing around it. Auto-acquire makes that invisible to somebody
        // alone at their keyboard, and it means the epoch check on every
        // keystroke is the real path rather than a branch only remotes take.
        let mut writer = WriterState::new(WriterPolicy::default());
        let epoch = writer
            .acquire(omt_types::Actor::Local, monotonic_ms(), false, true)
            .context("acquiring the writer token")?;
        Ok(Self {
            session,
            pane,
            screen: Screen::new(),
            writer,
            epoch,
            // The instance's binding for this session, never one minted here.
            // A pane that mints its own is a second machine for the same
            // agent: observation fed one and every reader asked the other, so
            // a pane with Claude Code in it reported "state not reported yet"
            // for as long as it ran.
            binding: instance.binding_for(session),
            agent: None,
            probed_at: 0,
            last_output_ms: 0,
            tile,
            // A pane somebody just opened is one they are looking at.
            seen: true,
            said: None,
        })
    }
}
