//! Where a process, a terminal and an event stream become one running session.
//!
//! This is the piece that makes everything else real: bytes come off a pty, go
//! through the parser, update a grid, and leave as events with positions on
//! them. Until it existed the terminal core and the pty were two well-tested
//! crates nothing linked.
//!
//! It is deliberately **not** async and owns no threads. `pump()` is called by
//! whoever owns the event loop — the TUI, the server, a test — which is what
//! lets a test drive a real shell through real bytes without a runtime, and
//! what keeps the ordering of "parse, then emit" a property of one function
//! rather than of a scheduler.

use std::io::{Read, Write};

use omt_events::{EventPayload, EventSourceTag, SessionTreeEvent, TerminalEvent};
use omt_pty::{ExitStatus, Pty, PtyConfig, PtyError, PtySize};
use omt_session::{Epoch, WriterError};
use omt_term::{GridSize, HostAction, TermConfig, Terminal};
use omt_types::SessionId;

/// How many bytes are read from a pty in one pass.
///
/// Bounded so one loud session cannot starve every other: `cat`ting a large
/// file is a thing people do, and a pump that read until EOF would hold the
/// loop for as long as the file took.
pub const READ_CHUNK: usize = 64 * 1024;

/// What one `pump` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pumped {
    /// Bytes read from the process.
    pub bytes: usize,
    /// Whether the screen changed and a renderer should redraw.
    pub dirty: bool,
    /// The process exited.
    pub exited: Option<ExitStatus>,
    /// Things the host must do or be told.
    pub actions: Vec<HostAction>,
}

impl Pumped {
    /// Whether anything happened at all.
    #[must_use]
    pub fn is_quiet(&self) -> bool {
        self.bytes == 0 && self.exited.is_none() && self.actions.is_empty()
    }
}

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// The process could not be started or managed.
    #[error(transparent)]
    Pty(#[from] PtyError),
    /// The write was refused.
    #[error(transparent)]
    Writer(#[from] WriterError),
    /// Reading or writing the pty failed.
    #[error("session I/O: {0}")]
    Io(String),
}

/// One live session: a process, its terminal, and the bridge between them.
pub struct SessionRuntime {
    /// Which session this is.
    pub id: SessionId,
    pty: Pty,
    terminal: Terminal,
    exited: Option<ExitStatus>,
}

impl SessionRuntime {
    /// Start a process on a pty and attach a terminal to it.
    ///
    /// The terminal's size and the kernel's are set from one value, here, so
    /// they cannot disagree — a program told one size while the kernel believes
    /// another draws a frame that does not fit and nobody can see why.
    ///
    /// # Errors
    /// Fails if no pty is available or the program cannot be executed.
    pub fn spawn(
        id: SessionId,
        config: &PtyConfig,
        scrollback: omt_term::ScrollbackLimits,
    ) -> Result<Self, RuntimeError> {
        let size = GridSize::new(config.size.cols, config.size.rows);
        let pty = Pty::spawn(config)?;
        // A polling caller must never be held by a quiet session. `pump` is
        // written to treat "nothing to read" as zero bytes, which only works if
        // the read actually returns.
        pty.set_nonblocking(true)?;
        let terminal = Terminal::new(TermConfig {
            size,
            scrollback,
            ..TermConfig::default()
        });
        Ok(Self {
            id,
            pty,
            terminal,
            exited: None,
        })
    }

    /// The terminal state a renderer reads.
    #[must_use]
    pub const fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// The terminal, mutably — resolving a scrollback position memoizes.
    pub const fn terminal_mut(&mut self) -> &mut Terminal {
        &mut self.terminal
    }

    /// The process.
    #[must_use]
    pub const fn pty(&self) -> &Pty {
        &self.pty
    }

    /// How the process ended, once it has.
    #[must_use]
    pub const fn exit_status(&self) -> Option<ExitStatus> {
        self.exited
    }

    /// Move whatever is waiting, once.
    ///
    /// Reads at most [`READ_CHUNK`] bytes, feeds them to the parser, and drains
    /// the host actions the parser produced. Returns without blocking on more.
    ///
    /// # Errors
    /// Fails only on an I/O error that is not the end of the stream. A process
    /// that has exited is an outcome, not an error — it is the normal way a
    /// session ends.
    pub fn pump(&mut self) -> Result<Pumped, RuntimeError> {
        let mut out = Pumped::default();

        let mut buf = [0u8; READ_CHUNK];
        let n = match self.pty.reader().read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => 0,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => 0,
            Err(e) => return Err(RuntimeError::Io(e.to_string())),
        };

        if n > 0 {
            let mut offset = 0;
            while offset < n {
                // `advance` consumes fewer bytes than offered only when an
                // undroppable host action could not be queued. Draining and
                // re-entering is backpressure; looping on the same slice
                // without draining would spin.
                let consumed = self.terminal.advance(&buf[offset..n]);
                out.actions.extend(self.terminal.drain_actions());
                if consumed == 0 {
                    break;
                }
                offset += consumed;
            }
            out.bytes = offset;
            out.dirty = offset > 0;
        }

        out.actions.extend(self.terminal.drain_actions());

        if self.exited.is_none()
            && let Some(status) = self.pty.try_wait()?
        {
            self.exited = Some(status);
            out.exited = Some(status);
            out.dirty = true;
        }

        Ok(out)
    }

    /// Write input to the process.
    ///
    /// Takes the epoch the caller believed it held, so input already in flight
    /// when the writer token changed hands is rejected here rather than landing
    /// in somebody else's command line.
    ///
    /// # Errors
    /// Fails if the epoch is stale, nobody holds the token, or the write does.
    pub fn write_input(
        &mut self,
        writer: &mut omt_session::WriterState,
        epoch: Epoch,
        now_ms: u64,
        bytes: &[u8],
    ) -> Result<usize, RuntimeError> {
        writer.authorize_write(epoch, now_ms)?;
        self.pty
            .writer()
            .write(bytes)
            .map_err(|e| RuntimeError::Io(e.to_string()))
    }

    /// Resize both halves.
    ///
    /// The terminal and the kernel are told together and in that order: the
    /// program is about to be handed `SIGWINCH` and must not find a grid that
    /// still believes the old width.
    ///
    /// # Errors
    /// Fails if the ioctl does.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), RuntimeError> {
        self.terminal.resize(GridSize::new(cols, rows));
        self.pty.resize(PtySize::new(cols, rows))?;
        Ok(())
    }

    /// Turn a host action into an event, where it is one a client should see.
    ///
    /// Most host actions are for the host and stop here. The ones that become
    /// events are the ones another surface has to learn about.
    #[must_use]
    pub fn event_for(&self, action: &HostAction) -> Option<(EventSourceTag, EventPayload)> {
        match action {
            // `Core`: omt observed the program do this. The tier is about how
            // omt knows, and it knows because it parsed the bytes itself.
            HostAction::SetTitle(title) => Some((
                EventSourceTag::Core,
                EventPayload::SessionTree(SessionTreeEvent::Renamed {
                    session: self.id,
                    title: title.clone(),
                }),
            )),
            HostAction::SetCwd { url } => Some((
                EventSourceTag::Core,
                EventPayload::SessionTree(SessionTreeEvent::CwdChanged {
                    session: self.id,
                    cwd: url.clone(),
                }),
            )),
            HostAction::Bell => Some((
                EventSourceTag::Core,
                EventPayload::Terminal(TerminalEvent::Bell),
            )),
            // Replies, clipboard and mode changes are the host's to act on.
            // Emitting them as events would invite a second actor to act on
            // them too.
            _ => None,
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn sh(script: &str) -> PtyConfig {
        PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), script.into()],
            size: PtySize::new(80, 24),
            ..PtyConfig::default()
        }
    }

    fn runtime(script: &str) -> SessionRuntime {
        SessionRuntime::spawn(
            SessionId::new(),
            &sh(script),
            omt_term::ScrollbackLimits::default(),
        )
        .expect("spawn")
    }

    /// Pump until the screen contains `needle`, or give up.
    fn pump_until(rt: &mut SessionRuntime, needle: &str, within: Duration) -> Vec<HostAction> {
        let deadline = Instant::now() + within;
        let mut actions = Vec::new();
        while Instant::now() < deadline {
            let p = rt.pump().expect("pump");
            let quiet = p.is_quiet();
            actions.extend(p.actions);
            if rt
                .terminal()
                .screen_text()
                .iter()
                .any(|l| l.contains(needle))
            {
                break;
            }
            if quiet {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        actions
    }

    #[test]
    fn a_real_program_lands_on_a_real_grid() {
        // The whole point of this file: until it existed, the terminal core and
        // the pty were two crates nothing linked.
        let mut rt = runtime("echo hello-from-a-real-shell");
        pump_until(&mut rt, "hello-from-a-real-shell", Duration::from_secs(5));
        assert!(
            rt.terminal()
                .screen_text()
                .iter()
                .any(|l| l.contains("hello-from-a-real-shell")),
            "{:?}",
            rt.terminal().screen_text()
        );
    }

    #[test]
    fn escape_sequences_from_a_real_program_are_parsed_not_printed() {
        // A shell emits these constantly. If the bridge passed bytes through
        // without parsing, the screen would fill with escape codes.
        let mut rt = runtime("printf '\\033[31mred\\033[0m done\\n'");
        pump_until(&mut rt, "done", Duration::from_secs(5));
        let text = rt.terminal().screen_text().join("");
        assert!(text.contains("red done"), "{text:?}");
        assert!(!text.contains("\u{1b}"), "escape bytes reached the screen");
    }

    #[test]
    fn typed_input_reaches_the_program_and_its_reply_reaches_the_grid() {
        // A round trip through the writer token, the pty and the parser.
        let mut rt = runtime("read line; echo got:$line");
        let mut writer = omt_session::WriterState::default();
        let epoch = writer
            .acquire(omt_types::Actor::Local, 0, false, false)
            .expect("acquire");

        rt.write_input(&mut writer, epoch, 1, b"typed\n")
            .expect("write");
        pump_until(&mut rt, "got:typed", Duration::from_secs(5));
        assert!(
            rt.terminal()
                .screen_text()
                .iter()
                .any(|l| l.contains("got:typed")),
            "{:?}",
            rt.terminal().screen_text()
        );
    }

    #[test]
    fn a_stale_epoch_cannot_write_to_a_real_process() {
        // The token's whole purpose, checked against a live pty rather than in
        // isolation.
        let mut rt = runtime("sleep 5");
        let mut writer = omt_session::WriterState::default();
        let first = writer
            .acquire(omt_types::Actor::Local, 0, false, false)
            .expect("first");
        writer
            .acquire(
                omt_types::Actor::Remote {
                    identity: omt_types::IdentityId::new(),
                    device: omt_types::DeviceId::new(),
                },
                1,
                true,
                false,
            )
            .expect("takeover");

        let err = rt
            .write_input(&mut writer, first, 2, b"should not arrive")
            .expect_err("must reject");
        assert!(matches!(err, RuntimeError::Writer(_)), "{err:?}");
    }

    #[test]
    fn a_title_set_by_a_real_program_becomes_an_event() {
        let mut rt = runtime("printf '\\033]0;my title\\007'; echo marker");
        let actions = pump_until(&mut rt, "marker", Duration::from_secs(5));
        let titled = actions
            .iter()
            .find(|a| matches!(a, HostAction::SetTitle(_)))
            .expect("the program set a title");
        assert!(rt.event_for(titled).is_some(), "and it became an event");
        assert_eq!(rt.terminal().title(), Some("my title"));
    }

    #[test]
    fn a_reply_stays_with_the_host_rather_than_becoming_an_event() {
        // Emitting it would invite a second actor to answer the program too.
        let rt = runtime("true");
        assert!(
            rt.event_for(&HostAction::Reply(b"\x1b[0n".to_vec()))
                .is_none()
        );
    }

    #[test]
    fn resizing_tells_the_program_and_the_grid_the_same_thing() {
        // A program told one size while the kernel believes another draws a
        // frame that does not fit, and nobody can see why.
        let mut rt = runtime("trap 'stty size' WINCH; echo ready; sleep 3");
        pump_until(&mut rt, "ready", Duration::from_secs(5));
        rt.resize(100, 40).expect("resize");
        pump_until(&mut rt, "40 100", Duration::from_secs(5));

        assert_eq!(rt.terminal().grid().size(), GridSize::new(100, 40));
        assert_eq!(rt.pty().size(), PtySize::new(100, 40));
        assert!(
            rt.terminal()
                .screen_text()
                .iter()
                .any(|l| l.contains("40 100")),
            "the program was told too: {:?}",
            rt.terminal().screen_text()
        );
    }

    #[test]
    fn an_exit_is_reported_once_and_is_not_an_error() {
        // The normal way a session ends.
        let mut rt = runtime("exit 3");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = None;
        while Instant::now() < deadline {
            let p = rt.pump().expect("pump");
            if let Some(status) = p.exited {
                seen = Some(status);
                break;
            }
        }
        assert_eq!(seen, Some(ExitStatus::Code(3)));

        // And not again: a second report would emit a second SessionEnd.
        let again = rt.pump().expect("pump");
        assert_eq!(again.exited, None);
        assert_eq!(rt.exit_status(), Some(ExitStatus::Code(3)));
    }

    #[test]
    fn output_scrolled_off_a_real_screen_reaches_scrollback() {
        let mut rt = SessionRuntime::spawn(
            SessionId::new(),
            &PtyConfig {
                program: PathBuf::from("/bin/sh"),
                args: vec![
                    "-c".into(),
                    "for i in 1 2 3 4 5 6 7 8; do echo line$i; done".into(),
                ],
                size: PtySize::new(40, 4),
                ..PtyConfig::default()
            },
            omt_term::ScrollbackLimits::default(),
        )
        .expect("spawn");

        pump_until(&mut rt, "line8", Duration::from_secs(5));
        assert!(
            rt.terminal()
                .scrollback()
                .lines()
                .any(|l| l.text().contains("line1")),
            "the first line scrolled off and was filed"
        );
    }

    #[test]
    fn a_quiet_session_pumps_without_doing_anything() {
        // The loop calls this constantly; it must be cheap and must not report
        // work that did not happen.
        let mut rt = runtime("sleep 3");
        // Drain whatever the shell said on startup.
        for _ in 0..3 {
            let _ = rt.pump().expect("pump");
        }
        let p = rt.pump().expect("pump");
        assert!(p.is_quiet() || p.bytes == 0, "{p:?}");
    }

    #[test]
    fn a_large_burst_is_read_in_bounded_chunks() {
        // One loud session must not be able to hold the loop for as long as a
        // large file takes to print.
        let mut rt = runtime("head -c 300000 /dev/zero | tr '\\0' 'x'; echo DONE");
        let mut passes = 0;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let p = rt.pump().expect("pump");
            assert!(p.bytes <= READ_CHUNK, "a pass read {} bytes", p.bytes);
            if p.bytes > 0 {
                passes += 1;
            }
            if rt
                .terminal()
                .screen_text()
                .iter()
                .any(|l| l.contains("DONE"))
            {
                break;
            }
        }
        assert!(passes > 1, "300 KB arrived in one pass");
    }
}
