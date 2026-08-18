//! Session input and geometry.
//!
//! Split out of one 6454-line module, which P1's own file-length rule
//! calls a design smell and which nothing measured until the check that
//! enforces it was written.

use super::*;

// ---------------------------------------------------------------------------
// Session input and geometry
// ---------------------------------------------------------------------------

/// Input to `session.write`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionWriteIn {
    /// Which session.
    pub session: String,
    /// What to type.
    pub text: String,
    /// The writer-token epoch the caller believed it held.
    ///
    /// Required, not optional. Input already in flight when the token changed
    /// hands must be rejected rather than landing in somebody else's command
    /// line, and a caller that could omit this would be a caller exempt from
    /// the check.
    pub epoch: u64,
}

/// What `session.write` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionWriteOut {
    /// How many bytes went out.
    ///
    /// Bytes written, not bytes delivered. The far side is a program omt does
    /// not own, so a successful write proves the pipe accepted it and nothing
    /// more.
    pub bytes: usize,
}

capability! {
    /// Type into a session.
    pub struct SessionWrite;
    input  = SessionWriteIn,
    output = SessionWriteOut,
    decl = Decl {
        name: "session.write",
        group: "session",
        verb: "write",
        title: "Type into a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_PTY,
        // Never replayed: re-sending the tail of a shell command is how a
        // retry becomes a disaster.
        intent: Some(Intent::RawStream),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Write input to a session, gated on the writer token's epoch.",
    },
}

pub(crate) struct SessionWriteHandler(pub(crate) State);

impl CapabilityHandler<SessionWrite> for SessionWriteHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionWriteIn,
    ) -> Result<SessionWriteOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;
        // Refused by name, because bytes accepted into a session with nothing
        // behind it would look exactly like typing that worked.
        if instance
            .session(id)
            .is_some_and(|s| !s.state.accepts_input())
        {
            return Err(CapabilityError::precondition_failed(
                "that session was restored from a snapshot and has no process — restart it first",
            ));
        }
        let bytes = instance
            .write_session_input(id, omt_session::Epoch(input.epoch), input.text.as_bytes())
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
        Ok(SessionWriteOut { bytes })
    }
}

/// Input to `session.resize`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionResizeIn {
    /// Which session.
    pub session: String,
    /// New width.
    pub cols: u16,
    /// New height.
    pub rows: u16,
}

/// What `session.resize` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionResizeOut {
    /// The size now in force.
    pub cols: u16,
    /// The size now in force.
    pub rows: u16,
}

capability! {
    /// Change a session's size.
    pub struct SessionResize;
    input  = SessionResizeIn,
    output = SessionResizeOut,
    decl = Decl {
        name: "session.resize",
        group: "session",
        verb: "resize",
        title: "Resize a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_PTY,
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Resize a session. The grid and the kernel are told together, so \
              a program is never handed a size the kernel disagrees with.",
    },
}

pub(crate) struct SessionResizeHandler(pub(crate) State);

impl CapabilityHandler<SessionResize> for SessionResizeHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionResizeIn,
    ) -> Result<SessionResizeOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;
        instance
            .runtime_mut(id)
            .ok_or_else(|| CapabilityError::not_found("no such session"))?
            .resize(input.cols, input.rows)
            .map_err(|e| CapabilityError::internal(e.to_string()))?;
        Ok(SessionResizeOut {
            cols: input.cols,
            rows: input.rows,
        })
    }
}

/// Input to `session.read`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionReadIn {
    /// Which session.
    pub session: String,
    /// How many lines of scrollback to include before the screen.
    #[serde(default)]
    pub history: u32,
}

/// What `session.read` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionReadOut {
    /// The visible screen, one string per row.
    pub screen: Vec<String>,
    /// Scrollback before it, oldest first.
    pub history: Vec<String>,
    /// Where the cursor is.
    pub cursor: (u16, u16),
    /// Whether a full-screen program is drawing.
    ///
    /// A client rendering a transcript needs to know: while this is true the
    /// program owns every cell and a line-oriented view of it is nonsense.
    pub alternate_screen: bool,
}

capability! {
    /// What is on a session's screen.
    pub struct SessionRead;
    input  = SessionReadIn,
    output = SessionReadOut,
    decl = Decl {
        name: "session.read",
        group: "session",
        verb: "read",
        title: "Read a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "The visible screen as text, optionally with scrollback.",
    },
}

pub(crate) struct SessionReadHandler(pub(crate) State);

impl CapabilityHandler<SessionRead> for SessionReadHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionReadIn,
    ) -> Result<SessionReadOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let instance = self.0.lock()?;
        let terminal = instance
            .runtime(id)
            .map(omt_daemon::SessionRuntime::terminal)
            .or_else(|| instance.orphan_terminal(id))
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;

        // Bounded rather than "all of it": a client asking for a million lines
        // over a phone link would be answered, slowly, and time out.
        let want = (input.history as usize).min(MAX_HISTORY_LINES);
        let all: Vec<String> = terminal.scrollback().lines().map(|l| l.text()).collect();
        let history = all[all.len().saturating_sub(want)..].to_vec();

        Ok(SessionReadOut {
            screen: terminal.screen_text(),
            history,
            cursor: (terminal.grid().cursor.row, terminal.grid().cursor.col),
            alternate_screen: terminal.active() == omt_term::Which::Alternate,
        })
    }
}

/// Input to `session.acquire`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionAcquireIn {
    /// Which session.
    pub session: String,
    /// Take it even though somebody else holds it.
    #[serde(default)]
    pub force: bool,
}

/// What `session.acquire` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionAcquireOut {
    /// The epoch the caller now holds, which every write is checked against.
    pub epoch: u64,
}

capability! {
    /// Take the writer token.
    pub struct SessionAcquire;
    input  = SessionAcquireIn,
    output = SessionAcquireOut,
    decl = Decl {
        name: "session.acquire",
        group: "session",
        verb: "acquire",
        title: "Take the writer token",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        // Compare-and-swap: the swap is against who holds the token, and a
        // repeat of the same intent must return the epoch it already granted
        // rather than minting a second one — which would invalidate the writes
        // the caller is making with the first.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Take the writer token for a session, returning the epoch every write is checked against.",
    },
}

pub(crate) struct SessionAcquireHandler(pub(crate) State);

impl CapabilityHandler<SessionAcquire> for SessionAcquireHandler {
    fn call(
        &self,
        ctx: &CallContext,
        input: SessionAcquireIn,
    ) -> Result<SessionAcquireOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;
        let epoch = instance
            .acquire_writer(id, ctx.actor.clone(), input.force)
            .map_err(|e| CapabilityError::precondition_failed(e.to_string()))?;
        Ok(SessionAcquireOut { epoch: epoch.0 })
    }
}

/// Input to `session.release`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionReleaseIn {
    /// Which session.
    pub session: String,
}

/// What `session.release` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionReleaseOut {
    /// Whether this caller had been holding it.
    pub released: bool,
}

capability! {
    /// Give the writer token up.
    pub struct SessionRelease;
    input  = SessionReleaseIn,
    output = SessionReleaseOut,
    decl = Decl {
        name: "session.release",
        group: "session",
        verb: "release",
        title: "Give up the writer token",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Give up the writer token. Idempotent: releasing one you do not hold is not an error.",
    },
}

pub(crate) struct SessionReleaseHandler(pub(crate) State);

impl CapabilityHandler<SessionRelease> for SessionReleaseHandler {
    fn call(
        &self,
        ctx: &CallContext,
        input: SessionReleaseIn,
    ) -> Result<SessionReleaseOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;
        Ok(SessionReleaseOut {
            released: instance.release_writer(id, &ctx.actor),
        })
    }
}

/// What a session spawned by this instance is told about omt.
///
/// Assembled here rather than at each call site so the two spawn paths — a
/// terminal's own and an instance's — cannot drift into telling an agent
/// different things.
fn session_env(state: &State, id: omt_types::SessionId) -> Vec<(String, String)> {
    // `shell.inject_env = false` is the user saying they want no agent
    // markers in panes; a session created from the web then carries none.
    if !crate::terminal_bits::inject_markers(state) {
        return Vec::new();
    }
    let mut env = vec![("OMT_SESSION".to_owned(), id.to_wire())];
    if let Some(socket) = state.socket() {
        env.push(("OMT_SOCK".to_owned(), socket.display().to_string()));
    }
    env.push((
        "OMT_AGENT_EVENTS".to_owned(),
        omt_term::AGENT_EVENT_PROTOCOL.to_string(),
    ));
    env
}

/// Input to `session.create`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionCreateIn {
    /// Which workspace to start it in.
    pub workspace: String,
    /// What to run. Absent means the user's shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// Arguments to it.
    #[serde(default)]
    pub args: Vec<String>,
    /// Columns to start at.
    #[serde(default = "default_cols")]
    pub cols: u16,
    /// Rows.
    #[serde(default = "default_rows")]
    pub rows: u16,
}

pub(crate) fn default_cols() -> u16 {
    80
}
pub(crate) fn default_rows() -> u16 {
    24
}

/// What `session.create` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionCreateOut {
    /// The new session.
    pub session: String,
}

capability! {
    /// Start a session.
    pub struct SessionCreate;
    input  = SessionCreateIn,
    output = SessionCreateOut,
    decl = Decl {
        name: "session.create",
        group: "session",
        verb: "create",
        title: "Start a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::SPAWNS_PROCESS,
        // Deduplicated by intent id, because the retry this protects against
        // is the ordinary one: a phone loses the connection between sending
        // and hearing back, resends, and without this the user has two shells
        // and no way to know which one their next keystroke reached. The
        // window outlives any plausible reconnect for the same reason.
        intent: Some(Intent::Append {
            dedup: DedupKey::IntentId,
            ttl_secs: 600,
        }),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Start a session in a workspace, running a program on a pty. Deduplicated by intent id, so a reconnect retry does not start a second process.",
    },
}

pub(crate) struct SessionCreateHandler(pub(crate) State);

impl CapabilityHandler<SessionCreate> for SessionCreateHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionCreateIn,
    ) -> Result<SessionCreateOut, CapabilityError> {
        let workspace = WorkspaceId::from_wire(&input.workspace).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a workspace id", input.workspace))
        })?;
        let mut instance = self.0.lock()?;
        let root = instance
            .workspace_root(workspace)
            .ok_or_else(|| CapabilityError::not_found("no such workspace"))?;

        let program = input
            .program
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".to_owned());

        let id = instance
            .create_session(
                workspace,
                omt_session::SessionKind::Shell,
                omt_session::SessionMode::Pty,
            )
            .map_err(|e| CapabilityError::internal(e.to_string()))?;

        let runtime = omt_daemon::SessionRuntime::spawn(
            id,
            &omt_pty::PtyConfig {
                program: program.into(),
                args: input.args,
                cwd: Some(root.into()),
                size: omt_pty::PtySize::new(input.cols, input.rows),
                // The same variables the local path sets, so a hook started by
                // a session this instance created knows which pane it belongs
                // to *and where to report* — exactly as one started from a
                // terminal that owns its own ptys does. The session id alone
                // was not enough: a hook with no socket exits at its guard
                // clause, which is indistinguishable from a hook nobody
                // installed.
                env: session_env(&self.0, id),
                ..omt_pty::PtyConfig::default()
            },
            crate::terminal_bits::session_limits(&self.0),
        )
        .map_err(|e| CapabilityError::internal(e.to_string()))?;

        instance
            .attach(runtime)
            .map_err(|e| CapabilityError::internal(e.to_string()))?;

        Ok(SessionCreateOut {
            session: id.to_wire(),
        })
    }
}

/// Input to `session.blocks`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionBlocksIn {
    /// Which session.
    pub session: String,
    /// Only the ones that failed.
    ///
    /// The filter a "jump to the last error" needs. 130 and 141 are not
    /// failures — the first is Ctrl-C and the second is what `… | head` does to
    /// everything upstream of it — so this is narrower than "non-zero".
    #[serde(default)]
    pub failed_only: bool,
}

/// One command and what came of it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct BlockSummary {
    /// Its id, which is what a client refers to when it asks to re-run one.
    pub id: String,
    /// The command as the shell marked it.
    pub command: String,
    /// Where its output starts, counting from the first line ever written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_row: Option<u64>,
    /// Where it ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_row: Option<u64>,
    /// `running`, `exited`, or `unknown` — the last meaning the shell said the
    /// command ended and did not say how, which is not the same as success.
    pub outcome: String,
    /// The exit code, where the shell reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Whether a surface should show this as a failure.
    pub failed: bool,
    /// Whether omt ever saw the command this block belongs to.
    ///
    /// False for a block opened from output alone — a shell with no OSC 133, a
    /// bare `ssh`, a container. A surface must not offer "re-run" for one of
    /// these: there is no command, and the alternative is scraping the screen
    /// for something that looks like a prompt.
    pub attributed: bool,
}

/// What `session.blocks` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionBlocksOut {
    /// Oldest first.
    pub blocks: Vec<BlockSummary>,
    /// Whether this shell emits the marks at all.
    ///
    /// False means no blocks will ever appear, which a UI should say rather
    /// than showing an empty list that looks like "you have run nothing".
    pub shell_integration: bool,
}

capability! {
    /// The commands run in a session.
    pub struct SessionBlocks;
    input  = SessionBlocksIn,
    output = SessionBlocksOut,
    decl = Decl {
        name: "session.blocks",
        group: "session",
        verb: "blocks",
        title: "List command blocks",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Commands run in a session with their output ranges and exit codes, from OSC 133. A shell that emits no marks reports none rather than guesses.",
    },
}

pub(crate) struct SessionBlocksHandler(pub(crate) State);

impl CapabilityHandler<SessionBlocks> for SessionBlocksHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionBlocksIn,
    ) -> Result<SessionBlocksOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let instance = self.0.lock()?;
        let terminal = instance
            .runtime(id)
            .map(omt_daemon::SessionRuntime::terminal)
            .or_else(|| instance.orphan_terminal(id))
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;

        let blocks: Vec<BlockSummary> = terminal
            .blocks()
            .iter()
            .filter(|b| !input.failed_only || b.outcome.is_failure())
            .map(|b| BlockSummary {
                id: b.id.to_wire(),
                command: b.command.clone(),
                output_row: b.output_row,
                end_row: b.end_row,
                outcome: match b.outcome {
                    omt_term::Outcome::Running => "running",
                    omt_term::Outcome::Exited { .. } => "exited",
                    omt_term::Outcome::Unknown => "unknown",
                }
                .to_owned(),
                exit_code: match b.outcome {
                    omt_term::Outcome::Exited { code } => Some(code),
                    _ => None,
                },
                failed: b.outcome.is_failure(),
                attributed: b.attributed,
            })
            .collect();

        Ok(SessionBlocksOut {
            // Whether the shell marks its commands. Blocks appear either way
            // now, but an unattributed one cannot be re-run and a UI should say
            // why rather than showing a button that does nothing.
            shell_integration: terminal.blocks().iter().any(|b| b.attributed),
            blocks,
        })
    }
}

/// Input to `session.restart`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionRestartIn {
    /// Which session.
    pub session: String,
    /// What to run. Absent means what it was running before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// Columns to start at.
    #[serde(default = "default_cols")]
    pub cols: u16,
    /// Rows.
    #[serde(default = "default_rows")]
    pub rows: u16,
}

/// What `session.restart` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionRestartOut {
    /// The session, which keeps its id — anything referring to it still does.
    pub session: String,
    /// How many lines of the old screen were kept above the separator.
    pub kept_lines: u32,
}

capability! {
    /// Put a process back behind a restored session.
    pub struct SessionRestart;
    input  = SessionRestartIn,
    output = SessionRestartOut,
    decl = Decl {
        name: "session.restart",
        group: "session",
        verb: "restart",
        title: "Restart a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::SPAWNS_PROCESS,
        // Compare-and-swap on the session's state: a repeat while it is already
        // running is refused rather than spawning a second process behind the
        // same session.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Respawn the command behind a session restored from a snapshot, keeping the old output above a separator.",
    },
}

pub(crate) struct SessionRestartHandler(pub(crate) State);

impl CapabilityHandler<SessionRestart> for SessionRestartHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionRestartIn,
    ) -> Result<SessionRestartOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let mut instance = self.0.lock()?;

        let session = instance
            .session(id)
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;
        // Only an orphan. Restarting a live session would leave two processes
        // on one pty, which is a mess nobody can untangle from the outside.
        if session.state != omt_session::SessionState::Orphaned {
            return Err(CapabilityError::precondition_failed(
                "that session already has a process",
            ));
        }
        let cwd = session.cwd.clone();
        let program = input
            .program
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".to_owned());

        let kept = instance
            .orphan_terminal(id)
            .map(|t| {
                t.screen_text()
                    .iter()
                    .filter(|l| !l.trim().is_empty())
                    .count()
            })
            .unwrap_or(0);

        instance
            .respawn(
                id,
                &omt_pty::PtyConfig {
                    program: program.into(),
                    args: Vec::new(),
                    cwd: cwd.map(Into::into),
                    size: omt_pty::PtySize::new(input.cols, input.rows),
                    env: vec![("OMT_SESSION".to_owned(), id.to_wire())],
                    ..omt_pty::PtyConfig::default()
                },
            )
            .map_err(|e| CapabilityError::internal(e.to_string()))?;

        Ok(SessionRestartOut {
            session: input.session,
            kept_lines: kept as u32,
        })
    }
}

/// Input to `session.snapshot`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionSnapshotIn {
    /// Which session.
    pub session: String,
}

/// A run of cells that share a style.
///
/// Runs rather than cells: a terminal screen is overwhelmingly long stretches
/// of one style, and a cell-per-element payload is roughly twenty times larger
/// for the same picture — which is the difference between a usable phone link
/// and an unusable one.
#[derive(Serialize, schemars::JsonSchema, PartialEq, Eq, Debug)]
pub struct StyledRun {
    /// The text.
    pub text: String,
    /// Foreground, as `#rrggbb`, or absent for the theme's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    /// Background, likewise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    /// Bold.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    /// Italic.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// Underlined.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    /// Inverse video, left for the client to apply so it can swap against
    /// whatever its own default colours are.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inverse: bool,
}

/// What `session.snapshot` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct SessionSnapshotOut {
    /// One entry per row, each a list of runs.
    pub rows: Vec<Vec<StyledRun>>,
    /// Columns, so a client can size itself without counting characters.
    pub cols: u16,
    /// Rows.
    pub rows_count: u16,
    /// Where the cursor is, as (row, col).
    pub cursor: (u16, u16),
    /// Whether the cursor should be drawn at all.
    pub cursor_visible: bool,
    /// Whether a full-screen program is drawing.
    pub alternate_screen: bool,
}

capability! {
    /// A session's screen with its colours.
    pub struct SessionSnapshot;
    input  = SessionSnapshotIn,
    output = SessionSnapshotOut,
    decl = Decl {
        name: "session.snapshot",
        group: "session",
        verb: "snapshot",
        title: "Snapshot a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "The visible screen as styled runs, so a remote client renders the same picture as the terminal without emulating one.",
    },
}

pub(crate) struct SessionSnapshotHandler(pub(crate) State);

impl CapabilityHandler<SessionSnapshot> for SessionSnapshotHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: SessionSnapshotIn,
    ) -> Result<SessionSnapshotOut, CapabilityError> {
        let id = session_id(&input.session)?;
        let instance = self.0.lock()?;
        // An orphan has no runtime and is still readable. That is the whole
        // point of restoring one: content you can read, search and copy, with
        // nothing behind it.
        let terminal = instance
            .runtime(id)
            .map(omt_daemon::SessionRuntime::terminal)
            .or_else(|| instance.orphan_terminal(id))
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;
        let grid = terminal.grid();
        let size = grid.size();

        let rows = (0..size.rows)
            .map(|row| runs_of(&grid.row(row).densified(size.cols)))
            .collect();

        Ok(SessionSnapshotOut {
            rows,
            cols: size.cols,
            rows_count: size.rows,
            cursor: (grid.cursor.row, grid.cursor.col),
            cursor_visible: grid.cursor.visible,
            alternate_screen: terminal.active() == omt_term::Which::Alternate,
        })
    }
}

/// Collapse a row of cells into runs that share a style.
pub(crate) fn runs_of(cells: &[omt_term::Cell]) -> Vec<StyledRun> {
    let mut out: Vec<StyledRun> = Vec::new();
    for cell in cells {
        // The spacer half of a wide character carries no glyph of its own.
        // Emitting anything for it would push every following column right by
        // one and the line would drift.
        if cell.flags.contains(omt_term::Flags::WIDE_SPACER) {
            continue;
        }
        let run = StyledRun {
            text: String::new(),
            fg: css_color(cell.fg),
            bg: css_color(cell.bg),
            bold: cell.flags.contains(omt_term::Flags::BOLD),
            italic: cell.flags.contains(omt_term::Flags::ITALIC),
            underline: cell.flags.underline() != omt_term::Underline::None,
            inverse: cell.flags.contains(omt_term::Flags::INVERSE),
        };
        let ch = match cell.resolve() {
            // An untouched cell is a space, not a NUL: a client rendering NUL
            // draws a hole where the terminal shows blank.
            omt_term::Resolved::Char('\0') => ' ',
            omt_term::Resolved::Char(c) => c,
            omt_term::Resolved::Grapheme(_) => '?',
        };
        match out.last_mut() {
            Some(last) if style_eq(last, &run) => last.text.push(ch),
            _ => {
                let mut fresh = run;
                fresh.text.push(ch);
                out.push(fresh);
            }
        }
    }
    out
}

pub(crate) fn style_eq(a: &StyledRun, b: &StyledRun) -> bool {
    a.fg == b.fg
        && a.bg == b.bg
        && a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.inverse == b.inverse
}

/// A colour the browser can use directly.
///
/// Resolved here rather than sent as a palette index, because the index means
/// nothing without the theme — and the theme is the instance's, so resolving it
/// on the client would render the user's terminal in someone else's colours.
pub(crate) fn css_color(color: omt_term::Color) -> Option<String> {
    match color.kind() {
        omt_term::ColorKind::Default => None,
        omt_term::ColorKind::Indexed(i) => Some(format!("var(--omt-ansi-{i})")),
        omt_term::ColorKind::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
    }
}

/// The most scrollback one read returns.
pub(crate) const MAX_HISTORY_LINES: usize = 5_000;
