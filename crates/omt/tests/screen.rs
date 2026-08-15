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
                stripped
            },
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
