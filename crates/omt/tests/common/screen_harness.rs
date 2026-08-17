//! The pty harness these tests drive omt through.
//!
//! Here rather than in the test file because that file crossed the length
//! limit, and here rather than in a second test *binary* because cargo runs
//! binaries in parallel — and a suite of real ptys that ran two at a time
//! would fail on timing rather than on behaviour.

#![allow(dead_code, reason = "each test binary uses a different part of this")]

use std::io::{Read, Write};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use omt_pty::{Pty, PtyConfig, PtySize};
use omt_term::{GridSize, ScrollbackLimits, TermConfig, Terminal};

/// One test at a time.
///
/// Every test here starts a real `omt` on a real pty: a shell, a terminal
/// emulator, a pump loop and an observation pass, per test. Sixteen of those
/// at once on one machine is not a test of omt, it is a test of the machine —
/// and it fails as one, on the assertions with a settle window in them, in a
/// pattern that changes with the load.
///
/// The web suite runs its browser cases with one worker for the same reason
/// and says so in its config. Here it belongs in the file, because a harness
/// that is only correct when somebody remembers a flag is one that will be run
/// without it.
pub static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Take the lock, ignoring poisoning.
///
/// A test that panicked already failed and reported why; refusing to run the
/// rest because of it turns one failure into sixteen and hides which was
/// first.
pub fn serially() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

pub const COLS: u16 = 100;
pub const ROWS: u16 = 30;

/// Everything in this process's environment that would make omt believe an
/// agent is running when none is.
///
/// This suite runs inside an agent session. `CLAUDECODE`,
/// `CLAUDE_CODE_ENTRYPOINT` and a dozen others are set in it, a pty child
/// inherits them, and omt's process probe matches on exactly those markers —
/// so it reported "Claude Code" for a plain shell, for the harness itself, for
/// anything at all. Three tests passed on that, one of them driving the real
/// binary, while the feature did not work: on a machine outside an agent
/// session omt reported no agent for an agent that was plainly running.
///
/// A test that can pass because of the environment it runs in is not a test.
/// Stripped by prefix rather than by a list, because the next variable an
/// agent adds is not going to be in a list written today.
fn inherited_agent_environment() -> Vec<String> {
    const PREFIXES: [&str; 6] = [
        "CLAUDE",
        "ANTHROPIC",
        "AI_AGENT",
        "CODEX",
        "OPENCODE",
        "GEMINI",
    ];
    std::env::vars()
        .map(|(name, _)| name)
        .filter(|name| {
            let upper = name.to_uppercase();
            PREFIXES.iter().any(|p| upper.starts_with(p))
        })
        .collect()
}

/// A running `omt`, with a terminal emulator watching its output.
pub struct Screen {
    pub pty: Pty,
    pub term: Terminal,
    /// Where this screen's session was started, for a test that needs to put
    /// a file where the shell can reach it.
    pub dir: std::path::PathBuf,
    /// Every byte omt has written, uninterpreted.
    pub raw: Vec<u8>,
}

impl Screen {
    /// Start `omt` on a pty of a known size.
    pub fn start() -> Self {
        Self::start_with_shell_and_terminal("/bin/sh", None)
    }

    /// The same, claiming to be a terminal that can raise notifications.
    pub fn start_notifying() -> Self {
        Self::start_with_shell_and_terminal("/bin/sh", Some("ghostty"))
    }

    /// A terminal that watches an instance rather than owning its sessions.
    ///
    /// The runtime directory is passed in because it decides *which* instance:
    /// two screens given the same one attach to the same sessions — which is
    /// what "come back to what you left" means — and two given different ones
    /// cannot see each other at all, which is what keeps these tests from
    /// sharing a daemon with the rest of the suite.
    pub fn start_attached_in(runtime: &std::path::Path) -> Self {
        std::fs::create_dir_all(runtime).expect("runtime dir");
        Self::start_full("/bin/sh", None, Some(runtime))
    }

    /// The same, under a named shell.
    ///
    /// Which shell is not a detail: how a shell starts a child decides what
    /// the foreground process group is, and that is the only thing the probe
    /// reads. A harness that always uses `/bin/sh` tests `/bin/sh`.
    pub fn start_with_shell(shell: &str) -> Self {
        Self::start_with_shell_and_terminal(shell, None)
    }

    /// The same, with control over what omt thinks it is drawing on.
    pub fn start_with_shell_and_terminal(shell: &str, term_program: Option<&str>) -> Self {
        Self::start_full(shell, term_program, None)
    }

    /// The whole set of choices a screen can be started with.
    pub fn start_full(
        shell: &str,
        term_program: Option<&str>,
        runtime: Option<&std::path::Path>,
    ) -> Self {
        let runtime_var = runtime.map(|r| r.display().to_string()).unwrap_or_default();
        // A state directory per screen, so what one test leaves behind cannot
        // decide what the next one starts with. The agent panel remembers
        // whether it is on; without this, the test that turns it on turns it
        // on for every test after it, in whatever order they happen to run.
        //
        // The same class as the four environment defects this suite already
        // has a gate for — a file rather than a variable, which is why the
        // gate did not catch it.
        let state = std::env::temp_dir().join(format!(
            "omt-screen-state-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&state).expect("state dir");
        let binary = std::path::Path::new(env!("CARGO_BIN_EXE_omt"));
        let dir = std::env::temp_dir().join(format!("omt-screen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let pty = Pty::spawn(&PtyConfig {
            program: binary.into(),
            args: Vec::new(),
            cwd: Some(dir.clone()),
            size: PtySize::new(COLS, ROWS),
            env: vec![
                ("TERM".to_owned(), "xterm-256color".to_owned()),
                // A prompt of exactly one known line, so what the *shell*
                // paints cannot be mistaken for what omt paints.
                ("PS1".to_owned(), "$ ".to_owned()),
                ("SHELL".to_owned(), shell.to_owned()),
                ("XDG_STATE_HOME".to_owned(), state.display().to_string()),
                // Attached only, and *absent* rather than empty otherwise: an
                // empty variable is a value, and reading one as "yes" made
                // every local test in this file try to attach.
                ("XDG_RUNTIME_DIR".to_owned(), runtime_var.clone()),
                // The local path is the opt-out now, so a screen that owns its
                // own sessions has to say so — and it is stripped rather than
                // set empty for an attached one, because an empty variable is
                // still a value.
                (
                    "OMT_LOCAL".to_owned(),
                    if runtime.is_some() { "" } else { "1" }.to_owned(),
                ),
                // Where a client writes a call that never answered. A terminal
                // that has frozen and one whose instance stopped replying look
                // identical on screen, and the screen is the one thing that
                // cannot say which.
                (
                    "OMT_CLIENT_LOG".to_owned(),
                    runtime
                        .map(|r| r.join("client.log").display().to_string())
                        .unwrap_or_default(),
                ),
                // Both are empty when this screen owns its sessions, and both
                // are stripped below in that case — a variable set to nothing
                // is still set, which is the distinction that broke a dozen
                // tests at once.
                (
                    "TERM_PROGRAM".to_owned(),
                    term_program.unwrap_or_default().to_owned(),
                ),
                // No sound in a test: a suite that plays audio on somebody's
                // machine is a suite they stop running.
                ("OMT_NO_SOUND".to_owned(), "1".to_owned()),
            ],
            env_remove: {
                // The agent markers, and everything that redirects where omt
                // keeps state. A harness that starts a real omt and lets it
                // find the machine's socket, snapshot, rules and blobs is a
                // harness sharing state with whatever else is running — which
                // is how a suite starts failing in an order that depends on
                // the load.
                let mut stripped = inherited_agent_environment();
                stripped.extend(
                    [
                        "OMT_SOCK",
                        "OMT_STATE_FILE",
                        "OMT_AGENT_RULES",
                        "OMT_MEDIA_DIR",
                    ]
                    .map(str::to_owned),
                );
                if runtime.is_none() {
                    // This screen owns its sessions. Inheriting a runtime
                    // directory from whoever ran the suite would point it at
                    // an instance the test knows nothing about.
                    stripped.push("XDG_RUNTIME_DIR".to_owned());
                } else {
                    stripped.push("OMT_LOCAL".to_owned());
                }
                stripped
            },
        })
        .expect("spawn omt");
        pty.set_nonblocking(true).expect("nonblocking");
        Self {
            pty,
            dir,
            raw: Vec::new(),
            term: Terminal::new(TermConfig {
                size: GridSize::new(COLS, ROWS),
                scrollback: ScrollbackLimits::default(),
                ..TermConfig::default()
            }),
        }
    }

    /// Read whatever omt has painted for a while, interpreting as it arrives.
    pub fn settle(&mut self, seconds: f32) {
        let deadline = Instant::now() + Duration::from_secs_f32(seconds);
        let mut buf = [0u8; 65536];
        let mut reader = self.pty.reader();
        while Instant::now() < deadline {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.raw.extend_from_slice(&buf[..n]);
                    self.term.advance(&buf[..n]);
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    /// Read until the screen says something, or give up.
    ///
    /// Waiting for a *condition* rather than for a duration. A fixed sleep is
    /// a guess about how fast this machine is, and under a full test run — a
    /// dozen other suites competing for the same cores — the guess is wrong,
    /// which is a test that passes alone and fails in CI.
    pub fn settle_until(&mut self, seconds: f32, mut done: impl FnMut(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs_f32(seconds);
        while Instant::now() < deadline {
            self.settle(0.25);
            if done(self) {
                return;
            }
        }
    }

    /// Everything omt has written, before the emulator interpreted it.
    ///
    /// The screen cannot answer every question: a notification is a sequence
    /// the emulator consumes and never draws, so a test that only looked at
    /// the grid would pass whether or not it was ever sent.
    pub fn raw(&self) -> String {
        String::from_utf8_lossy(&self.raw).into_owned()
    }

    pub fn type_in(&mut self, text: &str) {
        self.pty
            .writer()
            .write_all(text.as_bytes())
            .expect("write to omt");
    }

    /// How many bytes omt writes over a window, with the emulator kept fed.
    ///
    /// The unit the idle test is about: what a terminal on the other end of a
    /// link actually receives.
    pub fn bytes_read_during(&mut self, seconds: f32) -> usize {
        let deadline = Instant::now() + Duration::from_secs_f32(seconds);
        let mut buf = [0u8; 65536];
        let mut reader = self.pty.reader();
        let mut total = 0usize;
        while Instant::now() < deadline {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    total += n;
                    self.term.advance(&buf[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
        total
    }

    /// One row of the screen, as text.
    pub fn row(&self, row: u16) -> String {
        self.term.grid().row(row).text().trim_end().to_owned()
    }

    /// The whole screen, as lines.
    pub fn rows(&self) -> Vec<String> {
        (0..ROWS).map(|r| self.row(r)).collect()
    }

    pub fn screen(&self) -> String {
        self.rows().join("\n")
    }

    /// Whether the terminal is showing a cursor at all.
    pub fn cursor_visible(&self) -> bool {
        self.term.grid().cursor.visible
    }

    pub fn cursor(&self) -> (u16, u16) {
        let c = self.term.grid().cursor;
        (c.row, c.col)
    }

    /// Which row the last non-blank content is on.
    pub fn last_written_row(&self) -> u16 {
        self.rows()
            .iter()
            .rposition(|line| !line.trim().is_empty())
            .map_or(0, |r| r as u16)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        // Killed rather than left: a detach test that failed before detaching
        // would otherwise leave a shell running for the rest of the suite.
        let _ = self.pty.signal(omt_pty::Signal::Kill);
    }
}
