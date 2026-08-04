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
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let shell = program
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/sh".to_owned());

    let mut instance = Instance::new();
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_owned());
    let workspace = instance
        .open_workspace(&cwd)
        .context("opening a workspace")?;
    let id = instance
        .create_session(workspace, SessionKind::Shell, SessionMode::Pty)
        .context("creating a session")?;

    let runtime = SessionRuntime::spawn(
        id,
        &PtyConfig {
            program: shell.into(),
            args: Vec::new(),
            cwd: Some(cwd.into()),
            size: PtySize::new(cols, rows),
            env: vec![
                // What lets a hook this shell spawns know which pane it belongs
                // to, rather than having to guess.
                ("OMT_SESSION".to_owned(), id.to_wire()),
            ],
            ..PtyConfig::default()
        },
        omt_term::ScrollbackLimits::default(),
    )
    .context("starting the shell")?;
    instance.attach(runtime).context("attaching")?;

    // Entered last, so anything that fails above reports on a working terminal
    // rather than into raw mode.
    let _guard = RawGuard::enter().context("entering raw mode")?;
    let mut screen = Screen::new();
    let mut out = io::stdout();
    let mut armed = false;

    // The local terminal holds the token like any other actor, rather than
    // writing around it. Auto-acquire is what makes that invisible to somebody
    // alone at their own keyboard — and it means the epoch check on every
    // keystroke is the real path, exercised every time omt runs, instead of a
    // branch only remote clients take.
    let mut writer = WriterState::new(WriterPolicy::default());
    let epoch = writer
        .acquire(omt_types::Actor::Local, monotonic_ms(), false, true)
        .context("acquiring the writer token")?;

    loop {
        instance.pump_session(id).context("pumping the session")?;

        let Some(runtime) = instance.runtime(id) else {
            break;
        };
        let mode = EncodeMode {
            application_cursor: runtime
                .terminal()
                .modes()
                .contains(TermMode::ApplicationCursor),
        };
        screen
            .draw(&mut out, runtime.terminal().grid())
            .context("drawing")?;

        if instance
            .session(id)
            .is_some_and(omt_session::Session::is_finished)
        {
            break;
        }

        if crossterm::event::poll(TICK).context("polling for input")? {
            let event = crossterm::event::read().context("reading input")?;
            let (input, next) = omt_tui::translate(&event, armed, mode);
            armed = next;
            match input {
                Input::Bytes(bytes) => {
                    if let Some(rt) = instance.runtime_mut(id) {
                        rt.write_input(&mut writer, epoch, monotonic_ms(), &bytes)
                            .context("writing input")?;
                    }
                }
                Input::Resize { cols, rows } => {
                    if let Some(rt) = instance.runtime_mut(id) {
                        rt.resize(cols, rows).context("resizing")?;
                    }
                    // The real terminal's contents are no longer what we drew.
                    screen.invalidate();
                }
                Input::Detach => break,
                Input::Ignore => {}
            }
        }
    }

    out.flush().ok();
    Ok(())
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
