//! Running a session in the local terminal.
//!
//! The loop is here rather than in `omt-tui` because this is the layer allowed
//! to assemble surfaces: the TUI provides rendering and key translation, the
//! daemon provides the session, and joining them is a binary's job.

use std::io::{self, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use omt_daemon::{Instance, SessionRuntime};
use omt_input::EncodeMode;
use omt_pty::{PtyConfig, PtySize};
use omt_session::{SessionKind, SessionMode};
use omt_session::{WriterPolicy, WriterState};
use omt_term::TermMode;
use omt_tui::{Input, RawGuard, Screen};

/// How long to wait for input before checking the session again.
///
/// Short enough that output feels immediate, long enough that an idle session
/// is not a busy loop. The real fix is polling both the pty and stdin at once,
/// which needs an event source this does not have yet.
const TICK: Duration = Duration::from_millis(8);

/// Rows of chrome above the panes. The tab bar owns the first row.
const CHROME_ROWS: u16 = 1;

/// Where a pane's cursor lands on the real screen, one-based.
///
/// A function rather than an expression inlined at the one place it is used,
/// because it was wrong there: the pane is drawn at `rect.row + CHROME_ROWS`
/// and the cursor was placed at `rect.row`, so it sat one row above the
/// character it belonged after. A multi-line shell prompt shows that
/// immediately — the cursor appears on the line above the one being typed on.
const fn cursor_on_screen(rect: &omt_tui::Rect, cursor_row: u16, cursor_col: u16) -> (u16, u16) {
    (
        rect.row + CHROME_ROWS + cursor_row + 1,
        rect.col + cursor_col + 1,
    )
}

/// Run one shell in this terminal until it exits or the user detaches.
///
/// # Errors
/// Fails if the terminal cannot be put into raw mode, if no pty is available,
/// or if the program cannot be started.
pub fn run_shell(program: Option<String>) -> Result<()> {
    // Clamped, not just defaulted. A terminal really does report zero — under
    // `script`, in a pipeline, mid-drag — and a pty sized zero by zero is one
    // no program can draw on, so the fallback has to cover a bad answer and not
    // only a missing one.
    let (cols, rows) = match crossterm::terminal::size() {
        Ok((c, r)) if c > 0 && r > 1 => (c, r),
        _ => (80, 24),
    };
    let shell = program
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/sh".to_owned());

    // Printed before anything else, so it is above the session rather than
    // interleaved with it. Prints and continues — no question, no wait. Every
    // question is a step, and a step is where people stop.
    first_run_summary();

    let mut instance = Instance::new();
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_owned());
    let workspace = instance
        .open_workspace(&cwd)
        .context("opening a workspace")?;

    // Two rows are omt's: a tab bar at the top and the hint at the bottom. Taken
    // rather than drawn over, because a program that believes it owns a row will
    // draw there — and the screen was omt's *only* contribution to the display
    // before this, which made a multiplexer indistinguishable from the shell it
    // wraps.
    let usable_rows = rows.saturating_sub(2).max(1);
    let mut panes = vec![LocalPane::open(
        &mut instance,
        workspace,
        &shell,
        &cwd,
        PtySize::new(cols, usable_rows),
    )?];
    let mut focus = 0usize;

    // Entered last, so anything that fails above reports on a working terminal
    // rather than into raw mode.
    let _guard = RawGuard::enter().context("entering raw mode")?;
    let mut out = io::stdout();
    let mut armed = false;
    // When the leader was pressed, so the which-key strip can wait 250 ms
    // before appearing (16 §3.3). Somebody executing `⟨leader⟩ s` from muscle
    // memory never sees a flash; somebody who forgot gets the menu.
    let mut armed_at: Option<std::time::Instant> = None;
    let started = std::time::Instant::now();
    let mut has_used_prefix = false;
    let mut shown_hint: Option<Option<&'static str>> = None;
    let mut shown_bar: Option<String> = None;
    let mut shown_bottom: Option<String> = None;
    // The list on screen, if any: the palette, the agents, or the sessions.
    let mut overlay: Option<omt_tui::Overlay> = None;
    let mut chosen: Option<String> = None;
    let mut term_rows = rows;
    let mut term_cols = cols;
    let mut layout_dirty = true;
    // Built once. Rebuilding it twice a second per pane would be the most
    // expensive thing in this loop by an order of magnitude.
    let adapter_set = omt_agent_adapters::builtin();

    loop {
        for pane in &mut panes {
            // Whether anything came off the pty is the floor's only signal, so
            // it is recorded here rather than asked for later: by the time the
            // probe runs, the pump that saw the bytes has returned.
            let produced = instance
                .pump_session(pane.session)
                .context("pumping the session")?;
            if !produced.is_empty() {
                pane.last_output_ms = monotonic_ms();
            }
        }

        // What is running in each pane. The agent layer existed and nothing
        // fed it from here, so `⟨leader⟩ a` was structurally always empty:
        // starting Claude Code in a pane and asking omt about it showed
        // "nothing matches", because nothing had ever looked.
        let now = monotonic_ms();
        for pane in &mut panes {
            if now.saturating_sub(pane.probed_at) < crate::probe::PROBE_EVERY_MS {
                continue;
            }
            pane.probed_at = now;
            let Some(runtime) = instance.runtime(pane.session) else {
                continue;
            };
            let fd = runtime.pty().as_raw_fd();
            let quiet_ms = now.saturating_sub(pane.last_output_ms);
            let refs: Vec<&dyn omt_agent_adapters::AgentAdapter> = adapter_set.all();
            let detection = crate::probe::identify(fd, &refs);
            pane.agent = detection.map(|detection| crate::probe::Probed {
                activity: crate::probe::activity(quiet_ms < 1_000, false, quiet_ms / 1_000),
                detection,
            });
            if let Some(probed) = pane.agent.clone() {
                for reading in crate::probe::readings(&probed, now) {
                    instance.observe_agent(pane.binding, reading);
                }
            }
        }

        // Panes whose process ended stop being drawn. The session stays in the
        // tree — it is finished, not forgotten — but a dead pane holding screen
        // space is space the live ones need.
        let before = panes.len();
        panes.retain(|p| {
            !instance
                .session(p.session)
                .is_some_and(omt_session::Session::is_finished)
        });
        if panes.len() != before {
            layout_dirty = true;
            focus = focus.min(panes.len().saturating_sub(1));
        }
        if panes.is_empty() {
            break;
        }

        let rects = omt_tui::tile(
            term_cols,
            term_rows.saturating_sub(2).max(1),
            omt_tui::Split::Vertical,
            panes.len(),
        );
        if layout_dirty {
            // Everything moved. Diffing against where a pane used to be paints
            // its old contents into its neighbour.
            write!(out, "\x1b[2J").context("clearing")?;
            for pane in &mut panes {
                pane.screen.invalidate();
            }
            for (pane, rect) in panes.iter().zip(&rects) {
                if let Some(rt) = instance.runtime_mut(pane.session) {
                    rt.resize(rect.cols, rect.rows).context("resizing")?;
                }
            }
            draw_separators(&mut out, &rects).context("drawing separators")?;
            layout_dirty = false;
        }

        let mut mode = EncodeMode::default();
        for (i, (pane, rect)) in panes.iter_mut().zip(&rects).enumerate() {
            let Some(runtime) = instance.runtime(pane.session) else {
                continue;
            };
            if i == focus {
                mode = EncodeMode {
                    application_cursor: runtime
                        .terminal()
                        .modes()
                        .contains(TermMode::ApplicationCursor),
                };
            }
            pane.screen
                // Offset by the tab bar, which owns the first row.
                .draw_at(
                    &mut out,
                    runtime.terminal().grid(),
                    rect.col,
                    rect.row + CHROME_ROWS,
                )
                .context("drawing")?;
        }

        let focused_session = panes[focus].session;
        let situation = omt_tui::Situation {
            prefix_armed: armed
                && armed_at.is_some_and(|at| {
                    at.elapsed().as_millis() as u64 >= omt_tui::PENDING_STRIP_DELAY_MS
                }),
            blocked: instance
                .threads(focused_session)
                .map_or(0, |roster| roster.summary().blocked),
            agent_running: instance.threads(focused_session).is_some(),
            age_secs: started.elapsed().as_secs(),
            remote_clients: 0,
            has_used_prefix,
        };
        // The tab bar. Everything omt knows that the shell does not — which
        // workspace this is, what else is open, what is waiting on somebody —
        // and until now it appeared nowhere at all.
        let chrome = omt_tui::Chrome {
            workspace: std::path::Path::new(&cwd)
                .file_name()
                .map_or_else(|| cwd.clone(), |n| n.to_string_lossy().into_owned()),
            // Read once per frame from what git says, not cached: a branch
            // shown after a checkout that changed it is worse than none.
            branch: omt_workspace_fs::git::status(std::path::Path::new(&cwd))
                .ok()
                .and_then(|s| s.branch),
            tabs: panes
                .iter()
                .enumerate()
                .map(|(i, pane)| {
                    let roster = instance.threads(pane.session);
                    omt_tui::Tab {
                        title: instance
                            .session(pane.session)
                            .map_or_else(|| "shell".to_owned(), |s| s.title.clone()),
                        focused: i == focus,
                        blocked: roster.map_or(0, |r| r.awaiting_human().len()),
                        working: roster.is_some_and(|r| !r.is_empty()),
                    }
                })
                .collect(),
        };
        if let Some(open) = &mut overlay {
            // Rebuilt each frame, not only when it was opened. These lists
            // are opened *because* something is expected to change: an agent
            // that starts needing a human while the list is on screen is the
            // event it exists to show, and a snapshot cannot show it. The
            // filter and the selected row survive, so this is invisible to
            // somebody typing.
            match open.title.as_str() {
                "Agents" => open.set_choices(agent_rows(&mut instance, &panes, monotonic_ms())),
                "Sessions" => open.set_choices(session_rows(&instance, &panes)),
                // The palette is the catalog, which does not change while a
                // process is running.
                _ => {}
            }

            // Unconditionally, not only when its text changed. The panes
            // underneath repaint every frame from their own damage, so an
            // overlay drawn once is painted over within milliseconds and what
            // is left is half a list interleaved with a shell — which is
            // exactly what it looked like.
            let lines = omt_tui::render_overlay(open, term_cols, term_rows.saturating_sub(2));
            for (i, line) in lines.iter().enumerate() {
                write!(out, "\x1b[{};1H\x1b[7m{line}\x1b[0m", i as u16 + 2)
                    .context("drawing the overlay")?;
            }
        }

        let bar = omt_tui::tab_bar(&chrome, term_cols);
        if shown_bar.as_deref() != Some(bar.as_str()) {
            write!(out, "\x1b[1;1H\x1b[7m{bar}\x1b[0m").context("drawing the tab bar")?;
            shown_bar = Some(bar);
        }

        // The bottom row carries a hint when there is one to give, and says
        // where you are the rest of the time. A row reserved for a hint that
        // usually has none is a row spent on nothing — and the status it shows
        // instead is counted from the same tabs the bar above renders, so the
        // two cannot disagree.
        let hint = omt_tui::hint_for(&situation);
        let bottom = hint.map_or_else(
            || omt_tui::status_line(&chrome, term_cols),
            |text| omt_tui::render_hint(Some(&text), term_cols),
        );
        if shown_bottom.as_deref() != Some(bottom.as_str()) {
            // No save/restore: the cursor is placed once, after all of this,
            // which is the only ordering that can be right when three
            // different pieces of chrome each move it.
            write!(out, "\x1b[{};1H{bottom}", term_rows)
                .context("drawing the status row")?;
            shown_bottom = Some(bottom);
        }
        let _ = &mut shown_hint;

        // **Last.** Every write above moves the cursor, so placing it earlier
        // leaves it wherever the final piece of chrome finished — which is the
        // bottom row, and which is what "the cursor is dragged to the bottom
        // and does not line up" was. The save/restore on the status row was an
        // attempt at the same thing and could not work: it saved a position
        // the tab bar had already moved.
        if let (Some(pane), Some(rect)) = (panes.get(focus), rects.get(focus))
            && let Some(runtime) = instance.runtime(pane.session)
        {
            let cursor = runtime.terminal().grid().cursor;
            let (row, col) = cursor_on_screen(rect, cursor.row, cursor.col);
            write!(out, "\x1b[{row};{col}H").context("placing the cursor")?;
        }
        out.flush().context("flushing")?;

        if crossterm::event::poll(TICK).context("polling for input")? {
            let event = crossterm::event::read().context("reading input")?;
            // An open overlay takes every key: it is a list somebody is
            // choosing from, and letting a keystroke reach the shell underneath
            // would type into a program nobody is looking at.
            if let Some(open) = overlay.as_mut() {
                if let crossterm::event::Event::Key(key) = &event {
                    match key.code {
                        crossterm::event::KeyCode::Esc => overlay = None,
                        crossterm::event::KeyCode::Up => open.move_by(-1),
                        crossterm::event::KeyCode::Down => open.move_by(1),
                        crossterm::event::KeyCode::Backspace => open.backspace(),
                        crossterm::event::KeyCode::Enter => {
                            // Picking is closing. What a choice *does* depends
                            // on which list it came from, and that is decided
                            // where the list was built.
                            chosen = open.selected().map(|c| c.label.clone());
                            overlay = None;
                        }
                        crossterm::event::KeyCode::Char(ch) => open.push(ch),
                        _ => {}
                    }
                }
                layout_dirty = true;
                if let Some(label) = chosen.take() {
                    // A workspace row is a session to focus; anything else is a
                    // capability name, which the palette shows and the CLI runs.
                    if let Some(index) = panes.iter().position(|p| {
                        instance
                            .session(p.session)
                            .is_some_and(|s| s.title == label)
                    }) {
                        focus = index;
                    }
                }
                continue;
            }

            let (input, next) = omt_tui::translate(&event, armed, mode);
            // Recorded the moment it is first armed: somebody who has pressed
            // it knows it exists, and the introduction stops being useful.
            has_used_prefix = has_used_prefix || next;
            if next {
                // Only the first arming starts the clock. Re-arming on a key
                // that was already pending would restart the delay and the
                // strip would never appear.
                armed_at = armed_at.or_else(|| Some(std::time::Instant::now()));
            } else {
                armed_at = None;
            }
            armed = next;
            match input {
                Input::Bytes(bytes) => {
                    let pane = &mut panes[focus];
                    let session = pane.session;
                    if let Some(rt) = instance.runtime_mut(session) {
                        rt.write_input(&mut pane.writer, pane.epoch, monotonic_ms(), &bytes)
                            .context("writing input")?;
                    }
                }
                Input::Palette => {
                    // Built from the catalog, so the list is what this build
                    // actually registers rather than a menu maintained beside
                    // it — which is how a palette comes to offer something that
                    // is not there.
                    overlay = Some(omt_tui::Overlay::new(
                        "Commands",
                        omt_tui::ACTIONS
                            .iter()
                            .map(|a| omt_tui::Choice {
                                label: a.capability.to_owned(),
                                detail: String::new(),
                                chord: a.binding.map(str::to_owned),
                            })
                            .collect(),
                    ));
                    layout_dirty = true;
                }
                Input::Agents => {
                overlay = Some(omt_tui::Overlay::new(
                    "Agents",
                    agent_rows(&mut instance, &panes, monotonic_ms()),
                ));
                layout_dirty = true;
            }
            Input::Sessions => {
                    // Every session this instance holds, not only the ones with
                    // a pane on screen. A session that is running somewhere
                    // else is exactly the one somebody opens this list to find,
                    // and a list that showed only what was already visible
                    // answered a question nobody had.
                    overlay = Some(omt_tui::Overlay::new("Sessions", session_rows(&instance, &panes)));
                    layout_dirty = true;
                }
                Input::Resize { cols, rows } => {
                    term_rows = rows;
                    term_cols = cols;
                    layout_dirty = true;
                    shown_hint = None;
                    shown_bar = None;
                    shown_bottom = None;
                }
                Input::Split => {
                    // Refused rather than attempted when there is no room. Two
                    // unusable panes are worse than one usable one, and the
                    // user cannot see why nothing is legible.
                    let wanted = panes.len() + 1;
                    if omt_tui::how_many_fit(term_cols, term_rows, omt_tui::Split::Vertical, wanted)
                        == wanted
                    {
                        let size = PtySize::new(term_cols, term_rows.saturating_sub(1).max(1));
                        if let Ok(pane) =
                            LocalPane::open(&mut instance, workspace, &shell, &cwd, size)
                        {
                            panes.push(pane);
                            focus = panes.len() - 1;
                            layout_dirty = true;
                        }
                    }
                }
                Input::NextPane => {
                    focus = (focus + 1) % panes.len();
                    shown_hint = None;
                    shown_bar = None;
                    shown_bottom = None;
                }
                Input::ClosePane => {
                    // The pane goes; the session does not. Closing a view of
                    // something is not ending it.
                    let closed = panes.remove(focus);
                    if let Some(pane) = closed.pane {
                        instance.remove_pane(workspace, pane);
                    }
                    if panes.is_empty() {
                        break;
                    }
                    focus = focus.min(panes.len() - 1);
                    layout_dirty = true;
                }
                Input::Help => {
                    // The cheat sheet is a capability, and duplicating it here
                    // would be a second list to keep in step with the keymap.
                    shown_hint = None;
                    shown_bar = None;
                    shown_bottom = None;
                }
                Input::Detach => break,
                Input::Ignore => {}
            }
        }
    }

    out.flush().ok();
    Ok(())
}

/// One pane the local TUI is drawing.
///
/// Each carries its own `Screen`, because the damage tracking is per region:
/// one screen for the whole terminal would diff a pane's output against its
/// neighbour's and repaint both.
struct LocalPane {
    session: omt_types::SessionId,
    /// Its identity in the workspace's view, so closing it here closes the same
    /// pane a remote client sees.
    pane: Option<omt_types::PaneId>,
    screen: Screen,
    writer: WriterState,
    epoch: omt_session::Epoch,
    /// This pane's agent binding. Minted per pane rather than per detection:
    /// a binding is "the agent occupying this pane", and re-minting it when
    /// the detection changed would throw away the history the merge machine
    /// decides from.
    binding: omt_types::BindingId,
    /// What was last seen running here, and when it was last looked for.
    agent: Option<crate::probe::Probed>,
    probed_at: u64,
    /// When this pane last produced anything, which is the entirety of what
    /// the heuristic floor has to go on.
    last_output_ms: u64,
}

impl LocalPane {
    /// Start a session and take its writer token.
    fn open(
        instance: &mut Instance,
        workspace: omt_types::WorkspaceId,
        program: &str,
        cwd: &str,
        size: PtySize,
    ) -> Result<Self> {
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
                env: vec![("OMT_SESSION".to_owned(), session.to_wire())],
                ..PtyConfig::default()
            },
            omt_term::ScrollbackLimits::default(),
        )
        .context("starting the shell")?;
        instance.attach(runtime).context("attaching")?;
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
            binding: omt_types::BindingId::new(),
            agent: None,
            probed_at: 0,
            last_output_ms: 0,
        })
    }
}

/// Draw the separator between panes.
///
/// A vertical rule rather than a blank column: two shells side by side with
/// nothing between them read as one shell with very strange output.
/// The rows behind the agents list.
///
/// Built from what the probe saw in each pane, not only from thread rosters.
/// A roster is filled by hooks and protocols; a pty-only session has neither,
/// which is why this said "nothing matches" with an agent plainly running in
/// the pane.
fn agent_rows(
    instance: &mut Instance,
    panes: &[LocalPane],
    now: u64,
) -> Vec<omt_tui::Choice> {
    let mut rows: Vec<omt_tui::Choice> = Vec::new();
    for pane in panes {
        let title = instance
            .session(pane.session)
            .map_or_else(|| "shell".to_owned(), |s| s.title.clone());
        if let Some(probed) = &pane.agent {
            let state = instance
                .agent_state(pane.binding, now)
                .unwrap_or(omt_types::AgentState::Unknown);
            rows.push(omt_tui::Choice {
                // The evidence, not only the verdict: somebody looking at a
                // wrong detection needs to see what was matched, not be told
                // a name again.
                detail: format!(
                    "{} · {} · {}",
                    agent_name(probed.detection.agent),
                    state_word(&state),
                    probed.detection.evidence
                ),
                label: title.clone(),
                chord: None,
            });
        }
        let Some(roster) = instance.threads(pane.session) else {
            continue;
        };
        for thread in roster.threads() {
            rows.push(omt_tui::Choice {
                label: title.clone(),
                detail: format!("{:?}", thread.state).to_lowercase(),
                chord: None,
            });
        }
    }
    if rows.is_empty() {
        // An empty list and "omt found nothing" are different answers, and the
        // second is the one that says whether to go looking for a bug.
        rows.push(omt_tui::Choice {
            label: "no agent in any pane".to_owned(),
            detail: "omt looks every half second at what each pane is running".to_owned(),
            chord: None,
        });
    }
    // Waiting first, because that is the reason anybody opens it.
    rows.sort_by_key(|c| usize::from(!c.detail.contains("waiting")));
    rows
}

/// An agent's name, as a person reads it.
fn agent_name(kind: omt_types::AgentKind) -> String {
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

/// What an agent state reads as in one word.
fn state_word(state: &omt_types::AgentState) -> &'static str {
    match state {
        omt_types::AgentState::Starting => "starting",
        omt_types::AgentState::Idle => "idle",
        omt_types::AgentState::Working { .. } => "working",
        omt_types::AgentState::Blocked { .. } => "waiting on you",
        omt_types::AgentState::Exited { .. } => "exited",
        omt_types::AgentState::Unknown => "unknown",
    }
}

/// The rows behind the sessions list.
///
/// Built from the instance rather than from the panes, so a session running in
/// another workspace — or one whose pane was closed while it kept running — is
/// still findable. Whether a row is on screen is said in the row instead of
/// deciding whether the row exists at all: "it is running, you just cannot see
/// it" is the answer somebody is looking for, and omitting it looks like the
/// session is gone.
fn session_rows(instance: &Instance, panes: &[LocalPane]) -> Vec<omt_tui::Choice> {
    let mut rows: Vec<omt_tui::Choice> = Vec::new();
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
        rows.push(omt_tui::Choice {
            label: session.title.clone(),
            detail: parts.join(" · "),
            chord: None,
        });
    }
    // Anything waiting on a human first — that is why this list gets opened.
    rows.sort_by_key(|c| usize::from(!c.detail.contains("waiting on you")));
    rows
}

fn draw_separators(out: &mut impl Write, rects: &[omt_tui::Rect]) -> io::Result<()> {
    for pair in rects.windows(2) {
        let col = pair[0].col + pair[0].cols + 1;
        for row in 0..pair[0].rows {
            write!(out, "\x1b[{};{col}H\x1b[2m│\x1b[0m", row + 1)?;
        }
    }
    Ok(())
}

/// Say what omt found, once, on a machine that has never run it.
///
/// The trigger is the absence of a configuration directory, because that is
/// what "first run" means and it needs no state of its own. The directory is
/// created here, so a second run says nothing — including a second run that
/// changes nothing else.
fn first_run_summary() {
    let Some(home) = config_home() else {
        return;
    };
    if home.exists() {
        return;
    }
    // Created before printing. If creating it fails the summary is skipped
    // rather than printed on every start, which would be worse than never.
    if std::fs::create_dir_all(&home).is_err() {
        return;
    }

    let shell = std::env::var("SHELL")
        .ok()
        .and_then(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "your shell".to_owned());

    let agents = detected_agents();

    println!("omt {}", env!("CARGO_PKG_VERSION"));
    println!("  shell: {shell}");
    if agents.is_empty() {
        println!("  agents: none on PATH — omt runs anything, and detects these when present");
    } else {
        println!("  agents: {}", agents.join(", "));
    }
    let leader = omt_tui::leader();
    println!("  {leader} ? for every key · {leader} d to detach · `omt web` for a phone");
    println!();
}

/// Which agent CLIs are on PATH.
///
/// Named rather than counted: "3 agents detected" tells somebody nothing they
/// can act on, and the list is what makes it obvious omt already knows about
/// the one they use.
fn detected_agents() -> Vec<String> {
    [
        "claude",
        "codex",
        "copilot",
        "gemini",
        "cursor-agent",
        "opencode",
    ]
    .iter()
    .filter(|name| which(name))
    .map(|name| (*name).to_owned())
    .collect()
}

/// Whether a program is on PATH.
fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Where omt keeps its configuration.
fn config_home() -> Option<std::path::PathBuf> {
    std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .ok()
        .map(|base| base.join("omt"))
}

/// Milliseconds since the process started.
///
/// Monotonic rather than wall-clock: the writer token's idle timeout must not
/// move because somebody's clock was corrected.
fn monotonic_ms() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    start.elapsed().as_millis() as u64
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_lands_on_the_row_the_pane_was_drawn_at() {
        // The bug this pins: the pane is drawn `CHROME_ROWS` below its rect and
        // the cursor was not, so on a two-line shell prompt the cursor appeared
        // on the line above the one being typed on.
        let rect = omt_tui::Rect {
            row: 0,
            col: 0,
            rows: 10,
            cols: 80,
        };
        // Grid row 1 — the second line of the prompt — with a one-row tab bar
        // above it is screen row 3, one-based.
        assert_eq!(cursor_on_screen(&rect, 1, 2), (3, 3));
    }

    #[test]
    fn a_second_panes_cursor_carries_that_panes_offset() {
        let rect = omt_tui::Rect {
            row: 5,
            col: 40,
            rows: 5,
            cols: 40,
        };
        assert_eq!(cursor_on_screen(&rect, 0, 0), (7, 41));
    }
}
