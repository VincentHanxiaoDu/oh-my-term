//! What omt actually paints, read back through omt's own terminal.
//!
//! Every visual bug this project has shipped was invisible to its tests for
//! one reason: the tests asserted on *bytes*. A cursor placed a row too high, a
//! list painted over by the pane underneath, a shell prompt left halfway down
//! the screen on exit — all of those are correct byte streams. They are only
//! wrong once something interprets them.
//!
//! omt has that something. `omt-term` is a terminal emulator with two hundred
//! tests of its own, so the harness here is small: run the real binary on a
//! real pty, feed everything it writes into a `Terminal`, and ask the resulting
//! grid what is on screen and where the cursor ended up. That is the same
//! question a person looking at the screen is asking, and it is the one that
//! kept being answered by hand.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "in a test, expect() is the assertion"
)]

use std::io::{Read, Write};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

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
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Take the lock, ignoring poisoning.
///
/// A test that panicked already failed and reported why; refusing to run the
/// rest because of it turns one failure into sixteen and hides which was
/// first.
fn serially() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

use omt_pty::{Pty, PtyConfig, PtySize};
use omt_term::{GridSize, ScrollbackLimits, TermConfig, Terminal};

const COLS: u16 = 100;
const ROWS: u16 = 30;

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
struct Screen {
    pty: Pty,
    term: Terminal,
    /// Where this screen's session was started, for a test that needs to put
    /// a file where the shell can reach it.
    dir: std::path::PathBuf,
    /// Every byte omt has written, uninterpreted.
    raw: Vec<u8>,
}

impl Screen {
    /// Start `omt` on a pty of a known size.
    fn start() -> Self {
        Self::start_with_shell_and_terminal("/bin/sh", None)
    }

    /// The same, claiming to be a terminal that can raise notifications.
    fn start_notifying() -> Self {
        Self::start_with_shell_and_terminal("/bin/sh", Some("ghostty"))
    }

    /// A terminal that watches an instance rather than owning its sessions.
    ///
    /// The runtime directory is passed in because it decides *which* instance:
    /// two screens given the same one attach to the same sessions — which is
    /// what "come back to what you left" means — and two given different ones
    /// cannot see each other at all, which is what keeps these tests from
    /// sharing a daemon with the rest of the suite.
    fn start_attached_in(runtime: &std::path::Path) -> Self {
        std::fs::create_dir_all(runtime).expect("runtime dir");
        Self::start_full("/bin/sh", None, Some(runtime))
    }

    /// The same, under a named shell.
    ///
    /// Which shell is not a detail: how a shell starts a child decides what
    /// the foreground process group is, and that is the only thing the probe
    /// reads. A harness that always uses `/bin/sh` tests `/bin/sh`.
    fn start_with_shell(shell: &str) -> Self {
        Self::start_with_shell_and_terminal(shell, None)
    }

    /// The same, with control over what omt thinks it is drawing on.
    fn start_with_shell_and_terminal(shell: &str, term_program: Option<&str>) -> Self {
        Self::start_full(shell, term_program, None)
    }

    /// The whole set of choices a screen can be started with.
    fn start_full(
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
    fn settle(&mut self, seconds: f32) {
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
    fn settle_until(&mut self, seconds: f32, mut done: impl FnMut(&Self) -> bool) {
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
    fn raw(&self) -> String {
        String::from_utf8_lossy(&self.raw).into_owned()
    }

    fn type_in(&mut self, text: &str) {
        self.pty
            .writer()
            .write_all(text.as_bytes())
            .expect("write to omt");
    }

    /// How many bytes omt writes over a window, with the emulator kept fed.
    ///
    /// The unit the idle test is about: what a terminal on the other end of a
    /// link actually receives.
    fn bytes_read_during(&mut self, seconds: f32) -> usize {
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
    fn row(&self, row: u16) -> String {
        self.term.grid().row(row).text().trim_end().to_owned()
    }

    /// The whole screen, as lines.
    fn rows(&self) -> Vec<String> {
        (0..ROWS).map(|r| self.row(r)).collect()
    }

    fn screen(&self) -> String {
        self.rows().join("\n")
    }

    /// Whether the terminal is showing a cursor at all.
    fn cursor_visible(&self) -> bool {
        self.term.grid().cursor.visible
    }

    fn cursor(&self) -> (u16, u16) {
        let c = self.term.grid().cursor;
        (c.row, c.col)
    }

    /// Which row the last non-blank content is on.
    fn last_written_row(&self) -> u16 {
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

#[test]
fn the_cursor_sits_where_the_shell_is_typing() {
    let _serial = serially();
    // The bug: panes were drawn one row below their rect, to leave the tab bar
    // its row, and the cursor was placed without that offset — so it sat a row
    // above the line being typed on. Invisible to a byte-level test, obvious to
    // anybody looking at the screen.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("echo marker-one\r");
    s.settle(1.5);

    let (row, _) = s.cursor();
    let prompt_row = s
        .rows()
        .iter()
        .rposition(|line| line.contains('$'))
        .expect("a shell prompt somewhere on screen") as u16;
    assert_eq!(
        row,
        prompt_row,
        "the cursor is on row {row} and the prompt is on {prompt_row}:\n{}",
        s.screen()
    );
}

#[test]
fn the_cursor_is_never_left_on_omts_own_status_row() {
    let _serial = serially();
    // Where it ended up once every piece of chrome had written: the chrome is
    // drawn after the panes, each piece writes an absolute position, and the
    // last one to write was the status row.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("echo marker-two\r");
    s.settle(1.5);
    let (row, _) = s.cursor();
    assert_ne!(
        row,
        ROWS - 1,
        "the cursor was parked on the status row:\n{}",
        s.screen()
    );
}

#[test]
fn omt_takes_exactly_one_row_and_it_is_the_bottom_one() {
    let _serial = serially();
    // 19 §2.1: "One row. Always present. Never more than one row, in any
    // configuration." What shipped was two — a tab bar across the top and a
    // hint along the bottom — which on a 24-row terminal is 8% of the screen
    // given to chrome the document does not describe.
    //
    // The top row belongs to the program now. A shell's first prompt is on it,
    // which is the check: if omt were still drawing a bar there, the prompt
    // would be one row lower.
    let mut s = Screen::start();
    s.settle(2.5);

    let rows = s.rows();
    let bar = &rows[(ROWS - 1) as usize];
    assert!(
        !bar.trim().is_empty(),
        "no status bar on the bottom row:\n{}",
        s.screen()
    );
    // Cell E, which 19 §2.1 says survives every width and every configuration.
    assert!(
        bar.contains('?'),
        "the status bar has no escape hatch: {bar:?}"
    );

    // And the program starts at the top.
    assert!(
        !rows[0].trim().is_empty(),
        "row 0 is blank, so something is still reserving it:\n{}",
        s.screen()
    );
    assert!(
        !rows[0].contains('?') || rows[0] == *bar,
        "row 0 looks like chrome rather than the program's own output:\n{}",
        s.screen()
    );
}

#[test]
fn the_bar_says_which_agent_is_in_the_pane() {
    let _serial = serially();
    // Reported from real use: start omt, start Claude Code in it, and the bar
    // went on saying "shell". The probe found the agent — `⟨leader⟩ a` listed
    // it — but cell B was built from the thread roster, which is filled by
    // hooks and protocols and is empty for an agent on a plain pty. So the one
    // cell whose whole job is "what is running and does it need me" was the
    // last place in the product to find out.
    let mut s = Screen::start();
    s.settle(2.0);
    // A binary *named* `claude`, which is the exe-name half of the
    // fingerprint. Not `/bin/sleep` under an env marker: on macOS an
    // Apple-signed platform binary's environment cannot be read at all, so
    // that stand-in tests the kernel's mood rather than omt. A copy of omt's
    // own binary is not Apple-signed and does not exit.
    let fake = s.dir.join("claude");
    std::fs::copy(env!("CARGO_BIN_EXE_omt"), &fake).expect("copy");
    s.type_in(&format!(
        "{} serve --socket {}/probe.sock\r",
        fake.display(),
        s.dir.display()
    ));
    // Longer than the probe interval.
    s.settle(2.5);

    let bar = s.row(ROWS - 1);
    assert!(
        bar.contains("claude"),
        "the bar does not name the agent in the pane: {bar:?}\n{}",
        s.screen()
    );
    s.type_in("\x03");
}

#[test]
fn the_status_bar_names_the_sessions_rather_than_counting_them() {
    let _serial = serially();
    // Cell B is "the fastest possible answer to which agent needs me — one
    // glance, no navigation". A count is not that.
    let mut s = Screen::start();
    s.settle(2.5);
    let bar = s.row(ROWS - 1);
    assert!(
        bar.contains("1:"),
        "the bar has no session list: {bar:?}\n{}",
        s.screen()
    );
}

#[test]
fn an_open_list_is_still_on_screen_after_the_pane_repaints() {
    let _serial = serially();
    // The overlay redrew only when its text changed, while the panes redraw
    // every frame from their own damage — so the list was painted over within
    // milliseconds and what was left was half a list interleaved with a shell.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("\x02p"); // leader, palette
    s.settle(0.5);
    // Long enough for many frames of pane repainting to have happened.
    s.settle(1.5);
    assert!(
        s.screen().contains("Commands"),
        "the palette was painted over:\n{}",
        s.screen()
    );
}

#[test]
fn the_leader_strip_lists_the_keys_after_a_pause() {
    let _serial = serially();
    // 16 §3.3: after 250 ms, not instantly.
    //
    // What it lists is a priority order, not the whole table: there are more
    // actions than a row can hold — that was true at nine and is truer at
    // twelve — so the strip drops from the end and points at `⟨leader⟩ ?` for
    // the rest. What has to survive an ordinary terminal is what somebody
    // opened the strip to find: how to leave, how to arrange, and which agent
    // wants them.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("\x02");
    s.settle(1.0);
    let screen = s.screen();
    for label in ["detach", "split", "zoom", "waiting", "agents"] {
        assert!(screen.contains(label), "`{label}` missing:\n{screen}");
    }
    // And when something is dropped it is said, because a menu that quietly
    // shows less than it has is worse than one that admits it.
    let strip = screen
        .lines()
        .find(|line| line.contains('\u{25b8}'))
        .unwrap_or_default();
    assert!(
        !strip.contains('\u{2026}') || strip.contains('?'),
        "keys were dropped without pointing at the full list: {strip:?}"
    );
    // Spend the leader so the shell is not left with one armed.
    s.type_in("\x1b");
}

#[test]
fn a_paste_lands_in_one_piece() {
    let _serial = serially();
    // The loop read one input event per frame, and a frame cost two `git`
    // process spawns — so a pasted paragraph arrived a character at a time.
    let mut s = Screen::start();
    s.settle(2.0);
    let text: String = (0..40).map(|i| format!("w{i} ")).collect();
    s.type_in(&format!("echo \x1b[200~{text}\x1b[201~"));
    // Deliberately short: this is the assertion, not a convenience.
    s.settle(0.7);
    let screen = s.screen();
    assert!(screen.contains("w0"), "nothing arrived:\n{screen}");
    assert!(
        screen.contains("w39"),
        "the tail of the paste had not arrived:\n{screen}"
    );
    s.type_in("\x03");
}

#[test]
fn leaving_puts_the_shell_back_where_omt_started_it() {
    let _serial = serially();
    // Reported from real use: after detaching, the next prompt appears far
    // down the screen. omt prints its first-run summary on the primary screen
    // and then enters the alternate one; whatever it leaves behind on the way
    // out is what the user's shell inherits.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("\x02d");
    // The shell in the pane is a live session, so omt asks first now.
    s.settle_until(10.0, |s| s.screen().contains("ends when omt does"));
    s.type_in("\r");
    s.settle(1.5);

    let last = s.last_written_row();
    let (cursor_row, cursor_col) = s.cursor();
    assert!(
        cursor_row <= last + 1,
        "omt left the cursor on row {cursor_row} with content ending at row {last}:\n{}",
        s.screen()
    );
    assert_eq!(
        cursor_col,
        0,
        "omt left the cursor mid-row, so the next prompt is indented:\n{}",
        s.screen()
    );
}

#[test]
fn the_bar_names_the_real_claude_and_not_only_a_binary_called_claude() {
    let _serial = serially();
    // The test above copies omt's own binary to a file named `claude`, which
    // exercises the exe-name half of the fingerprint and nothing else. The
    // report was about the actual agent, and the actual agent differs in every
    // way that could matter: it is a node script behind a wrapper, so the
    // process in the foreground may not be the one whose name was typed, and
    // its argv and environment are its own.
    //
    // Skipped rather than failed when Claude Code is not installed. A test
    // that cannot run on a machine is not a licence to assert nothing on the
    // machines where it can.
    let Some(claude) = which("claude") else {
        eprintln!("skipped: no claude on PATH");
        return;
    };

    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in(&format!("{} --help\r", claude.display()));
    // `--help` runs, prints and exits, which is not what is wanted here: the
    // probe reads whatever is in the foreground *now*. So start it for real
    // and give it long enough to be the foreground process across at least
    // two probe intervals.
    s.settle(1.0);
    s.type_in(&format!("{}\r", claude.display()));
    s.settle(6.0);

    let bar = s.row(ROWS - 1);
    let named = bar.to_lowercase().contains("claude");
    // Interrupt and leave before asserting, so a failure does not leave a
    // real agent running on somebody's machine.
    s.type_in("\x03");
    s.settle(0.5);
    s.type_in("\x04");

    assert!(
        named,
        "the bar does not name the real claude in the pane: {bar:?}\n{}",
        s.screen()
    );
}

/// Where a program is on `PATH`, if it is anywhere.
fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

#[test]
fn the_agent_list_finds_the_real_claude_that_the_bar_already_found() {
    let _serial = serially();
    // Reported from real use, and in this exact order: start omt, start Claude
    // Code, press the leader and `a`. The list said there was no agent while
    // the bar in the same frame said `claude`. Two readers of one fact
    // disagreeing is worse than either being wrong, because whichever one
    // somebody happens to look at is the one they believe.
    let Some(claude) = which("claude") else {
        eprintln!("skipped: no claude on PATH");
        return;
    };

    // The user's shell, not the harness's `/bin/sh`. How a shell starts a
    // child decides what the foreground process group is, which is the one
    // thing the probe reads.
    let mut s = Screen::start_with_shell("/bin/zsh");
    s.settle(2.0);
    s.type_in(&format!("{}\r", claude.display()));
    // What a person does: start it and reach for the leader. Not eight
    // seconds of politely waiting for a probe interval nobody knows exists.
    s.settle(2.0);

    let bar = s.row(ROWS - 1);
    s.type_in("\x02a");
    s.settle(1.5);
    let listed = s.screen();

    // Leave before asserting, so a failure does not strand a real agent.
    s.type_in("\x1b");
    s.settle(0.3);
    s.type_in("\x03");
    s.settle(0.5);
    s.type_in("\x04");

    assert!(
        listed.to_lowercase().contains("claude"),
        "the agent list does not name the agent the bar found.\nbar: {bar:?}\n{listed}"
    );

    // The agent's name in the column the eye lands on, and never the word
    // "unknown" beside an agent omt has found. The row that shipped read
    // `shell   Claude Code · unknown · ...`, so a list opened to find out
    // whether omt knows about Claude Code answered with "shell" and "unknown".
    // The selected row of the panel, not the echoed command line above it:
    // the shell printed the path that was typed, and it starts with a `/`.
    // The row is inside the panel's borders and carries the selection mark.
    let row = listed
        .lines()
        .find(|l| l.contains('\u{276f}'))
        .unwrap_or_default()
        .split('\u{276f}')
        .nth(1)
        .unwrap_or_default()
        .trim()
        .trim_end_matches('\u{2502}')
        .trim()
        // The state mark leads the row, then the name. A mark is a shape
        // rather than a colour because a colour is the first thing a terminal
        // theme takes away.
        .trim_start_matches(['\u{25cf}', '\u{25cb}', '\u{b7}'])
        .trim()
        .to_owned();
    assert!(
        row.to_lowercase().starts_with("claude"),
        "the agent is not the first thing on its own row: {row:?}"
    );
    assert!(
        !row.split(' ').any(|word| word == "unknown"),
        "a found agent is described as unknown: {row:?}"
    );
}

#[test]
fn closing_a_list_brings_the_pane_back_without_waiting_for_it_to_repaint() {
    let _serial = serially();
    // Reported from real use: open `⟨leader⟩ a` over Claude Code, press Esc,
    // and the list stays on the screen for a long time — until the agent
    // happens to paint again, which for one sitting at a prompt is never.
    //
    // An overlay is written straight to the terminal, over cells each pane's
    // diffing renderer believes it still owns. The diff then sees no change
    // and writes nothing, so what is on the screen is a closed list.
    let mut s = Screen::start();
    s.settle(2.0);
    // Something distinctive, and a program that will not repaint on its own.
    s.type_in("printf 'PANE-MARKER-9137\\n'\r");
    s.settle(1.5);
    assert!(
        s.screen().contains("PANE-MARKER-9137"),
        "the marker never appeared:\n{}",
        s.screen()
    );

    s.type_in("\x02a");
    s.settle(1.0);
    assert!(
        s.screen().contains("Agents"),
        "the list did not open:\n{}",
        s.screen()
    );

    s.type_in("\x1b");
    // One second is far longer than a frame and far shorter than "ages". The
    // shell is sitting at a prompt and will not paint anything by itself.
    s.settle(1.0);
    let after = s.screen();
    assert!(
        !after.contains("Agents"),
        "the closed list is still on the screen:\n{after}"
    );
    assert!(
        after.contains("PANE-MARKER-9137"),
        "the pane did not come back after the list closed:\n{after}"
    );
    s.type_in("\x04");
}

#[test]
fn an_idle_omt_writes_nothing_at_all() {
    let _serial = serially();
    // The frame runs every `TICK`, and the cursor was placed unconditionally
    // in each one: 619 bytes a second into a terminal where nobody was typing
    // and nothing was running, measured before this was fixed.
    //
    // That is not free anywhere, and it is expensive in the two places this
    // product lives. Over ssh it is a steady trickle on a link whose whole
    // value is latency. Interleaved with an agent's own repaints it is what
    // makes a screen look like it is struggling to keep up — which is what it
    // was reported as.
    let mut s = Screen::start();
    s.settle(2.5);

    let quiet = s.bytes_read_during(2.0);
    assert_eq!(
        quiet, 0,
        "an idle omt wrote {quiet} bytes in two seconds, and nothing happened in them"
    );
    s.type_in("\x04");
}

#[test]
fn the_state_comes_from_what_the_agent_says_about_itself() {
    let _serial = serially();
    // The pane's program sets its own title, and Claude Code puts its state in
    // the first glyph of it: a braille spinner frame while it works, `✳` at
    // rest. That is the agent telling its host what it is doing, through a
    // channel that needs no permission and survives ssh — as against watching
    // bytes arrive, which cannot tell working from a program that happens to
    // be printing.
    //
    // Driven with `printf` rather than a real agent, because the point under
    // test is that omt reads the title at all, and a real agent's timing makes
    // that a coin toss.
    let mut s = Screen::start();
    s.settle(2.0);

    // Working: a spinner frame, then quiet. The floor would call this idle
    // after two seconds; the title says otherwise and outranks it.
    s.type_in("printf '\\e]0;\u{2801} Claude Code\\a'\r");
    s.settle(3.0);
    s.type_in("\x02a");
    s.settle(1.5);
    let working = s.screen();
    s.type_in("\x1b");
    s.settle(0.5);

    assert!(
        working.to_lowercase().contains("claude"),
        "the title did not identify the agent:\n{working}"
    );
    assert!(
        working.contains("working"),
        "the agent said it was working and omt did not:\n{working}"
    );

    // At rest: the same agent, a different glyph, a different state.
    s.type_in("printf '\\e]0;\u{2733} Claude Code\\a'\r");
    s.settle(2.0);
    s.type_in("\x02a");
    s.settle(1.5);
    let resting = s.screen();
    s.type_in("\x1b");
    s.settle(0.3);
    s.type_in("\x04");

    assert!(
        resting.contains("idle"),
        "the agent said it was idle and omt did not:\n{resting}"
    );
    // And never the glyph itself, which is a codepoint nobody reads.
    assert!(
        !resting.contains('\u{2733}') || !resting.contains("\u{2733} \u{2733}"),
        "the state glyph reached the screen:\n{resting}"
    );
}

#[test]
fn a_question_on_the_agents_own_screen_is_somebody_being_waited_for() {
    let _serial = serially();
    // The state no amount of watching a pty can see. An agent sitting at a
    // permission prompt produces no output and rings no bell: to everything
    // below this it is indistinguishable from an agent that finished. It is
    // distinguishable by looking at what it drew, in the place it draws
    // questions — below the last rule.
    //
    // Driven with `printf` rather than a real agent because the point under
    // test is that omt reads the screen at all, and a real agent's timing
    // makes that a coin toss.
    let mut s = Screen::start();
    s.settle(2.0);

    // Identify as the agent, then draw what the agent draws when it asks.
    s.type_in("printf '\\e]0;\u{2733} Claude Code\\a'\r");
    s.settle(1.0);
    s.type_in("printf '%s\\n' '────────────────' 'Do you want to proceed?' '❯ 1. Yes' '  2. No' 'Esc to cancel'\r");
    s.settle(3.0);

    s.type_in("\x02a");
    s.settle(1.5);
    let listed = s.screen();
    s.type_in("\x1b");
    s.settle(0.3);
    s.type_in("\x04");

    assert!(
        listed.contains("waiting on you"),
        "an agent asking a question is not reported as waiting:\n{listed}"
    );
}

#[test]
fn a_found_agent_is_never_described_as_unknown() {
    let _serial = serially();
    // The word describes omt's knowledge rather than the agent, and beside an
    // agent omt has just found it reads as "omt has no idea what this is" —
    // which is how it was read, twice, by the person it was shown to.
    //
    // Whether "done" or "idle" is the right word for a finished agent is
    // decided by `omt_tui::state_word`, which is a pure function of the state
    // and whether the pane has been seen, and is tested there. Driving that
    // timing through a shell would test the shell.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("printf '\\e]0;\u{2801} Claude Code\\a'\r");
    s.settle(2.0);
    s.type_in("\x02a");
    s.settle(1.2);

    let listed = s.screen();
    s.type_in("\x1b");
    s.settle(0.3);
    s.type_in("\x04");

    assert!(
        listed.to_lowercase().contains("claude"),
        "the agent was not found at all:\n{listed}"
    );
    assert!(
        !listed.contains("unknown"),
        "a found agent is described as unknown:\n{listed}"
    );
}

#[test]
fn a_split_puts_two_panes_on_the_screen_and_a_second_split_nests() {
    let _serial = serially();
    // What `tile` could not express: an arrangement that is not N equal
    // shares. Split once side by side, then split the right-hand pane
    // downwards, and there are three panes in two columns — which is a tree,
    // not a row.
    let mut s = Screen::start();
    s.settle(2.0);

    // Something identifiable in the first pane, so the panes can be told apart
    // on the screen rather than by counting separators.
    s.type_in("printf 'PANE-ONE\\n'\r");
    s.settle(1.0);

    s.type_in("\x02s");
    s.settle(2.0);
    let two = s.screen();
    assert!(
        two.contains('\u{2502}'),
        "no separator after a split — the screen is still one pane:\n{two}"
    );
    // The first pane's output is still there, in its own column.
    assert!(
        two.contains("PANE-ONE"),
        "the original pane was lost:\n{two}"
    );

    // And a second split, downwards, inside the pane the focus is in.
    s.type_in("\x02S");
    s.settle(2.0);
    let three = s.screen();
    s.type_in("\x02x");
    s.settle(1.0);
    s.type_in("\x02x");
    s.settle(1.0);
    s.type_in("\x04");

    assert!(
        three.contains("PANE-ONE"),
        "a nested split lost the first pane:\n{three}"
    );
}

#[test]
fn the_leader_strip_fits_the_terminal_it_is_drawn_in() {
    let _serial = serially();
    // It is a menu, and a menu that is built too wide is truncated — losing
    // its *last* entries, which are the ones added most recently. Adding
    // direction keys silently removed `sessions` and `agents` from the only
    // place they are advertised, and nothing failed.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("\x02");
    s.settle(1.0);

    let screen = s.screen();
    s.type_in("\x1b");
    s.settle(0.3);
    s.type_in("\x04");

    let strip = screen
        .lines()
        .find(|line| line.contains('\u{25b8}'))
        .unwrap_or_default();
    assert!(
        strip.chars().count() <= usize::from(COLS),
        "the strip is {} columns wide in a {COLS}-column terminal: {strip:?}",
        strip.chars().count()
    );
}

#[test]
fn a_list_is_a_bounded_panel_and_not_the_whole_screen() {
    let _serial = serially();
    // What it looked like before: full-width inverted rows jammed against the
    // program underneath, with a label at one edge and its detail at the
    // other. Nothing said where the list stopped and the pane began, and on a
    // wide terminal the two halves of a row could not be read as one row.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("\x02p");
    s.settle(1.5);

    let screen = s.screen();
    s.type_in("\x1b");
    s.settle(0.3);
    s.type_in("\x04");

    let top = screen
        .lines()
        .find(|line| line.contains('\u{250c}'))
        .unwrap_or_else(|| panic!("no panel border:\n{screen}"));
    let chars: Vec<char> = top.chars().collect();
    let left = chars
        .iter()
        .position(|c| *c == '\u{250c}')
        .expect("a corner");
    let right = chars
        .iter()
        .rposition(|c| *c == '\u{2510}')
        .expect("a corner");

    assert!(left > 0, "the panel starts at the screen edge:\n{screen}");
    assert!(
        right < usize::from(COLS) - 1,
        "the panel runs to the screen edge:\n{screen}"
    );
    assert!(
        screen.lines().any(|line| line.contains('\u{2514}')),
        "the panel has no bottom:\n{screen}"
    );
}

#[test]
fn the_cursor_leaves_the_pane_while_a_list_is_over_it() {
    let _serial = serially();
    // A list takes every key, so a cursor blinking in the program behind it
    // says the typing goes there — and it does not. Somebody typing to filter
    // watches a cursor sit still somewhere else, which is the single clearest
    // way to look broken.
    let mut s = Screen::start();
    s.settle(2.0);
    assert!(
        s.cursor_visible(),
        "the cursor is not in the pane to begin with"
    );

    s.type_in("\x02p");
    s.settle(1.5);
    let over = s.cursor_visible();

    s.type_in("\x1b");
    s.settle(1.0);
    let back = s.cursor_visible();
    s.type_in("\x04");

    assert!(!over, "the cursor stayed in the pane under an open list");
    assert!(back, "the cursor did not come back when the list closed");
}

#[test]
fn a_screen_that_changes_on_a_timer_still_repaints() {
    let _serial = serially();
    // The frame is only drawn when something happened, which is what keeps an
    // idle omt off the processor. The danger in that is everything that
    // changes *with time* rather than with input: it has no event to hang a
    // repaint on, and a gate that only knows about events skips the frame that
    // would show it.
    //
    // Two of those shipped broken within minutes of the gate being added — the
    // which-key strip, which appears 250 ms after the leader, and the status
    // bar naming an agent found by a probe on its own timer. This is the guard
    // for the next one.
    let mut s = Screen::start();
    s.settle(2.0);

    // Nothing is typed after the leader. The strip has to appear anyway.
    s.type_in("\x02");
    s.settle(1.5);
    let strip = s.screen();
    s.type_in("\x1b");
    s.settle(0.5);

    assert!(
        strip.contains("detach"),
        "the strip never appeared, so a timed repaint was skipped:\n{strip}"
    );

    // And a program that identifies itself is named without anything else
    // happening: the title is parsed by the pump, the probe reads it on its
    // own interval, and no keystroke is involved.
    s.type_in("printf '\\e]0;\u{2733} Claude Code\\a'\r");
    s.settle(3.0);
    let bar = s.row(ROWS - 1);
    s.type_in("\x04");

    assert!(
        bar.to_lowercase().contains("claude"),
        "the bar did not repaint when the probe found an agent: {bar:?}"
    );
}

#[test]
fn the_agent_panel_appears_where_it_fits_and_refuses_where_it_does_not() {
    let _serial = serially();
    // 19 §2.1b: a panel is chrome that takes columns, and it faces the same
    // test every status cell does — it must justify the space. Twenty-four
    // columns of a hundred is worth a list of every agent; of sixty it is not,
    // and the panes have the screen.
    //
    // The check is against the same minimum a split is refused for: a panel
    // may never do what splitting is not allowed to do.
    let mut wide = Screen::start();
    wide.settle(2.0);
    wide.type_in("\x02g");
    wide.settle(1.5);
    let with_panel = wide.screen();
    wide.type_in("\x02g");
    wide.settle(1.0);
    let without = wide.screen();
    wide.type_in("\x04");

    assert!(
        with_panel.contains("agents"),
        "the panel did not appear on a {COLS}-column terminal:\n{with_panel}"
    );
    assert!(
        !without.contains("agents"),
        "the panel did not go away when it was turned off:\n{without}"
    );
}

#[test]
fn zoom_fills_the_screen_and_gives_the_arrangement_back() {
    let _serial = serially();
    // The most-used key in every multiplexer, because splitting is for
    // comparing and zooming is for working, and both happen in the same
    // minute. omt had no answer at all until the inventory was counted.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("printf 'LEFT-PANE\\n'\r");
    s.settle(1.0);
    s.type_in("\x02s");
    s.settle(2.0);

    let split = s.screen();
    assert!(
        split.contains('\u{2502}'),
        "there is no split to zoom out of:\n{split}"
    );

    s.type_in("\x02z");
    s.settle(1.5);
    let zoomed = s.screen();

    s.type_in("\x02z");
    s.settle(1.5);
    let back = s.screen();
    s.type_in("\x02x");
    s.settle(1.0);
    s.type_in("\x04");

    assert!(
        !zoomed.contains('\u{2502}'),
        "zooming left the separator on screen:\n{zoomed}"
    );
    assert!(
        back.contains('\u{2502}'),
        "the arrangement did not come back:\n{back}"
    );
    assert!(
        back.contains("LEFT-PANE"),
        "the first pane's output was lost:\n{back}"
    );
}

#[test]
fn a_pane_can_be_reached_by_the_number_the_bar_gives_it() {
    let _serial = serially();
    // The status bar writes `1:zsh 2:claude`, and everybody's fingers reach
    // for those numbers before they read any documentation. A key that
    // disagreed with what is written on the screen would be worse than none.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("printf 'PANE-A\\n'\r");
    s.settle(1.0);
    s.type_in("\x02s");
    s.settle(2.0);
    s.type_in("printf 'PANE-B\\n'\r");
    s.settle(1.5);

    // Back to the first by its number, and type something only it can show.
    s.type_in("\x021");
    s.settle(1.0);
    s.type_in("printf 'BACK-IN-A\\n'\r");
    s.settle(1.5);

    let screen = s.screen();
    s.type_in("\x02x");
    s.settle(0.8);
    s.type_in("\x04");

    assert!(
        screen.contains("BACK-IN-A"),
        "typing after `⟨leader⟩ 1` did not land in the first pane:\n{screen}"
    );
    assert!(
        screen.contains("PANE-B"),
        "the second pane's output was lost:\n{screen}"
    );
}

#[test]
fn a_pane_can_be_moved_rather_than_the_focus() {
    let _serial = serially();
    // The pane moves and the focus follows it. Focus staying put means the
    // pane just moved is no longer the one being typed in, which is what every
    // implementation that does it that way gets reports about.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("printf 'ORIGINAL-LEFT\\n'\r");
    s.settle(1.0);
    s.type_in("\x02s");
    s.settle(2.0);

    // The focused pane is the right one. Move it left.
    s.type_in("\x02H");
    s.settle(1.5);
    s.type_in("printf 'MOVED-HERE\\n'\r");
    s.settle(1.5);

    let screen = s.screen();
    s.type_in("\x02x");
    s.settle(0.8);
    s.type_in("\x04");

    // Both panes are still there, and typing still reaches the pane that moved.
    assert!(
        screen.contains("ORIGINAL-LEFT"),
        "a pane was lost in the swap:\n{screen}"
    );
    assert!(
        screen.contains("MOVED-HERE"),
        "focus did not follow the pane it moved:\n{screen}"
    );
}

#[test]
fn the_conversation_view_opens_and_says_when_there_is_nothing_in_it() {
    let _serial = serially();
    // Until this key existed, the transcript an agent produces was readable
    // from a phone and not from the keyboard the agent was started at. A shell
    // has no conversation, and the honest answer is a sentence — an empty
    // overlay reads as a conversation omt lost.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("\x02t"); // leader, transcript
    s.settle(1.0);
    let screen = s.screen();
    assert!(
        screen.contains("Conversation"),
        "the conversation view did not open:\n{screen}"
    );
    assert!(
        screen.contains("nothing said yet"),
        "an empty transcript was shown as an empty list:\n{screen}"
    );
}

#[test]
fn a_hook_fired_from_inside_a_pane_reaches_the_conversation() {
    let _serial = serially();
    // The whole path, on a real terminal: omt spawns a pane, an agent in that
    // pane runs its hook, and what the agent said appears in the transcript
    // this terminal can show. Every piece of this passed its own test while
    // the path itself reached nothing, because nothing bound a socket for the
    // pane to report to.
    let mut s = Screen::start();
    s.settle(2.0);

    // target/debug/deps/<test> → target/debug/omt-hook
    let hook = std::env::current_exe()
        .expect("test exe")
        .parent()
        .and_then(std::path::Path::parent)
        .expect("target dir")
        .join("omt-hook");
    assert!(hook.exists(), "{} was not built", hook.display());

    // Run from *inside* the pane, so `$OMT_SOCK` and `$OMT_SESSION` are
    // whatever omt actually put there — which is the thing under test. A hook
    // with no socket exits at its guard clause and looks exactly like a hook
    // that was never installed.
    let payload = "{\\\"tool_name\\\":\\\"AskUserQuestion\\\",\\\"tool_input\\\":{\\\"questions\\\":[{\\\"question\\\":\\\"Which database?\\\",\\\"header\\\":\\\"Database\\\",\\\"multiSelect\\\":false,\\\"options\\\":[{\\\"label\\\":\\\"Postgres\\\"},{\\\"label\\\":\\\"SQLite\\\"}]}]}}";
    s.type_in(&format!(
        "printf '{payload}' | OMT_HOOK_EVENT=PreToolUse {} --agent claude-code >/dev/null\n",
        hook.display()
    ));
    // Until the hook has been through the socket and back, however long this
    // machine takes about it.
    s.settle(2.0);

    s.type_in("\x02t"); // leader, transcript
    s.settle_until(20.0, |s| s.screen().contains("Which database?"));
    let screen = s.screen();
    assert!(
        screen.contains("Which database?"),
        "the hook ran in the pane and the question never arrived:\n{screen}"
    );
}

#[test]
fn an_agent_that_starts_waiting_reaches_somebody_who_is_looking_elsewhere() {
    let _serial = serially();
    // The premise of the whole product is that agents work while you do
    // something else — and every surface omt draws answers "who needs me"
    // only to somebody already looking at the screen. This is the one channel
    // that reaches a person in a browser, in another window, on another
    // machine over ssh. It is a sequence the emulator consumes and never
    // draws, so the grid cannot answer whether it was sent.
    let mut s = Screen::start_notifying();
    s.settle(2.0);

    let hook = std::env::current_exe()
        .expect("test exe")
        .parent()
        .and_then(std::path::Path::parent)
        .expect("target dir")
        .join("omt-hook");
    assert!(hook.exists(), "{} was not built", hook.display());
    let payload = "{\\\"tool_name\\\":\\\"AskUserQuestion\\\",\\\"tool_input\\\":{\\\"questions\\\":[{\\\"question\\\":\\\"Which database?\\\",\\\"header\\\":\\\"Database\\\",\\\"multiSelect\\\":false,\\\"options\\\":[{\\\"label\\\":\\\"Postgres\\\"},{\\\"label\\\":\\\"SQLite\\\"}]}]}}";
    s.type_in(&format!(
        "printf '{payload}' | OMT_HOOK_EVENT=PreToolUse {} --agent claude-code >/dev/null\n",
        hook.display()
    ));
    s.settle_until(20.0, |s| s.raw().contains("waiting for you"));

    let raw = s.raw();
    assert!(
        raw.contains("\u{1b}]9;") && raw.contains("waiting for you"),
        "no notification was raised for an agent that started waiting"
    );
}

#[test]
fn copy_mode_shows_what_has_already_scrolled_off_the_screen() {
    let _serial = serially();
    // The keymap has had these keys since the keymap existed and the help
    // advertises them; nothing was underneath. A multiplexer that cannot
    // scroll back is one people keep a second terminal beside.
    let mut s = Screen::start();
    s.settle(2.0);
    // More lines than the screen holds, so the early ones are only reachable
    // through the history.
    s.type_in("seq -f marker-%g 1 60\n");
    s.settle(2.0);
    assert!(
        !s.screen().contains("marker-1\n") && !s.screen().contains("marker-2 "),
        "the test did not actually scroll anything away"
    );

    s.type_in("\x02"); // leader
    s.settle(0.3);
    s.type_in("["); // copy mode
    s.settle(0.5);
    s.type_in("g"); // to the very top
    s.settle_until(20.0, |s| s.screen().contains("marker-1"));
    let screen = s.screen();
    assert!(
        screen.contains("marker-1"),
        "the history was not reachable:\n{screen}"
    );
    assert!(
        screen.contains("Esc leave"),
        "a mode with no visible way out:\n{screen}"
    );

    // And it gives the pane back.
    s.type_in("\x1b");
    s.settle(1.0);
    assert!(
        !s.screen().contains("Esc leave"),
        "copy mode did not end:\n{}",
        s.screen()
    );
}

#[test]
fn a_search_in_copy_mode_jumps_to_the_line_that_has_it() {
    let _serial = serially();
    // The `/` this document has specified for as long as it has specified copy
    // mode. Scrolling by hand through a thousand lines of an agent's output to
    // find the error is the thing a scrollback is for.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("seq -f marker-%g 1 60\n");
    s.settle(2.0);
    s.type_in("\x02["); // leader, copy mode
    s.settle(0.5);
    s.type_in("/marker-3\n");
    s.settle_until(20.0, |s| s.screen().contains("marker-3"));
    assert!(
        s.screen().contains("marker-3"),
        "the search did not reach it:\n{}",
        s.screen()
    );
}

#[test]
fn a_program_in_a_pane_can_reach_somebody_who_left_the_window() {
    let _serial = serially();
    // The long build that finishes while its window is behind a browser. omt
    // is the terminal it is talking to; a multiplexer that swallowed these is
    // why people run their builds outside one.
    let mut s = Screen::start_notifying();
    s.settle(2.0);
    // `\0ooo` octal, which is what POSIX printf takes — `\033` is printed
    // literally by the shell this harness runs.
    s.type_in("printf '\\033]9;omt-notify-probe\\007' | cat\n");
    s.settle_until(15.0, |s| s.raw().contains("\u{1b}]9;omt-notify-probe"));
    assert!(
        s.raw().contains("\u{1b}]9;omt-notify-probe"),
        "the notification never left omt"
    );
}

#[test]
fn quitting_says_what_it_is_about_to_end() {
    let _serial = serially();
    // The key is bound where every multiplexer binds "detach", and it was
    // labelled that way — while ending every pty this process holds. An agent
    // mid-turn is work somebody is waiting on, and a word that promised
    // otherwise cost them that work with no warning. Until a session can
    // outlive this process (05 §4.1b) the honest behaviour is to say so and
    // ask.
    let mut s = Screen::start();
    s.settle(2.0);
    // The probe writes its own pid where the test can read it. Matching on a
    // command line does not work: a shell running one command execs it and
    // replaces its own arguments, so any marker put there is gone by the time
    // anything looks.
    let pidfile = s.dir.join("detach-probe.pid");
    // In the *foreground* of the pane, which is where an agent runs. A
    // backgrounded job is a different question: nobody signals it when the pty
    // goes, so it would survive either way and prove nothing.
    s.type_in(&format!(
        "sh -c 'echo $$ > {}; exec sleep 300'\n",
        pidfile.display()
    ));
    s.settle_until(15.0, |_| pidfile.exists());
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .expect("the probe wrote no pid")
        .trim()
        .parse()
        .expect("a pid");

    let alive = |pid: i32| {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    assert!(alive(pid), "the probe never started");

    s.type_in("\x02d");
    s.settle_until(10.0, |s| s.screen().contains("ends when omt does"));
    // The first press asks, because something is running.
    assert!(
        s.screen().contains("ends when omt does"),
        "quitting took a running session with no warning:\n{}",
        s.screen()
    );
    // Enter is the answer, and it is answered *in the list*: an open overlay
    // takes every key, so a confirmation that let its answer through to the
    // keymap could never be answered at all.
    s.type_in("\r");
    s.settle(3.0);

    let survived = alive(pid);
    // Tidy up whatever is left, either way.
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
    // And then it does what it said: this process ends, and every pty it
    // holds ends with it. Written down as the behaviour rather than left as a
    // surprise — 05 §4.1b says what it would take for this to be otherwise.
    assert!(
        !survived,
        "the session outlived omt, which would mean the warning is now wrong"
    );
}

#[test]
fn attached_to_an_instance_a_pane_shows_what_that_session_shows() {
    let _serial = serially();
    // The terminal watching sessions it does not own. Everything above the
    // seam is the same code the local path runs — the renderer, the layout,
    // the status bar — so what this proves is the seam: bytes from a socket
    // reach a pane and are drawn like any other.
    let runtime = instance_dir("attached-one");
    let _cleanup = Instance(runtime.clone());
    let mut s = Screen::start_attached_in(&runtime);
    s.settle(3.0);
    s.type_in("printf 'from-the-instance'\n");
    s.settle_until(20.0, |s| s.screen().contains("from-the-instance"));
    assert!(
        s.screen().contains("from-the-instance"),
        "an attached pane never showed its session:\n{}",
        s.screen()
    );
    drop(s);
}

#[test]
fn what_was_left_running_is_what_comes_back() {
    let _serial = serially();
    // The promise. A terminal quits with work in it; the next one in the same
    // directory finds that work rather than a fresh shell.
    let runtime = instance_dir("attached-return");
    let _cleanup = Instance(runtime.clone());
    let mut first = Screen::start_attached_in(&runtime);
    first.settle(3.0);
    let marker = format!("survives-{}", std::process::id());
    first.type_in(&format!("printf '{marker}'\n"));
    first.settle_until(20.0, |s| s.screen().contains(&marker));
    assert!(
        first.screen().contains(&marker),
        "the marker never appeared"
    );

    // Gone: not detached politely, *gone* — which is the case somebody's
    // laptop lid closing produces.
    drop(first);

    // What the instance itself says, before a second terminal is blamed for
    // not showing it. This assertion is why the defect was found in one run:
    // the daemon had been dying with the terminal that started it, because a
    // child left in the terminal's foreground process group receives the
    // hangup the kernel sends when that terminal closes.
    // The one that answers, not the first one on disk. A terminal binds a
    // socket of its own for its agents' hooks, so the directory holds more
    // than the instance's — and picking by name found the dead terminal's.
    let mut answering = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && answering.is_none() {
        // omt keeps its sockets in a subdirectory of the runtime one, because
        // it narrows the permissions of what it writes into and the runtime
        // directory is not omt's to narrow.
        let sockets = runtime.join("omt");
        for entry in std::fs::read_dir(&sockets).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "sock")
                && let Ok(client) = omt::client::Client::connect(&path)
            {
                answering = Some(client);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut probe = answering.expect("no instance in the runtime directory answered");
    let listed = probe
        .call("session.list", serde_json::json!({}), false)
        .expect("session.list");
    let count = listed["sessions"].as_array().map_or(0, Vec::len);
    assert!(count > 0, "the instance kept no sessions: {listed}");

    let mut second = Screen::start_attached_in(&runtime);
    second.settle_until(20.0, |s| s.screen().contains(&marker));
    assert!(
        second.screen().contains(&marker),
        "coming back showed none of what was left running:\n{}",
        second.screen()
    );
    drop(second);
}

/// A runtime directory of this test's own, plus the promise to clean up.
///
/// A guard rather than a call at the end: a test that fails skips whatever
/// came after the assertion, and the daemons those runs left behind were still
/// running hours later, competing for the machine the next suite needed.
struct Instance(std::path::PathBuf);

impl Drop for Instance {
    fn drop(&mut self) {
        end_instance(&self.0);
    }
}

/// A runtime directory of this test's own, so its instance is nobody else's.
fn instance_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("omt-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("runtime dir");
    dir
}

/// Stop the instance a test started, and take its sessions with it.
///
/// Left running, it would outlive the suite — which is the whole point of the
/// thing being tested, and a leak in a test. The daemon is found by the socket
/// path it was started with, because that path is unique to this test.
fn end_instance(runtime: &std::path::Path) {
    let _ = std::process::Command::new("pkill")
        .arg("-f")
        .arg(runtime.display().to_string())
        .status();
    let _ = std::fs::remove_dir_all(runtime);
}

#[test]
fn a_split_in_an_attached_terminal_makes_a_session_that_outlives_it_too() {
    let _serial = serially();
    // A split is another place to work, and in a terminal whose whole promise
    // is that work survives it, a pane spawned in *this* process would be the
    // one place that quietly did not.
    let runtime = instance_dir("attached-split");
    let _cleanup = Instance(runtime.clone());
    let mut s = Screen::start_attached_in(&runtime);
    s.settle(3.0);
    s.type_in("\x02s"); // leader, split
    s.settle(3.0);
    s.type_in("printf 'in-the-split'\n");
    s.settle_until(20.0, |s| s.screen().contains("in-the-split"));
    assert!(
        s.screen().contains("in-the-split"),
        "the split never showed its session:\n{}",
        s.screen()
    );

    // Two sessions in the instance, not one — and both of them its own.
    let mut answering = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && answering.is_none() {
        // omt keeps its sockets in a subdirectory of the runtime one, because
        // it narrows the permissions of what it writes into and the runtime
        // directory is not omt's to narrow.
        let sockets = runtime.join("omt");
        for entry in std::fs::read_dir(&sockets).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "sock")
                && let Ok(client) = omt::client::Client::connect(&path)
            {
                answering = Some(client);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut probe = answering.expect("no instance answered");
    let listed = probe
        .call("session.list", serde_json::json!({}), false)
        .expect("session.list");
    let count = listed["sessions"].as_array().map_or(0, Vec::len);
    assert_eq!(count, 2, "the split did not reach the instance: {listed}");

    drop(s);
}

#[test]
fn attached_quitting_asks_nothing_because_it_ends_nothing() {
    let _serial = serially();
    // The confirmation exists because quitting used to take an agent mid-turn
    // with it. Attached, it does not — and a question about a consequence that
    // no longer happens is one people learn to dismiss without reading.
    let runtime = instance_dir("attached-quit");
    let _cleanup = Instance(runtime.clone());
    let mut s = Screen::start_attached_in(&runtime);
    s.settle(3.0);
    s.type_in("sleep 30 &\n");
    s.settle(1.5);
    s.type_in("\x02d");
    s.settle(2.0);
    assert!(
        !s.screen().contains("ends when omt does"),
        "it asked about ending sessions it does not end:\n{}",
        s.screen()
    );
}

#[test]
fn attached_copy_mode_reaches_history_the_terminal_never_saw() {
    let _serial = serially();
    // The mirror holds only what has streamed since this terminal attached,
    // and its rows are numbered from *there*. History has to come from the
    // instance, whose numbering starts at the session's first line — blending
    // the two would put a reader on a different line than the one they see.
    let runtime = instance_dir("attached-copy");
    let _cleanup = Instance(runtime.clone());
    let mut first = Screen::start_attached_in(&runtime);
    first.settle(3.0);
    first.type_in("seq -f older-%g 1 60\n");
    first.settle_until(20.0, |s| s.screen().contains("older-60"));
    drop(first);

    // A terminal that was not there when any of that was printed.
    let mut second = Screen::start_attached_in(&runtime);
    second.settle_until(20.0, |s| s.screen().contains("older-"));
    assert!(
        second.screen().contains("older-"),
        "the second terminal saw nothing of the session at all:\n{}",
        second.screen()
    );
    second.type_in("\x02["); // leader, copy mode
    second.settle(0.5);
    second.settle(1.5);
    assert!(
        second.screen().contains("copy:") || second.screen().contains("search:"),
        "copy mode never drew at all:\n{}",
        second.screen()
    );
    second.type_in("g"); // to the very top
    second.settle_until(20.0, |s| s.screen().contains("older-1"));
    assert!(
        second.screen().contains("older-1"),
        "history the terminal never saw was unreachable:\n{}",
        second.screen()
    );
}

#[test]
fn the_agents_own_commands_are_listed_and_typed_into_its_own_interface() {
    let _serial = serially();
    // omt's palette lists what *omt* can do. This lists what the agent in this
    // pane says it can do — a different question with a different answer per
    // pane — and it types the choice into the agent's own interface, because
    // omt has no composer and the agent's menu is the one that knows what the
    // arguments mean.
    let runtime = instance_dir("agent-commands");
    let _cleanup = Instance(runtime.clone());
    let mut s = Screen::start_attached_in(&runtime);
    s.settle(3.0);

    // Nothing has told omt what this shell offers, and the list says so
    // rather than coming up empty.
    s.type_in("\x02/");
    s.settle_until(15.0, |s| s.screen().contains("has not said what it offers"));
    assert!(
        s.screen().contains("has not said what it offers"),
        "an empty list came up blank instead of saying why:\n{}",
        s.screen()
    );
    s.type_in("\x1b");
    s.settle(1.0);
}
