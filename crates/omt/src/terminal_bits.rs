//! The pieces of a running terminal that are not the frame loop.
//!
//! Split out because `run.rs` was carrying the loop, the observation pass, the
//! layout, the row builders *and* these — over the length the repository sets
//! for a file, which exists so that a reader can hold one of those at a time.
//!
//! What is here has one thing in common: it is asked a question and answers
//! it, with no timing and no borrow of the instance. Settings, the colours to
//! report to a program that asks, the noise to make when an agent needs
//! somebody, and what one key means while reading history.

use std::io::{self, Write};

use crate::rows::agent_name;
use crate::run::LocalPane;

/// One configured value, as text.
///
/// The default when it is unset or unreadable, because a terminal that refused
/// to start over a settings file is worse than one with default settings — and
/// the schema's default is a real answer, not a guess.
pub(crate) fn setting(state: &crate::state::State, key: &str) -> String {
    state.config().ok().map_or_else(String::new, |resolved| {
        resolved
            .get(key)
            .map(ToString::to_string)
            .unwrap_or_default()
    })
}

/// What colour the terminal omt is drawing on actually is.
///
/// Configuration first, then `$COLORFGBG`, which is what terminals that
/// announce this at all set. Not by querying the terminal: the reply would
/// arrive in the same stream as everything somebody types, and a query whose
/// answer was mistaken for keystrokes would put escape text into their shell —
/// which is a worse failure than not knowing.
pub(crate) fn host_colours() -> (Option<String>, Option<String>) {
    let configured = |key: &str| {
        std::env::var(key)
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    // The environment omt was started with, which a wrapper script can set and
    // which survives ssh, where a query cannot.
    let foreground = configured("OMT_TERM_FOREGROUND");
    let background = configured("OMT_TERM_BACKGROUND");
    if foreground.is_some() || background.is_some() {
        return (foreground, background);
    }
    let Some(pair) = std::env::var("COLORFGBG").ok() else {
        return (None, None);
    };
    // `fg;bg` as palette indexes. Only the two ends are answered: an index in
    // between names a colour from a palette omt has not been told, and a
    // guessed midtone is the answer that renders unreadably.
    let mut parts = pair.split(';');
    let fg = parts.next().and_then(ansi_end);
    let bg = parts.next_back().and_then(ansi_end);
    (fg, bg)
}

/// A palette index, where it is one of the two omt can name honestly.
pub(crate) fn ansi_end(index: &str) -> Option<String> {
    match index.trim() {
        "0" => Some("rgb:0000/0000/0000".to_owned()),
        "7" | "15" => Some("rgb:ffff/ffff/ffff".to_owned()),
        _ => None,
    }
}

/// A configured sound file, where somebody named one.
///
/// An empty setting is not a path: treated as one it becomes a player invoked
/// on the current directory, which fails silently and looks exactly like a
/// machine with no audio.
pub(crate) fn sound_file(state: &crate::state::State, key: &str) -> Option<std::path::PathBuf> {
    let named = setting(state, key);
    let trimmed = named.trim().trim_matches('"');
    (!trimmed.is_empty()).then(|| std::path::PathBuf::from(trimmed))
}

/// Say that something happened, on whatever channels this terminal has.
///
/// Both are best-effort by design: a terminal that cannot raise a notification
/// and a machine with no audio are ordinary, and neither is worth a message on
/// somebody's screen about a message that could not be shown.
pub(crate) fn announce(
    moment: omt_notify::Moment,
    pane: &LocalPane,
    env: &omt_notify::Environment,
    sounds: &omt_notify::sound::Sounds,
) {
    let name = pane
        .agent
        .as_ref()
        .map_or_else(|| "an agent".to_owned(), |p| agent_name(p.detection.agent));
    let (title, sound) = match moment {
        omt_notify::Moment::NeedsYou => (
            format!("{name} is waiting for you"),
            omt_notify::Sound::NeedsYou,
        ),
        omt_notify::Moment::Finished => (format!("{name} finished"), omt_notify::Sound::Finished),
    };
    // To the terminal omt is drawing on, which is the one somebody's window
    // manager knows about. Written directly rather than through the frame
    // buffer: it is not part of the frame, and holding it until the next one
    // would delay by exactly as long as nothing else is happening.
    let _ = omt_notify::raise(&mut io::stdout(), env, &title, None);
    omt_notify::play(sound, sounds);
}

/// What one key means while reading history.
pub(crate) enum CopyKey {
    /// Move.
    Go(omt_tui::CopyMove),
    /// Start or clear a selection.
    Mark,
    /// Take a copy and leave.
    Yank,
    /// Open the search field.
    Search,
    /// Repeat the last search, forwards or backwards.
    Repeat(bool),
    /// Leave without copying.
    Leave,
    /// Nothing. An unbound key does nothing rather than reaching the shell:
    /// a keystroke that fell through to a program nobody is looking at is how
    /// a reader ends up having typed into their own session.
    Ignore,
}

/// The keys, which are the ones people already have in their fingers.
pub(crate) fn copy_key(key: &crossterm::event::KeyEvent) -> CopyKey {
    use crossterm::event::KeyCode;
    use omt_tui::CopyMove as Go;
    match key.code {
        KeyCode::Char('h') | KeyCode::Left => CopyKey::Go(Go::Left),
        KeyCode::Char('j') | KeyCode::Down => CopyKey::Go(Go::Down),
        KeyCode::Char('k') | KeyCode::Up => CopyKey::Go(Go::Up),
        KeyCode::Char('l') | KeyCode::Right => CopyKey::Go(Go::Right),
        KeyCode::Char('0') | KeyCode::Home => CopyKey::Go(Go::LineStart),
        KeyCode::Char('$') | KeyCode::End => CopyKey::Go(Go::LineEnd),
        KeyCode::Char('g') => CopyKey::Go(Go::Top),
        KeyCode::Char('G') => CopyKey::Go(Go::Bottom),
        KeyCode::PageUp | KeyCode::Char('b') => CopyKey::Go(Go::PageUp),
        KeyCode::PageDown | KeyCode::Char('f') => CopyKey::Go(Go::PageDown),
        KeyCode::Char('v') | KeyCode::Char(' ') => CopyKey::Mark,
        KeyCode::Char('/') => CopyKey::Search,
        KeyCode::Char('n') => CopyKey::Repeat(true),
        KeyCode::Char('N') => CopyKey::Repeat(false),
        KeyCode::Char('y') | KeyCode::Enter => CopyKey::Yank,
        KeyCode::Esc | KeyCode::Char('q') => CopyKey::Leave,
        _ => CopyKey::Ignore,
    }
}

/// Paint a pane's history, with the selection marked and the way out shown.
///
/// Reverse video for the selection: it is the one highlight every terminal
/// has, in every theme, including the ones with no colour at all.
pub(crate) fn draw_history(
    out: &mut impl Write,
    reading: &omt_tui::CopyMode,
    rect: omt_tui::Rect,
    terminal: Option<&omt_term::Terminal>,
) -> io::Result<()> {
    let lines: Vec<(u64, String)> = terminal.map_or_else(Vec::new, |terminal| {
        (0..rect.rows)
            .map(|offset| {
                let row = reading.top + u64::from(offset);
                (
                    row,
                    terminal
                        .line_at(row)
                        .map(omt_term::Line::text)
                        .unwrap_or_default(),
                )
            })
            .collect()
    });
    for (offset, (row, text)) in lines.iter().enumerate() {
        let screen_row = rect.row + crate::run::CHROME_ROWS + offset as u16 + 1;
        write!(out, "\x1b[{screen_row};{}H\x1b[K", rect.col + 1)?;
        for (col, ch) in text.chars().take(rect.cols as usize).enumerate() {
            if reading.selected(*row, col as u16) {
                write!(out, "\x1b[7m{ch}\x1b[27m")?;
            } else {
                write!(out, "{ch}")?;
            }
        }
    }
    // Said on the screen rather than left to be remembered: this is a mode,
    // and a mode nobody can see the way out of is a terminal somebody
    // force-quits.
    let hint = reading.typing.as_ref().map_or_else(
        || "copy: hjkl move · / search · v select · y copy · Esc leave".to_owned(),
        // The field itself, because a search somebody cannot see what they
        // have typed into is a search they retype.
        |typed| format!("search: {typed}"),
    );
    let hint_row = rect.row + crate::run::CHROME_ROWS + rect.rows;
    write!(out, "\x1b[{hint_row};{}H\x1b[7m{hint}\x1b[0m", rect.col + 1)
}

/// Whether copy mode should stay open after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reading {
    /// Still reading.
    Stay,
    /// Done — the pane comes back.
    Leave,
}

/// One key, while somebody is reading history rather than typing into a pane.
///
/// Every key belongs to this mode, like an overlay and for the same reason:
/// somebody is reading, and a keystroke that reached the shell would type into
/// a program nobody is looking at.
pub(crate) fn reading_key(
    key: &crossterm::event::KeyEvent,
    reading: &mut omt_tui::CopyMode,
    rect: omt_tui::Rect,
    terminal: Option<&omt_term::Terminal>,
) -> Reading {
    use crossterm::event::KeyCode;

    let rows = rect.rows.max(1);
    let cols = if rect.cols == 0 { 80 } else { rect.cols };
    let total = terminal.map_or(0, omt_term::Terminal::total_rows);
    let line = |row: u64| {
        terminal
            .and_then(|t| t.line_at(row))
            .map(omt_term::Line::text)
    };

    // While the search field is open every key is text, apart from the ones
    // that close it. Checked first, because otherwise typing `y` into a search
    // would copy and leave.
    if let Some(typed) = reading.typing.as_mut() {
        match key.code {
            KeyCode::Char(ch) => typed.push(ch),
            KeyCode::Backspace => {
                typed.pop();
            }
            KeyCode::Enter => {
                if reading.submit().is_some() {
                    reading.jump(true, total, rows, line);
                }
            }
            KeyCode::Esc => reading.typing = None,
            _ => {}
        }
        return Reading::Stay;
    }

    match copy_key(key) {
        CopyKey::Go(towards) => reading.go(towards, total, rows, cols),
        CopyKey::Mark => reading.mark(),
        CopyKey::Search => reading.search(),
        CopyKey::Repeat(forwards) => {
            reading.jump(forwards, total, rows, line);
        }
        CopyKey::Yank => {
            let text = omt_tui::selected_text(reading, line);
            // Through the terminal omt is drawing on, which is the only
            // clipboard reachable from the far end of an ssh link — where
            // these sessions actually run.
            if !text.is_empty()
                && let Ok(sequence) = omt_media::clipboard::osc52(
                    &text,
                    &std::env::var("TERM").unwrap_or_default(),
                    omt_media::clipboard::DEFAULT_MAX_BYTES,
                )
            {
                let mut stdout = io::stdout();
                let _ = stdout.write_all(sequence.as_bytes());
                let _ = stdout.flush();
            }
            return Reading::Leave;
        }
        CopyKey::Leave => return Reading::Leave,
        CopyKey::Ignore => {}
    }
    Reading::Stay
}

/// What is still running, as rows somebody reads before ending it.
///
/// Every pane with a live session, with what its agent is doing where omt
/// knows. Ordered by what needs a person first, for the same reason the agent
/// list is: the row that changes somebody's mind is the one at the top.
pub(crate) fn still_working(watched: &[crate::rows::Watched]) -> Vec<omt_tui::Choice> {
    let mut rows: Vec<(u8, omt_tui::Choice)> = Vec::new();
    for session in watched {
        // An exited session has nothing left to lose, and listing it would
        // make the warning look like noise.
        if matches!(session.lifecycle, omt_session::SessionState::Exited { .. }) {
            continue;
        }
        rows.push((
            omt_tui::attention(&session.state, session.seen),
            omt_tui::Choice {
                label: session.title.clone(),
                detail: format!(
                    "{} · ends when omt does",
                    omt_tui::state_word(&session.state, session.seen)
                ),
                chord: None,
                mark: Some(omt_tui::state_mark(&session.state, session.seen)),
            },
        ));
    }
    rows.sort_by_key(|(rank, _)| *rank);
    let mut out: Vec<omt_tui::Choice> = rows.into_iter().map(|(_, choice)| choice).collect();
    if !out.is_empty() {
        // The instruction, in the list rather than beside it: this overlay is
        // what somebody is looking at, and an instruction they have to find
        // elsewhere is one they do not read.
        out.push(omt_tui::Choice {
            label: "press Enter to quit anyway".to_owned(),
            detail: "Esc keeps them".to_owned(),
            chord: None,
            mark: None,
        });
    }
    out
}
