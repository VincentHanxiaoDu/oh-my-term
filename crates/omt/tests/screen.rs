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
use std::time::{Duration, Instant};

use omt_pty::{Pty, PtyConfig, PtySize};
use omt_term::{GridSize, ScrollbackLimits, TermConfig, Terminal};

const COLS: u16 = 100;
const ROWS: u16 = 30;

/// A running `omt`, with a terminal emulator watching its output.
struct Screen {
    pty: Pty,
    term: Terminal,
    /// Where this screen's session was started, for a test that needs to put
    /// a file where the shell can reach it.
    dir: std::path::PathBuf,
}

impl Screen {
    /// Start `omt` on a pty of a known size.
    fn start() -> Self {
        Self::start_with_shell("/bin/sh")
    }

    /// The same, under a named shell.
    ///
    /// Which shell is not a detail: how a shell starts a child decides what
    /// the foreground process group is, and that is the only thing the probe
    /// reads. A harness that always uses `/bin/sh` tests `/bin/sh`.
    fn start_with_shell(shell: &str) -> Self {
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
            ],
            ..PtyConfig::default()
        })
        .expect("spawn omt");
        pty.set_nonblocking(true).expect("nonblocking");
        Self {
            pty,
            dir,
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
                    self.term.advance(&buf[..n]);
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    fn type_in(&mut self, text: &str) {
        self.pty
            .writer()
            .write_all(text.as_bytes())
            .expect("write to omt");
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
    // 16 §3.3: after 250 ms, not instantly, and every key rather than four of
    // nine.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("\x02");
    s.settle(1.0);
    let screen = s.screen();
    for label in ["detach", "sessions", "agents", "palette"] {
        assert!(screen.contains(label), "`{label}` missing:\n{screen}");
    }
    // Spend the leader so the shell is not left with one armed.
    s.type_in("\x1b");
}

#[test]
fn a_paste_lands_in_one_piece() {
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
    // Reported from real use: after detaching, the next prompt appears far
    // down the screen. omt prints its first-run summary on the primary screen
    // and then enters the alternate one; whatever it leaves behind on the way
    // out is what the user's shell inherits.
    let mut s = Screen::start();
    s.settle(2.0);
    s.type_in("\x02d");
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
    // The selected row of the overlay, not the echoed command line above it:
    // the shell printed the path that was typed, and it starts with a `/`.
    let row = listed
        .lines()
        .find(|l| l.trim_start().starts_with('>'))
        .unwrap_or_default()
        .trim_start()
        .trim_start_matches('>')
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
