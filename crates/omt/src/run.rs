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

    // One row is the hint's, so every pane is sized one short of the terminal.
    // Taking it rather than drawing over it is the only version that works: a
    // program that believes it owns the last row will draw there.
    let usable_rows = rows.saturating_sub(1).max(1);
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
    let started = std::time::Instant::now();
    let mut has_used_prefix = false;
    let mut shown_hint: Option<Option<&'static str>> = None;
    let mut term_rows = rows;
    let mut term_cols = cols;
    let mut layout_dirty = true;

    loop {
        for pane in &panes {
            instance
                .pump_session(pane.session)
                .context("pumping the session")?;
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
            term_rows.saturating_sub(1).max(1),
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
                .draw_at(&mut out, runtime.terminal().grid(), rect.col, rect.row)
                .context("drawing")?;
        }

        // Drawn last so the cursor ends inside the pane that has focus —
        // otherwise it sits wherever the final pane left it and typing looks
        // like it is going somewhere else.
        if let (Some(pane), Some(rect)) = (panes.get(focus), rects.get(focus))
            && let Some(runtime) = instance.runtime(pane.session)
        {
            let cursor = runtime.terminal().grid().cursor;
            write!(
                out,
                "\x1b[{};{}H",
                rect.row + cursor.row + 1,
                rect.col + cursor.col + 1
            )
            .context("placing the cursor")?;
        }

        let focused_session = panes[focus].session;
        let situation = omt_tui::Situation {
            prefix_armed: armed,
            blocked: instance
                .threads(focused_session)
                .map_or(0, |roster| roster.summary().blocked),
            agent_running: instance.threads(focused_session).is_some(),
            age_secs: started.elapsed().as_secs(),
            remote_clients: 0,
            has_used_prefix,
        };
        let hint = omt_tui::hint_for(&situation);
        if shown_hint != Some(hint) {
            // Saved and restored around the write, or the cursor ends parked on
            // the hint row and the program's own cursor appears to vanish.
            write!(
                out,
                "\x1b7\x1b[{};1H{}\x1b8",
                term_rows,
                omt_tui::render_hint(hint, term_cols)
            )
            .context("drawing the hint")?;
            shown_hint = Some(hint);
        }
        out.flush().context("flushing")?;

        if crossterm::event::poll(TICK).context("polling for input")? {
            let event = crossterm::event::read().context("reading input")?;
            let (input, next) = omt_tui::translate(&event, armed, mode);
            // Recorded the moment it is first armed: somebody who has pressed
            // it knows it exists, and the introduction stops being useful.
            has_used_prefix = has_used_prefix || next;
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
                Input::Resize { cols, rows } => {
                    term_rows = rows;
                    term_cols = cols;
                    layout_dirty = true;
                    shown_hint = None;
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
        })
    }
}

/// Draw the separator between panes.
///
/// A vertical rule rather than a blank column: two shells side by side with
/// nothing between them read as one shell with very strange output.
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
    println!("  Ctrl-A ? for every key · Ctrl-A d to detach · `omt web` for a phone");
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
